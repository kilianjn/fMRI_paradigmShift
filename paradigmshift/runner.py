"""Presents a paradigm in the scanner and logs triggers, events and grip responses.

Timing: the paradigm clock starts at the first SyncBox trigger after the dummy
scans (t = 0). Planned onsets are relative to that trigger; actual onsets are
measured with ``time.perf_counter`` when each frame is drawn. All triggers are
logged too, so the timing can be verified volume by volume afterwards.
"""
from __future__ import annotations

import csv
import os
import re
from datetime import datetime

from PySide6.QtCore import QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QFont, QGuiApplication, QPainter
from PySide6.QtWidgets import QWidget

from .hardware import InputRouter, now
from .model import Block, Paradigm
from .stimulus import gonogo_visible, load_pixmap, paint_stimulus


def bids_label(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9]", "", text) or "task"


def output_stem(paradigm: Paradigm, s: dict) -> str:
    parts = [f"sub-{bids_label(s['sub'])}"]
    if s.get("ses"):
        parts.append(f"ses-{bids_label(s['ses'])}")
    parts.append(f"task-{bids_label(paradigm.name)}")
    parts.append(f"run-{int(s.get('run', 1)):02d}")
    return "_".join(parts)


class RunState:
    """Pure-Python bookkeeping of a run (kept separate from the window for testing)."""

    def __init__(self, paradigm: Paradigm):
        self.p = paradigm
        self.triggers: list[float] = []
        self.t0: float | None = None
        self.log: list[dict] = []          # raw log: triggers, buttons, onsets
        self.events: dict[str, dict] = {}  # block id -> event row
        self.ratings: dict[str, dict] = {}  # block id -> {"value", "confirmed", "rt"}

    def on_trigger(self, t: float) -> bool:
        """Returns True when this trigger starts the paradigm."""
        self.triggers.append(t)
        n = len(self.triggers)
        started = False
        if self.t0 is None and n == self.p.dummy_scans + 1:
            self.t0 = t
            started = True
        self.log.append({"time": self.rel(t), "type": "trigger", "value": n, "info": "dummy" if self.t0 is None
                         or (n <= self.p.dummy_scans) else f"volume {n - self.p.dummy_scans}"})
        return started

    def rel(self, t: float) -> float | str:
        return round(t - self.t0, 4) if self.t0 is not None else "n/a"

    def onset(self, b: Block, t: float) -> None:
        if b.id in self.events:
            return
        self.events[b.id] = ev = {
            "onset": round(t - self.t0, 4), "duration": b.duration, "planned_onset": b.start,
            "trial_type": b.condition or b.kind, "block": b.kind, "label": b.display_label,
            "stim_file": os.path.basename(b.params.get("path", "")) if b.params.get("path") else "n/a",
            "response": "n/a", "response_time": "n/a", "rating": "n/a", "correct": "n/a", "outcome": "n/a",
        }
        if b.kind == "match":
            ev["stim_file"] = os.path.basename(b.params.get("top", "")) or "n/a"
        elif b.kind == "gonogo":
            ev["trial_type"] = b.condition or b.params.get("trial", "go")
        self.log.append({"time": self.rel(t), "type": "onset", "value": b.kind, "info": b.display_label})
        if b.kind == "rating":
            lo, hi = int(b.params.get("min", 1)), int(b.params.get("max", 4))
            start = (lo + hi + 1) // 2 if b.params.get("mode") == "cursor" else None
            self.ratings[b.id] = {"value": start, "confirmed": False, "rt": None}

    def on_button(self, name: str, t: float) -> list[str]:
        """Apply a grip press to whatever is active. Returns ids of blocks that changed."""
        self.log.append({"time": self.rel(t), "type": "button", "value": name, "info": ""})
        if self.t0 is None:
            return []
        tt = t - self.t0
        changed = []
        hw = self.p.hardware
        for b in self.p.active_blocks(tt):
            ev = self.events.get(b.id)
            if ev is None:
                continue
            if b.kind == "response":
                want = b.params.get("buttons", "any")
                if want not in ("any", name) or ev["response"] != "n/a":
                    continue
                ev["response"] = name
                ev["response_time"] = round(tt - ev["onset"], 4)
                if b.params.get("correct"):
                    ev["correct"] = int(name == b.params["correct"])
                changed.append(b.id)
            elif b.kind == "match":
                if ev["response"] != "n/a":
                    continue
                side = {b.params.get("left_button", "Left index"): "left",
                        b.params.get("right_button", "Right index"): "right"}.get(name)
                if side is None:
                    continue
                ev["response"] = side
                ev["response_time"] = round(tt - ev["onset"], 4)
                ev["correct"] = int(side == b.params.get("correct", "left"))
                changed.append(b.id)
            elif b.kind == "gonogo":
                want = b.params.get("button", "any")
                if want not in ("any", name) or ev["response"] != "n/a":
                    continue
                ev["response"] = name
                ev["response_time"] = round(tt - ev["onset"], 4)
                go = b.params.get("trial", "go") == "go"
                ev["outcome"] = "hit" if go else "false_alarm"
                ev["correct"] = int(go)
                changed.append(b.id)
            elif b.kind == "rating":
                st = self.ratings[b.id]
                if st["confirmed"]:
                    continue
                lo, hi = int(b.params.get("min", 1)), int(b.params.get("max", 4))
                if b.params.get("mode", "direct") == "direct":
                    names = hw.button_names()
                    if name in names and lo + names.index(name) <= hi:
                        st.update(value=lo + names.index(name), confirmed=True, rt=tt - ev["onset"])
                else:
                    if name == hw.move_left:
                        st["value"] = max(lo, (st["value"] or lo) - 1)
                    elif name == hw.move_right:
                        st["value"] = min(hi, (st["value"] or lo) + 1)
                    elif name == hw.confirm:
                        st.update(confirmed=True, rt=tt - ev["onset"])
                if st["confirmed"]:
                    ev["rating"] = st["value"]
                    ev["response"] = name
                    ev["response_time"] = round(st["rt"], 4)
                changed.append(b.id)
        return changed

    def write(self, out_dir: str, stem: str) -> list[str]:
        os.makedirs(out_dir, exist_ok=True)
        # last chosen cursor position counts as an (unconfirmed) answer
        for bid, st in self.ratings.items():
            ev = self.events.get(bid)
            if ev and not st["confirmed"] and st["value"] is not None:
                ev["rating"] = f"{st['value']} (unconfirmed)"
        for bid, ev in self.events.items():
            b = self.p.block(bid)
            if b is not None and b.kind == "gonogo" and ev["response"] == "n/a":
                go = b.params.get("trial", "go") == "go"
                ev["outcome"] = "miss" if go else "correct_rejection"
                ev["correct"] = int(not go)
            elif b is not None and b.kind == "match" and ev["response"] == "n/a":
                ev["outcome"] = "miss"
                ev["correct"] = 0
        ev_path = os.path.join(out_dir, f"{stem}_events.tsv")
        cols = ["onset", "duration", "trial_type", "block", "label", "stim_file", "response",
                "response_time", "rating", "correct", "outcome", "planned_onset"]
        with open(ev_path, "w", newline="", encoding="utf-8") as fh:
            w = csv.DictWriter(fh, cols, delimiter="\t", extrasaction="ignore")
            w.writeheader()
            for row in sorted(self.events.values(), key=lambda r: r["onset"]):
                w.writerow(row)
        log_path = os.path.join(out_dir, f"{stem}_log.tsv")
        with open(log_path, "w", newline="", encoding="utf-8") as fh:
            w = csv.DictWriter(fh, ["time", "type", "value", "info"], delimiter="\t")
            w.writeheader()
            w.writerows(self.log)
        return [ev_path, log_path]


def performance_summary(state: "RunState") -> list[str]:
    """Short accuracy summary shown on the participant screen after the run."""
    rows = list(state.events.values())
    lines = []
    gng = [r for r in rows if r["block"] == "gonogo"]
    if gng:
        n = {o: sum(r["outcome"] == o for r in gng) for o in ("hit", "miss", "false_alarm", "correct_rejection")}
        lines.append(f"Go/No-Go: {n['hit']} hits, {n['miss']} misses, {n['false_alarm']} false alarms, "
                     f"{n['correct_rejection']} correct rejections")
    match = [r for r in rows if r["block"] == "match"]
    if match:
        ok = sum(r["correct"] == 1 for r in match)
        lines.append(f"Matching: {ok}/{len(match)} correct")
    return lines


class PresentationWindow(QWidget):
    """The participant screen. Waits for the scanner, then plays the timeline."""
    finished = Signal(list)   # written files

    def __init__(self, paradigm: Paradigm, settings: dict):
        super().__init__()
        self.setWindowTitle(f"{paradigm.name} – participant screen")
        self.setCursor(Qt.BlankCursor)
        self.setFocusPolicy(Qt.StrongFocus)
        self.p = paradigm
        self.settings = settings
        self.state = RunState(paradigm)
        self.phase = "waiting"
        self.current: Block | None = None
        self.files: list[str] = []
        self._esc_at = 0.0
        self._visible = True
        for b in paradigm.blocks:          # preload pictures – no disk access while running
            for f in b.image_files():
                load_pixmap(f)
        self.router = InputRouter(paradigm.hardware, settings.get("simulate", False), paradigm.tr, self)
        self.router.trigger.connect(self._on_trigger)
        self.router.button.connect(self._on_button)
        self.router.error.connect(lambda m: setattr(self, "error", m) or self.update())
        self.error = ""
        self.timer = QTimer(self)
        self.timer.setTimerType(Qt.PreciseTimer)
        self.timer.timeout.connect(self._tick)

    def start(self):
        screens = QGuiApplication.screens()
        scr = screens[min(self.settings.get("screen", 0), len(screens) - 1)]
        self.setGeometry(scr.geometry())
        if self.settings.get("fullscreen", True):
            self.showFullScreen()
        else:
            self.resize(1280, 720)
            self.show()
        self.activateWindow()
        self.setFocus()
        self.router.start()
        if self.settings.get("simulate"):
            QTimer.singleShot(1500, self.router.start_simulation)

    # -- input -----------------------------------------------------------------
    def keyPressEvent(self, e):
        if e.key() == Qt.Key_Escape:
            t = now()
            if self.phase == "done" or t - self._esc_at < 1.5:
                self._finish(aborted=self.phase != "done")
                self.close()
            self._esc_at = t
            self.update()
            return
        if not e.isAutoRepeat():
            self.router.feed(e.text(), now())

    def _on_trigger(self, t):
        if self.phase == "done":
            return
        if self.state.on_trigger(t):
            self.phase = "running"
            self.timer.start(1)
        self.update()

    def _on_button(self, name, t):
        if self.state.on_button(name, t):
            self.update()

    # -- clock -----------------------------------------------------------------
    def elapsed(self) -> float:
        return now() - self.state.t0 if self.state.t0 is not None else 0.0

    def _tick(self):
        t = self.elapsed()
        if t >= self.p.duration:
            self._finish()
            return
        b = self.p.stimulus_at(t)
        changed = b is not self.current
        self.current = b
        if b is not None and b.kind == "gonogo":
            visible = gonogo_visible(b, t - b.start)
            if visible != self._visible:
                changed = True
            self._visible = visible
        for blk in self.p.active_blocks(t):
            if blk.id not in self.state.events:
                self.state.onset(blk, now())
                changed = True
        if changed:
            self.repaint()

    def _finish(self, aborted: bool = False):
        if self.phase == "done" and self.files:
            return
        self.timer.stop()
        self.router.stop()
        self.phase = "done"
        self.aborted = aborted
        if self.state.t0 is not None:
            stem = output_stem(self.p, self.settings)
            if aborted:
                stem += "_ABORTED"
            self.files = self.state.write(self.settings["out_dir"], stem)
        self.setCursor(Qt.ArrowCursor)
        self.update()
        self.finished.emit(self.files)

    def closeEvent(self, e):
        if self.phase != "done":
            self._finish(aborted=True)
        super().closeEvent(e)

    # -- drawing -----------------------------------------------------------------
    def paintEvent(self, _e):
        p = QPainter(self)
        r = QRectF(self.rect())
        if self.phase == "running":
            st = dict(self.state.ratings.get(self.current.id, {})) if self.current is not None else {}
            if self.current is not None:
                st["t"] = self.elapsed() - self.current.start
            paint_stimulus(p, r, self.p, self.current, st, ref_height=r.height())
        else:
            p.fillRect(r, QColor(self.p.background))
            p.setPen(QColor("#9AA0AA"))
            f = QFont("Inter")
            f.setPixelSize(max(14, int(r.height() * .025)))
            p.setFont(f)
            if self.phase == "waiting":
                n = len(self.state.triggers)
                hw = self.p.hardware
                src = (f"key “{hw.trigger_key}”" if hw.sync_mode == "keyboard" else f"serial {hw.serial_port}")
                msg = f"Waiting for scanner …\n\n{n} / {self.p.dummy_scans + 1} triggers   ·   {src}"
                if self.settings.get("simulate"):
                    msg += "\n(simulated scanner)"
            else:
                msg = ("Run aborted." if getattr(self, "aborted", False) else "Run complete.") + "\n\n"
                summary = performance_summary(self.state)
                if summary:
                    msg += "\n".join(summary) + "\n\n"
                msg += ("Saved:\n" + "\n".join(os.path.basename(f) for f in self.files)) if self.files \
                    else "No data recorded (the scanner never started)."
                msg += "\n\nPress Esc to close."
            if self.error:
                msg += f"\n\n⚠ {self.error}"
            if self.phase != "done" and now() - self._esc_at < 1.5:
                msg += "\n\nPress Esc again to abort"
            p.drawText(r, Qt.AlignCenter, msg)
        p.end()


def timestamp() -> str:
    return datetime.now().strftime("%Y%m%d-%H%M%S")
