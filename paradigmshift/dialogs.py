"""Dialogs: hardware setup (SyncBox + grips), quick design wizard, run setup."""
from __future__ import annotations

import copy
import os

from PySide6.QtCore import QElapsedTimer, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QGuiApplication, QPainter
from PySide6.QtWidgets import (QCheckBox, QComboBox, QDialog, QDialogButtonBox, QFileDialog, QFormLayout,
                               QFrame, QGridLayout, QHBoxLayout, QLabel, QLineEdit, QPushButton,
                               QRadioButton, QSpinBox, QTableWidget, QTableWidgetItem, QTabWidget,
                               QVBoxLayout, QWidget, QHeaderView, QButtonGroup)

from . import theme
from .hardware import InputRouter, list_serial_ports, now, serial_available
from .model import GripButton, Hardware, Paradigm, generate_block_design, generate_event_design
from .panels import dspin, ispin
from .timeline import IMAGE_EXT, fmt_time, is_image


def _hint(text: str) -> QLabel:
    lab = QLabel(text)
    lab.setObjectName("Hint")
    lab.setWordWrap(True)
    return lab


# =============================================================================
# Hardware
# =============================================================================
class Lamp(QWidget):
    """A round indicator that flashes when an input arrives."""

    def __init__(self, color: str = theme.OK, size: int = 18):
        super().__init__()
        self.color = QColor(color)
        self.level = 0.0
        self.setFixedSize(size, size)
        self._t = QTimer(self)
        self._t.timeout.connect(self._fade)

    def flash(self):
        self.level = 1.0
        self._t.start(30)
        self.update()

    def _fade(self):
        self.level = max(0.0, self.level - .06)
        if self.level == 0:
            self._t.stop()
        self.update()

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        off = QColor("#2A2F3A")
        c = QColor(
            int(off.red() + (self.color.red() - off.red()) * self.level),
            int(off.green() + (self.color.green() - off.green()) * self.level),
            int(off.blue() + (self.color.blue() - off.blue()) * self.level))
        p.setBrush(c)
        p.setPen(QColor(theme.BORDER))
        p.drawEllipse(self.rect().adjusted(1, 1, -1, -1))
        p.end()


class KeyCaptureButton(QPushButton):
    """Click, then press a key / grip button to assign it."""
    captured = Signal(str)

    def __init__(self, key: str):
        super().__init__()
        self.key = key
        self.setCheckable(True)
        self.setMinimumWidth(110)
        self.toggled.connect(self._refresh)
        self._refresh()

    def _refresh(self, *_):
        if self.isChecked():
            self.setText("press now…")
            self.setStyleSheet(f"border-color: {theme.ACCENT}; color: {theme.ACCENT};")
        else:
            self.setText(f"key  “{self.key}”" if self.key else "not set")
            self.setStyleSheet("")

    def take(self, text: str) -> bool:
        if not self.isChecked():
            return False
        self.key = text
        self.setChecked(False)
        self.captured.emit(text)
        return True


class HardwareDialog(QDialog):
    """Set up and live-test the NNL SyncBox and ResponseGrips."""

    def __init__(self, hw: Hardware, tr: float, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Scanner hardware – NNL SyncBox & ResponseGrips")
        self.hw = copy.deepcopy(hw)
        self.tr = tr
        self.resize(720, 560)
        lay = QVBoxLayout(self)
        lay.setSpacing(12)
        head = QLabel("Scanner hardware")
        head.setObjectName("Big")
        lay.addWidget(head)
        lay.addWidget(_hint("Everything here can be tested live: press keys or grip buttons and watch the lamps. "
                            "Click a key field, then press the button you want to assign."))
        tabs = QTabWidget()
        tabs.addTab(self._sync_tab(), "SyncBox  (scanner trigger)")
        tabs.addTab(self._grips_tab(), "ResponseGrips  (buttons)")
        lay.addWidget(tabs, 1)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.button(QDialogButtonBox.Ok).setObjectName("Primary")
        bb.button(QDialogButtonBox.Ok).setText("Save hardware setup")
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        lay.addWidget(bb)

        self._router = None
        self._triggers: list[float] = []
        self._restart_router()

    # -- SyncBox ---------------------------------------------------------------
    def _sync_tab(self):
        w = QWidget()
        lay = QVBoxLayout(w)
        lay.setContentsMargins(16, 16, 16, 16)
        lay.setSpacing(12)
        lay.addWidget(_hint("The NNL SyncBox converts the scanner's optical/TTL pulse into a key press (USB) "
                            "or a character on a serial port. Check the SyncBox's own menu for which "
                            "trigger character it sends."))
        modes = QHBoxLayout()
        self.rb_kb = QRadioButton("USB keyboard mode  (recommended)")
        self.rb_ser = QRadioButton("Serial / COM port")
        grp = QButtonGroup(w)
        grp.addButton(self.rb_kb)
        grp.addButton(self.rb_ser)
        (self.rb_kb if self.hw.sync_mode == "keyboard" else self.rb_ser).setChecked(True)
        modes.addWidget(self.rb_kb)
        modes.addWidget(self.rb_ser)
        modes.addStretch()
        lay.addLayout(modes)

        form = QFormLayout()
        form.setLabelAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.trig_btn = KeyCaptureButton(self.hw.trigger_key)
        self.trig_btn.captured.connect(lambda k: setattr(self.hw, "trigger_key", k))
        form.addRow("Trigger key / character", self.trig_btn)
        self.port = QComboBox()
        self.port.setEditable(True)
        for dev, desc in list_serial_ports():
            self.port.addItem(desc, dev)
        self.port.setCurrentText(self.hw.serial_port)
        self.port.lineEdit().setPlaceholderText("e.g. COM3 or /dev/ttyUSB0")
        form.addRow("Serial port", self.port)
        self.baud = QComboBox()
        for b in (9600, 19200, 38400, 57600, 115200):
            self.baud.addItem(str(b), b)
        self.baud.setCurrentText(str(self.hw.baud_rate))
        form.addRow("Baud rate", self.baud)
        self.ser_note = _hint("" if serial_available() else "⚠ Serial mode needs the 'pyserial' package (pip install pyserial).")
        form.addRow("", self.ser_note)
        lay.addLayout(form)

        def mode_changed():
            ser = self.rb_ser.isChecked()
            self.port.setEnabled(ser)
            self.baud.setEnabled(ser)
            self.hw.sync_mode = "serial" if ser else "keyboard"
            self._restart_router()
        self.rb_kb.toggled.connect(mode_changed)
        self.port.currentTextChanged.connect(lambda _t: self._port_changed())
        self.baud.currentIndexChanged.connect(lambda _i: self._port_changed())
        self.port.setEnabled(self.rb_ser.isChecked())
        self.baud.setEnabled(self.rb_ser.isChecked())

        test = QFrame()
        test.setObjectName("Panel")
        tl = QGridLayout(test)
        tl.setContentsMargins(14, 12, 14, 12)
        title = QLabel("LIVE TRIGGER MONITOR")
        title.setObjectName("PanelTitle")
        tl.addWidget(title, 0, 0, 1, 3)
        self.trig_lamp = Lamp(theme.ACCENT, 34)
        tl.addWidget(self.trig_lamp, 1, 0, 2, 1)
        self.trig_count = QLabel("0 pulses")
        self.trig_count.setObjectName("Big")
        tl.addWidget(self.trig_count, 1, 1)
        self.trig_tr = QLabel(f"measured TR: –   (paradigm TR {self.tr:g}s)")
        self.trig_tr.setObjectName("Hint")
        tl.addWidget(self.trig_tr, 2, 1)
        self.sim_btn = QPushButton("Simulate scanner")
        self.sim_btn.setCheckable(True)
        self.sim_btn.setToolTip("Generate trigger pulses at the paradigm TR – handy without a scanner.")
        self.sim_btn.toggled.connect(self._toggle_sim)
        tl.addWidget(self.sim_btn, 1, 2)
        reset = QPushButton("Reset")
        reset.clicked.connect(self._reset_count)
        tl.addWidget(reset, 2, 2)
        tl.setColumnStretch(1, 1)
        lay.addWidget(test)
        lay.addStretch()
        return w

    def _port_changed(self):
        self.hw.serial_port = self.port.currentData() or self.port.currentText().split(" ")[0]
        self.hw.baud_rate = int(self.baud.currentData() or 57600)
        if self.hw.sync_mode == "serial":
            self._restart_router()

    def _toggle_sim(self, on):
        if on:
            self._sim = QTimer(self)
            self._sim.timeout.connect(lambda: self._on_trigger(now()))
            self._sim.start(int(self.tr * 1000))
            self._on_trigger(now())
        elif getattr(self, "_sim", None):
            self._sim.stop()

    def _reset_count(self):
        self._triggers.clear()
        self.trig_count.setText("0 pulses")
        self.trig_tr.setText(f"measured TR: –   (paradigm TR {self.tr:g}s)")

    def _on_trigger(self, t):
        self._triggers.append(t)
        self.trig_lamp.flash()
        n = len(self._triggers)
        self.trig_count.setText(f"{n} pulse{'s' if n != 1 else ''}")
        if n > 1:
            d = [b - a for a, b in zip(self._triggers[-6:], self._triggers[-5:])]
            m = sum(d) / len(d)
            ok = abs(m - self.tr) < 0.05
            self.trig_tr.setText(f"measured TR: {m:.3f}s   (paradigm TR {self.tr:g}s)  " + ("✓" if ok else "⚠ differs"))

    # -- grips -----------------------------------------------------------------
    def _grips_tab(self):
        w = QWidget()
        lay = QVBoxLayout(w)
        lay.setContentsMargins(16, 16, 16, 16)
        lay.setSpacing(10)
        lay.addWidget(_hint("Each grip button arrives as a key (USB mode) or character (serial mode). "
                            "Name the buttons the way you instruct your participants."))
        self.grid = QGridLayout()
        self.grid.setHorizontalSpacing(10)
        lay.addLayout(self.grid)
        self._rows = []
        for b in self.hw.buttons:
            self._add_row(b)
        add = QPushButton("＋ Add button")
        add.clicked.connect(lambda: self._add_row(GripButton(f"Button {len(self._rows) + 1}", ""), new=True))
        lay.addWidget(add, 0, Qt.AlignLeft)

        box = QFrame()
        box.setObjectName("Panel")
        bl = QFormLayout(box)
        bl.setContentsMargins(14, 12, 14, 12)
        t = QLabel("RATING SCALES IN CURSOR MODE")
        t.setObjectName("PanelTitle")
        bl.addRow(t)
        self.cmb = {}
        for attr, label in (("move_left", "Move left ◀"), ("move_right", "Move right ▶"), ("confirm", "Confirm ✓")):
            c = QComboBox()
            c.currentTextChanged.connect(lambda txt, a=attr: setattr(self.hw, a, txt))
            self.cmb[attr] = c
            bl.addRow(label, c)
        self._refresh_combos()
        lay.addWidget(box)
        self.last = _hint("Press a grip button…")
        lay.addWidget(self.last)
        lay.addStretch()
        return w

    def _add_row(self, b: GripButton, new: bool = False):
        r = len(self._rows)
        if new:
            self.hw.buttons.append(b)
        lamp = Lamp(theme.OK, 20)
        name = QLineEdit(b.name)
        key = KeyCaptureButton(b.key)
        rm = QPushButton("✕")
        rm.setFixedWidth(34)
        rm.setToolTip("Remove button")
        name.textEdited.connect(lambda txt, b=b: (setattr(b, "name", txt), self._refresh_combos()))
        key.captured.connect(lambda k, b=b: setattr(b, "key", k))
        row = (lamp, name, key, rm, b)
        rm.clicked.connect(lambda _=False, row=row: self._remove_row(row))
        for c, wdg in enumerate(row[:4]):
            self.grid.addWidget(wdg, r, c)
        self._rows.append(row)
        if new:
            self._refresh_combos()

    def _remove_row(self, row):
        for wdg in row[:4]:
            wdg.hide()
            self.grid.removeWidget(wdg)
        self._rows.remove(row)
        self.hw.buttons.remove(row[4])
        self._refresh_combos()

    def _refresh_combos(self):
        if not hasattr(self, "cmb"):
            return
        names = self.hw.button_names()
        for attr, c in self.cmb.items():
            cur = getattr(self.hw, attr)
            c.blockSignals(True)
            c.clear()
            c.addItems(names)
            c.setCurrentText(cur if cur in names else (names[0] if names else ""))
            c.blockSignals(False)
            setattr(self.hw, attr, c.currentText())

    def _on_button(self, name, _t):
        for lamp, _n, _k, _r, b in self._rows:
            if b.name == name:
                lamp.flash()
        self.last.setText(f"Last button: {name}")

    # -- input -----------------------------------------------------------------
    def _restart_router(self):
        if self._router:
            self._router.stop()
        self._router = InputRouter(self.hw, parent=self)
        self._router.trigger.connect(self._on_trigger)
        self._router.button.connect(self._on_button)
        self._router.other.connect(lambda k, _t: self.last.setText(f"Key “{k}” is not assigned to a button")
                                   if hasattr(self, "last") else None)
        self._router.error.connect(lambda m: self.ser_note.setText("⚠ " + m))
        self._router.start()

    def keyPressEvent(self, e):
        text = e.text()
        if e.key() in (Qt.Key_Escape, Qt.Key_Return, Qt.Key_Enter) or not text or not text.isprintable():
            return super().keyPressEvent(e)
        captors = [self.trig_btn] + [r[2] for r in self._rows]
        for c in captors:
            if c.take(text):
                return
        if self.hw.sync_mode == "keyboard" or self.hw.button_for_key(text):
            self._router.feed(text, now())

    def done(self, r):
        if self._router:
            self._router.stop()
        if getattr(self, "_sim", None):
            self._sim.stop()
        super().done(r)


# =============================================================================
# Quick design wizard
# =============================================================================
def _pick_images(parent) -> list[str]:
    files, _ = QFileDialog.getOpenFileNames(parent, "Choose pictures", "",
                                            "Images (" + " ".join("*" + e for e in IMAGE_EXT) + ")")
    return files


class QuickDesignDialog(QDialog):
    """Generate a complete block or event-related design in a few clicks."""

    def __init__(self, paradigm: Paradigm, media: list[str], parent=None):
        super().__init__(parent)
        self.setWindowTitle("Quick design")
        self.paradigm = paradigm
        self.media = media
        self.resize(760, 600)
        lay = QVBoxLayout(self)
        t = QLabel("✦ Quick design")
        t.setObjectName("Big")
        lay.addWidget(t)
        lay.addWidget(_hint("Pick a template, fill in a few numbers, and the whole timeline is built for you. "
                            "You can fine-tune every block afterwards."))
        self.tabs = QTabWidget()
        self.tabs.addTab(self._block_tab(), "Block design  (ON / OFF)")
        self.tabs.addTab(self._event_tab(), "Event-related  (single pictures)")
        self.tabs.currentChanged.connect(self._update)
        lay.addWidget(self.tabs, 1)
        self.summary = QLabel()
        self.summary.setStyleSheet(f"color: {theme.ACCENT}; font-weight: 600;")
        lay.addWidget(self.summary)
        bottom = QHBoxLayout()
        self.replace = QCheckBox("Replace the current timeline")
        self.replace.setChecked(not paradigm.blocks)
        bottom.addWidget(self.replace)
        bottom.addStretch()
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.button(QDialogButtonBox.Ok).setText("Build timeline")
        bb.button(QDialogButtonBox.Ok).setObjectName("Primary")
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        bottom.addWidget(bb)
        lay.addLayout(bottom)
        self._update()

    # -- block design ------------------------------------------------------------
    def _block_tab(self):
        w = QWidget()
        lay = QVBoxLayout(w)
        lay.setContentsMargins(14, 14, 14, 14)
        self.cond_table = QTableWidget(0, 3)
        self.cond_table.setHorizontalHeaderLabels(["Condition", "Shows", "Pictures"])
        self.cond_table.horizontalHeader().setSectionResizeMode(0, QHeaderView.Stretch)
        self.cond_table.horizontalHeader().setSectionResizeMode(1, QHeaderView.ResizeToContents)
        self.cond_table.horizontalHeader().setSectionResizeMode(2, QHeaderView.Stretch)
        self.cond_table.verticalHeader().setVisible(False)
        self.cond_table.setMinimumHeight(150)
        self._conds: list[dict] = []
        lay.addWidget(self.cond_table)
        row = QHBoxLayout()
        b1 = QPushButton("＋ Condition with pictures…")
        b1.clicked.connect(self._add_picture_condition)
        b2 = QPushButton("＋ Text / task condition")
        b2.clicked.connect(lambda: self._add_condition({"name": f"task{len(self._conds) + 1}", "kind": "text"}))
        b3 = QPushButton("Remove")
        b3.clicked.connect(self._remove_condition)
        row.addWidget(b1)
        row.addWidget(b2)
        row.addWidget(b3)
        row.addStretch()
        lay.addLayout(row)
        tr = self.paradigm.tr or 1
        form = QFormLayout()
        form.setLabelAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.bd_block = dspin(tr * 10, 0.5, 600, tr, 2)
        self.bd_rest = dspin(tr * 10, 0, 600, tr, 2)
        self.bd_rest.setToolTip("Fixation between blocks. 0 = no rest.")
        self.bd_reps = ispin(4, 1, 100)
        self.bd_reps.setSuffix(" cycles")
        self.bd_order = QComboBox()
        self.bd_order.addItem("Alternating  A B A B …", "alternate")
        self.bd_order.addItem("Randomised per cycle", "random")
        self.bd_lead = dspin(tr * 5, 0, 600, tr, 2)
        self.bd_lead.setToolTip("Baseline fixation before the first block")
        form.addRow("Block length", self.bd_block)
        form.addRow("Rest between blocks", self.bd_rest)
        form.addRow("Repetitions", self.bd_reps)
        form.addRow("Order", self.bd_order)
        form.addRow("Baseline at start", self.bd_lead)
        for wdg in (self.bd_block, self.bd_rest, self.bd_lead):
            wdg.valueChanged.connect(self._update)
        self.bd_reps.valueChanged.connect(self._update)
        lay.addLayout(form)
        if self.media:
            self._add_condition({"name": "pictures", "kind": "image", "images": list(self.media)})
        return w

    def _add_picture_condition(self):
        files = _pick_images(self)
        if files:
            self._add_condition({"name": f"cond{len(self._conds) + 1}", "kind": "image", "images": files})

    def _add_condition(self, cond):
        self._conds.append(cond)
        r = self.cond_table.rowCount()
        self.cond_table.insertRow(r)
        name = QTableWidgetItem(cond["name"])
        self.cond_table.setItem(r, 0, name)
        kind = QTableWidgetItem("Pictures" if cond.get("images") else "Instruction text")
        kind.setFlags(Qt.ItemIsEnabled)
        self.cond_table.setItem(r, 1, kind)
        n = len(cond.get("images", []))
        pics = QTableWidgetItem(f"{n} pictures" if n else "–")
        pics.setFlags(Qt.ItemIsEnabled)
        self.cond_table.setItem(r, 2, pics)
        self._update()

    def _remove_condition(self):
        r = self.cond_table.currentRow()
        if r >= 0:
            self.cond_table.removeRow(r)
            self._conds.pop(r)
            self._update()

    # -- event related -------------------------------------------------------------
    def _event_tab(self):
        w = QWidget()
        lay = QVBoxLayout(w)
        lay.setContentsMargins(14, 14, 14, 14)
        self.ev_images = list(self.media)
        row = QHBoxLayout()
        self.ev_label = QLabel()
        row.addWidget(self.ev_label, 1)
        pick = QPushButton("Choose pictures…")
        pick.clicked.connect(self._ev_pick)
        row.addWidget(pick)
        lay.addLayout(row)
        form = QFormLayout()
        form.setLabelAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.ev_cond = QLineEdit("picture")
        form.addRow("Condition name", self.ev_cond)
        self.ev_dur = dspin(4, 0.1, 120, 0.5, 2)
        form.addRow("Picture on screen", self.ev_dur)
        jit = QHBoxLayout()
        self.ev_min = dspin(2, 0, 120, 0.5, 2)
        self.ev_max = dspin(6, 0, 120, 0.5, 2)
        jit.addWidget(self.ev_min)
        jit.addWidget(QLabel("to"))
        jit.addWidget(self.ev_max)
        jw = QWidget()
        jit.setContentsMargins(0, 0, 0, 0)
        jw.setLayout(jit)
        form.addRow("Fixation between (jitter)", jw)
        self.ev_rating = QCheckBox("Participant rates each picture with the grips")
        self.ev_rating.setChecked(True)
        form.addRow("", self.ev_rating)
        self.ev_shuffle = QCheckBox("Shuffle picture order")
        self.ev_shuffle.setChecked(True)
        form.addRow("", self.ev_shuffle)
        for wdg in (self.ev_dur, self.ev_min, self.ev_max):
            wdg.valueChanged.connect(self._update)
        lay.addLayout(form)
        lay.addStretch()
        self._ev_refresh()
        return w

    def _ev_pick(self):
        files = _pick_images(self)
        if files:
            self.ev_images = files
            self._ev_refresh()
            self._update()

    def _ev_refresh(self):
        n = len(self.ev_images)
        self.ev_label.setText(f"{n} pictures selected" if n else "No pictures yet – choose some, or import them "
                                                                  "into the Pictures bin first.")

    # -- result ----------------------------------------------------------------
    def blocks(self, seed: int | None = None):
        if self.tabs.currentIndex() == 0:
            conds = []
            for r, c in enumerate(self._conds):
                c = dict(c)
                item = self.cond_table.item(r, 0)
                c["name"] = item.text() if item else c["name"]
                conds.append(c)
            if not conds:
                return []
            blocks = generate_block_design(conds, self.bd_block.value(), self.bd_rest.value(),
                                           self.bd_reps.value(), self.bd_order.currentData(),
                                           lead_in=self.bd_lead.value(), seed=seed)
            for b in blocks:
                if b.kind == "text" and b.condition:
                    b.params["text"] = b.condition.upper()
            return blocks
        if not self.ev_images:
            return []
        lo, hi = sorted((self.ev_min.value(), self.ev_max.value()))
        return generate_event_design(self.ev_images, self.ev_dur.value(), lo, hi, self.ev_cond.text(),
                                     rating=self.ev_rating.isChecked(), shuffle=self.ev_shuffle.isChecked(),
                                     tr=self.paradigm.tr, seed=seed)

    def _update(self, *_):
        if not hasattr(self, "summary"):  # still constructing
            return
        blocks = self.blocks(seed=1)
        if not blocks:
            self.summary.setText("Add at least one condition / picture to build a timeline.")
            return
        dur = max(b.end for b in blocks)
        vols = int(-(-dur // self.paradigm.tr)) if self.paradigm.tr else 0
        n = len([b for b in blocks if b.track == 0])
        self.summary.setText(f"→ {n} blocks  ·  {fmt_time(dur)} min  ·  {vols} volumes at TR {self.paradigm.tr:g}s")


# =============================================================================
# Run setup
# =============================================================================
class RunDialog(QDialog):
    def __init__(self, paradigm: Paradigm, out_dir: str, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Run paradigm")
        self.resize(520, 480)
        lay = QVBoxLayout(self)
        t = QLabel("▶  Run in the scanner")
        t.setObjectName("Big")
        lay.addWidget(t)
        hw = paradigm.hardware
        sync = (f"SyncBox via USB keyboard, trigger key “{hw.trigger_key}”" if hw.sync_mode == "keyboard"
                else f"SyncBox via serial port {hw.serial_port or '(not set!)'}")
        lay.addWidget(_hint(f"{paradigm.name}  ·  {fmt_time(paradigm.duration)}  ·  {paradigm.volumes} volumes "
                            f"(+{paradigm.dummy_scans} dummy)\n{sync}"))
        form = QFormLayout()
        form.setLabelAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.sub = QLineEdit("01")
        self.ses = QLineEdit("")
        self.ses.setPlaceholderText("optional")
        self.run = QSpinBox()
        self.run.setRange(1, 99)
        form.addRow("Participant", self.sub)
        form.addRow("Session", self.ses)
        form.addRow("Run", self.run)
        out = QHBoxLayout()
        self.out = QLineEdit(out_dir)
        browse = QPushButton("Browse…")
        browse.clicked.connect(self._browse)
        out.addWidget(self.out, 1)
        out.addWidget(browse)
        ow = QWidget()
        out.setContentsMargins(0, 0, 0, 0)
        ow.setLayout(out)
        form.addRow("Save logs to", ow)
        self.screen = QComboBox()
        screens = QGuiApplication.screens()
        for i, s in enumerate(screens):
            g = s.geometry()
            self.screen.addItem(f"{i + 1}: {s.name()}  {g.width()}×{g.height()}", i)
        if len(screens) > 1:
            self.screen.setCurrentIndex(1)   # the projector is usually the 2nd screen
        form.addRow("Show on screen", self.screen)
        self.fullscreen = QCheckBox("Full screen")
        self.fullscreen.setChecked(True)
        form.addRow("", self.fullscreen)
        self.simulate = QCheckBox("Simulate scanner (no SyncBox connected – for testing)")
        form.addRow("", self.simulate)
        lay.addLayout(form)
        lay.addWidget(_hint("During the run: Esc twice aborts (data so far is saved). Grip buttons can be "
                            "simulated with their keys."))
        lay.addStretch()
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.button(QDialogButtonBox.Ok).setText("Start – wait for scanner")
        bb.button(QDialogButtonBox.Ok).setObjectName("Run")
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        lay.addWidget(bb)

    def _browse(self):
        d = QFileDialog.getExistingDirectory(self, "Save logs to", self.out.text())
        if d:
            self.out.setText(d)

    def settings(self) -> dict:
        return {
            "sub": self.sub.text().strip() or "01",
            "ses": self.ses.text().strip(),
            "run": self.run.value(),
            "out_dir": self.out.text().strip() or os.getcwd(),
            "screen": self.screen.currentData() or 0,
            "fullscreen": self.fullscreen.isChecked(),
            "simulate": self.simulate.isChecked(),
        }


# =============================================================================
# Paradigm templates (Hariri, Go/No-Go, emotion regulation)
# =============================================================================
_OPT_LABELS = {
    "tr": ("TR", " s"), "face_blocks": ("Face blocks", ""), "trials_per_block": ("Trials per block", ""),
    "trial_dur": ("Trial length", " s"), "instruction_dur": ("Instruction", " s"),
    "faces_dir": ("Faces folder", ""), "shapes_dir": ("Shapes folder", ""), "seed": ("Random seed", ""),
    "n_trials": ("Trials", ""), "nogo_percent": ("No-Go trials", " %"), "stim_time": ("Letter visible", " s"),
    "go_letters": ("Go letters", ""), "nogo_letter": ("No-Go letter", ""),
    "rest_every": ("Rest after every", " trials"), "rest_dur": ("Rest length", " s"),
    "jitter_max": ("Extra jitter up to", " s"), "button": ("Go button", ""),
    "trials_per_condition": ("Trials per condition", ""), "include_increase": ("Add INCREASE condition", ""),
    "cue_dur": ("Cue", " s"), "picture_dur": ("Picture", " s"), "rating_dur": ("Rating", " s"),
    "iti_min": ("Relax from", " s"), "iti_max": ("Relax up to", " s"), "rating_max": ("Rating scale 1 to", ""),
    "neutral_dir": ("Neutral pictures", ""), "negative_dir": ("Negative pictures", ""),
}
_OPT_TIPS = {
    "faces_dir": "Folder with angry and fearful faces (e.g. NimStim or KDEF). Empty = built-in placeholder faces.",
    "shapes_dir": "Folder with shape pictures. Empty = built-in circles and ellipses.",
    "neutral_dir": "Folder with neutral pictures (e.g. IAPS/OASIS). Empty = built-in placeholders.",
    "negative_dir": "Folder with negative pictures (e.g. IAPS/OASIS). Empty = built-in placeholders.",
    "nogo_percent": "Share of No-Go trials. They never follow each other directly.",
    "trial_dur": "Time per trial (stimulus onset to next onset).",
    "seed": "Change to get a different random order. Same seed = same paradigm.",
}


class FolderField(QWidget):
    changed = Signal()

    def __init__(self, path: str):
        super().__init__()
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        self.edit = QLineEdit(path)
        self.edit.setPlaceholderText("built-in placeholders")
        self.edit.editingFinished.connect(self.changed)
        b = QPushButton("Browse…")
        b.clicked.connect(self._browse)
        lay.addWidget(self.edit, 1)
        lay.addWidget(b)

    def _browse(self):
        d = QFileDialog.getExistingDirectory(self, "Choose folder", self.edit.text())
        if d:
            self.edit.setText(d)
            self.changed.emit()

    def value(self) -> str:
        return self.edit.text().strip()


class TemplateCard(QFrame):
    clicked = Signal(str)

    def __init__(self, t):
        super().__init__()
        self.key = t.key
        self.color = t.color
        self.setCursor(Qt.PointingHandCursor)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(10, 10, 10, 10)
        icon = QLabel()
        icon.setPixmap(theme.kind_icon(t.kinds[0], t.color, 36).pixmap(36, 36))
        lay.addWidget(icon)
        col = QVBoxLayout()
        col.setSpacing(0)
        a = QLabel(t.title)
        a.setStyleSheet("font-weight: 600; font-size: 14px; background: transparent;")
        b = QLabel(t.subtitle)
        b.setStyleSheet(f"color: {theme.TEXT_DIM}; font-size: 11px; background: transparent;")
        col.addWidget(a)
        col.addWidget(b)
        lay.addLayout(col, 1)
        self.set_selected(False)

    def set_selected(self, on: bool):
        border = self.color if on else theme.BORDER
        bg = "#2B303B" if on else theme.PANEL_2
        self.setStyleSheet(f"TemplateCard {{ background: {bg}; border: 1px solid {border}; border-radius: 8px; }}")

    def mousePressEvent(self, _e):
        self.clicked.emit(self.key)


class TemplateDialog(QDialog):
    """Start a new paradigm from a validated task design."""

    def __init__(self, hw: Hardware, stim_dir: str, parent=None, start: str = "hariri"):
        super().__init__(parent)
        from .templates import TEMPLATES
        self.setWindowTitle("New paradigm from template")
        self.resize(940, 640)
        self.hw = hw
        self.stim_dir = stim_dir
        self.templates = {t.key: t for t in TEMPLATES}
        self.paradigm: Paradigm | None = None
        root = QVBoxLayout(self)
        t = QLabel("Paradigm templates")
        t.setObjectName("Big")
        root.addWidget(t)
        root.addWidget(_hint("Pick a classic task, adjust the numbers, and get a complete, editable timeline. "
                             "Without your own stimulus folders, placeholder pictures are generated – replace "
                             "them with a validated set for real studies."))
        body = QHBoxLayout()
        side = QVBoxLayout()
        self.cards = {}
        for tpl in TEMPLATES:
            c = TemplateCard(tpl)
            c.clicked.connect(self.select)
            side.addWidget(c)
            self.cards[tpl.key] = c
        side.addStretch()
        sw = QWidget()
        sw.setLayout(side)
        sw.setFixedWidth(290)
        body.addWidget(sw)
        right = QFrame()
        right.setObjectName("Panel")
        self.rlay = QVBoxLayout(right)
        self.rlay.setContentsMargins(16, 14, 16, 14)
        self.title = QLabel()
        self.title.setObjectName("Big")
        self.desc = _hint("")
        self.desc.setStyleSheet(f"color: {theme.TEXT}; font-size: 13px;")
        self.ref = _hint("")
        self.rlay.addWidget(self.title)
        self.rlay.addWidget(self.desc)
        self.rlay.addWidget(self.ref)
        self.form_host = QVBoxLayout()
        self.rlay.addLayout(self.form_host, 1)
        body.addWidget(right, 1)
        root.addLayout(body, 1)
        self.summary = QLabel()
        self.summary.setStyleSheet(f"color: {theme.ACCENT}; font-weight: 600;")
        self.summary.setWordWrap(True)
        bottom = QHBoxLayout()
        bottom.addWidget(self.summary, 1)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        self.ok = bb.button(QDialogButtonBox.Ok)
        self.ok.setText("Create paradigm")
        self.ok.setObjectName("Primary")
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        bottom.addWidget(bb)
        root.addLayout(bottom)
        self.key = None
        self.select(start)

    def select(self, key: str):
        from dataclasses import fields
        self.key = key
        tpl = self.templates[key]
        for k, c in self.cards.items():
            c.set_selected(k == key)
        self.title.setText(tpl.title)
        self.desc.setText(tpl.description)
        self.ref.setText("Reference: " + tpl.reference)
        while self.form_host.count():
            w = self.form_host.takeAt(0).widget()
            if w:
                w.deleteLater()
        holder = QWidget()
        form = QFormLayout(holder)
        form.setLabelAlignment(Qt.AlignRight | Qt.AlignVCenter)
        form.setContentsMargins(0, 8, 0, 0)
        defaults = tpl.options()
        self.inputs = {}
        for f in fields(defaults):
            v = getattr(defaults, f.name)
            label, unit = _OPT_LABELS.get(f.name, (f.name, ""))
            if f.name.endswith("_dir"):
                w = FolderField(v)
                w.changed.connect(self._update)
            elif f.name == "button":
                w = QComboBox()
                w.addItem("Any grip button", "any")
                for n in self.hw.button_names():
                    w.addItem(n, n)
                w.currentIndexChanged.connect(self._update)
            elif isinstance(v, bool):
                w = QCheckBox()
                w.setChecked(v)
                w.toggled.connect(self._update)
            elif isinstance(v, int):
                w = ispin(v, 0, 10000)
                w.setSuffix(unit)
                w.valueChanged.connect(self._update)
            elif isinstance(v, float):
                w = dspin(v, 0, 10000, 0.5 if unit == " s" else 1, 2, unit)
                w.valueChanged.connect(self._update)
            else:
                w = QLineEdit(v)
                w.editingFinished.connect(self._update)
            if f.name in _OPT_TIPS:
                w.setToolTip(_OPT_TIPS[f.name])
            self.inputs[f.name] = w
            form.addRow(label, w)
        self.form_host.addWidget(holder)
        self._update()

    def options(self):
        tpl = self.templates[self.key]
        o = tpl.options()
        for name, w in self.inputs.items():
            if isinstance(w, FolderField):
                val = w.value()
            elif isinstance(w, QComboBox):
                val = w.currentData()
            elif isinstance(w, QCheckBox):
                val = w.isChecked()
            elif isinstance(w, (QSpinBox,)):
                val = w.value()
            elif isinstance(w, QLineEdit):
                val = w.text()
            else:
                val = float(w.value())
            setattr(o, name, val)
        return o

    def _update(self, *_):
        from .templates import build
        try:
            QGuiApplication.setOverrideCursor(Qt.WaitCursor)
            par = build(self.key, self.options(), self.stim_dir)
        except (ValueError, OSError) as exc:
            self.paradigm = None
            self.summary.setText(f"⚠ {exc}")
            self.summary.setStyleSheet(f"color: {theme.DANGER}; font-weight: 600;")
            self.ok.setEnabled(False)
            return
        finally:
            QGuiApplication.restoreOverrideCursor()
        self.paradigm = par
        conds = [c for c in par.conditions() if c not in ("instruction", "cue", "rating")]
        self.summary.setStyleSheet(f"color: {theme.ACCENT}; font-weight: 600;")
        self.summary.setText(f"→ {fmt_time(par.duration)} min  ·  {par.volumes} volumes at TR {par.tr:g}s  ·  "
                             f"{len(par.blocks)} blocks  ·  conditions: {', '.join(conds)}")
        self.ok.setEnabled(True)
