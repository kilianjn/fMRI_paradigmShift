"""Dark, editing-suite look plus painter-drawn icons (no image assets needed)."""
from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QFont, QIcon, QPainter, QPainterPath, QPen, QPixmap

BG = "#16181D"
PANEL = "#1E2128"
PANEL_2 = "#252932"
BORDER = "#323743"
TEXT = "#E6E8EE"
TEXT_DIM = "#8C93A3"
ACCENT = "#F5A524"     # playhead / primary actions
DANGER = "#F0506E"
OK = "#2FB37A"

STYLESHEET = f"""
* {{ font-family: "Inter", "Segoe UI", "Helvetica Neue", sans-serif; font-size: 13px; color: {TEXT}; }}
QMainWindow, QDialog {{ background: {BG}; }}
QWidget#Panel, QFrame#Panel {{ background: {PANEL}; border: 1px solid {BORDER}; border-radius: 8px; }}
QLabel#PanelTitle {{ color: {TEXT_DIM}; font-size: 11px; font-weight: 600; letter-spacing: 1px; }}
QLabel#Hint {{ color: {TEXT_DIM}; font-size: 12px; }}
QLabel#Big {{ font-size: 22px; font-weight: 600; }}
QLabel#Timecode {{ font-family: "DejaVu Sans Mono", "Consolas", monospace; font-size: 20px; color: {ACCENT}; }}
QToolTip {{ background: {PANEL_2}; color: {TEXT}; border: 1px solid {BORDER}; padding: 6px; }}
QMenuBar {{ background: {BG}; }}
QMenuBar::item:selected {{ background: {PANEL_2}; }}
QMenu {{ background: {PANEL}; border: 1px solid {BORDER}; padding: 4px; }}
QMenu::item {{ padding: 6px 24px 6px 12px; border-radius: 4px; }}
QMenu::item:selected {{ background: {PANEL_2}; }}
QMenu::separator {{ height: 1px; background: {BORDER}; margin: 4px 8px; }}
QToolBar {{ background: {BG}; border: none; spacing: 6px; padding: 6px; }}
QToolButton {{ background: transparent; border: 1px solid transparent; border-radius: 6px; padding: 6px 10px; }}
QToolButton:hover {{ background: {PANEL_2}; border-color: {BORDER}; }}
QToolButton:disabled {{ color: #5A6070; }}
QToolBar::separator {{ background: {BORDER}; width: 1px; margin: 6px 8px; }}
QToolButton:checked {{ background: {PANEL_2}; border-color: {ACCENT}; }}
QPushButton {{ background: {PANEL_2}; border: 1px solid {BORDER}; border-radius: 6px; padding: 7px 14px; }}
QPushButton:hover {{ border-color: #4A5162; background: #2C313C; }}
QPushButton:pressed {{ background: #20242C; }}
QPushButton:disabled {{ color: #5A6070; }}
QPushButton#Primary {{ background: {ACCENT}; color: #1B1B1B; border: none; font-weight: 600; }}
QPushButton#Primary:hover {{ background: #FFB840; }}
QPushButton#Run {{ background: {OK}; color: #0E1A14; border: none; font-weight: 700; padding: 8px 18px; }}
QPushButton#Run:hover {{ background: #3CCB8C; }}
QLineEdit, QSpinBox, QDoubleSpinBox, QComboBox, QPlainTextEdit, QTextEdit {{
    background: {BG}; border: 1px solid {BORDER}; border-radius: 6px; padding: 5px 8px;
    selection-background-color: {ACCENT}; selection-color: #000;
}}
QLineEdit:focus, QSpinBox:focus, QDoubleSpinBox:focus, QComboBox:focus, QPlainTextEdit:focus {{ border-color: {ACCENT}; }}
QComboBox QAbstractItemView {{ background: {PANEL}; border: 1px solid {BORDER}; selection-background-color: {PANEL_2}; }}
QSpinBox::up-button, QDoubleSpinBox::up-button, QSpinBox::down-button, QDoubleSpinBox::down-button {{ width: 16px; border: none; }}
QCheckBox::indicator, QRadioButton::indicator {{ width: 16px; height: 16px; border: 1px solid #4A5162; background: {BG}; }}
QCheckBox::indicator {{ border-radius: 4px; }}
QRadioButton::indicator {{ border-radius: 9px; }}
QCheckBox::indicator:checked, QRadioButton::indicator:checked {{ background: {ACCENT}; border-color: {ACCENT}; }}
QCheckBox::indicator:hover, QRadioButton::indicator:hover {{ border-color: {ACCENT}; }}
QGroupBox {{ border: 1px solid {BORDER}; border-radius: 8px; margin-top: 18px; padding: 12px 10px 10px 10px; }}
QGroupBox::title {{ subcontrol-origin: margin; left: 10px; padding: 0 4px; color: {TEXT_DIM}; font-weight: 600; }}
QTabWidget::pane {{ border: 1px solid {BORDER}; border-radius: 8px; top: -1px; }}
QTabBar::tab {{ background: transparent; padding: 8px 16px; color: {TEXT_DIM}; border-bottom: 2px solid transparent; }}
QTabBar::tab:selected {{ color: {TEXT}; border-bottom-color: {ACCENT}; }}
QScrollBar:horizontal {{ background: {BG}; height: 12px; margin: 0; }}
QScrollBar::handle:horizontal {{ background: #3A404D; border-radius: 5px; min-width: 40px; margin: 2px; }}
QScrollBar:vertical {{ background: {BG}; width: 12px; margin: 0; }}
QScrollBar::handle:vertical {{ background: #3A404D; border-radius: 5px; min-height: 40px; margin: 2px; }}
QScrollBar::add-line, QScrollBar::sub-line {{ width: 0; height: 0; }}
QScrollArea {{ border: none; background: transparent; }}
QScrollArea > QWidget > QWidget {{ background: transparent; }}
QWidget#InspectorBody {{ background: {PANEL}; }}
QSplitter::handle {{ background: {BG}; }}
QStatusBar {{ background: {BG}; color: {TEXT_DIM}; }}
QTableWidget {{ background: {BG}; border: 1px solid {BORDER}; border-radius: 6px; gridline-color: {BORDER}; }}
QHeaderView::section {{ background: {PANEL_2}; border: none; padding: 6px; color: {TEXT_DIM}; }}
QListWidget {{ background: {BG}; border: 1px solid {BORDER}; border-radius: 6px; }}
QSlider::groove:horizontal {{ height: 4px; background: {BORDER}; border-radius: 2px; }}
QSlider::handle:horizontal {{ background: {ACCENT}; width: 14px; margin: -6px 0; border-radius: 7px; }}
"""


def ui_font(size: float = 10, bold: bool = False) -> QFont:
    f = QFont("Inter")
    f.setPointSizeF(size)
    f.setBold(bold)
    return f


# --------------------------------------------------------------------------
# Icons – drawn with QPainter so the app ships without image files
# --------------------------------------------------------------------------
def draw_kind_glyph(p: QPainter, kind: str, r: QRectF, color: QColor) -> None:
    p.save()
    p.setRenderHint(QPainter.Antialiasing)
    pen = QPen(color, max(1.6, r.width() / 11), Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
    p.setPen(pen)
    p.setBrush(Qt.NoBrush)
    c = r.center()
    w, h = r.width(), r.height()
    if kind == "fixation":
        p.drawLine(QPointF(c.x(), r.top() + h * .18), QPointF(c.x(), r.bottom() - h * .18))
        p.drawLine(QPointF(r.left() + w * .18, c.y()), QPointF(r.right() - w * .18, c.y()))
    elif kind == "image":
        fr = r.adjusted(w * .12, h * .2, -w * .12, -h * .2)
        p.drawRoundedRect(fr, 2, 2)
        path = QPainterPath(QPointF(fr.left() + 1, fr.bottom() - 1))
        path.lineTo(fr.left() + fr.width() * .4, fr.top() + fr.height() * .45)
        path.lineTo(fr.left() + fr.width() * .6, fr.top() + fr.height() * .7)
        path.lineTo(fr.left() + fr.width() * .75, fr.top() + fr.height() * .55)
        path.lineTo(fr.right() - 1, fr.bottom() - 1)
        p.drawPath(path)
        p.drawEllipse(QPointF(fr.right() - fr.width() * .22, fr.top() + fr.height() * .28), w * .05, w * .05)
    elif kind == "rating":
        y = c.y() + h * .12
        p.drawLine(QPointF(r.left() + w * .14, y), QPointF(r.right() - w * .14, y))
        for i in range(4):
            x = r.left() + w * .14 + i * (w * .72) / 3
            p.drawLine(QPointF(x, y - h * .08), QPointF(x, y + h * .08))
        p.setBrush(color)
        x = r.left() + w * .14 + 2 * (w * .72) / 3
        tri = QPainterPath(QPointF(x, y - h * .14))
        tri.lineTo(x - w * .09, y - h * .34)
        tri.lineTo(x + w * .09, y - h * .34)
        tri.closeSubpath()
        p.drawPath(tri)
    elif kind == "text":
        for i, frac in enumerate((.8, .62, .72)):
            y = r.top() + h * (.3 + .2 * i)
            p.drawLine(QPointF(r.left() + w * .16, y), QPointF(r.left() + w * (.16 + .68 * frac), y))
    elif kind == "rest":
        bw = w * .12
        p.setBrush(color)
        p.setPen(Qt.NoPen)
        p.drawRoundedRect(QRectF(c.x() - bw * 1.6, r.top() + h * .24, bw, h * .52), 1.5, 1.5)
        p.drawRoundedRect(QRectF(c.x() + bw * .6, r.top() + h * .24, bw, h * .52), 1.5, 1.5)
    elif kind == "response":
        # a hand grip with a button
        body = QRectF(c.x() - w * .14, r.top() + h * .22, w * .28, h * .6)
        p.drawRoundedRect(body, w * .12, w * .12)
        p.setBrush(color)
        p.drawEllipse(QPointF(c.x(), r.top() + h * .36), w * .06, w * .06)
    elif kind == "match":
        bw, bh = w * .26, h * .26
        for x, y in ((c.x() - bw / 2, r.top() + h * .14), (r.left() + w * .14, r.top() + h * .58),
                     (r.right() - w * .14 - bw, r.top() + h * .58)):
            p.drawRoundedRect(QRectF(x, y, bw, bh), 2, 2)
    elif kind == "gonogo":
        p.drawEllipse(QPointF(r.left() + w * .32, c.y()), w * .15, w * .15)
        p.setBrush(color)
        p.drawEllipse(QPointF(r.right() - w * .32, c.y()), w * .15, w * .15)
    elif kind == "trigger":
        path = QPainterPath(QPointF(r.left() + w * .12, c.y()))
        for i, x in enumerate((.3, .3, .5, .5, .7, .7, .88)):
            y = c.y() - h * .25 if i % 4 in (1, 2) else c.y()
            path.lineTo(r.left() + w * x, y)
        p.drawPath(path)
    p.restore()


def kind_icon(kind: str, color: str | None = None, size: int = 32) -> QIcon:
    from .model import BLOCK_TYPES
    pm = QPixmap(size, size)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    base = QColor(color or BLOCK_TYPES[kind].color)
    p.setBrush(base)
    p.setPen(Qt.NoPen)
    p.drawRoundedRect(QRectF(0, 0, size, size), size * .22, size * .22)
    draw_kind_glyph(p, kind, QRectF(0, 0, size, size), QColor("white"))
    p.end()
    return QIcon(pm)


def glyph_icon(name: str, color: str = TEXT, size: int = 20) -> QIcon:
    """Small monochrome UI icons: play, pause, stop, plus, undo, ..."""
    pm = QPixmap(size * 2, size * 2)
    pm.setDevicePixelRatio(2)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    col = QColor(color)
    pen = QPen(col, 1.8, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
    p.setPen(pen)
    s = size
    if name == "play":
        path = QPainterPath(QPointF(s * .3, s * .2))
        path.lineTo(s * .8, s * .5)
        path.lineTo(s * .3, s * .8)
        path.closeSubpath()
        p.setBrush(col)
        p.drawPath(path)
    elif name == "pause":
        p.setBrush(col)
        p.setPen(Qt.NoPen)
        p.drawRoundedRect(QRectF(s * .26, s * .2, s * .16, s * .6), 1.5, 1.5)
        p.drawRoundedRect(QRectF(s * .58, s * .2, s * .16, s * .6), 1.5, 1.5)
    elif name == "stop":
        p.setBrush(col)
        p.setPen(Qt.NoPen)
        p.drawRoundedRect(QRectF(s * .25, s * .25, s * .5, s * .5), 2, 2)
    elif name == "start":
        p.drawLine(QPointF(s * .25, s * .22), QPointF(s * .25, s * .78))
        path = QPainterPath(QPointF(s * .75, s * .22))
        path.lineTo(s * .35, s * .5)
        path.lineTo(s * .75, s * .78)
        path.closeSubpath()
        p.setBrush(col)
        p.drawPath(path)
    elif name in ("undo", "redo"):
        path = QPainterPath()
        if name == "undo":
            path.moveTo(s * .3, s * .42)
            path.cubicTo(s * .45, s * .22, s * .85, s * .3, s * .75, s * .72)
            p.drawPath(path)
            p.drawLine(QPointF(s * .3, s * .42), QPointF(s * .28, s * .2))
            p.drawLine(QPointF(s * .3, s * .42), QPointF(s * .52, s * .44))
        else:
            path.moveTo(s * .7, s * .42)
            path.cubicTo(s * .55, s * .22, s * .15, s * .3, s * .25, s * .72)
            p.drawPath(path)
            p.drawLine(QPointF(s * .7, s * .42), QPointF(s * .72, s * .2))
            p.drawLine(QPointF(s * .7, s * .42), QPointF(s * .48, s * .44))
    elif name == "new":
        p.drawRoundedRect(QRectF(s * .25, s * .15, s * .5, s * .7), 2, 2)
        p.drawLine(QPointF(s * .5, s * .38), QPointF(s * .5, s * .62))
        p.drawLine(QPointF(s * .38, s * .5), QPointF(s * .62, s * .5))
    elif name == "open":
        path = QPainterPath(QPointF(s * .15, s * .75))
        path.lineTo(s * .15, s * .25)
        path.lineTo(s * .4, s * .25)
        path.lineTo(s * .48, s * .33)
        path.lineTo(s * .85, s * .33)
        path.lineTo(s * .85, s * .75)
        path.closeSubpath()
        p.drawPath(path)
    elif name == "save":
        p.drawRoundedRect(QRectF(s * .2, s * .2, s * .6, s * .6), 2, 2)
        p.drawRect(QRectF(s * .32, s * .2, s * .36, s * .2))
        p.drawRect(QRectF(s * .32, s * .55, s * .36, s * .25))
    elif name == "wand":
        p.drawLine(QPointF(s * .22, s * .78), QPointF(s * .62, s * .38))
        for a, b in (((.72, .14), (.72, .34)), ((.62, .24), (.82, .24)), ((.86, .46), (.86, .58)), ((.8, .52), (.92, .52))):
            p.drawLine(QPointF(s * a[0], s * a[1]), QPointF(s * b[0], s * b[1]))
    elif name == "hardware":
        p.drawRoundedRect(QRectF(s * .15, s * .3, s * .7, s * .42), 3, 3)
        for i in range(3):
            p.setBrush(col if i == 0 else Qt.NoBrush)
            p.drawEllipse(QPointF(s * (.32 + .18 * i), s * .51), s * .05, s * .05)
        p.drawLine(QPointF(s * .5, s * .3), QPointF(s * .5, s * .14))
    elif name == "export":
        p.drawLine(QPointF(s * .5, s * .15), QPointF(s * .5, s * .6))
        p.drawLine(QPointF(s * .32, s * .33), QPointF(s * .5, s * .15))
        p.drawLine(QPointF(s * .68, s * .33), QPointF(s * .5, s * .15))
        p.drawLine(QPointF(s * .2, s * .6), QPointF(s * .2, s * .82))
        p.drawLine(QPointF(s * .2, s * .82), QPointF(s * .8, s * .82))
        p.drawLine(QPointF(s * .8, s * .82), QPointF(s * .8, s * .6))
    elif name == "magnet":
        path = QPainterPath(QPointF(s * .25, s * .2))
        path.lineTo(s * .25, s * .5)
        path.arcTo(QRectF(s * .25, s * .25, s * .5, s * .5), 180, 180)
        path.lineTo(s * .75, s * .2)
        p.drawPath(path)
        p.drawLine(QPointF(s * .18, s * .32), QPointF(s * .32, s * .32))
        p.drawLine(QPointF(s * .68, s * .32), QPointF(s * .82, s * .32))
    elif name == "zoomin" or name == "zoomout":
        p.drawEllipse(QPointF(s * .44, s * .44), s * .24, s * .24)
        p.drawLine(QPointF(s * .62, s * .62), QPointF(s * .82, s * .82))
        p.drawLine(QPointF(s * .33, s * .44), QPointF(s * .55, s * .44))
        if name == "zoomin":
            p.drawLine(QPointF(s * .44, s * .33), QPointF(s * .44, s * .55))
    elif name == "template":
        for x, y in ((.18, .18), (.54, .18), (.18, .54), (.54, .54)):
            p.drawRoundedRect(QRectF(s * x, s * y, s * .28, s * .28), 2, 2)
    elif name == "check":
        p.drawLine(QPointF(s * .22, s * .52), QPointF(s * .42, s * .72))
        p.drawLine(QPointF(s * .42, s * .72), QPointF(s * .8, s * .3))
    elif name == "fit":
        for a, b, c2 in (((.2, .38), (.2, .2), (.38, .2)), ((.62, .2), (.8, .2), (.8, .38)),
                         ((.8, .62), (.8, .8), (.62, .8)), ((.38, .8), (.2, .8), (.2, .62))):
            path = QPainterPath(QPointF(s * a[0], s * a[1]))
            path.lineTo(s * b[0], s * b[1])
            path.lineTo(s * c2[0], s * c2[1])
            p.drawPath(path)
    p.end()
    return QIcon(pm)


def app_icon() -> QIcon:
    size = 128
    pm = QPixmap(size, size)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    p.setBrush(QColor(PANEL_2))
    p.setPen(Qt.NoPen)
    p.drawRoundedRect(QRectF(4, 4, 120, 120), 26, 26)
    cols = ["#4C8DF6", "#E0913A", "#D45C9E", "#2FB37A"]
    xs = [(14, 36), (52, 28), (82, 32)]
    for i, (x, w) in enumerate(xs):
        p.setBrush(QColor(cols[i]))
        p.drawRoundedRect(QRectF(x, 38, w, 22), 5, 5)
    p.setBrush(QColor(cols[3]))
    p.drawRoundedRect(QRectF(40, 70, 44, 22), 5, 5)
    p.setPen(QPen(QColor(ACCENT), 5, Qt.SolidLine, Qt.RoundCap))
    p.drawLine(QPointF(66, 22), QPointF(66, 106))
    p.end()
    return QIcon(pm)
