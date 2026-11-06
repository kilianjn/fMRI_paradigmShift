"""Presents a paradigm in the scanner and logs triggers, events and grip responses.

Timing: the paradigm clock starts at the first SyncBox trigger after the dummy
scans (t = 0). Planned onsets are relative to that trigger; actual onsets are
measured with ``time.perf_counter`` when the stimulus is switched. All triggers
are logged too, so the timing can be verified volume by volume afterwards.

A run has one :class:`RunController` (clock, hardware input, logging) and two
views on it: the :class:`PresentationWindow` the participant sees, and the
control room window (:mod:`paradigmshift.controlroom`) for the operator.
"""
from __future__ import annotations

import csv
import os
import re
from datetime import datetime

from PySide6.QtCore import QEvent, QObject, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QGuiApplication, QPainter
from PySide6.QtWidgets import QApplication, QWidget

from .hardware import InputRouter, now
from .model import SCORED_TRIAL_KINDS, Block, Paradigm
from .stimulus import load_pixmap, paint_stimulus, paint_waiting, visual_key


def bids_label(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9]", "", text) or "task"


def output_stem(paradigm: Paradigm, s: dict) -> str:
    parts = [f"sub-{bids_label(s.get('sub', '01'))}"]
    if s.get("ses"):
        parts.append(f"ses-{bids_label(s['ses'])}")
    parts.append(f"task-{bids_label(paradigm.name)}")
    parts.append(f"run-{int(s.get('run', 1)):02d}")
    return "_".join(parts)


def trial_success(b: Block, ev: dict) -> bool:
    """Was a finished trial done right? (Unanswered trials count as over – used for feedback blocks.)"""
    answered = ev["response"] != "n/a"
    if b.kind == "gonogo":
        return answered == (b.params.get("trial", "go") == "go")
    if b.kind == "choice":
        corr = b.params.get("correct", "")
        return (not answered) if corr == "" else (answered and ev["correct"] == 1)
    if b.kind == "match":
        return ev["correct"] == 1
    return False


class RunState:
    """Pure-Python bookkeeping of a run (kept separate from the window for testing)."""

    def __init__(self, paradigm: Paradigm):
        self.p = paradigm
        self.triggers: list[float] = []
        self.t0: float | None = None
        self.log: list[dict] = []          # raw log: triggers, buttons, onsets
        self.events: dict[str, dict] = {}  # block id -> event row
        self.ratings: dict[str, dict] = {}  # block id -> {"value", "confirmed", "rt"}
        self.feedback: dict[str, bool] = {}  # feedback block id -> success of the trial it reports
        self.presses: list[dict] = []       # every grip/key press of the whole run (also written live to a file)
        self.version = 0                   # bumped on every change, lets views refresh cheaply

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
        self.version += 1
        return started

    def record_press(self, key: str, name: str, t: float, t_open: float) -> dict:
        """One row of the button-press backup log."""
        tt = t - self.t0 if self.t0 is not None else None
        blk = self.p.stimulus_at(tt) if tt is not None else None
        row = {
            "time": round(tt, 4) if tt is not None else "n/a",
            "time_since_open": round(t - t_open, 4),
            "volume": self.volumes_seen(),
            "wall_clock": datetime.now().isoformat(timespec="milliseconds"),
            "key": key, "button": name or "unassigned",
            "block": blk.display_label if blk else ("before start" if tt is None else "–"),
            "block_kind": blk.kind if blk else "", "condition": blk.condition if blk else "",
        }
        self.presses.append(dict(row, button=name))
        return row

    def force_start(self, t: float) -> None:
        """Start without waiting for the scanner (operator override, logged as such)."""
        if self.t0 is None:
            self.t0 = t
            self.log.append({"time": 0.0, "type": "manual_start", "value": "", "info": "started without trigger"})
            self.version += 1

    @property
    def dummies_seen(self) -> int:
        return min(len(self.triggers), self.p.dummy_scans)

    def volumes_seen(self) -> int:
        return max(0, len(self.triggers) - self.p.dummy_scans)

    def measured_tr(self, last: int = 8) -> float | None:
        ts = self.triggers[-last:]
        if len(ts) < 2:
            return None
        return (ts[-1] - ts[0]) / (len(ts) - 1)

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
        elif b.kind == "feedback":
            prev = self.p.previous_scored(b)
            pev = self.events.get(prev.id) if prev else None
            ok = bool(prev and pev and trial_success(prev, pev))
            ev["outcome"] = "hit" if ok else "miss"
            ev["label"] = b.params.get("hit_text" if ok else "miss_text", "")
            self.feedback[b.id] = ok
        self.log.append({"time": self.rel(t), "type": "onset", "value": b.kind, "info": b.display_label})
        if b.kind == "rating":
            lo, hi = int(b.params.get("min", 1)), int(b.params.get("max", 4))
            start = (lo + hi + 1) // 2 if b.params.get("mode") == "cursor" else None
            self.ratings[b.id] = {"value": start, "confirmed": False, "rt": None}
        self.version += 1

    def paint_state(self, b: Block | None, t: float) -> dict:
        """What the renderer needs to draw *b* at paradigm time *t* (rating cursor, chosen picture…)."""
        if b is None:
            return {}
        st = dict(self.ratings.get(b.id, {}))
        st["t"] = t - b.start
        ev = self.events.get(b.id)
        if b.kind == "match" and ev and ev["response"] in ("left", "right"):
            st["chosen"] = ev["response"]
        if b.kind == "feedback":
            st["success"] = self.feedback.get(b.id, False)
        return st

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
            if b.kind == "match":
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
            elif b.kind == "choice":
                want = b.params.get("buttons", "any")
                if want not in ("any", name) or ev["response"] != "n/a":
                    continue
                corr = b.params.get("correct", "")
                ev["response"] = name
                ev["response_time"] = round(tt - ev["onset"], 4)
                if corr == "":
                    ev["outcome"], ev["correct"] = "false_alarm", 0
                elif corr == "any":
                    ev["outcome"], ev["correct"] = "hit", 1
                else:
                    ok = name == corr
                    ev["outcome"], ev["correct"] = ("correct" if ok else "incorrect"), int(ok)
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
        if changed:
            self.version += 1
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
            elif b is not None and b.kind == "choice" and ev["response"] == "n/a":
                expect = b.params.get("correct", "") != ""
                ev["outcome"] = "miss" if expect else "correct_rejection"
                ev["correct"] = int(not expect)
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


# =============================================================================
# Live evaluation – used by the control room and the end-of-run summary
# =============================================================================
SCORED_KINDS = ("match", "gonogo", "choice", "rating")


def live_result(b: Block, ev: dict, t: float | None) -> tuple[str, str]:
    """(text, tone) describing a trial's result so far. tone: ok | bad | warn | pending | info."""
    answered = ev["response"] != "n/a"
    over = t is None or t >= b.end
    if b.kind == "gonogo":
        go = b.params.get("trial", "go") == "go"
        if answered:
            return ("hit", "ok") if go else ("false alarm", "bad")
        if not over:
            return "…", "pending"
        return ("miss", "bad") if go else ("correct rejection", "ok")
    if b.kind == "choice":
        corr = b.params.get("correct", "")
        if answered:
            if corr == "":
                return "false alarm", "bad"
            if corr == "any":
                return "hit", "ok"
            return (f"{ev['response']} ✓", "ok") if ev["correct"] == 1 else (f"{ev['response']} ✗", "bad")
        if not over:
            return "…", "pending"
        return ("correct rejection", "ok") if corr == "" else ("miss", "bad")
    if b.kind == "match":
        if answered:
            ok = ev["correct"] == 1
            return (f"{ev['response']} ✓" if ok else f"{ev['response']} ✗"), ("ok" if ok else "bad")
        return ("…", "pending") if not over else ("no answer", "warn")
    if b.kind == "rating":
        if ev["rating"] != "n/a":
            return f"rated {ev['rating']}", "info"
        return ("…", "pending") if not over else ("no rating", "warn")
    return (ev["response"], "info") if answered else ("…", "pending")


def _mean(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    return sum(xs) / len(xs) if xs else None


def live_stats(state: RunState, t: float | None = None, watchdog: int = 5) -> list[tuple[str, str, str]]:
    """Performance so far as ``(title, text, tone)`` lines. ``t=None`` means the run is over."""
    out = []
    rows = [(state.p.block(i), ev) for i, ev in state.events.items()]
    rows = [(b, ev) for b, ev in rows if b is not None]

    def done(b):
        return t is None or t >= b.end or state.events[b.id]["response"] != "n/a"

    match = [(b, ev) for b, ev in rows if b.kind == "match" and done(b)]
    if match:
        ok = sum(ev["correct"] == 1 for _, ev in match)
        miss = sum(ev["response"] == "n/a" for _, ev in match)
        rt = _mean(ev["response_time"] for _, ev in match)
        per = []
        for cond in dict.fromkeys(b.condition for b, _ in match):
            sub = [ev for b, ev in match if b.condition == cond]
            per.append(f"{cond or 'trials'} {sum(e['correct'] == 1 for e in sub)}/{len(sub)}")
        acc = ok / len(match)
        out.append(("Matching", f"{ok}/{len(match)} correct ({acc:.0%})  ·  {miss} no answer"
                    + (f"  ·  RT {rt:.2f}s" if rt else "") + "\n" + "  ·  ".join(per),
                    "ok" if acc >= .8 else "warn"))
    gng = [(b, ev) for b, ev in rows if b.kind == "gonogo" and done(b)]
    if gng:
        res = [live_result(b, ev, t)[0] for b, ev in gng]
        n = {k: res.count(k) for k in ("hit", "miss", "false alarm", "correct rejection")}
        go = n["hit"] + n["miss"]
        nogo = n["false alarm"] + n["correct rejection"]
        rt = _mean(ev["response_time"] for b, ev in gng if b.params.get("trial") == "go")
        text = (f"{n['hit']} hits · {n['miss']} misses · {n['false alarm']} false alarms · "
                f"{n['correct rejection']} correct rejections")
        rates = []
        if go:
            rates.append(f"hit rate {n['hit'] / go:.0%}")
        if nogo:
            rates.append(f"false alarm rate {n['false alarm'] / nogo:.0%}")
        if rt:
            rates.append(f"Go RT {rt:.2f}s")
        out.append(("Go / No-Go", text + "\n" + "  ·  ".join(rates),
                    "ok" if not go or n["hit"] / go >= .8 else "warn"))
    ch = [(b, ev) for b, ev in rows if b.kind == "choice" and done(b)]
    if ch:
        ok = [(b, ev) for b, ev in ch if trial_success(b, ev)]
        acc = len(ok) / len(ch)
        per = []
        for cond in dict.fromkeys(b.condition for b, _ in ch):
            sub = [(b, ev) for b, ev in ch if b.condition == cond]
            good = [(b, ev) for b, ev in sub if trial_success(b, ev)]
            rt = _mean(ev["response_time"] for b, ev in good if ev["response"] != "n/a")
            per.append(f"{cond or 'trials'} {len(good)}/{len(sub)}" + (f" RT {rt:.2f}s" if rt else ""))
        fa = sum(ev["outcome"] == "false_alarm" for _, ev in ch)
        miss = sum(b.params.get("correct", "") != "" and ev["response"] == "n/a" for b, ev in ch)
        out.append(("Choice trials", f"{len(ok)}/{len(ch)} correct ({acc:.0%})  ·  {miss} misses  ·  "
                    f"{fa} false alarms\n" + "  ·  ".join(per), "ok" if acc >= .8 else "warn"))
    rat = [(b, ev) for b, ev in rows if b.kind == "rating" and done(b)]
    if rat:
        given = [(b, ev) for b, ev in rat if ev["rating"] != "n/a"]
        per = []
        # rating blocks often share one condition ("rating"); group by the stimulus shown before instead
        for b, ev in given:
            prev = [o for o in state.p.blocks_on(b.track) if o.end <= b.start + 1e-6 and o.condition
                    and o.kind in ("image", "match", "text") and o.condition not in ("cue", "instruction")]
            key = (prev[-1].condition if prev else "") if b.condition in ("", "rating") else b.condition
            per.append((key or "all", ev["rating"]))
        groups = {}
        for k, v in per:
            if isinstance(v, (int, float)):
                groups.setdefault(k, []).append(v)
        text = f"{len(given)}/{len(rat)} rated"
        if groups:
            text += "\n" + "  ·  ".join(f"{k} ⌀ {sum(v) / len(v):.1f}" for k, v in groups.items())
        out.append(("Ratings", text, "ok" if len(given) == len(rat) else "warn"))
    if state.presses:
        per = {}
        for pr in state.presses:
            per[pr["button"] or f"key {pr['key']}"] = per.get(pr["button"] or f"key {pr['key']}", 0) + 1
        out.append(("Grip presses", f"{len(state.presses)} in total  ·  " +
                    "  ·  ".join(f"{k} {v}" for k, v in per.items()), "info"))
    # watchdog: participant stopped answering?
    if t is not None and watchdog > 0:
        recent = sorted(((b, ev) for b, ev in rows if b.kind in SCORED_KINDS and t >= b.end
                         and not (b.kind == "gonogo" and b.params.get("trial") == "nogo")
                         and not (b.kind == "choice" and b.params.get("correct", "") == "")),
                        key=lambda x: x[0].start)[-watchdog:]
        if len(recent) == watchdog and all(ev["response"] == "n/a" for _, ev in recent):
            out.insert(0, ("Attention", f"No response in the last {watchdog} trials – check the grips and the "
                           "participant.", "bad"))
    return out


def performance_summary(state: RunState) -> list[str]:
    """Plain-text accuracy summary (end of run)."""
    return [f"{title}: {text.splitlines()[0]}" for title, text, _ in live_stats(state, None)
            if title != "Attention"]


# =============================================================================
# Run controller – one per run, shared by participant screen and control room
# =============================================================================
class RunController(QObject):
    stimulusChanged = Signal()     # participant screen must repaint
    changed = Signal()             # anything noteworthy (trigger, onset, response, phase)
    abortArmed = Signal(bool)      # Esc pressed once: ask for confirmation
    finished = Signal(list)        # written files

    def __init__(self, paradigm: Paradigm, settings: dict, parent=None):
        super().__init__(parent)
        self.p = paradigm
        self.settings = settings
        self.state = RunState(paradigm)
        self.current: Block | None = None
        self.files: list[str] = []
        self.aborted = False
        self.error = ""
        self._done = False
        self._t_end: float | None = None    # paradigm time at which the run ended (frozen clock)
        self._visual_key = ("init",)
        self._abort_armed_at = 0.0
        for b in paradigm.blocks:          # preload pictures – no disk access while running
            for f in b.image_files():
                load_pixmap(f)
        # a controlled SyncBox is told how many volumes to expect (+ a margin, so it never stops early)
        self.router = InputRouter(paradigm.hardware, settings.get("simulate", False), paradigm.tr, self,
                                  volumes=paradigm.volumes + paradigm.dummy_scans + 20)
        self.hw_status = ""
        self.router.status.connect(self._on_hw_status)
        self.router.trigger.connect(self._on_trigger)
        self.router.button.connect(self._on_button)
        self.router.pressed.connect(self._on_press)
        self.router.error.connect(self._on_error)
        self.timer = QTimer(self)
        self.timer.setTimerType(Qt.PreciseTimer)
        self.timer.timeout.connect(self._tick)

    # -- lifecycle ---------------------------------------------------------------
    PRESS_COLUMNS = ["time", "time_since_open", "volume", "wall_clock", "key", "button", "block", "block_kind",
                     "condition"]

    def _open_press_log(self):
        """Every press is appended – and flushed – the moment it happens (crash-safe backup)."""
        self._t_open = now()
        self._press_file = None
        out = self.settings.get("out_dir")
        if not out:
            return
        try:
            os.makedirs(out, exist_ok=True)
            self.press_path = os.path.join(out, output_stem(self.p, self.settings) + "_buttons.tsv")
            self._press_file = open(self.press_path, "w", newline="", encoding="utf-8")
            self._press_writer = csv.DictWriter(self._press_file, self.PRESS_COLUMNS, delimiter="\t")
            self._press_writer.writeheader()
            self._press_file.flush()
        except OSError as exc:
            self.error = f"Could not create the button-press log: {exc}"
            self._press_file = None

    def _on_press(self, key, name, t):
        row = self.state.record_press(key, name, t, getattr(self, "_t_open", t))
        if getattr(self, "_press_file", None) is not None:
            try:
                self._press_writer.writerow(row)
                self._press_file.flush()
            except OSError as exc:
                self.error = f"Button-press log: {exc}"
        self.changed.emit()

    def _close_press_log(self):
        f = getattr(self, "_press_file", None)
        if f is None:
            return
        f.close()
        self._press_file = None
        if self.state.presses or self.state.t0 is not None:
            self.files.append(self.press_path)
        else:                            # never started, nothing pressed: no empty file left behind
            try:
                os.remove(self.press_path)
            except OSError:
                pass

    def start(self):
        self._open_press_log()
        app = QApplication.instance()
        if app is not None:
            app.installEventFilter(self)   # SyncBox/grip keys reach the run whichever app window has focus
        self.router.start()
        if self.settings.get("simulate"):
            QTimer.singleShot(1500, self.router.start_simulation)
        self.changed.emit()

    @property
    def phase(self) -> str:
        if self._done:
            return "done"
        if self.state.t0 is not None:
            return "running"
        return "dummy" if self.state.triggers else "waiting"

    def elapsed(self) -> float:
        """Paradigm time. Stops at the end of the run – also when it was aborted."""
        if self.state.t0 is None:
            return 0.0
        if self._t_end is not None:
            return self._t_end
        return now() - self.state.t0

    @property
    def ended_at(self) -> float | None:
        return self._t_end

    def start_now(self):
        """Operator override: start the paradigm clock without a scanner trigger."""
        if self.state.t0 is None and not self._done:
            self.state.force_start(now())
            self._begin()

    def request_abort(self):
        """First call arms, a second call within 3 s aborts (so a stray key press can't end a run)."""
        if self._done:
            return
        if not self.settings.get("confirm_abort", True) or now() - self._abort_armed_at < 3.0:
            self.finish(aborted=True)
        else:
            self._abort_armed_at = now()
            self.abortArmed.emit(True)
            QTimer.singleShot(3000, lambda: self.abortArmed.emit(False))

    def finish(self, aborted: bool = False):
        if self._done:
            return
        if self.state.t0 is not None:
            self._t_end = min(now() - self.state.t0, self.p.duration) if aborted else self.p.duration
        self._done = True
        self.aborted = aborted
        self.timer.stop()
        self.router.stop()
        app = QApplication.instance()
        if app is not None:
            app.removeEventFilter(self)
        if self.state.t0 is not None:
            if aborted:
                self.state.log.append({"time": round(self._t_end, 4), "type": "abort", "value": "",
                                       "info": "run aborted by operator"})
            stem = output_stem(self.p, self.settings) + ("_ABORTED" if aborted else "")
            try:
                self.files = self.state.write(self.settings["out_dir"], stem)
                if self.settings.get("save_paradigm_with_data", True):
                    path = os.path.join(self.settings["out_dir"], stem + "_paradigm.json")
                    self.p.save(path)
                    self.files.append(path)
            except OSError as exc:
                self.error = f"Could not save the logs: {exc}"
        self._close_press_log()
        self.stimulusChanged.emit()
        self.changed.emit()
        self.finished.emit(self.files)

    # -- input -------------------------------------------------------------------
    def eventFilter(self, obj, e):
        if e.type() == QEvent.ShortcutOverride and not self._done:
            e.accept()      # claim the key, so editor shortcuts (e.g. "S") can't swallow a SyncBox trigger
            return True
        if e.type() == QEvent.KeyPress and not self._done:
            if e.key() == Qt.Key_Escape:
                if not e.isAutoRepeat():
                    self.request_abort()
                return True
            if not e.isAutoRepeat() and e.text():
                self.router.feed(e.text(), now())
            return True
        return False

    def _on_trigger(self, t):
        if self._done:
            return
        if self.state.on_trigger(t):
            self._begin()
        self.changed.emit()

    def _begin(self):
        self.timer.start(1)
        self._tick()
        self.changed.emit()

    def _on_button(self, name, t):
        if self.state.on_button(name, t):
            self.stimulusChanged.emit()
        self.changed.emit()

    def _on_hw_status(self, msg):
        self.hw_status = msg
        self.changed.emit()

    def _on_error(self, msg):
        self.error = msg
        self.changed.emit()

    # -- clock -------------------------------------------------------------------
    def _tick(self):
        t = self.elapsed()
        if t >= self.p.duration:
            self.finish()
            return
        b = self.p.stimulus_at(t)
        self.current = b
        key = visual_key(b, t - b.start) if b is not None else None
        changed = key != self._visual_key
        self._visual_key = key
        onset = False
        for blk in self.p.active_blocks(t):
            if blk.id not in self.state.events:
                self.state.onset(blk, now())
                onset = True
        if changed or onset:
            self.stimulusChanged.emit()
        if onset:
            self.changed.emit()

    # -- shared drawing ----------------------------------------------------------
    def paint_participant(self, p: QPainter, r: QRectF, ref_height: float | None = None, blur: bool = False):
        """Exactly what the participant sees – used by their screen and the control room mirror.

        *blur* is only ever used for the control-room mirror, never for the participant screen."""
        ref = ref_height or r.height()
        ph = self.phase
        if ph == "running":
            t = self.elapsed()
            paint_stimulus(p, r, self.p, self.current, self.state.paint_state(self.current, t), ref_height=ref,
                           blur=blur)
        elif ph == "done":   # aborted: neutral empty screen instead of the "thank you" text
            paint_waiting(p, r, self.p, "" if self.aborted else (self.p.end_text or ""), ref_height=ref)
        else:
            paint_waiting(p, r, self.p, ref_height=ref)


class PresentationWindow(QWidget):
    """The participant screen. Shows only the paradigm – never status or technical text."""

    def __init__(self, controller: RunController):
        super().__init__()
        self.c = controller
        self.setWindowTitle(f"{controller.p.name} – participant screen")
        self.setCursor(Qt.BlankCursor)
        self.setFocusPolicy(Qt.StrongFocus)
        controller.stimulusChanged.connect(self.repaint)
        controller.changed.connect(self.update)

    def show_on_screen(self):
        s = self.c.settings
        screens = QGuiApplication.screens()
        scr = screens[min(s.get("screen", 0), len(screens) - 1)]
        self.setGeometry(scr.geometry())
        if s.get("fullscreen", True):
            self.showFullScreen()
        else:
            self.resize(1280, 720)
            self.show()

    def closeEvent(self, e):
        if self.c.phase != "done":
            self.c.finish(aborted=True)
        super().closeEvent(e)

    def paintEvent(self, _e):
        p = QPainter(self)
        self.c.paint_participant(p, QRectF(self.rect()))
        p.end()


def timestamp() -> str:
    return datetime.now().strftime("%Y%m%d-%H%M%S")
