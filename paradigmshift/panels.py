"""Side panels: block library + media bin, participant-view monitor, inspector."""
from __future__ import annotations

import os

from PySide6.QtCore import QMimeData, QPoint, QRectF, QSize, Qt, QTimer, QUrl, Signal
from PySide6.QtGui import QColor, QIcon, QPainter, QPen
from PySide6.QtWidgets import (QAbstractItemView, QCheckBox, QColorDialog, QComboBox, QDoubleSpinBox,
                               QFileDialog, QFormLayout, QFrame, QGridLayout, QHBoxLayout, QLabel, QLineEdit,
                               QListWidget, QListWidgetItem, QPlainTextEdit, QPushButton, QScrollArea,
                               QSizePolicy, QSpinBox, QTabWidget, QVBoxLayout, QWidget)

from . import theme
from .document import Document
from .model import BLOCK_TYPES, Block
from . import stimulus as stimulus_mod
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

    def __init__(self, kind: str, preset=None):
        super().__init__()
        if preset is not None:     # a task block from presets.py
            self.kind = "preset:" + preset.key
            self.icon_kind, color = preset.icon_kind, preset.color
            title, subtitle, desc = preset.title, preset.subtitle, preset.description
            if preset.variants:
                desc += "<br>Variants: " + ", ".join(v[0] for v in preset.variants) + " (chosen when you drop it)"
        else:
            bt = BLOCK_TYPES[kind]
            self.kind, self.icon_kind, color = kind, kind, bt.color
            title, desc = bt.title, bt.description
            subtitle = f"{bt.default_duration:g}s · {'stimulus' if bt.default_track == 0 else 'response track'}"
        self.color = color
        self.setCursor(Qt.OpenHandCursor)
        self.setToolTip(f"<b>{title}</b><br>{desc}<br><br>"
                        "<span style='color:#8C93A3'>Drag onto the timeline · double-click to add at the end</span>")
        self.setStyleSheet(f"""
            BlockCard {{ background: {theme.PANEL_2}; border: 1px solid {theme.BORDER}; border-radius: 8px; }}
            BlockCard:hover {{ border-color: {color}; background: #2B303B; }}""")
        lay = QHBoxLayout(self)
        lay.setContentsMargins(8, 4, 8, 4)
        lay.setSpacing(10)
        icon = QLabel()
        icon.setPixmap(theme.kind_icon(self.icon_kind, color, size=26).pixmap(26, 26))
        lay.addWidget(icon)
        txt = QVBoxLayout()
        txt.setSpacing(0)
        name = QLabel(title)
        name.setStyleSheet("font-weight: 600; background: transparent; border: none;")
        sub = QLabel(subtitle)
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
            start_kind_drag(self, self.kind, self.icon_kind, self.color)

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

    def refresh_icons(self):
        """Re-render the thumbnails (e.g. after switching the blur preference)."""
        for i in range(self.count()):
            it = self.item(i)
            th = thumbnail(it.data(Qt.UserRole), 48)
            it.setIcon(QIcon(th) if th else QIcon())

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
        from .presets import PRESETS
        f, lay = panel("Blocks", hint="drag ➜ timeline")
        self.tabs = QTabWidget()
        self.tabs.setDocumentMode(True)
        lay.addWidget(self.tabs, 1)

        def scroll_page(build):
            page = QWidget()
            pl = QVBoxLayout(page)
            pl.setContentsMargins(0, 6, 4, 0)
            pl.setSpacing(5)
            build(pl)
            pl.addStretch()
            sc = QScrollArea()
            sc.setWidget(page)
            sc.setWidgetResizable(True)
            sc.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
            return sc

        def basic(pl):
            for kind in BLOCK_TYPES:
                card = BlockCard(kind)
                card.activated.connect(self.addKind)
                pl.addWidget(card)

        def tasks(pl):
            cat = None
            for pr in PRESETS:
                if pr.category != cat:
                    cat = pr.category
                    head = QLabel(cat.upper())
                    head.setObjectName("PanelTitle")
                    head.setStyleSheet("margin-top: 4px;")
                    pl.addWidget(head)
                card = BlockCard("", preset=pr)
                card.activated.connect(self.addKind)
                pl.addWidget(card)

        self.tabs.addTab(scroll_page(basic), "Basic")
        self.tabs.addTab(scroll_page(tasks), "Task blocks")
        outer.addWidget(f, 3)
        m, mlay = panel("Pictures")
        self.media = MediaBin()
        mlay.addWidget(self.media, 1)
        b = QPushButton("＋  Import pictures…")
        b.clicked.connect(self._import)
        mlay.addWidget(b)
        outer.addWidget(m, 1)
        self.setMinimumWidth(240)
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
        paint_stimulus(p, r, par, b, state, blur=stimulus_mod.EDITOR_BLUR)
        p.setPen(QPen(QColor(theme.BORDER), 1))
        p.setBrush(Qt.NoBrush)
        p.drawRect(r)
        # overlay: what is happening right now
        p.setFont(theme.ui_font(8, True))
        tags = []
        if b is not None:
            from .presets import TRIAL_TITLES
            title = b.display_label if b.kind == "gonogo" else TRIAL_TITLES.get(b.params.get("preset"), b.type.title)
            tags.append((title + (f" · {b.condition}" if b.condition else ""), b.color))
        else:
            tags.append(("background", "#5B6577"))
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
    def _set(self, target, **kw):
        """Set fields/params on one block id or on a list of ids (one undo step)."""
        ids = [target] if isinstance(target, str) else list(target)

        def fn(par):
            for block_id in ids:
                b = par.block(block_id)
                if b is None:
                    continue
                for k, v in kw.items():
                    if k in ("start", "duration", "label", "condition", "track"):
                        setattr(b, k, v)
                    else:
                        b.params[k] = v
        self._apply(fn)

    @staticmethod
    def _mixed(blocks, key, default=None) -> bool:
        return len({repr(b.params.get(key, default)) for b in blocks}) > 1

    def _row_label(self, text: str, blocks, *keys) -> QLabel:
        """Form label that turns orange (≠) when the selected blocks differ in this setting."""
        lab = QLabel(text)
        if len(blocks) > 1 and any(self._mixed(blocks, k) for k in keys):
            lab.setText(text + " ≠")
            lab.setStyleSheet(f"color: {theme.ACCENT};")
            lab.setToolTip("The selected blocks have different values here.\n"
                           "Changing it sets the same value for all of them.")
        return lab

    def _line(self, blocks, key, placeholder=""):
        """QLineEdit for a text param; with several blocks only an actual edit is applied to all."""
        ids = [b.id for b in blocks]
        mixed = self._mixed(blocks, key, "")
        w = QLineEdit("" if mixed else str(blocks[0].params.get(key, "")))
        w.setPlaceholderText("different texts – type to set all" if mixed else placeholder)
        initial = w.text()

        def commit():
            if w.text() != initial:      # untouched fields (incl. "different texts") never overwrite anything
                self._set(ids, **{key: w.text()})
        w.editingFinished.connect(commit)
        return w

    def _combo(self, blocks, key, items, default):
        """Non-editable combo for a param. Shows nothing selected when the blocks differ."""
        ids = [b.id for b in blocks]
        cb = QComboBox()
        for text, data in items:
            cb.addItem(text, data)
        if self._mixed(blocks, key, default):
            cb.setCurrentIndex(-1)
            cb.setPlaceholderText("different – choose to set all")
        else:
            cb.setCurrentIndex(max(0, cb.findData(blocks[0].params.get(key, default))))
        cb.activated.connect(lambda i: self._set(ids, **{key: cb.itemData(i)}))
        return cb

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
        wait = QComboBox()
        wait.addItem("Fixation cross", "fixation")
        wait.addItem("Empty screen", "blank")
        wait.addItem("Text", "text")
        wait.setCurrentIndex(max(0, wait.findData(par.waiting_screen)))
        wait.setToolTip("What the participant sees until the paradigm starts (incl. dummy scans).\n"
                        "Trigger counts are only shown in the control room.")
        wait.currentIndexChanged.connect(
            lambda i: (self._apply(lambda p: setattr(p, "waiting_screen", wait.itemData(i))),
                       QTimer.singleShot(0, self.rebuild)))
        form.addRow("Before start", wait)
        if par.waiting_screen == "text":
            wt = QLineEdit(par.waiting_text)
            wt.editingFinished.connect(lambda: self._apply(lambda p: setattr(p, "waiting_text", wt.text())))
            form.addRow("", wt)
        et = QLineEdit(par.end_text)
        et.setPlaceholderText("empty screen")
        et.setToolTip("Shown to the participant after the last block")
        et.editingFinished.connect(lambda: self._apply(lambda p: setattr(p, "end_text", et.text())))
        form.addRow("After the run", et)
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
        from .presets import TRIAL_TITLES
        t = QLabel(TRIAL_TITLES.get(b.params.get("preset"), bt.title))
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

        self._build_params(form, [b])
        same = [o for o in self.doc.paradigm.blocks if o.kind == b.kind]
        if len(same) > 1:
            self._section(form, "")
            allb = QPushButton(f"Edit all {len(same)} {bt.title} blocks together")
            allb.setToolTip("Selects every block of this type – changes in the inspector then apply to all of them.")
            allb.clicked.connect(lambda: self.doc.select([o.id for o in same]))
            form.addRow(allb)
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

    # task trials (Stroop / Flanker / N-back) ---------------------------------------------------
    def _set_task(self, ids, key, value):
        from .presets import derive

        def fn(par):
            for i in ids:
                b = par.block(i)
                if b is not None:
                    b.params[key] = value
                    derive(b)
        self._apply(fn)
        QTimer.singleShot(0, self.rebuild)   # text, colour, correct answer and condition changed too

    def _build_task_fields(self, form, blocks):
        from .presets import TRIAL_FIELDS, TRIAL_TITLES
        presets = {b.params.get("preset") for b in blocks}
        if len(presets) != 1 or next(iter(presets)) not in TRIAL_FIELDS:
            return
        key = next(iter(presets))
        ids = [b.id for b in blocks]
        self._section(form, TRIAL_TITLES[key].upper() + ("" if len(blocks) == 1 else f"  ·  {len(blocks)} TRIALS"))
        for param, label, spec in TRIAL_FIELDS[key]:
            lab = self._row_label(label, blocks, param)
            cur = blocks[0].params.get(param)
            if spec == "text":
                w = self._line(blocks, param)
                w.editingFinished.disconnect()
                start_text = w.text()
                w.editingFinished.connect(lambda w=w, p_=param, st=start_text:
                                          w.text() != st and self._set_task(ids, p_, w.text()))
            elif spec == "int":
                w = ispin(int(cur or 0), 0, 9)
                w.valueChanged.connect(lambda v, p_=param: self._set_task(ids, p_, v))
            else:
                w = QComboBox()
                for text, data in spec:
                    w.addItem(text, data)
                if self._mixed(blocks, param):
                    w.setCurrentIndex(-1)
                    w.setPlaceholderText("different – choose to set all")
                else:
                    w.setCurrentIndex(max(0, w.findData(cur)))
                w.activated.connect(lambda i, w=w, p_=param: self._set_task(ids, p_, w.itemData(i)))
            form.addRow(lab, w)
        hint = QLabel("Text, colour, correct button and condition follow these settings automatically.")
        hint.setObjectName("Hint")
        hint.setWordWrap(True)
        form.addRow("", hint)

    # block-type settings – for one block or several blocks of the same type --------------------
    def _build_params(self, form, blocks: list[Block]):
        b = blocks[0]
        prm = b.params
        ids = [x.id for x in blocks]
        bid = ids if len(blocks) > 1 else b.id
        single = len(blocks) == 1
        L = lambda text, *keys: self._row_label(text, blocks, *keys)  # noqa: E731
        self._build_task_fields(form, blocks)
        if b.kind in ("image", "rating"):
            self._section(form, "PICTURE")
            if single:
                pick = ImagePicker(prm.get("path", ""))
                pick.pathChanged.connect(lambda path: self._set(bid, path=path))
                form.addRow(pick)
            sc = dspin(float(prm.get("scale", .8)) * 100, 5, 100, 5, 0, " %")
            sc.setToolTip("How much of the screen the picture may fill")
            sc.valueChanged.connect(lambda v: self._set(bid, scale=v / 100))
            form.addRow(L("Size", "scale"), sc)
            if b.kind == "image":
                hz = dspin(float(prm.get("flicker_hz", 0) or 0), 0, 60, 1, 1, " Hz")
                hz.setToolTip("Alternate with a second picture this many times per second (e.g. 8 Hz checkerboard). "
                              "0 = no flicker.")
                hz.valueChanged.connect(lambda v: self._set(bid, flicker_hz=v))
                form.addRow(L("Flicker", "flicker_hz"), hz)
                if single and float(prm.get("flicker_hz", 0) or 0) > 0:
                    fp = ImagePicker(prm.get("flicker_path", ""), 72)
                    fp.pathChanged.connect(lambda path: self._set(bid, flicker_path=path))
                    cap = QLabel("Alternates with")
                    cap.setObjectName("Hint")
                    form.addRow(cap)
                    form.addRow(fp)
        if b.kind == "fixation":
            self._section(form, "APPEARANCE")
            sym = QComboBox()
            sym.setEditable(True)
            sym.addItems(["+", "•", "×", "○"])
            sym.setCurrentText(prm.get("symbol", "+"))
            sym.activated.connect(lambda _i: self._set(bid, symbol=sym.currentText() or "+"))
            sym.lineEdit().editingFinished.connect(
                lambda: sym.currentText() != prm.get("symbol", "+") and self._set(bid, symbol=sym.currentText() or "+"))
            form.addRow(L("Symbol", "symbol"), sym)
            size = ispin(int(prm.get("size", 72)), 8, 600)
            size.setSuffix(" px")
            size.valueChanged.connect(lambda v: self._set(bid, size=v))
            form.addRow(L("Size", "size"), size)
            col = ColorButton(prm.get("color", "#FFFFFF"))
            col.colorChanged.connect(lambda c: self._set(bid, color=c))
            form.addRow(L("Colour", "color"), col)
        if b.kind == "text":
            self._section(form, "TEXT")
            mixed_txt = self._mixed(blocks, "text", "")
            txt = CommitTextEdit("" if mixed_txt else prm.get("text", ""))
            txt.setFixedHeight(90)
            if mixed_txt:
                txt.setPlaceholderText("Different texts – type here to give all selected blocks the same text.\n"
                                       "(To change only a word everywhere: Edit → Find and replace)")
            start_txt = txt.toPlainText()
            txt.committed.connect(lambda s: s != start_txt and self._set(bid, text=s))
            form.addRow(L("Text", "text"))
            form.addRow(txt)
            size = ispin(int(prm.get("size", 40)), 8, 300)
            size.setSuffix(" px")
            size.valueChanged.connect(lambda v: self._set(bid, size=v))
            form.addRow(L("Size", "size"), size)
            col = ColorButton(prm.get("color", "#FFFFFF"))
            col.colorChanged.connect(lambda c: self._set(bid, color=c))
            form.addRow(L("Colour", "color"), col)
        if b.kind == "rating":
            self._section(form, "RATING SCALE")
            form.addRow(L("Question", "question"), self._line(blocks, "question"))
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
            form.addRow(L("Values", "min", "max"), rw2)
            form.addRow(L("Left label", "left_label"), self._line(blocks, "left_label"))
            form.addRow(L("Right label", "right_label"), self._line(blocks, "right_label"))
            mode = self._combo(blocks, "mode", [("One grip button per value", "direct"),
                                                ("Move cursor ◀ ▶ and confirm", "cursor")], "direct")
            mode.activated.connect(lambda _i: QTimer.singleShot(0, self.rebuild))
            form.addRow(L("Answer with", "mode"), mode)
            hw_ = self.doc.paradigm.hardware
            if self._mixed(blocks, "mode", "direct"):
                expl = "Some selected ratings use buttons, some the cursor."
            elif prm.get("mode", "direct") == "direct":
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
            self._section(form, "PICTURES  –  TOP IS THE TARGET" if single else "MATCHING TRIALS")
            for key, title in ((("top", "Top (target)"), ("left", "Bottom left"), ("right", "Bottom right"))
                               if single else ()):
                pk = ImagePicker(prm.get(key, ""), 72)
                pk.pathChanged.connect(lambda path, k=key: self._set(bid, **{k: path}))
                cap = QLabel(title)
                cap.setObjectName("Hint")
                form.addRow(cap)
                form.addRow(pk)
            if single:
                corr = self._combo(blocks, "correct", [("Bottom left", "left"), ("Bottom right", "right")], "left")
                form.addRow("Correct match", corr)
            frame = self._combo(blocks, "show_choice", [("Frame the chosen picture", True),
                                                        ("No frame", False)], True)
            form.addRow(L("Feedback", "show_choice"), frame)
            fcol = ColorButton(prm.get("choice_color", "#FFD23F"))
            fcol.colorChanged.connect(lambda c: self._set(bid, choice_color=c))
            form.addRow(L("Frame colour", "choice_color"), fcol)
            names = [(n, n) for n in self.doc.paradigm.hardware.button_names()]
            for key, title, dflt in (("left_button", "Button = left", "Left index"),
                                     ("right_button", "Button = right", "Right index")):
                form.addRow(L(title, key), self._combo(blocks, key, names, dflt))
        if b.kind == "gonogo":
            self._section(form, "GO / NO-GO")
            trial = self._combo(blocks, "trial", [("Go  – participant presses", "go"),
                                                  ("No-Go  – participant holds back", "nogo")], "go")
            form.addRow(L("Trial", "trial"), trial)
            form.addRow(L("Stimulus", "text"), self._line(blocks, "text", "letter or word"))
            st = dspin(float(prm.get("stim_time", 0.5)), 0.05, 60, 0.1, 2)
            st.setToolTip("How long the stimulus is visible; the rest of the trial shows the fixation symbol.")
            st.valueChanged.connect(lambda v: self._set(bid, stim_time=v))
            form.addRow(L("Visible for", "stim_time"), st)
            names = [("Any grip button", "any")] + [(n, n) for n in self.doc.paradigm.hardware.button_names()]
            form.addRow(L("Go button", "button"), self._combo(blocks, "button", names, "any"))
            if single:
                pk = ImagePicker(prm.get("path", ""), 72)
                pk.pathChanged.connect(lambda path: self._set(bid, path=path))
                cap = QLabel("…or show a picture instead of the text")
                cap.setObjectName("Hint")
                form.addRow(cap)
                form.addRow(pk)
        if b.kind == "choice":
            self._section(form, "STIMULUS")
            form.addRow(L("Text", "text"), self._line(blocks, "text", "word, letter or symbol"))
            col = ColorButton(prm.get("color", "#FFFFFF"))
            col.colorChanged.connect(lambda c: self._set(bid, color=c))
            form.addRow(L("Colour", "color"), col)
            size = ispin(int(prm.get("size", 120)), 8, 600)
            size.setSuffix(" px")
            size.valueChanged.connect(lambda v: self._set(bid, size=v))
            form.addRow(L("Size", "size"), size)
            st = dspin(float(prm.get("stim_time", 0) or 0), 0, 60, 0.1, 2)
            st.setSpecialValueText("whole trial")
            st.setToolTip("How long the stimulus is visible; afterwards the fixation symbol. 0 = whole trial.")
            st.valueChanged.connect(lambda v: self._set(bid, stim_time=v))
            form.addRow(L("Visible for", "stim_time"), st)
            self._section(form, "ANSWER")
            names = [(n, n) for n in self.doc.paradigm.hardware.button_names()]
            form.addRow(L("Correct", "correct"), self._combo(
                blocks, "correct", [("Any button (hit)", "any")] + names + [("No press (withhold)", "")],
                "Right index"))
            form.addRow(L("Accept", "buttons"),
                        self._combo(blocks, "buttons", [("Any grip button", "any")] + names, "any"))
            if single:
                pk = ImagePicker(prm.get("path", ""), 72)
                pk.pathChanged.connect(lambda path: self._set(bid, path=path))
                cap = QLabel("…or a picture")
                cap.setObjectName("Hint")
                form.addRow(cap)
                form.addRow(pk)
        if b.kind == "feedback":
            self._section(form, "FEEDBACK  –  ABOUT THE PREVIOUS TRIAL")
            form.addRow(L("If done right", "hit_text"), self._line(blocks, "hit_text"))
            hc = ColorButton(prm.get("hit_color", "#2FB37A"))
            hc.colorChanged.connect(lambda c: self._set(bid, hit_color=c))
            form.addRow(L("Colour", "hit_color"), hc)
            form.addRow(L("If missed / wrong", "miss_text"), self._line(blocks, "miss_text"))
            mc = ColorButton(prm.get("miss_color", "#F0506E"))
            mc.colorChanged.connect(lambda c: self._set(bid, miss_color=c))
            form.addRow(L("Colour", "miss_color"), mc)
            if single:
                prev = self.doc.paradigm.previous_scored(b)
                info = QLabel(f"Reports on: {prev.type.title} “{prev.display_label}” at {fmt_time(prev.start)}"
                              if prev else "⚠ No scored trial before this block.")
                info.setObjectName("Hint")
                info.setWordWrap(True)
                form.addRow("", info)


    def _round_tr(self, bid):
        tr = self.doc.paradigm.tr

        def fn(par):
            b = par.block(bid)
            b.start = round(b.start / tr) * tr
            b.duration = max(tr, round(b.duration / tr) * tr)
        self._apply(fn)
        QTimer.singleShot(0, self.rebuild)

    # many blocks -----------------------------------------------------------------
    def _build_multi(self, form, blocks):
        kind_set = {b.kind for b in blocks}
        head = QHBoxLayout()
        if len(kind_set) == 1:
            ic = QLabel()
            ic.setPixmap(theme.kind_icon(blocks[0].kind, size=34).pixmap(34, 34))
            head.addWidget(ic)
        t = QLabel(f"{len(blocks)} {blocks[0].type.title} blocks" if len(kind_set) == 1 else f"{len(blocks)} blocks")
        t.setObjectName("Big")
        head.addWidget(t, 1)
        hw = QWidget()
        hw.setLayout(head)
        form.addRow(hw)
        span = max(b.end for b in blocks) - min(b.start for b in blocks)
        h = QLabel(f"Span {span:.2f}s · starting at {min(b.start for b in blocks):.2f}s\n"
                   "Changes below apply to all selected blocks.")
        h.setObjectName("Hint")
        form.addRow(h)
        ids = [b.id for b in blocks]
        kinds = {}
        for b in blocks:
            kinds.setdefault(b.kind, []).append(b.id)
        if len(kinds) > 1:
            box = QLabel("Different block types are selected. To change type-specific settings (e.g. how a "
                         "rating is answered) for all of them at once, keep only one type:")
            box.setObjectName("Hint")
            box.setWordWrap(True)
            form.addRow(box)
            grid = QGridLayout()
            grid.setContentsMargins(0, 0, 0, 0)
            for i, (kind, kids) in enumerate(sorted(kinds.items(), key=lambda kv: -len(kv[1]))):
                btn = QPushButton(f"{len(kids)} × {BLOCK_TYPES[kind].title}")
                btn.setIcon(theme.kind_icon(kind, size=18))
                btn.setToolTip(f"Keep only the {BLOCK_TYPES[kind].title} blocks selected")
                btn.clicked.connect(lambda _=False, k=kids: self.doc.select(k))
                grid.addWidget(btn, i // 2, i % 2)
            gw = QWidget()
            gw.setLayout(grid)
            form.addRow(gw)
        cond = QComboBox()
        cond.setEditable(True)
        cond.addItems([""] + self.doc.paradigm.conditions())
        shared = {b.condition for b in blocks}
        cond_mixed = len(shared) > 1
        cond.setCurrentText("" if cond_mixed else shared.pop())
        if cond_mixed:
            cond.lineEdit().setPlaceholderText("different – type to set all")
        initial_cond = cond.currentText()

        def set_cond():
            c = cond.currentText().strip()
            if c == initial_cond.strip():      # nothing typed: keep every block's own condition
                return
            self._apply(lambda par: [setattr(par.block(i), "condition", c) for i in ids])
        cond.lineEdit().editingFinished.connect(set_cond)
        cond.activated.connect(lambda _i: set_cond())
        clab = QLabel("Condition ≠" if cond_mixed else "Condition")
        if cond_mixed:
            clab.setStyleSheet(f"color: {theme.ACCENT};")
        form.addRow(clab, cond)
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
        if len(kinds) == 1:
            self._build_params(form, blocks)
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
