"""Control room window: what the operator sees on the stimulus PC while a run is going.

Mirror of the participant screen, scanner/trigger status, progress, the current
and next block, a live feed of answers and a running performance summary – so
you can see at a glance that the experiment works the way you intended.
"""
from __future__ import annotations

import os

from PySide6.QtCore import QPointF, QRectF, QSettings, Qt, QTimer, QUrl, Signal
from PySide6.QtGui import QColor, QDesktopServices, QGuiApplication, QPainter, QPen
from PySide6.QtWidgets import (QAbstractItemView, QFrame, QGridLayout, QHBoxLayout, QHeaderView, QLabel,
                               QMenu, QMessageBox, QPushButton, QSizePolicy, QSplitter, QTableWidget,
                               QTableWidgetItem, QToolButton, QVBoxLayout, QWidget)

from . import theme
from .hardware import now
from .runner import SCORED_KINDS, PresentationWindow, RunController, live_result, live_stats
from .timeline import fmt_time

TONE = {"ok": theme.OK, "bad": theme.DANGER, "warn": theme.ACCENT, "pending": theme.TEXT_DIM, "info": theme.TEXT}


def _title(text: str) -> QLabel:
    lab = QLabel(text.upper())
    lab.setObjectName("PanelTitle")
    return lab


class StatCard(QFrame):
    def __init__(self, title: str):
        super().__init__()
        self.setObjectName("Panel")
        lay = QVBoxLayout(self)
        lay.setContentsMargins(12, 8, 12, 8)
        lay.setSpacing(0)
        lay.addWidget(_title(title))
        self.value = QLabel("–")
        self.value.setStyleSheet("font-size: 22px; font-weight: 600; font-family: 'DejaVu Sans Mono', monospace;")
        self.sub = QLabel("")
        self.sub.setObjectName("Hint")
        lay.addWidget(self.value)
        lay.addWidget(self.sub)

    def set(self, value: str, sub: str = "", color: str | None = None):
        self.value.setText(value)
        self.sub.setText(sub)
        self.value.setStyleSheet("font-size: 22px; font-weight: 600; font-family: 'DejaVu Sans Mono', monospace;"
                                 + (f"color: {color};" if color else ""))


class Section(QFrame):
    """A control-room panel that folds to its title bar (with a one-line summary)."""
    foldedChanged = Signal(bool)

    def __init__(self, key: str, title: str, content: QWidget, folded: bool = False, plain: bool = False):
        super().__init__()
        self.key, self.title, self.content = key, title, content
        self.setObjectName("Panel")
        lay = QVBoxLayout(self)
        lay.setContentsMargins(12, 6, 12, 10)
        lay.setSpacing(6)
        head = QHBoxLayout()
        self.btn = QToolButton()
        self.btn.setStyleSheet(f"QToolButton {{ border: none; padding: 2px 4px; color: {theme.TEXT_DIM};"
                               "font-size: 11px; font-weight: 600; letter-spacing: 1px; }"
                               f"QToolButton:hover {{ color: {theme.TEXT}; }}")
        self.btn.setToolButtonStyle(Qt.ToolButtonTextOnly)
        self.btn.setCursor(Qt.PointingHandCursor)
        self.btn.setToolTip("Click to fold / unfold")
        self.btn.clicked.connect(lambda: self.set_folded(not self.folded))
        head.addWidget(self.btn)
        self.summary = QLabel()
        self.summary.setObjectName("Hint")
        head.addWidget(self.summary, 1)
        head.addStretch()
        lay.addLayout(head)
        lay.addWidget(content, 1)
        self.folded = None
        self.set_folded(folded, emit=False)

    def set_folded(self, folded: bool, emit: bool = True):
        if folded == self.folded:
            return
        self.folded = folded
        self.content.setVisible(not folded)
        self.summary.setVisible(folded)
        self.btn.setText(("▸  " if folded else "▾  ") + self.title.upper().replace("&", "&&"))
        self.setSizePolicy(QSizePolicy.Preferred, QSizePolicy.Maximum if folded else QSizePolicy.Preferred)
        if emit:
            self.foldedChanged.emit(folded)

    def set_summary(self, text: str):
        self.summary.setText(text)


class MirrorView(QWidget):
    """A small copy of the participant screen."""

    def __init__(self, c: RunController):
        super().__init__()
        self.c = c
        self.setMinimumSize(360, 210)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.blur = bool(c.settings.get("blur_control_mirror", False))
        c.stimulusChanged.connect(self.update)

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.fillRect(self.rect(), QColor(theme.PANEL))
        r = QRectF(self.rect()).adjusted(2, 2, -2, -2)
        w = min(r.width(), r.height() * 16 / 9)
        box = QRectF(0, 0, w, w * 9 / 16)
        box.moveCenter(r.center())
        self.c.paint_participant(p, box, ref_height=box.height(), blur=self.blur)
        p.setPen(QPen(QColor(theme.BORDER), 1))
        p.setBrush(Qt.NoBrush)
        p.drawRect(box)
        p.end()


class ProgressStrip(QWidget):
    """The whole run as one strip: blocks in their colours, what's done dimmed, a playhead."""

    def __init__(self, c: RunController):
        super().__init__()
        self.c = c
        self.setFixedHeight(54)

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        r = QRectF(self.rect()).adjusted(1, 4, -1, -16)
        p.fillRect(r, QColor("#1B1E24"))
        par = self.c.p
        dur = max(par.duration, 1e-6)
        sx = r.width() / dur
        for b in par.blocks_on(0):
            br = QRectF(r.left() + b.start * sx, r.top(), max(1.0, b.duration * sx - .5), r.height())
            p.fillRect(br, QColor(b.color))
        for b in par.blocks:
            if b.track != 0:   # response windows: a thin green line underneath
                p.fillRect(QRectF(r.left() + b.start * sx, r.bottom() - 4, max(1.0, b.duration * sx), 4),
                           QColor(theme.OK))
        t = min(self.c.elapsed() if self.c.phase in ("running", "done") else 0.0, dur)
        x = r.left() + t * sx
        p.fillRect(QRectF(r.left(), r.top(), x - r.left(), r.height()), QColor(0, 0, 0, 140))
        aborted = self.c.phase == "done" and self.c.aborted
        if aborted:   # the part that never ran: greyed out and hatched
            rest = QRectF(x, r.top(), r.right() - x, r.height())
            p.fillRect(rest, QColor(20, 22, 27, 200))
            p.save()
            p.setClipRect(rest)
            p.setPen(QPen(QColor(240, 80, 110, 90), 2))
            xx = rest.left() - rest.height()
            while xx < rest.right():
                p.drawLine(QPointF(xx, rest.bottom()), QPointF(xx + rest.height(), rest.top()))
                xx += 9
            p.restore()
        p.setPen(QPen(QColor(theme.DANGER if aborted else theme.ACCENT), 3 if aborted else 2))
        p.drawLine(QPointF(x, r.top() - 3), QPointF(x, r.bottom() + 3))
        if aborted:
            p.setFont(theme.ui_font(8, True))
            p.setPen(QColor(theme.DANGER))
            p.drawText(QRectF(x + 4, r.bottom() + 2, 200, 14), Qt.AlignLeft, f"ABORTED at {fmt_time(t)}")
        p.setPen(QColor(theme.TEXT_DIM))
        p.setFont(theme.ui_font(8))
        p.drawText(QRectF(r.left(), r.bottom() + 2, 80, 14), Qt.AlignLeft, "00:00")
        p.drawText(QRectF(r.right() - 80, r.bottom() + 2, 80, 14), Qt.AlignRight, fmt_time(dur, 0))
        p.end()


class ControlRoomWindow(QWidget):
    def __init__(self, c: RunController, participant: PresentationWindow | None = None):
        super().__init__()
        self.c = c
        self.participant = participant
        s = c.settings
        self.setWindowTitle(f"Control room – {c.p.name}")
        self.setWindowIcon(theme.app_icon())
        self.setStyleSheet(f"ControlRoomWindow {{ background: {theme.BG}; }}")
        self.resize(1440, 900)
        root = QVBoxLayout(self)
        root.setContentsMargins(14, 12, 14, 12)
        root.setSpacing(10)

        # header -----------------------------------------------------------------
        head = QHBoxLayout()
        self.pill = QLabel()
        head.addWidget(self.pill)
        who = f"sub-{s.get('sub', '?')}" + (f" · ses-{s['ses']}" if s.get("ses") else "") + f" · run-{s.get('run', 1):02d}"
        info = QLabel(f"<b>{c.p.name}</b>   <span style='color:{theme.TEXT_DIM}'>{who}</span>")
        info.setStyleSheet("font-size: 15px;")
        head.addWidget(info)
        if s.get("simulate"):
            sim = QLabel("SIMULATED SCANNER")
            sim.setStyleSheet(f"background: {theme.PANEL_2}; color: {theme.ACCENT}; border: 1px solid {theme.ACCENT};"
                              "border-radius: 4px; padding: 2px 8px; font-size: 11px; font-weight: 700;")
            head.addWidget(sim)
        head.addStretch()
        self.start_btn = QPushButton("Start now without trigger")
        self.start_btn.setToolTip("Start the paradigm clock immediately (logged as a manual start). "
                                  "Only for tests or if the trigger cable fails.")
        self.start_btn.clicked.connect(self._start_now)
        head.addWidget(self.start_btn)
        self.abort_btn = QPushButton("■  Abort run")
        self.abort_btn.setStyleSheet(f"color: {theme.DANGER}; font-weight: 600;")
        self.abort_btn.clicked.connect(c.request_abort)
        head.addWidget(self.abort_btn)
        self.folder_btn = QPushButton("Open data folder")
        self.folder_btn.clicked.connect(lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(s["out_dir"])))
        self.close_btn = QPushButton("Close")
        self.close_btn.setObjectName("Primary")
        self.close_btn.clicked.connect(self.close)
        head.addWidget(self.folder_btn)
        head.addWidget(self.close_btn)
        root.addLayout(head)

        self.banner = QLabel()
        self.banner.setWordWrap(True)
        self.banner.hide()
        root.addWidget(self.banner)

        # foldable sections ------------------------------------------------------------
        self.sections: dict[str, Section] = {}

        cards_w = QWidget()
        cards = QHBoxLayout(cards_w)
        cards.setContentsMargins(0, 0, 0, 0)
        cards.setSpacing(8)
        self.card_elapsed = StatCard("Elapsed")
        self.card_left = StatCard("Remaining")
        self.card_vol = StatCard("Volume")
        self.card_trig = StatCard("Triggers")
        self.card_tr = StatCard("Measured TR")
        for card in (self.card_elapsed, self.card_left, self.card_vol, self.card_trig, self.card_tr):
            cards.addWidget(card)
        root.addWidget(self._section("status", "Status  ·  time, volumes, triggers", cards_w, plain=True))
        self.strip = ProgressStrip(c)
        root.addWidget(self._section("progress", "Run progress", self.strip, plain=True))

        self.mirror = MirrorView(c)
        self.now_lbl = QLabel()
        self.now_lbl.setStyleSheet("font-size: 14px;")
        self.next_lbl = QLabel()
        self.next_lbl.setObjectName("Hint")
        nn = QWidget()
        nl = QVBoxLayout(nn)
        nl.setContentsMargins(0, 0, 0, 0)
        nl.addWidget(self.now_lbl)
        nl.addWidget(self.next_lbl)

        perf_w = QWidget()
        self.perf_body = QVBoxLayout(perf_w)
        self.perf_body.setContentsMargins(0, 0, 0, 0)
        self.table = QTableWidget(0, 6)
        self.table.setHorizontalHeaderLabels(["Time", "Trial", "Condition", "Answer", "RT", "Result"])
        self.table.verticalHeader().setVisible(False)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.setSelectionMode(QAbstractItemView.NoSelection)
        self.table.setFocusPolicy(Qt.NoFocus)
        hh = self.table.horizontalHeader()
        hh.setSectionResizeMode(QHeaderView.ResizeToContents)
        hh.setSectionResizeMode(1, QHeaderView.Stretch)

        self.columns = []
        self.splitter = QSplitter(Qt.Horizontal)
        for spec in ((("mirror", "Participant screen  (live mirror)", self.mirror, True),
                      ("nownext", "Now & next", nn, False)),
                     (("perf", "Performance so far", perf_w, False),
                      ("answers", "Answers  (newest first)", self.table, True))):
            col = QWidget()
            cl = QVBoxLayout(col)
            cl.setContentsMargins(0, 0, 0, 0)
            cl.setSpacing(10)
            keys = []
            for key, title, w, stretchy in spec:
                sec = self._section(key, title, w, stretchy=stretchy)
                cl.addWidget(sec)
                keys.append(key)
            cl.addStretch(0)
            self.columns.append((col, cl, keys))
            self.splitter.addWidget(col)
        self.splitter.setChildrenCollapsible(False)
        self.splitter.setSizes([800, 620])
        root.addWidget(self.splitter, 1)

        # header menu to show / hide sections
        self.sections_btn = QToolButton()
        self.sections_btn.setText("☰ Sections")
        self.sections_btn.setPopupMode(QToolButton.InstantPopup)
        menu = QMenu(self.sections_btn)
        for key, sec in self.sections.items():
            a = menu.addAction(sec.title)
            a.setCheckable(True)
            a.setChecked(not sec.folded)
            a.toggled.connect(lambda on, k=key: self.sections[k].set_folded(not on))
            sec.foldedChanged.connect(lambda folded, a=a: (a.blockSignals(True), a.setChecked(not folded),
                                                           a.blockSignals(False)))
        menu.addSeparator()
        menu.addAction("Unfold all", lambda: [x.set_folded(False) for x in self.sections.values()])
        menu.addAction("Fold all", lambda: [x.set_folded(True) for x in self.sections.values()])
        self.sections_btn.setMenu(menu)
        head.insertWidget(head.count() - 4, self.sections_btn)
        self._relayout()

        self._shown_version = -1
        self._shown_rows = -1
        self._abort_armed = False
        c.changed.connect(self.refresh)
        c.abortArmed.connect(self._on_abort_armed)
        c.finished.connect(self._on_finished)
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.refresh)
        self.timer.start(int(s.get("control_refresh_ms", 100)))
        self.refresh()

    def _on_finished(self, _files):
        self.timer.stop()        # nothing changes any more – the display stays frozen at the end
        self._abort_armed = False
        self.refresh()

    # ------------------------------------------------------------------ sections
    def _section(self, key, title, widget, stretchy=False, plain=False) -> Section:
        folded = key in self._folded_keys()
        sec = Section(key, title, widget, folded, plain)
        sec.stretchy = stretchy
        sec.foldedChanged.connect(lambda _f: (self._save_folded(), self._relayout()))
        self.sections[key] = sec
        return sec

    @staticmethod
    def _folded_keys() -> set:
        v = QSettings("ParadigmShift", "ParadigmShift").value("controlroom/folded", [])
        if isinstance(v, str):
            v = [v] if v else []
        return set(v or [])

    def _save_folded(self):
        QSettings("ParadigmShift", "ParadigmShift").setValue(
            "controlroom/folded", [k for k, sct in self.sections.items() if sct.folded])

    def _relayout(self):
        """Give the space of folded sections to the open ones; shrink a column that is fully folded."""
        open_cols = []
        for col, cl, keys in self.columns:
            any_stretch = False
            for key in keys:
                sec = self.sections[key]
                grow = sec.stretchy and not sec.folded
                cl.setStretchFactor(sec, 1 if grow else 0)
                any_stretch |= grow
            cl.setStretch(cl.count() - 1, 0 if any_stretch else 1)   # trailing spacer keeps folded bars on top
            open_cols.append(any_stretch)   # a column with only small open sections needs little width
        total = max(1, sum(self.splitter.sizes()) or self.width())
        if all(open_cols) or not any(open_cols):
            self.splitter.setSizes([int(total * .56), int(total * .44)])
        else:
            narrow = 440
            self.splitter.setSizes([total - narrow, narrow] if open_cols[0] else [narrow, total - narrow])

    # ------------------------------------------------------------------ actions
    def _start_now(self):
        if QMessageBox.question(self, "Start without trigger?",
                                "Start the paradigm now without waiting for the scanner?\n"
                                "The onsets will then not be locked to the scan.") == QMessageBox.Yes:
            self.c.start_now()

    def _on_abort_armed(self, on: bool):
        self._abort_armed = on
        self.abort_btn.setText("■  Click again to abort" if on else "■  Abort run")
        self.refresh()

    def closeEvent(self, e):
        if self.c.phase != "done":
            if QMessageBox.warning(self, "Abort the run?", "The run is still going. Abort it and save the data "
                                   "recorded so far?", QMessageBox.Yes | QMessageBox.No) != QMessageBox.Yes:
                e.ignore()
                return
            self.c.finish(aborted=True)
        self.timer.stop()
        if self.participant is not None:
            self.participant.close()
        super().closeEvent(e)

    # ------------------------------------------------------------------ refresh
    def _set_pill(self, text: str, color: str):
        self.pill.setText(text)
        self.pill.setStyleSheet(f"background: {color}; color: #111; border-radius: 11px; padding: 3px 12px;"
                                "font-weight: 700; font-size: 12px;")

    def _banner(self, text: str, color: str):
        if not text:
            self.banner.hide()
            return
        self.banner.setText(text)
        self.banner.setStyleSheet(f"background: {theme.PANEL_2}; border: 1px solid {color}; border-left: 4px solid "
                                  f"{color}; border-radius: 6px; padding: 8px 12px; font-size: 13px;")
        self.banner.show()

    def refresh(self):
        c, par, st = self.c, self.c.p, self.c.state
        ph = c.phase
        tr = par.tr
        t = c.elapsed() if ph in ("running", "done") else 0.0
        dur = par.duration

        # status pill + buttons
        if ph == "waiting":
            self._set_pill("WAITING FOR SCANNER", theme.ACCENT)
        elif ph == "dummy":
            self._set_pill(f"DUMMY SCANS  {st.dummies_seen} / {par.dummy_scans}", theme.ACCENT)
        elif ph == "running":
            self._set_pill("● RUNNING", theme.OK)
        else:
            if c.aborted:
                self._set_pill(f"■ ABORTED at {fmt_time(t)}", theme.DANGER)
            else:
                self._set_pill("✓ FINISHED", "#7FB2FF")
            self.setWindowTitle(f"Control room – {par.name} – {'ABORTED' if c.aborted else 'finished'}")
        self.start_btn.setVisible(ph in ("waiting", "dummy"))
        self.abort_btn.setVisible(ph != "done")
        self.folder_btn.setVisible(ph == "done" and bool(c.files))
        self.close_btn.setVisible(ph == "done")

        # cards
        shown_t = min(t, dur)
        if ph == "done" and c.aborted:
            self.card_elapsed.set(fmt_time(shown_t), f"stopped – of {fmt_time(dur)}", theme.DANGER)
            self.card_left.set("—", f"aborted after {shown_t / dur:.0%}" if dur else "aborted", theme.DANGER)
        else:
            self.card_elapsed.set(fmt_time(shown_t), f"of {fmt_time(dur)}")
            self.card_left.set(fmt_time(max(0.0, dur - shown_t)),
                               f"{shown_t / dur:.0%} done" if dur else "")
        total_vol = par.volumes
        cur_vol = min(total_vol, int(shown_t // tr) + 1) if tr > 0 and ph == "running" else st.volumes_seen()
        self.card_vol.set(f"{cur_vol} / {total_vol}", f"{st.volumes_seen()} counted by trigger")
        need = par.dummy_scans + 1
        if ph in ("waiting", "dummy"):
            self.card_trig.set(f"{len(st.triggers)} / {need}", f"start after {par.dummy_scans} dummy scans",
                               theme.ACCENT)
        else:
            self.card_trig.set(str(len(st.triggers)), f"incl. {st.dummies_seen} dummy")
        mtr = st.measured_tr()
        if mtr is None:
            self.card_tr.set("–", f"expected {tr:g} s")
        else:
            ok = abs(mtr - tr) < 0.05
            self.card_tr.set(f"{mtr:.3f} s", f"expected {tr:g} s  " + ("✓" if ok else "⚠ differs"),
                             None if ok else theme.DANGER)
        self.strip.update()

        # now / next
        if ph == "running":
            b = c.current
            if b is None:
                self.now_lbl.setText("<b>NOW</b>  background (gap)")
            else:
                cond = f" · <span style='color:{theme.TEXT_DIM}'>{b.condition}</span>" if b.condition else ""
                self.now_lbl.setText(f"<b>NOW</b>  <span style='color:{b.color}'>■</span> {b.type.title}: "
                                     f"{b.display_label}{cond}  —  {max(0.0, b.end - t):.1f} s left")
            upcoming = [x for x in par.blocks_on(0) if x.start > t + 1e-6]
            if upcoming:
                n = upcoming[0]
                self.next_lbl.setText(f"NEXT  {n.type.title}: {n.display_label}"
                                      + (f" · {n.condition}" if n.condition else "") + f"  in {n.start - t:.1f} s")
            else:
                self.next_lbl.setText("NEXT  end of run")
        elif ph == "done" and c.aborted:
            self.now_lbl.setText("<b>Participant sees:</b> an empty screen (run aborted)")
            self.next_lbl.setText("")
        elif ph == "done":
            self.now_lbl.setText(f"<b>Participant sees:</b> end screen “{par.end_text}”" if par.end_text
                                 else "<b>Participant sees:</b> empty background")
            self.next_lbl.setText("")
        else:
            what = {"fixation": "a fixation cross", "blank": "an empty screen",
                    "text": f"“{par.waiting_text}”"}.get(par.waiting_screen, "")
            self.now_lbl.setText(f"<b>Participant sees:</b> {what}  (no scanner information is shown to them)")
            first = par.blocks_on(0)[0] if par.blocks_on(0) else None
            self.next_lbl.setText(f"FIRST  {first.type.title}: {first.display_label}" if first else "")

        # banner: most important message first
        app_state = QGuiApplication.applicationState()
        last_trig = now() - st.triggers[-1] if st.triggers else None
        if ph == "done":
            if c.aborted and st.t0 is None:
                msg = "<b>Run aborted</b> before the scanner started."
            elif c.aborted:
                vol = int(t // tr) + 1 if tr > 0 else 0
                msg = (f"<b>Run aborted at {fmt_time(t)}</b> (volume {vol} of {par.volumes}). The clock has "
                       "stopped and the data recorded until then was saved (file names end in _ABORTED).")
            else:
                msg = "<b>Run complete.</b>"
            if c.files:
                msg += "<br>Saved: " + ", ".join(os.path.basename(f) for f in c.files)
            elif not c.error:
                msg += "<br>No data was recorded (the scanner never started)."
            if c.error:
                msg += f"<br>⚠ {c.error}"
            self._banner(msg, theme.DANGER if (c.aborted or c.error) else theme.OK)
        elif self._abort_armed:
            self._banner("Press <b>Esc</b> again or click <b>Click again to abort</b> to stop the run.", theme.DANGER)
        elif c.error:
            self._banner(f"⚠ Hardware: {c.error}", theme.DANGER)
        elif par.hardware.sync_mode == "keyboard" and app_state != Qt.ApplicationActive:
            self._banner("⚠ ParadigmShift is not the active application – SyncBox triggers and grip presses "
                         "arrive as key presses and will be lost. Click this window.", theme.DANGER)
        elif (ph == "running" and tr > 0 and last_trig is not None and t < dur
              and last_trig > float(c.settings.get("trigger_warning_trs", 2.5)) * tr):
            self._banner(f"⚠ No scanner trigger for {last_trig:.1f} s – has the scanner stopped?", theme.ACCENT)
        elif ph == "waiting":
            hw = par.hardware
            src = (f"key “{hw.trigger_key}” (USB keyboard mode)" if hw.sync_mode == "keyboard"
                   else f"serial port {hw.serial_port}")
            self._banner(f"Waiting for the first trigger from the SyncBox via {src}. "
                         f"{par.dummy_scans} dummy scan(s) will be skipped, the paradigm starts with trigger "
                         f"{par.dummy_scans + 1}.", theme.ACCENT)
        elif ph == "dummy":
            left = par.dummy_scans + 1 - len(st.triggers)
            self._banner(f"Scanner is running – dummy scan {st.dummies_seen} of {par.dummy_scans} received. "
                         f"The paradigm starts with trigger {par.dummy_scans + 1} "
                         f"(in about {left * tr:.0f} s). The participant still sees the waiting screen.",
                         theme.ACCENT)
        else:
            self._banner("", theme.OK)

        self._update_summaries(ph, t, dur)

        # performance + feed (only when something changed)
        rows_done = sum(1 for i in st.events if (b := par.block(i)) and b.kind in SCORED_KINDS and t >= b.end)
        if st.version != self._shown_version or rows_done != self._shown_rows or ph == "done":
            self._shown_version, self._shown_rows = st.version, rows_done
            self._refresh_perf(None if ph == "done" else t)
            self._refresh_feed(None if ph == "done" else t)

    def _update_summaries(self, ph, t, dur):
        """One-line versions of each section, shown while it is folded."""
        import re as _re
        c, par, st = self.c, self.c.p, self.c.state
        sec = self.sections
        shown = min(t, dur)
        pill = self.pill.text()
        sec["status"].set_summary(f"{pill}  ·  {fmt_time(shown)} / {fmt_time(dur)}  ·  "
                                  f"volume {self.card_vol.value.text()}  ·  {len(st.triggers)} triggers")
        sec["progress"].set_summary(f"{shown / dur:.0%} done" if dur else "")
        b = c.current if ph == "running" else None
        sec["mirror"].set_summary(f"showing: {b.type.title} – {b.display_label}" if b else
                                  ("waiting screen" if ph in ("waiting", "dummy") else "end screen"))
        sec["nownext"].set_summary(_re.sub("<[^>]+>", "", self.now_lbl.text()))
        n = self.table.rowCount()
        last = self.table.item(0, 5).text() if n and self.table.item(0, 5) else ""
        sec["answers"].set_summary(f"{n} answers" + (f"  ·  latest: {last}" if last else ""))

    def _refresh_perf(self, t):
        while self.perf_body.count():
            w = self.perf_body.takeAt(0).widget()
            if w:
                w.deleteLater()
        lines = live_stats(self.c.state, t, int(self.c.settings.get("watchdog_trials", 5)))
        self.sections["perf"].set_summary(
            "  ·  ".join(f"{title}: {text.splitlines()[0]}" for title, text, _ in lines[:2]) or "nothing scored yet")
        if not lines:
            empty = QLabel("Nothing to score yet – answers appear here as soon as trials end.")
            empty.setObjectName("Hint")
            self.perf_body.addWidget(empty)
        for title, text, tone in lines:
            lab = QLabel(f"<span style='color:{TONE[tone]}; font-weight:600'>● {title}</span><br>"
                         + text.replace("\n", "<br>"))
            lab.setWordWrap(True)
            lab.setStyleSheet("font-size: 13px; padding: 2px 0;")
            self.perf_body.addWidget(lab)

    def _refresh_feed(self, t):
        par, st = self.c.p, self.c.state
        rows = []
        for bid, ev in st.events.items():
            b = par.block(bid)
            if b is None or b.kind not in SCORED_KINDS:
                continue
            res, tone = live_result(b, ev, t)
            rows.append((ev["onset"], b, ev, res, tone))
        rows.sort(key=lambda r: -r[0])
        rows = rows[:300]
        self.table.setRowCount(len(rows))
        for i, (onset, b, ev, res, tone) in enumerate(rows):
            ans = ev["rating"] if b.kind == "rating" and ev["rating"] != "n/a" else ev["response"]
            rt = ev["response_time"]
            vals = [fmt_time(onset), f"{b.type.title}: {b.display_label}", b.condition or "",
                    "" if ans == "n/a" else str(ans), f"{rt:.3f}" if isinstance(rt, (int, float)) else "", res]
            for col, v in enumerate(vals):
                item = QTableWidgetItem(v)
                if col == 5:
                    item.setForeground(QColor(TONE[tone]))
                self.table.setItem(i, col, item)


def open_run(paradigm, settings, parent_window=None) -> tuple[RunController, PresentationWindow, ControlRoomWindow]:
    """Create controller + both windows and start waiting for the scanner."""
    c = RunController(paradigm, settings)
    pw = PresentationWindow(c)
    cr = ControlRoomWindow(c, pw)
    pw.show_on_screen()
    # control room on the operator's screen (the editor's screen), never on top of the participant's
    if parent_window is not None and parent_window.screen() is not None:
        geo = parent_window.screen().availableGeometry()
        cr.move(geo.center() - cr.rect().center())
    cr.show()
    cr.raise_()
    cr.activateWindow()
    c.start()
    return c, pw, cr
