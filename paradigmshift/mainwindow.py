"""Main editor window: toolbar, library | participant view | inspector, timeline."""
from __future__ import annotations

import csv
import os

from PySide6.QtCore import QSettings, QSize, QStandardPaths, Qt, QTimer
from PySide6.QtGui import QAction, QKeySequence
from PySide6.QtWidgets import (QFileDialog, QFrame, QHBoxLayout, QLabel, QMainWindow, QMessageBox,
                               QPushButton, QSizePolicy, QSplitter, QToolBar, QToolButton, QVBoxLayout,
                               QWidget)

from . import theme
from .dialogs import HardwareDialog, QuickDesignDialog, RunDialog, TemplateDialog
from .document import Document
from .hardware import now
from .model import FILE_SUFFIX, Paradigm
from .panels import InspectorPanel, LibraryPanel, MonitorView, panel
from .runner import PresentationWindow
from .timeline import TimelineView, fmt_time

FILE_FILTER = f"fMRI paradigm (*{FILE_SUFFIX});;JSON (*.json)"


class MainWindow(QMainWindow):
    def __init__(self, doc: Document | None = None):
        super().__init__()
        self.doc = doc or Document()
        self.setWindowIcon(theme.app_icon())
        self.resize(1560, 940)
        self.settings = QSettings("ParadigmShift", "ParadigmShift")
        self._playing = False
        self._play_t0 = 0.0
        self._play_from = 0.0
        self._play_timer = QTimer(self)
        self._play_timer.setInterval(16)
        self._play_timer.timeout.connect(self._play_tick)
        self._runner = None

        self.timeline = TimelineView(self.doc)
        self.library = LibraryPanel()
        self.monitor = MonitorView(self.doc)
        self.inspector = InspectorPanel(self.doc)
        self.library.addKind.connect(lambda k: self.timeline.add_block(k))
        self.timeline.playheadMoved.connect(self._on_playhead)
        self.timeline.openInspector.connect(self.inspector.setFocus)

        self._build_actions()
        self._build_toolbar()
        self._build_layout()
        self.doc.changed.connect(self._refresh)
        self.doc.pathChanged.connect(self._refresh)
        self._refresh()
        self._on_playhead(0.0)

    # ------------------------------------------------------------------ actions
    def _act(self, text, slot, shortcut=None, icon=None, tip=None):
        a = QAction(text, self)
        if icon:
            a.setIcon(theme.glyph_icon(icon))
        if shortcut:
            a.setShortcut(QKeySequence(shortcut))
        a.setToolTip((tip or text) + (f"   ({QKeySequence(shortcut).toString()})" if shortcut else ""))
        a.triggered.connect(slot)
        self.addAction(a)
        return a

    def _build_actions(self):
        self.a_new = self._act("New", self.new_file, QKeySequence.New, "new")
        self.a_open = self._act("Open…", self.open_file, QKeySequence.Open, "open")
        self.a_save = self._act("Save", self.save_file, QKeySequence.Save, "save")
        self.a_save_as = self._act("Save as…", lambda: self.save_file(True), QKeySequence.SaveAs)
        self.a_undo = self._act("Undo", self.doc.undo, QKeySequence.Undo, "undo")
        self.a_redo = self._act("Redo", self.doc.redo, "Ctrl+Shift+Z", "redo")
        self.a_template = self._act("Templates", self.new_from_template, "Ctrl+T", "template",
                                    "Start from a classic task: Hariri, Go/No-Go, emotion regulation")
        self.a_wizard = self._act("Quick design", self.quick_design, "Ctrl+Shift+N", "wand",
                                  "Build a block or event-related design in a few clicks")
        self.a_hw = self._act("Hardware", self.hardware_setup, "Ctrl+Shift+H", "hardware",
                              "Set up and test the NNL SyncBox and ResponseGrips")
        self.a_export = self._act("Export onsets…", self.export_onsets, "Ctrl+E", "export",
                                  "Export planned onsets (BIDS events.tsv + FSL 3-column files)")
        self.a_play = self._act("Play", self.toggle_play, "Space", "play", "Preview the run in the participant view")
        self.a_home = self._act("Go to start", lambda: self._seek(0), "Home", "start")
        self.a_zin = self._act("Zoom in", self.timeline.zoom_in, "Ctrl+=", "zoomin")
        self.a_zout = self._act("Zoom out", self.timeline.zoom_out, "Ctrl+-", "zoomout")
        self.a_fit = self._act("Fit whole run", self.timeline.zoom_fit, "Ctrl+0", "fit")
        self.a_snap = self._act("Snap", self._toggle_snap, "N", "magnet",
                                "Snap to TR grid and block edges (hold Alt while dragging to place freely)")
        self.a_snap.setCheckable(True)
        self.a_snap.setChecked(True)
        self.a_validate = self._act("Check paradigm", self.validate, "Ctrl+K", "check")
        mb = self.menuBar()
        m = mb.addMenu("&File")
        m.addAction(self.a_new)
        m.addAction("New from template…", self.new_from_template)
        for a in (self.a_open, self.a_save, self.a_save_as):
            m.addAction(a)
        ex = m.addMenu("Open example")
        ex_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "examples")
        if os.path.isdir(ex_dir):
            for f in sorted(os.listdir(ex_dir)):
                if f.endswith(FILE_SUFFIX):
                    ex.addAction(f.replace(FILE_SUFFIX, ""), lambda p=os.path.join(ex_dir, f): self.open_file(p))
        m.addSeparator()
        m.addAction(self.a_export)
        m.addSeparator()
        m.addAction("Quit", self.close, QKeySequence.Quit)
        m = mb.addMenu("&Edit")
        m.addAction(self.a_undo)
        m.addAction(self.a_redo)
        m.addSeparator()
        m.addAction("Duplicate", self.timeline.duplicate_selected, "Ctrl+D")
        m.addAction("Repeat selection ×N…", self.timeline.repeat_selected, "Ctrl+R")
        m.addAction("Split at playhead", self.timeline.split_at_playhead, "S")
        m.addAction("Delete", self.timeline.delete_selected)
        m.addAction("Delete and close gap", lambda: self.timeline.delete_selected(True), "Shift+Del")
        m.addAction("Select all", lambda: self.doc.select([b.id for b in self.doc.paradigm.blocks]), "Ctrl+A")
        m = mb.addMenu("&Paradigm")
        m.addAction(self.a_template)
        m.addAction(self.a_wizard)
        m.addAction(self.a_validate)
        m.addAction(self.a_hw)
        m.addSeparator()
        m.addAction("Run…", self.run_paradigm, "F5")
        m = mb.addMenu("&View")
        for a in (self.a_play, self.a_home, self.a_zin, self.a_zout, self.a_fit, self.a_snap):
            m.addAction(a)
        m = mb.addMenu("&Help")
        m.addAction("Keyboard shortcuts", self.show_shortcuts, "F1")

    def _build_toolbar(self):
        tb = QToolBar()
        tb.setMovable(False)
        tb.setIconSize(QSize(20, 20))
        tb.setToolButtonStyle(Qt.ToolButtonTextBesideIcon)
        self.addToolBar(Qt.TopToolBarArea, tb)
        for a in (self.a_new, self.a_open, self.a_save):
            tb.addAction(a)
        tb.addSeparator()
        tb.addAction(self.a_undo)
        tb.addAction(self.a_redo)
        tb.addSeparator()
        tb.addAction(self.a_template)
        tb.addAction(self.a_wizard)
        tb.addAction(self.a_hw)
        tb.addAction(self.a_export)
        spacer = QWidget()
        spacer.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        tb.addWidget(spacer)
        self.title = QLabel()
        self.title.setStyleSheet(f"color: {theme.TEXT_DIM}; padding-right: 14px;")
        tb.addWidget(self.title)
        self.run_btn = QPushButton("  Run in scanner")
        self.run_btn.setObjectName("Run")
        self.run_btn.setIcon(theme.glyph_icon("play", "#0E1A14"))
        self.run_btn.setToolTip("Present the paradigm, wait for the SyncBox trigger and log grip responses (F5)")
        self.run_btn.clicked.connect(self.run_paradigm)
        tb.addWidget(self.run_btn)

    def _transport(self) -> QWidget:
        w = QWidget()
        lay = QHBoxLayout(w)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(6)
        for a in (self.a_home, self.a_play):
            b = QToolButton()
            b.setDefaultAction(a)
            b.setIconSize(QSize(22, 22))
            b.setToolButtonStyle(Qt.ToolButtonIconOnly)
            lay.addWidget(b)
            if a is self.a_play:
                self.play_btn = b
        self.timecode = QLabel("00:00.00")
        self.timecode.setObjectName("Timecode")
        lay.addWidget(self.timecode)
        self.volinfo = QLabel()
        self.volinfo.setObjectName("Hint")
        lay.addWidget(self.volinfo)
        lay.addStretch()
        return w

    def _build_layout(self):
        central = QWidget()
        root = QVBoxLayout(central)
        root.setContentsMargins(8, 0, 8, 4)
        vsplit = QSplitter(Qt.Vertical)
        top = QSplitter(Qt.Horizontal)
        top.addWidget(self.library)
        mon, mlay = panel("Participant view", hint="what the participant sees at the playhead")
        mlay.addWidget(self.monitor, 1)
        mlay.addWidget(self._transport())
        top.addWidget(mon)
        top.addWidget(self.inspector)
        top.setStretchFactor(1, 1)
        top.setSizes([260, 860, 370])
        vsplit.addWidget(top)

        tl, tlay = panel("Timeline")
        bar = QHBoxLayout()
        bar.setSpacing(4)
        self.tl_info = QLabel()
        self.tl_info.setObjectName("Hint")
        bar.addWidget(self.tl_info)
        bar.addStretch()
        hint = QLabel("drag to move · drag edges to trim · Ctrl+wheel zoom · right-click for more")
        hint.setObjectName("Hint")
        bar.addWidget(hint)
        bar.addSpacing(12)
        for a in (self.a_snap, self.a_zout, self.a_zin, self.a_fit):
            b = QToolButton()
            b.setDefaultAction(a)
            b.setToolButtonStyle(Qt.ToolButtonIconOnly if a is not self.a_snap else Qt.ToolButtonTextBesideIcon)
            bar.addWidget(b)
        # Panel header row already has the title; put the bar under it
        tlay.addLayout(bar)
        tlay.addWidget(self.timeline, 1)
        vsplit.addWidget(tl)
        vsplit.setStretchFactor(0, 3)
        vsplit.setStretchFactor(1, 2)
        vsplit.setSizes([520, 330])
        root.addWidget(vsplit)
        self.setCentralWidget(central)
        self.status = QLabel()
        self.statusBar().addPermanentWidget(self.status)

    # ------------------------------------------------------------------ state
    def _refresh(self):
        par = self.doc.paradigm
        name = os.path.basename(self.doc.path) if self.doc.path else "unsaved"
        self.setWindowTitle(f"{par.name} — ParadigmShift" + (" •" if self.doc.dirty else ""))
        self.title.setText(f"{par.name}  ·  {name}{' •' if self.doc.dirty else ''}")
        self.a_undo.setEnabled(self.doc.can_undo())
        self.a_redo.setEnabled(self.doc.can_redo())
        conds = par.conditions()
        self.tl_info.setText(f"{fmt_time(par.duration)}  ·  {par.volumes} volumes @ TR {par.tr:g}s  ·  "
                             f"{len(par.blocks)} blocks" + (f"  ·  {len(conds)} conditions" if conds else ""))
        problems = par.validate()
        self.status.setText(("⚠ " + f"{len(problems)} issue(s) – Ctrl+K to check") if problems
                            else "✓ ready to run")
        self.status.setStyleSheet(f"color: {theme.DANGER if problems else theme.OK}; padding-right: 8px;")
        self._on_playhead(self.timeline.playhead)

    def _on_playhead(self, t):
        self.monitor.set_time(t)
        par = self.doc.paradigm
        self.timecode.setText(fmt_time(t, 2))
        if par.tr > 0:
            self.volinfo.setText(f"volume {int(t // par.tr) + 1} / {par.volumes}   ·   end {fmt_time(par.duration)}")

    def _toggle_snap(self):
        self.timeline.snap = self.a_snap.isChecked()

    # ------------------------------------------------------------------ playback
    def toggle_play(self):
        if self._playing:
            self._playing = False
            self._play_timer.stop()
            self.a_play.setIcon(theme.glyph_icon("play"))
            self.a_play.setText("Play")
        else:
            if self.timeline.playhead >= self.doc.paradigm.duration:
                self.timeline.set_playhead(0)
            self._playing = True
            self._play_t0 = now()
            self._play_from = self.timeline.playhead
            self._play_timer.start()
            self.a_play.setIcon(theme.glyph_icon("pause"))
            self.a_play.setText("Pause")

    def _play_tick(self):
        t = self._play_from + now() - self._play_t0
        if t >= self.doc.paradigm.duration:
            t = self.doc.paradigm.duration
            self.toggle_play()
        self.timeline.set_playhead(t, follow=True)

    def _seek(self, t):
        self._play_from, self._play_t0 = t, now()
        self.timeline.set_playhead(t, follow=True)

    # ------------------------------------------------------------------ files
    def _confirm_discard(self) -> bool:
        if not self.doc.dirty:
            return True
        r = QMessageBox.question(self, "Unsaved changes", "Save changes to the current paradigm first?",
                                 QMessageBox.Save | QMessageBox.Discard | QMessageBox.Cancel)
        if r == QMessageBox.Save:
            return self.save_file()
        return r == QMessageBox.Discard

    def new_file(self):
        if self._confirm_discard():
            hw = self.doc.paradigm.hardware
            self.doc.reset(Paradigm(hardware=hw))
            self.timeline.set_playhead(0)

    def open_file(self, path: str | None = None):
        if not self._confirm_discard():
            return
        if not path:
            path, _ = QFileDialog.getOpenFileName(self, "Open paradigm", self.settings.value("dir", ""), FILE_FILTER)
        if path:
            try:
                self.doc.load(path)
            except Exception as exc:  # noqa: BLE001
                QMessageBox.critical(self, "Could not open", str(exc))
                return
            self.settings.setValue("dir", os.path.dirname(path))
            imgs = sorted({f for b in self.doc.paradigm.blocks for f in b.image_files()})
            self.library.media.clear()
            self.library.media.add_files(imgs)
            self.timeline.set_playhead(0)
            QTimer.singleShot(0, self.timeline.zoom_fit)

    def save_file(self, save_as: bool = False) -> bool:
        path = self.doc.path
        if save_as or not path:
            suggested = os.path.join(self.settings.value("dir", ""), self.doc.paradigm.name + FILE_SUFFIX)
            path, _ = QFileDialog.getSaveFileName(self, "Save paradigm", suggested, FILE_FILTER)
            if not path:
                return False
            if not path.endswith(".json"):
                path += FILE_SUFFIX
        self.doc.save(path)
        self.settings.setValue("dir", os.path.dirname(path))
        self.statusBar().showMessage(f"Saved {path}", 3000)
        self._refresh()
        return True

    def closeEvent(self, e):
        if self._confirm_discard():
            e.accept()
        else:
            e.ignore()

    # ------------------------------------------------------------------ tools
    def new_from_template(self, key: str = "hariri"):
        if not self._confirm_discard():
            return
        stim_dir = os.path.join(QStandardPaths.writableLocation(QStandardPaths.AppDataLocation),
                                "placeholder-stimuli")
        hw = self.doc.paradigm.hardware
        dlg = TemplateDialog(hw, stim_dir, self, start=key if isinstance(key, str) else "hariri")
        if not dlg.exec() or dlg.paradigm is None:
            return
        par = dlg.paradigm
        par.hardware = hw
        for b in par.blocks:   # left/right answers follow the cursor buttons from the hardware setup
            if b.kind == "match":
                b.params["left_button"], b.params["right_button"] = hw.move_left, hw.move_right
        self.doc.reset(par)
        self.doc.dirty = True
        files = sorted({f for b in par.blocks for f in b.image_files()})
        self.library.media.clear()
        self.library.media.add_files(files)
        self.timeline.set_playhead(0)
        self._refresh()
        QTimer.singleShot(0, self.timeline.zoom_fit)

    def quick_design(self):
        dlg = QuickDesignDialog(self.doc.paradigm, self.library.media.files(), self)
        if dlg.exec():
            blocks = dlg.blocks()
            if not blocks:
                return
            replace = dlg.replace.isChecked()

            def fn(par):
                if replace:
                    par.blocks = []
                offset = 0.0 if replace else par.duration
                for b in blocks:
                    b.start = round(b.start + offset, 4)
                    par.blocks.append(b)
            self.doc.edit(fn)
            self.library.media.add_files([b.params["path"] for b in blocks if b.params.get("path")])
            self.doc.select([])
            QTimer.singleShot(0, self.timeline.zoom_fit)

    def hardware_setup(self):
        dlg = HardwareDialog(self.doc.paradigm.hardware, self.doc.paradigm.tr, self)
        if dlg.exec():
            self.doc.paradigm.hardware = dlg.hw
            self.doc.dirty = True
            self.doc.changed.emit()
            self.inspector.rebuild()

    def validate(self):
        problems = self.doc.paradigm.validate()
        par = self.doc.paradigm
        off_grid = [b for b in par.blocks if par.tr > 0 and b.kind != "response"
                    and abs(b.start / par.tr - round(b.start / par.tr)) > 1e-6]
        msg = "\n".join("• " + p for p in problems) if problems else "✓ No problems found."
        if off_grid:
            msg += (f"\n\nℹ {len(off_grid)} block(s) start between volumes. That is fine for event-related "
                    "designs (jitter), but unusual for block designs.")
        QMessageBox.information(self, "Check paradigm", msg)

    def export_onsets(self):
        par = self.doc.paradigm
        if not par.blocks:
            return
        d = QFileDialog.getExistingDirectory(self, "Export onsets to folder", self.settings.value("dir", ""))
        if not d:
            return
        files = export_onsets(par, d)
        QMessageBox.information(self, "Exported", "Written:\n" + "\n".join(os.path.basename(f) for f in files))

    def run_paradigm(self):
        par = self.doc.paradigm
        if not par.blocks:
            QMessageBox.information(self, "Nothing to run", "Add some blocks to the timeline first.")
            return
        problems = par.validate()
        if problems and QMessageBox.warning(
                self, "Paradigm has issues", "\n".join(problems[:10]) + "\n\nRun anyway?",
                QMessageBox.Yes | QMessageBox.No) != QMessageBox.Yes:
            return
        out = self.settings.value("out_dir", os.path.join(os.path.expanduser("~"), "ParadigmShift-data"))
        dlg = RunDialog(par, out, self)
        if not dlg.exec():
            return
        s = dlg.settings()
        self.settings.setValue("out_dir", s["out_dir"])
        if self._playing:
            self.toggle_play()
        self._runner = PresentationWindow(par.copy(), s)
        self._runner.finished.connect(
            lambda files: self.statusBar().showMessage("Run finished – saved " + ", ".join(files), 15000))
        self._runner.start()

    def show_shortcuts(self):
        QMessageBox.information(self, "Keyboard shortcuts", (
            "Space\tplay / pause preview\nHome\tgo to start\n"
            "Del\tdelete selected   (Shift+Del closes the gap)\nCtrl+D\tduplicate\nCtrl+R\trepeat ×N\n"
            "S\tsplit at playhead\n← →\tnudge (Shift: by one TR)\nAlt + drag\tplace without snapping\n"
            "Ctrl+wheel\tzoom\nCtrl+0\tfit whole run\nCtrl+Z / Ctrl+Shift+Z\tundo / redo\nF5\trun"))


def export_onsets(par: Paradigm, folder: str) -> list[str]:
    """Planned onsets: a BIDS-style events.tsv plus one FSL 3-column file per condition."""
    os.makedirs(folder, exist_ok=True)
    stem = "".join(c for c in par.name if c.isalnum()) or "paradigm"
    files = []
    path = os.path.join(folder, f"{stem}_planned_events.tsv")
    with open(path, "w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh, delimiter="\t")
        w.writerow(["onset", "duration", "trial_type", "block", "stim_file"])
        for b in sorted(par.blocks, key=lambda b: (b.start, b.track)):
            w.writerow([f"{b.start:.3f}", f"{b.duration:.3f}", b.condition or b.kind, b.kind,
                        os.path.basename(b.thumbnail_path) or "n/a"])
    files.append(path)
    for cond in par.conditions():
        path = os.path.join(folder, f"{stem}_{cond}.txt")
        with open(path, "w", encoding="utf-8") as fh:
            for b in sorted((b for b in par.blocks if b.condition == cond and b.track == 0), key=lambda b: b.start):
                fh.write(f"{b.start:.3f}\t{b.duration:.3f}\t1\n")
        files.append(path)
    return files
