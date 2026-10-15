"""Side panels: block library + media bin, participant-view monitor, inspector."""
from __future__ import annotations

import os

from PySide6.QtCore import QMimeData, QPoint, QRectF, QSize, Qt, QUrl, Signal
from PySide6.QtGui import QColor, QIcon, QPainter, QPen
from PySide6.QtWidgets import (QAbstractItemView, QCheckBox, QColorDialog, QComboBox, QDoubleSpinBox,
                               QFileDialog, QFormLayout, QFrame, QHBoxLayout, QLabel, QLineEdit,
                               QListWidget, QListWidgetItem, QPlainTextEdit, QPushButton, QScrollArea,
                               QSizePolicy, QSpinBox, QVBoxLayout, QWidget)

from . import theme
from .document import Document
from .model import BLOCK_TYPES, Block
from .stimulus import paint_stimulus, thumbnail
from .timeline import IMAGE_EXT, fmt_time, is_image, start_kind_drag


def panel(title: str, *, hint: str = "") -> tuple[QFrame, QVBoxLayout]:
    f = QFrame()
    f.setObjectName("Panel")
    lay = QVBoxLayout(f)
    lay.setContentsMargins(12, 10, 12, 12)
    lay.setSpacing(8)
    head = QHBoxLayout()
    t = QLabel(title.upper())
    t.setObjectName("PanelTitle")
    head.addWidget(t)
    head.addStretch()
    if hint:
        h = QLabel(hint)
        h.setObjectName("Hint")
        head.addWidget(h)
    lay.addLayout(head)
    return f, lay


# =============================================================================
# Library
# =============================================================================
class BlockCard(QFrame):
    """A draggable block type. Drag onto the timeline, or double-click to append."""
    activated = Signal(str)

    def __init__(self, kind: str):
        super().__init__()
        self.kind = kind
        bt = BLOCK_TYPES[kind]
        self.setCursor(Qt.OpenHandCursor)
        self.setToolTip(f"<b>{bt.title}</b><br>{bt.description}<br><br>"
                        "<span style='color:#8C93A3'>Drag onto the timeline · double-click to add at the end</span>")
        self.setStyleSheet(f"""
            BlockCard {{ background: {theme.PANEL_2}; border: 1px solid {theme.BORDER}; border-radius: 8px; }}
            BlockCard:hover {{ border-color: {bt.color}; background: #2B303B; }}""")
        lay = QHBoxLayout(self)
        lay.setContentsMargins(8, 4, 8, 4)
        lay.setSpacing(10)
        icon = QLabel()
        icon.setPixmap(theme.kind_icon(kind, size=26).pixmap(26, 26))
        lay.addWidget(icon)
        txt = QVBoxLayout()
        txt.setSpacing(0)
        name = QLabel(bt.title)
        name.setStyleSheet("font-weight: 600; background: transparent; border: none;")
        sub = QLabel(f"{bt.default_duration:g}s · {'stimulus' if bt.default_track == 0 else 'response track'}")
        sub.setStyleSheet(f"color: {theme.TEXT_DIM}; font-size: 11px; background: transparent; border: none;")
        txt.addWidget(name)
        txt.addWidget(sub)
        lay.addLayout(txt, 1)
        self._press = None

    def mousePressEvent(self, e):
        self._press = e.position().toPoint()

    def mouseMoveEvent(self, e):
        if self._press is not None and (e.position().toPoint() - self._press).manhattanLength() > 6:
            self._press = None
            start_kind_drag(self, self.kind)

    def mouseReleaseEvent(self, e):
        self._press = None

    def mouseDoubleClickEvent(self, e):
        self.activated.emit(self.kind)


class MediaBin(QListWidget):
    """Imported pictures. Drag them to the timeline like files from the desktop."""

    def __init__(self):
        super().__init__()
        self.setViewMode(QListWidget.IconMode)
        self.setIconSize(QSize(64, 48))
        self.setGridSize(QSize(76, 70))
        self.setResizeMode(QListWidget.Adjust)
        self.setMovement(QListWidget.Static)
        self.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.setDragEnabled(True)
        self.setAcceptDrops(True)
        self.setDragDropMode(QAbstractItemView.DragDrop)
        self.setWordWrap(False)
        self.setToolTip("Your pictures. Drag one or many onto the timeline.\nDrop image files or folders here to import.")
        self.setMinimumHeight(90)

    def add_files(self, files: list[str]):
        have = {self.item(i).data(Qt.UserRole) for i in range(self.count())}
        for f in files:
            if f in have or not is_image(f):
                continue
            th = thumbnail(f, 48)
            it = QListWidgetItem(QIcon(th) if th else QIcon(), os.path.splitext(os.path.basename(f))[0][:11])
            it.setData(Qt.UserRole, f)
            it.setToolTip(f)
            self.addItem(it)

    def files(self) -> list[str]:
        return [self.item(i).data(Qt.UserRole) for i in range(self.count())]

    def mimeData(self, items):
        m = QMimeData()
        m.setUrls([QUrl.fromLocalFile(i.data(Qt.UserRole)) for i in items])
        return m

    def mimeTypes(self):
        return ["text/uri-list"]

    def dragEnterEvent(self, e):
        if e.source() is self:
            e.ignore()
        elif e.mimeData().hasUrls():
            e.acceptProposedAction()

    def dragMoveEvent(self, e):
        if e.source() is not self and e.mimeData().hasUrls():
            e.acceptProposedAction()

    def dropEvent(self, e):
        files = []
        for u in e.mimeData().urls():
            f = u.toLocalFile()
            if os.path.isdir(f):
                files += sorted(os.path.join(f, n) for n in os.listdir(f) if is_image(n))
            else:
                files.append(f)
        self.add_files(files)
        e.acceptProposedAction()

    def paintEvent(self, e):
        super().paintEvent(e)
        if self.count() == 0:
            p = QPainter(self.viewport())
            p.setPen(QColor(theme.TEXT_DIM))
            p.setFont(theme.ui_font(9))
            p.drawText(self.viewport().rect().adjusted(8, 8, -8, -8), Qt.AlignCenter | Qt.TextWordWrap,
                       "Drop pictures or folders here")
            p.end()


class LibraryPanel(QFrame):
    addKind = Signal(str)

    def __init__(self):
        super().__init__()
        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(8)
        f, lay = panel("Blocks", hint="drag ➜ timeline")
        lay.setSpacing(5)
        for kind in BLOCK_TYPES:
            card = BlockCard(kind)
            card.activated.connect(self.addKind)
            lay.addWidget(card)
        outer.addWidget(f)
        m, mlay = panel("Pictures")
        self.media = MediaBin()
        mlay.addWidget(self.media, 1)
        b = QPushButton("＋  Import pictures…")
        b.clicked.connect(self._import)
        mlay.addWidget(b)
        outer.addWidget(m, 1)
        self.setMinimumWidth(230)
        self.setMaximumWidth(300)

    def _import(self):
        files, _ = QFileDialog.getOpenFileNames(
            self, "Import pictures", "", "Images (" + " ".join("*" + e for e in IMAGE_EXT) + ")")
        self.media.add_files(files)


# =============================================================================
# Participant view (program monitor)
# =============================================================================
class MonitorView(QWidget):
    """Shows what the participant sees at the playhead, like a program monitor."""

    def __init__(self, doc: Document):
        super().__init__()
        self.doc = doc
        self.t = 0.0
        self.setMinimumSize(320, 200)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        doc.changed.connect(self.update)

    def set_time(self, t: float):
        self.t = t
        self.update()

    def screen_rect(self) -> QRectF:
        r = QRectF(self.rect()).adjusted(4, 4, -4, -4)
        w = min(r.width(), r.height() * 16 / 9)
        h = w * 9 / 16
        out = QRectF(0, 0, w, h)
        out.moveCenter(r.center())
        return out

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.fillRect(self.rect(), QColor(theme.PANEL))
        r = self.screen_rect()
        par = self.doc.paradigm
        b = par.stimulus_at(self.t)
        state = {}
        if b is not None and b.kind == "rating":
            lo, hi = int(b.params.get("min", 1)), int(b.params.get("max", 4))
            state = {"value": (lo + hi + 1) // 2} if b.params.get("mode") == "cursor" else {}
        if b is not None:
            state["t"] = self.t - b.start
        paint_stimulus(p, r, par, b, state)
        p.setPen(QPen(QColor(theme.BORDER), 1))
        p.setBrush(Qt.NoBrush)
        p.drawRect(r)
        # overlay: what is happening right now
        p.setFont(theme.ui_font(8, True))
        resp = [x for x in par.active_blocks(self.t) if x.kind == "response"]
        tags = []
        if b is not None:
            title = b.display_label if b.kind == "gonogo" else b.type.title
            tags.append((title + (f" · {b.condition}" if b.condition else ""), b.color))
        else:
            tags.append(("background", "#5B6577"))
        if resp:
            tags.append(("● recording grips", theme.OK))
        x = r.left() + 8
        for text, col in tags:
            w = p.fontMetrics().horizontalAdvance(text) + 14
            tag = QRectF(x, r.top() + 8, w, 18)
            c = QColor(col)
            c.setAlpha(210)
            p.setBrush(c)
            p.setPen(Qt.NoPen)
            p.drawRoundedRect(tag, 9, 9)
            p.setPen(QColor("white"))
            p.drawText(tag, Qt.AlignCenter, text)
            x += w + 6
        p.end()


# =============================================================================
# Inspector
# =============================================================================
class ColorButton(QPushButton):
    colorChanged = Signal(str)

    def __init__(self, color: str):
        super().__init__()
        self.setFixedHeight(30)
        self.set_color(color)
        self.clicked.connect(self._pick)

    def set_color(self, color: str):
        self.color = color
        self.setText(color.upper())
        fg = "#000" if QColor(color).lightness() > 140 else "#fff"
        self.setStyleSheet(f"background: {color}; color: {fg}; border: 1px solid {theme.BORDER};")

    def _pick(self):
        c = QColorDialog.getColor(QColor(self.color), self, "Choose colour")
        if c.isValid():
            self.set_color(c.name())
            self.colorChanged.emit(c.name())


class ImagePicker(QWidget):
    pathChanged = Signal(str)

    def __init__(self, path: str, height: int = 110):
        super().__init__()
        self._h = height
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        self.preview = QLabel()
        self.preview.setAlignment(Qt.AlignCenter)
        self.preview.setFixedHeight(height)
        self.preview.setStyleSheet(f"background: {theme.BG}; border: 1px dashed {theme.BORDER}; border-radius: 6px;")
        lay.addWidget(self.preview)
        row = QHBoxLayout()
        self.name = QLabel()
        self.name.setObjectName("Hint")
        self.name.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        row.addWidget(self.name, 1)
        btn = QPushButton("Choose…")
        btn.clicked.connect(self._choose)
        row.addWidget(btn)
        lay.addLayout(row)
        self.set_path(path)

    def set_path(self, path: str):
        self.path = path
        th = thumbnail(path, self._h - 6) if path else None
        if th:
            self.preview.setPixmap(th)
        else:
            self.preview.setText("No picture – click Choose…" if not path else "File not found")
        self.name.setText(os.path.basename(path) if path else "")

    def _choose(self):
        f, _ = QFileDialog.getOpenFileName(self, "Choose picture", os.path.dirname(self.path or ""),
                                           "Images (" + " ".join("*" + e for e in IMAGE_EXT) + ")")
        if f:
            self.set_path(f)
            self.pathChanged.emit(f)


class CommitTextEdit(QPlainTextEdit):
    """Multi-line text that reports its content when the user leaves the field."""
    committed = Signal(str)

    def focusOutEvent(self, e):
        self.committed.emit(self.toPlainText())
        super().focusOutEvent(e)


def dspin(value: float, lo=0.0, hi=100000.0, step=0.5, dec=2, suffix=" s") -> QDoubleSpinBox:
    s = QDoubleSpinBox()
    s.setRange(lo, hi)
    s.setDecimals(dec)
    s.setSingleStep(step)
    s.setValue(value)
    s.setSuffix(suffix)
    s.setKeyboardTracking(False)
    return s


def ispin(value: int, lo=0, hi=100000) -> QSpinBox:
    s = QSpinBox()
    s.setRange(lo, hi)
    s.setValue(value)
    s.setKeyboardTracking(False)
    return s


class InspectorPanel(QFrame):
    """Context sensitive properties: paradigm settings, one block, or many blocks."""

    def __init__(self, doc: Document):
        super().__init__()
        self.doc = doc
        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        self.frame, lay = panel("Inspector")
        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        lay.addWidget(self.scroll)
        outer.addWidget(self.frame)
        self.setMinimumWidth(330)
        self.setMaximumWidth(420)
        self._rebuilding = False
        doc.selectionChanged.connect(self.rebuild)
        doc.changed.connect(self._on_changed)
        self._shown = None
        self.rebuild()

    def _on_changed(self):
        if not self._rebuilding:
            self.rebuild()

    # helpers ----------------------------------------------------------------
    def _set(self, block_id: str, **kw):
        def fn(par):
            b = par.block(block_id)
            if b is None:
                return
            for k, v in kw.items():
                if k in ("start", "duration", "label", "condition", "track"):
                    setattr(b, k, v)
                else:
                    b.params[k] = v
        self._apply(fn)

    def _apply(self, fn):
        self._rebuilding = True
        try:
            self.doc.edit(fn)
        finally:
            self._rebuilding = False
        self._refresh_info()

    def _section(self, lay, text):
        lab = QLabel(text)
        lab.setStyleSheet(f"color: {theme.TEXT_DIM}; font-size: 11px; font-weight: 600; margin-top: 8px;")
        lay.addRow(lab)

    def rebuild(self):
        w = QWidget()
        w.setObjectName("InspectorBody")
        form = QFormLayout(w)
        form.setContentsMargins(2, 2, 6, 2)
        form.setLabelAlignment(Qt.AlignRight | Qt.AlignVCenter)
        form.setVerticalSpacing(8)
        self._info = None
        blocks = self.doc.selected_blocks()
        if not blocks:
            self._build_paradigm(form)
        elif len(blocks) == 1:
            self._build_block(form, blocks[0])
        else:
            self._build_multi(form, blocks)
        for cb in w.findChildren(QComboBox):   # never let long entries widen the panel
            cb.setSizeAdjustPolicy(QComboBox.AdjustToMinimumContentsLengthWithIcon)
            cb.setMinimumContentsLength(6)
        if self._info is not None:
            self._info.setWordWrap(True)
        self.scroll.setWidget(w)

    def _refresh_info(self):
        if self._info is None:
            return
        blocks = self.doc.selected_blocks()
        par = self.doc.paradigm
        if len(blocks) == 1 and par.tr > 0:
            b = blocks[0]
            self._info.setText(f"Volume {int(b.start // par.tr) + 1} → {int(max(b.start, b.end - 1e-6) // par.tr) + 1}"
                               f"  ·  {b.duration / par.tr:.2f} TR"
                               + ("" if abs(b.start / par.tr - round(b.start / par.tr)) < 1e-6
                                  else "  ·  not on a TR boundary"))
        elif not blocks:
            self._info.setText(self._summary())

    def _summary(self) -> str:
        par = self.doc.paradigm
        conds = par.conditions()
        return (f"Duration  {fmt_time(par.duration)}\n"
                f"Volumes  {par.volumes} (+{par.dummy_scans} dummy)\n"
                f"Blocks  {len(par.blocks)}\n"
                f"Conditions  {', '.join(conds) if conds else '–'}")

    # paradigm ----------------------------------------------------------------
    def _build_paradigm(self, form):
        par = self.doc.paradigm
        title = QLabel("Paradigm")
        title.setObjectName("Big")
        form.addRow(title)
        hint = QLabel("Nothing selected – these settings apply to the whole run.\n"
                      "Click a block in the timeline to edit it.")
        hint.setObjectName("Hint")
        hint.setWordWrap(True)
        form.addRow(hint)
        name = QLineEdit(par.name)
        name.editingFinished.connect(lambda: self._apply(lambda p: setattr(p, "name", name.text())))
        form.addRow("Name", name)
        self._section(form, "SCANNER")
        tr = dspin(par.tr, 0.1, 20, 0.1, 3)
        tr.setToolTip("Repetition time of your EPI sequence. The timeline grid and volume counter follow it.")
        tr.valueChanged.connect(lambda v: self._apply(lambda p: setattr(p, "tr", v)))
        form.addRow("TR", tr)
        dummy = ispin(par.dummy_scans, 0, 50)
        dummy.setSuffix(" volumes")
        dummy.setToolTip("Triggers to ignore before the paradigm starts (scanner warm-up).")
        dummy.valueChanged.connect(lambda v: self._apply(lambda p: setattr(p, "dummy_scans", v)))
        form.addRow("Dummy scans", dummy)
        self._section(form, "SCREEN")
        bg = ColorButton(par.background)
        bg.colorChanged.connect(lambda c: self._apply(lambda p: setattr(p, "background", c)))
        form.addRow("Background", bg)
        self._section(form, "SUMMARY")
        self._info = QLabel(self._summary())
        self._info.setStyleSheet("line-height: 150%;")
        form.addRow(self._info)

    # one block -----------------------------------------------------------------
    def _build_block(self, form, b: Block):
        bid = b.id
        bt = b.type
        head = QHBoxLayout()
        ic = QLabel()
        ic.setPixmap(theme.kind_icon(b.kind, size=34).pixmap(34, 34))
        head.addWidget(ic)
        t = QLabel(bt.title)
        t.setObjectName("Big")
        head.addWidget(t, 1)
        hw = QWidget()
        hw.setLayout(head)
        form.addRow(hw)
        d = QLabel(bt.description)
        d.setObjectName("Hint")
        d.setWordWrap(True)
        form.addRow(d)

        label = QLineEdit(b.label)
        label.setPlaceholderText(b.display_label)
        label.editingFinished.connect(lambda: self._set(bid, label=label.text()))
        form.addRow("Label", label)
        cond = QComboBox()
        cond.setEditable(True)
        cond.addItems([""] + self.doc.paradigm.conditions())
        cond.setCurrentText(b.condition)
        cond.lineEdit().setPlaceholderText("e.g. positive, negative…")
        cond.setToolTip("Condition name – becomes 'trial_type' in the exported events file.")
        cond.lineEdit().editingFinished.connect(lambda: self._set(bid, condition=cond.currentText().strip()))
        cond.activated.connect(lambda _i: self._set(bid, condition=cond.currentText().strip()))
        form.addRow("Condition", cond)

        self._section(form, "TIMING")
        start = dspin(b.start, 0, 100000, self.doc.paradigm.tr or 0.5, 3)
        start.valueChanged.connect(lambda v: self._set(bid, start=v))
        form.addRow("Start", start)
        dur = dspin(b.duration, 0.05, 100000, self.doc.paradigm.tr or 0.5, 3)
        dur.valueChanged.connect(lambda v: self._set(bid, duration=v))
        row = QHBoxLayout()
        row.addWidget(dur, 1)
        if self.doc.paradigm.tr > 0:
            fit = QPushButton("⟷ TR")
            fit.setToolTip("Round start and duration to whole TRs")
            fit.clicked.connect(lambda: self._round_tr(bid))
            row.addWidget(fit)
        rw = QWidget()
        rw.setLayout(row)
        row.setContentsMargins(0, 0, 0, 0)
        form.addRow("Duration", rw)
        self._info = QLabel()
        self._info.setObjectName("Hint")
        form.addRow("", self._info)
        self._refresh_info()

        prm = b.params
        if b.kind in ("image", "rating"):
            self._section(form, "PICTURE")
            pick = ImagePicker(prm.get("path", ""))
            pick.pathChanged.connect(lambda path: self._set(bid, path=path))
            form.addRow(pick)
            sc = dspin(float(prm.get("scale", .8)) * 100, 5, 100, 5, 0, " %")
            sc.setToolTip("How much of the screen the picture may fill")
            sc.valueChanged.connect(lambda v: self._set(bid, scale=v / 100))
            form.addRow("Size", sc)
        if b.kind == "fixation":
            self._section(form, "APPEARANCE")
            sym = QComboBox()
            sym.setEditable(True)
            sym.addItems(["+", "•", "×", "○"])
            sym.setCurrentText(prm.get("symbol", "+"))
            sym.currentTextChanged.connect(lambda s: self._set(bid, symbol=s or "+"))
            form.addRow("Symbol", sym)
            size = ispin(int(prm.get("size", 72)), 8, 600)
            size.setSuffix(" px")
            size.valueChanged.connect(lambda v: self._set(bid, size=v))
            form.addRow("Size", size)
            col = ColorButton(prm.get("color", "#FFFFFF"))
            col.colorChanged.connect(lambda c: self._set(bid, color=c))
            form.addRow("Colour", col)
        if b.kind == "text":
            self._section(form, "TEXT")
            txt = CommitTextEdit(prm.get("text", ""))
            txt.setFixedHeight(90)
            txt.committed.connect(lambda s: self._set(bid, text=s))
            form.addRow(txt)
            size = ispin(int(prm.get("size", 40)), 8, 300)
            size.setSuffix(" px")
            size.valueChanged.connect(lambda v: self._set(bid, size=v))
            form.addRow("Size", size)
            col = ColorButton(prm.get("color", "#FFFFFF"))
            col.colorChanged.connect(lambda c: self._set(bid, color=c))
            form.addRow("Colour", col)
        if b.kind == "rating":
            self._section(form, "RATING SCALE")
            q = QLineEdit(prm.get("question", ""))
            q.editingFinished.connect(lambda: self._set(bid, question=q.text()))
            form.addRow("Question", q)
            rng = QHBoxLayout()
            lo = ispin(int(prm.get("min", 1)), -100, 100)
            hi = ispin(int(prm.get("max", 4)), -100, 100)
            lo.valueChanged.connect(lambda v: self._set(bid, min=v))
            hi.valueChanged.connect(lambda v: self._set(bid, max=v))
            rng.addWidget(lo)
            rng.addWidget(QLabel("to"))
            rng.addWidget(hi)
            rng.setContentsMargins(0, 0, 0, 0)
            rw2 = QWidget()
            rw2.setLayout(rng)
            form.addRow("Values", rw2)
            ll = QLineEdit(prm.get("left_label", ""))
            ll.editingFinished.connect(lambda: self._set(bid, left_label=ll.text()))
            form.addRow("Left label", ll)
            rl = QLineEdit(prm.get("right_label", ""))
            rl.editingFinished.connect(lambda: self._set(bid, right_label=rl.text()))
            form.addRow("Right label", rl)
            mode = QComboBox()
            mode.addItem("One grip button per value", "direct")
            mode.addItem("Move cursor ◀ ▶ and confirm", "cursor")
            mode.setCurrentIndex(0 if prm.get("mode", "direct") == "direct" else 1)
            mode.currentIndexChanged.connect(lambda i: self._set(bid, mode=mode.itemData(i)))
            form.addRow("Answer with", mode)
            hw_ = self.doc.paradigm.hardware
            if prm.get("mode", "direct") == "direct":
                lines = [f"{lo_v} = {n}" for lo_v, n in
                         zip(range(int(prm.get("min", 1)), int(prm.get("max", 4)) + 1), hw_.button_names())]
                expl = "Grips:  " + "  ·  ".join(lines)
                if int(prm.get("max", 4)) - int(prm.get("min", 1)) + 1 > len(hw_.buttons):
                    expl += "\n⚠ more values than grip buttons – use cursor mode"
            else:
                expl = f"◀ {hw_.move_left}   ▶ {hw_.move_right}   ✓ {hw_.confirm}"
            ex = QLabel(expl)
            ex.setObjectName("Hint")
            ex.setWordWrap(True)
            form.addRow("", ex)
        if b.kind == "match":
            self._section(form, "PICTURES  –  TOP IS THE TARGET")
            for key, title in (("top", "Top (target)"), ("left", "Bottom left"), ("right", "Bottom right")):
                pk = ImagePicker(prm.get(key, ""), 72)
                pk.pathChanged.connect(lambda path, k=key: self._set(bid, **{k: path}))
                cap = QLabel(title)
                cap.setObjectName("Hint")
                form.addRow(cap)
                form.addRow(pk)
            corr = QComboBox()
            corr.addItem("Bottom left", "left")
            corr.addItem("Bottom right", "right")
            corr.setCurrentIndex(0 if prm.get("correct", "left") == "left" else 1)
            corr.currentIndexChanged.connect(lambda i: self._set(bid, correct=corr.itemData(i)))
            form.addRow("Correct match", corr)
            for key, title in (("left_button", "Button = left"), ("right_button", "Button = right")):
                cb = QComboBox()
                cb.addItems(self.doc.paradigm.hardware.button_names())
                cb.setCurrentText(prm.get(key, ""))
                cb.currentTextChanged.connect(lambda txt, k=key: self._set(bid, **{k: txt}))
                form.addRow(title, cb)
        if b.kind == "gonogo":
            self._section(form, "GO / NO-GO")
            trial = QComboBox()
            trial.addItem("Go  – participant presses", "go")
            trial.addItem("No-Go  – participant holds back", "nogo")
            trial.setCurrentIndex(0 if prm.get("trial", "go") == "go" else 1)
            trial.currentIndexChanged.connect(lambda i: self._set(bid, trial=trial.itemData(i)))
            form.addRow("Trial", trial)
            txt = QLineEdit(prm.get("text", ""))
            txt.setPlaceholderText("letter or word")
            txt.editingFinished.connect(lambda: self._set(bid, text=txt.text()))
            form.addRow("Stimulus", txt)
            st = dspin(float(prm.get("stim_time", 0.5)), 0.05, 60, 0.1, 2)
            st.setToolTip("How long the stimulus is visible; the rest of the trial shows the fixation symbol.")
            st.valueChanged.connect(lambda v: self._set(bid, stim_time=v))
            form.addRow("Visible for", st)
            btn = QComboBox()
            btn.addItem("Any grip button", "any")
            for n in self.doc.paradigm.hardware.button_names():
                btn.addItem(n, n)
            btn.setCurrentIndex(max(0, btn.findData(prm.get("button", "any"))))
            btn.currentIndexChanged.connect(lambda i: self._set(bid, button=btn.itemData(i)))
            form.addRow("Go button", btn)
            pk = ImagePicker(prm.get("path", ""), 72)
            pk.pathChanged.connect(lambda path: self._set(bid, path=path))
            cap = QLabel("…or show a picture instead of the text")
            cap.setObjectName("Hint")
            form.addRow(cap)
            form.addRow(pk)
        if b.kind == "response":
            self._section(form, "RESPONSE")
            btns = QComboBox()
            btns.addItem("Any grip button", "any")
            for n in self.doc.paradigm.hardware.button_names():
                btns.addItem(n, n)
            i = btns.findData(prm.get("buttons", "any"))
            btns.setCurrentIndex(max(0, i))
            btns.currentIndexChanged.connect(lambda i: self._set(bid, buttons=btns.itemData(i)))
            form.addRow("Accept", btns)
            corr = QComboBox()
            corr.addItem("– (no correct answer)", "")
            for n in self.doc.paradigm.hardware.button_names():
                corr.addItem(n, n)
            corr.setCurrentIndex(max(0, corr.findData(prm.get("correct", ""))))
            corr.currentIndexChanged.connect(lambda i: self._set(bid, correct=corr.itemData(i)))
            form.addRow("Correct", corr)

        self._section(form, "")
        row = QHBoxLayout()
        row.setContentsMargins(0, 0, 0, 0)
        dup = QPushButton("Duplicate")
        dup.clicked.connect(lambda: self.window().timeline.duplicate_selected())
        rm = QPushButton("Delete")
        rm.setStyleSheet(f"color: {theme.DANGER};")
        rm.clicked.connect(lambda: self.window().timeline.delete_selected())
        row.addWidget(dup)
        row.addWidget(rm)
        bw = QWidget()
        bw.setLayout(row)
        form.addRow(bw)

    def _round_tr(self, bid):
        tr = self.doc.paradigm.tr

        def fn(par):
            b = par.block(bid)
            b.start = round(b.start / tr) * tr
            b.duration = max(tr, round(b.duration / tr) * tr)
        self._apply(fn)
        self.rebuild()

    # many blocks -----------------------------------------------------------------
    def _build_multi(self, form, blocks):
        t = QLabel(f"{len(blocks)} blocks")
        t.setObjectName("Big")
        form.addRow(t)
        span = max(b.end for b in blocks) - min(b.start for b in blocks)
        h = QLabel(f"Span {span:.2f}s · starting at {min(b.start for b in blocks):.2f}s\n"
                   "Changes below apply to all selected blocks.")
        h.setObjectName("Hint")
        form.addRow(h)
        ids = [b.id for b in blocks]
        cond = QComboBox()
        cond.setEditable(True)
        cond.addItems([""] + self.doc.paradigm.conditions())
        shared = {b.condition for b in blocks}
        cond.setCurrentText(shared.pop() if len(shared) == 1 else "")

        def set_cond():
            c = cond.currentText().strip()
            self._apply(lambda par: [setattr(par.block(i), "condition", c) for i in ids])
        cond.lineEdit().editingFinished.connect(set_cond)
        cond.activated.connect(lambda _i: set_cond())
        form.addRow("Condition", cond)
        durs = {round(b.duration, 4) for b in blocks}
        dur = dspin(durs.pop() if len(durs) == 1 else blocks[0].duration, 0.05, 100000,
                    self.doc.paradigm.tr or .5, 3)

        def set_dur(v):
            def fn(par):
                # keep the sequence tight: shift following blocks on each track
                for i in sorted(ids, key=lambda i: par.block(i).start):
                    b = par.block(i)
                    delta = v - b.duration
                    for o in par.blocks:
                        if o.track == b.track and o.start >= b.end - 1e-9 and o.id != b.id:
                            o.start = round(o.start + delta, 4)
                    b.duration = v
            self._apply(fn)
        dur.valueChanged.connect(set_dur)
        form.addRow("Duration", dur)
        note = QLabel("Changing the duration moves the following blocks so no gaps or overlaps appear.")
        note.setObjectName("Hint")
        note.setWordWrap(True)
        form.addRow("", note)
        row = QHBoxLayout()
        row.setContentsMargins(0, 0, 0, 0)
        for text, slot in (("Duplicate", lambda: self.window().timeline.duplicate_selected()),
                           ("Repeat ×N…", lambda: self.window().timeline.repeat_selected()),
                           ("Delete", lambda: self.window().timeline.delete_selected())):
            btn = QPushButton(text)
            btn.clicked.connect(slot)
            row.addWidget(btn)
        bw = QWidget()
        bw.setLayout(row)
        form.addRow(bw)
