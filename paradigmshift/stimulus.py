"""Draws what the participant sees. Shared by the editor preview and the runner."""
from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QFont, QPainter, QPen, QPixmap

from .model import Block, Paradigm

_cache: dict[str, QPixmap] = {}


def load_pixmap(path: str) -> QPixmap | None:
    if not path:
        return None
    pm = _cache.get(path)
    if pm is None:
        pm = QPixmap(path)
        if pm.isNull():
            return None
        _cache[path] = pm
    return pm


def thumbnail(path: str, height: int) -> QPixmap | None:
    key = f"{path}@thumb{height}"
    pm = _cache.get(key)
    if pm is None:
        src = load_pixmap(path)
        if src is None:
            return None
        pm = src.scaledToHeight(height, Qt.SmoothTransformation)
        _cache[key] = pm
    return pm


def _draw_image(p: QPainter, rect: QRectF, path: str, scale: float, area: QRectF | None = None) -> QRectF:
    area = area or rect
    pm = load_pixmap(path)
    box = QRectF(0, 0, area.width() * scale, area.height() * scale)
    box.moveCenter(area.center())
    if pm is None:
        p.setPen(QPen(QColor("#666"), 2, Qt.DashLine))
        p.setBrush(Qt.NoBrush)
        p.drawRect(box)
        p.setPen(QColor("#888"))
        p.setFont(_font(rect, 0.035))
        p.drawText(box, Qt.AlignCenter, "no image" if not path else "image missing")
        return box
    s = min(box.width() / pm.width(), box.height() / pm.height())
    target = QRectF(0, 0, pm.width() * s, pm.height() * s)
    target.moveCenter(box.center())
    p.drawPixmap(target, pm, QRectF(pm.rect()))
    return target


def _font(rect: QRectF, frac: float, bold: bool = False) -> QFont:
    f = QFont("Inter")
    f.setPixelSize(max(6, int(rect.height() * frac)))
    f.setBold(bold)
    return f


def paint_stimulus(p: QPainter, rect: QRectF, paradigm: Paradigm, block: Block | None,
                   state: dict | None = None, ref_height: float = 1080.0) -> None:
    """Paint *block* into *rect*.

    ``ref_height`` is the screen height the pixel sizes in the block params refer
    to, so the small editor preview looks like a scaled-down scanner screen.
    ``state`` carries live rating info (``value`` / ``confirmed``) during a run.
    """
    p.save()
    p.setRenderHint(QPainter.Antialiasing)
    p.setRenderHint(QPainter.SmoothPixmapTransform)
    p.fillRect(rect, QColor(paradigm.background))
    if block is None:
        p.restore()
        return
    k = rect.height() / ref_height
    prm = block.params
    if block.kind == "fixation":
        f = QFont("DejaVu Sans")
        f.setPixelSize(max(4, int(prm.get("size", 72) * k)))
        p.setFont(f)
        p.setPen(QColor(prm.get("color", "#FFFFFF")))
        p.drawText(rect, Qt.AlignCenter, prm.get("symbol", "+"))
    elif block.kind == "image":
        _draw_image(p, rect, prm.get("path", ""), float(prm.get("scale", 0.8)))
    elif block.kind == "text":
        f = QFont("Inter")
        f.setPixelSize(max(4, int(prm.get("size", 40) * k)))
        p.setFont(f)
        p.setPen(QColor(prm.get("color", "#FFFFFF")))
        p.drawText(rect.adjusted(rect.width() * .1, 0, -rect.width() * .1, 0),
                   Qt.AlignCenter | Qt.TextWordWrap, prm.get("text", ""))
    elif block.kind == "rating":
        _paint_rating(p, rect, block, state or {})
    elif block.kind == "match":
        _paint_match(p, rect, block)
    elif block.kind == "gonogo":
        _paint_gonogo(p, rect, block, (state or {}).get("t", 0.0), k)
    p.restore()


def gonogo_visible(block: Block, t_in_block: float) -> bool:
    """Go/No-Go stimuli are only shown for ``stim_time`` seconds, then the ISI symbol."""
    return t_in_block < float(block.params.get("stim_time", 0.5))


def _paint_gonogo(p: QPainter, rect: QRectF, block: Block, t: float, k: float) -> None:
    prm = block.params
    if gonogo_visible(block, t):
        if prm.get("path"):
            _draw_image(p, rect, prm["path"], 0.5)
            return
        f = QFont("Inter")
        f.setBold(True)
        f.setPixelSize(max(4, int(prm.get("size", 140) * k)))
        p.setFont(f)
        p.setPen(QColor(prm.get("color", "#FFFFFF")))
        p.drawText(rect, Qt.AlignCenter, prm.get("text", ""))
    elif prm.get("isi_symbol"):
        f = QFont("DejaVu Sans")
        f.setPixelSize(max(4, int(72 * k)))
        p.setFont(f)
        p.setPen(QColor("#FFFFFF"))
        p.drawText(rect, Qt.AlignCenter, prm["isi_symbol"])


def _paint_match(p: QPainter, rect: QRectF, block: Block) -> None:
    """Hariri layout: target on top, two choices below."""
    prm = block.params
    sc = float(prm.get("scale", 0.9))
    h = rect.height() * .44
    w = h * 0.8
    gap = rect.width() * .05
    top = QRectF(0, 0, w, h)
    top.moveCenter(QPointF(rect.center().x(), rect.top() + rect.height() * .27))
    left = QRectF(0, 0, w, h)
    left.moveCenter(QPointF(rect.center().x() - w / 2 - gap, rect.top() + rect.height() * .74))
    right = QRectF(0, 0, w, h)
    right.moveCenter(QPointF(rect.center().x() + w / 2 + gap, rect.top() + rect.height() * .74))
    for area, key in ((top, "top"), (left, "left"), (right, "right")):
        _draw_image(p, rect, prm.get(key, ""), sc, area)


def _paint_rating(p: QPainter, rect: QRectF, block: Block, state: dict) -> None:
    prm = block.params
    img_area = QRectF(rect.left(), rect.top() + rect.height() * .04, rect.width(), rect.height() * .6)
    if prm.get("path"):
        _draw_image(p, rect, prm["path"], float(prm.get("scale", 0.9)), img_area)
    lo, hi = int(prm.get("min", 1)), int(prm.get("max", 4))
    n = max(1, hi - lo + 1)
    p.setPen(QColor("#FFFFFF"))
    p.setFont(_font(rect, .04))
    q_rect = QRectF(rect.left(), rect.top() + rect.height() * .66, rect.width(), rect.height() * .08)
    p.drawText(q_rect, Qt.AlignCenter, prm.get("question", ""))

    y = rect.top() + rect.height() * .83
    x0, x1 = rect.left() + rect.width() * .22, rect.right() - rect.width() * .22
    p.setPen(QPen(QColor("#BBBBBB"), max(1.0, rect.height() * .004)))
    p.drawLine(QPointF(x0, y), QPointF(x1, y))
    value = state.get("value")
    confirmed = state.get("confirmed", False)
    r = rect.height() * .022
    for i in range(n):
        v = lo + i
        x = x0 + (x1 - x0) * (i / (n - 1) if n > 1 else .5)
        sel = value == v
        col = QColor("#F5A524") if sel else QColor("#DDDDDD")
        if sel and confirmed:
            col = QColor("#2FB37A")
        p.setBrush(col if sel else QColor("#000000"))
        p.setPen(QPen(col, max(1.0, rect.height() * .004)))
        p.drawEllipse(QPointF(x, y), r * (1.35 if sel else 1), r * (1.35 if sel else 1))
        p.setPen(QColor("#FFFFFF"))
        p.setFont(_font(rect, .03, bold=sel))
        p.drawText(QRectF(x - 40, y + r * 1.6, 80, rect.height() * .05), Qt.AlignHCenter | Qt.AlignTop, str(v))
    p.setFont(_font(rect, .026))
    p.setPen(QColor("#AAAAAA"))
    p.drawText(QRectF(x0 - rect.width() * .2, y - rect.height() * .02, rect.width() * .18, rect.height() * .04),
               Qt.AlignRight | Qt.AlignVCenter, prm.get("left_label", ""))
    p.drawText(QRectF(x1 + rect.width() * .02, y - rect.height() * .02, rect.width() * .18, rect.height() * .04),
               Qt.AlignLeft | Qt.AlignVCenter, prm.get("right_label", ""))
