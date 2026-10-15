"""The film-editor style timeline: ruler, tracks, draggable/resizable blocks."""
from __future__ import annotations

import math
import os

from PySide6.QtCore import QMimeData, QPoint, QPointF, QRectF, Qt, Signal
from PySide6.QtGui import (QAction, QBrush, QColor, QCursor, QDrag, QFont, QLinearGradient,
                           QPainter, QPainterPath, QPen)
from PySide6.QtWidgets import QAbstractScrollArea, QInputDialog, QMenu, QToolTip

from . import theme
from .document import Document
from .model import BLOCK_TYPES, Block
from .stimulus import thumbnail

MIME_KIND = "application/x-paradigmshift-kind"
IMAGE_EXT = (".png", ".jpg", ".jpeg", ".bmp", ".gif", ".tif", ".tiff", ".webp")

RULER_H = 52
HEADER_W = 156
TRACK_H = 76
TRACK_PAD = 8
EDGE = 7            # px grab zone for resizing
SNAP_PX = 10        # magnetic snapping distance
MIN_DUR = 0.05


def fmt_time(t: float, decimals: int = 1) -> str:
    sign = "-" if t < 0 else ""
    t = abs(t)
    m, s = divmod(t, 60)
    return f"{sign}{int(m):02d}:{s:0{3 + decimals if decimals else 2}.{decimals}f}"


def is_image(path: str) -> bool:
    return path.lower().endswith(IMAGE_EXT)


class TimelineView(QAbstractScrollArea):
    playheadMoved = Signal(float)
    openInspector = Signal()

    def __init__(self, doc: Document, parent=None):
        super().__init__(parent)
        self.doc = doc
        self.pps = 18.0            # pixels per second (zoom)
        self.playhead = 0.0
        self.snap = True
        self.setAcceptDrops(True)
        self.viewport().setAcceptDrops(True)
        self.viewport().setMouseTracking(True)
        self.setFocusPolicy(Qt.StrongFocus)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOn)
        self.horizontalScrollBar().valueChanged.connect(self.viewport().update)
        self.verticalScrollBar().valueChanged.connect(self.viewport().update)
        self.setMinimumHeight(RULER_H + 2 * TRACK_H + 16)

        self._drag = None          # interactive drag state
        self._ghost = None         # (kind/list, start, track) while dragging from library
        self._band = None          # rubber band QRectF
        self._snap_line = None     # time of last snap for visual feedback
        doc.changed.connect(self._on_changed)
        doc.selectionChanged.connect(self.viewport().update)
        self._on_changed()

    # ------------------------------------------------------------------ geometry
    @property
    def p(self):
        return self.doc.paradigm

    def _scroll_x(self) -> int:
        return self.horizontalScrollBar().value()

    def _scroll_y(self) -> int:
        return self.verticalScrollBar().value()

    def x_of(self, t: float) -> float:
        return HEADER_W + t * self.pps - self._scroll_x()

    def t_of(self, x: float) -> float:
        return (x - HEADER_W + self._scroll_x()) / self.pps

    def track_top(self, i: int) -> float:
        return RULER_H + i * TRACK_H - self._scroll_y()

    def track_at(self, y: float) -> int | None:
        if y < RULER_H:
            return None
        i = int((y - RULER_H + self._scroll_y()) // TRACK_H)
        return i if 0 <= i < len(self.p.tracks) else None

    def block_rect(self, b: Block) -> QRectF:
        top = self.track_top(b.track) + TRACK_PAD / 2
        return QRectF(self.x_of(b.start), top, max(2.0, b.duration * self.pps), TRACK_H - TRACK_PAD)

    def block_at(self, pos: QPointF) -> tuple[Block | None, str]:
        for b in sorted(self.p.blocks, key=lambda b: -b.start):
            r = self.block_rect(b)
            if r.adjusted(-2, 0, 2, 0).contains(pos):
                edge = "move"
                if r.width() > 3 * EDGE:
                    if pos.x() <= r.left() + EDGE:
                        edge = "left"
                    elif pos.x() >= r.right() - EDGE:
                        edge = "right"
                return b, edge
        return None, ""

    def _on_changed(self):
        self._update_scrollbars()
        self.viewport().update()

    def _update_scrollbars(self):
        content = (max(self.p.duration, self.playhead) + 120) * self.pps + HEADER_W
        sb = self.horizontalScrollBar()
        sb.setRange(0, max(0, int(content - self.viewport().width())))
        sb.setPageStep(self.viewport().width())
        sb.setSingleStep(20)
        h = RULER_H + len(self.p.tracks) * TRACK_H
        vb = self.verticalScrollBar()
        vb.setRange(0, max(0, h - self.viewport().height()))
        vb.setPageStep(self.viewport().height())

    def resizeEvent(self, e):
        super().resizeEvent(e)
        self._update_scrollbars()

    # ------------------------------------------------------------------ zoom
    def set_zoom(self, pps: float, anchor_x: float | None = None):
        pps = max(1.5, min(400.0, pps))
        if anchor_x is None:
            anchor_x = self.x_of(self.playhead) if 0 <= self.x_of(self.playhead) <= self.viewport().width() \
                else HEADER_W + (self.viewport().width() - HEADER_W) / 2
        t = self.t_of(anchor_x)
        self.pps = pps
        self._update_scrollbars()
        self.horizontalScrollBar().setValue(int(t * pps - (anchor_x - HEADER_W)))
        self.viewport().update()

    def zoom_in(self):
        self.set_zoom(self.pps * 1.4)

    def zoom_out(self):
        self.set_zoom(self.pps / 1.4)

    def zoom_fit(self):
        dur = max(self.p.duration, 10.0)
        self.set_zoom((self.viewport().width() - HEADER_W - 110) / dur)
        self.horizontalScrollBar().setValue(0)

    def set_playhead(self, t: float, follow: bool = False):
        self.playhead = max(0.0, t)
        if follow:
            x = self.x_of(self.playhead)
            w = self.viewport().width()
            if x > w - 60 or x < HEADER_W:
                self.horizontalScrollBar().setValue(int(self.playhead * self.pps - (w - HEADER_W) * .2))
        self.viewport().update()
        self.playheadMoved.emit(self.playhead)

    # ------------------------------------------------------------------ snapping
    def grid_step(self) -> float:
        """Fine time grid depending on zoom (at least ~6 px per step)."""
        for s in (0.01, 0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10):
            if s * self.pps >= 6:
                return s
        return 10

    def snap_time(self, t: float, exclude: set[str] = frozenset(), mods=None) -> float:
        self._snap_line = None
        free = mods is not None and bool(mods & Qt.AltModifier)
        if not self.snap or free:
            return round(t / 0.01) * 0.01
        cands = [0.0, self.playhead]
        tr = self.p.tr
        if tr > 0:
            cands.append(round(t / tr) * tr)
        for b in self.p.blocks:
            if b.id not in exclude:
                cands += [b.start, b.end]
        best = min(cands, key=lambda c: abs(c - t))
        if abs(best - t) * self.pps <= SNAP_PX:
            self._snap_line = best
            return best
        g = self.grid_step()
        return round(t / g) * g

    # ------------------------------------------------------------------ painting
    def paintEvent(self, _e):
        p = QPainter(self.viewport())
        p.setRenderHint(QPainter.Antialiasing)
        w, h = self.viewport().width(), self.viewport().height()
        p.fillRect(0, 0, w, h, QColor(theme.BG))
        self._paint_tracks(p, w, h)
        p.save()
        p.setClipRect(QRectF(HEADER_W, RULER_H, w - HEADER_W, h - RULER_H))
        self._paint_grid(p, w, h)
        self._paint_end_marker(p, h)
        for b in sorted(self.p.blocks, key=lambda b: b.id in self.doc.selection):
            self._paint_block(p, b)
        self._paint_ghost(p)
        if self._band is not None:
            p.setPen(QPen(QColor(theme.ACCENT), 1, Qt.DashLine))
            p.setBrush(QColor(245, 165, 36, 30))
            p.drawRect(self._band)
        if self._snap_line is not None and (self._drag or self._ghost):
            x = self.x_of(self._snap_line)
            p.setPen(QPen(QColor("#7FD3FF"), 1, Qt.DashLine))
            p.drawLine(QPointF(x, RULER_H), QPointF(x, h))
        if not self.p.blocks and self._ghost is None:
            self._paint_empty_hint(p, w)
        p.restore()
        self._paint_ruler(p, w)
        self._paint_headers(p, h)
        self._paint_playhead(p, h)
        p.end()

    def _paint_tracks(self, p, w, h):
        for i, _name in enumerate(self.p.tracks):
            top = self.track_top(i)
            p.fillRect(QRectF(HEADER_W, top, w - HEADER_W, TRACK_H), QColor("#1B1E24" if i % 2 == 0 else "#191B21"))
            p.setPen(QColor(theme.BORDER))
            p.drawLine(QPointF(HEADER_W, top + TRACK_H - .5), QPointF(w, top + TRACK_H - .5))

    def _paint_headers(self, p, h):
        p.fillRect(QRectF(0, RULER_H, HEADER_W, h - RULER_H), QColor(theme.PANEL))
        for i, name in enumerate(self.p.tracks):
            top = self.track_top(i)
            if top + TRACK_H < RULER_H:
                continue
            r = QRectF(0, top, HEADER_W, TRACK_H)
            p.setPen(QColor(theme.BORDER))
            p.drawLine(r.bottomLeft() + QPointF(0, -.5), r.bottomRight() + QPointF(0, -.5))
            badge = QRectF(12, top + TRACK_H / 2 - 13, 26, 26)
            p.setBrush(QColor(theme.PANEL_2))
            p.setPen(QPen(QColor(theme.BORDER)))
            p.drawRoundedRect(badge, 6, 6)
            p.setFont(theme.ui_font(9, True))
            p.setPen(QColor(theme.ACCENT if i == 0 else theme.OK))
            p.drawText(badge, Qt.AlignCenter, ("V" if i == 0 else "R") + str(i + 1))
            p.setPen(QColor(theme.TEXT))
            p.setFont(theme.ui_font(10, True))
            p.drawText(QRectF(46, top + 14, HEADER_W - 50, 22), Qt.AlignLeft | Qt.AlignVCenter, name)
            p.setPen(QColor(theme.TEXT_DIM))
            p.setFont(theme.ui_font(8))
            sub = "on screen" if i == 0 else "grip presses" if i == 1 else "extra"
            p.drawText(QRectF(46, top + 36, HEADER_W - 50, 18), Qt.AlignLeft | Qt.AlignVCenter, sub)
        p.setPen(QColor(theme.BORDER))
        p.drawLine(QPointF(HEADER_W - .5, RULER_H), QPointF(HEADER_W - .5, h))

    def _tick_step(self) -> tuple[float, float]:
        for major in (1, 2, 5, 10, 15, 30, 60, 120, 300, 600):
            if major * self.pps >= 70:
                minor = major / 5 if major in (1, 5, 10, 60, 300, 600) else major / 4 if major != 15 else 5
                return major, minor
        return 600, 120

    def _paint_ruler(self, p, w):
        p.fillRect(QRectF(0, 0, w, RULER_H), QColor(theme.PANEL))
        # header corner
        p.setPen(QColor(theme.TEXT_DIM))
        p.setFont(theme.ui_font(8, True))
        p.drawText(QRectF(12, 4, HEADER_W - 16, 24), Qt.AlignLeft | Qt.AlignVCenter, "TIME")
        p.drawText(QRectF(12, 28, HEADER_W - 16, 22), Qt.AlignLeft | Qt.AlignVCenter, f"VOLUMES · TR {self.p.tr:g}s")
        p.save()
        p.setClipRect(QRectF(HEADER_W, 0, w - HEADER_W, RULER_H))
        t0, t1 = self.t_of(HEADER_W), self.t_of(w)
        major, minor = self._tick_step()
        p.setFont(theme.ui_font(8))
        t = math.floor(t0 / minor) * minor
        while t <= t1:
            x = self.x_of(t)
            is_major = abs(t / major - round(t / major)) < 1e-6
            p.setPen(QColor("#5A6172" if is_major else "#3A404D"))
            p.drawLine(QPointF(x, 26 - (10 if is_major else 5)), QPointF(x, 26))
            if is_major and t >= 0:
                p.setPen(QColor(theme.TEXT_DIM))
                p.drawText(QRectF(x + 4, 2, 80, 16), Qt.AlignLeft | Qt.AlignVCenter, fmt_time(t, 0))
            t += minor
        # volumes row
        tr = self.p.tr
        vols = self.p.volumes
        if tr > 0:
            v0 = max(0, int(t0 // tr))
            v = v0
            label_every = 1
            while tr * label_every * self.pps < 28:
                label_every *= 2 if label_every < 4 else 5 if label_every == 4 else 2
            while v * tr <= t1:
                x0, x1 = self.x_of(v * tr), self.x_of((v + 1) * tr)
                inside = v < vols
                col = QColor("#2A3040" if v % 2 == 0 else "#262B37") if inside else QColor("#1F232B")
                p.fillRect(QRectF(x0, 28, x1 - x0, 20), col)
                if v % label_every == 0:
                    p.setPen(QColor(theme.TEXT if inside else "#4A5060"))
                    p.drawText(QRectF(x0 + 3, 28, max(26, x1 - x0), 20), Qt.AlignLeft | Qt.AlignVCenter, str(v + 1))
                v += 1
        # end-of-run flag
        dur = self.p.duration
        if dur > 0:
            x = self.x_of(dur)
            p.setFont(theme.ui_font(7.5, True))
            text = f"END  {fmt_time(dur)}"
            tw = p.fontMetrics().horizontalAdvance(text) + 12
            flag = QRectF(x, 3, tw, 17)
            p.setBrush(QColor("#5A6172"))
            p.setPen(Qt.NoPen)
            p.drawRoundedRect(flag, 3, 3)
            p.setPen(QColor("white"))
            p.drawText(flag, Qt.AlignCenter, text)
        p.restore()
        p.setPen(QColor(theme.BORDER))
        p.drawLine(QPointF(0, RULER_H - .5), QPointF(w, RULER_H - .5))

    def _paint_grid(self, p, w, h):
        tr = self.p.tr
        if tr <= 0 or tr * self.pps < 6:
            return
        p.setPen(QPen(QColor(255, 255, 255, 12), 1))
        t0, t1 = self.t_of(HEADER_W), self.t_of(w)
        v = max(0, int(t0 // tr))
        while v * tr <= t1:
            x = self.x_of(v * tr)
            p.drawLine(QPointF(x, RULER_H), QPointF(x, h))
            v += 1

    def _paint_end_marker(self, p, h):
        dur = self.p.duration
        if dur <= 0:
            return
        x = self.x_of(dur)
        p.fillRect(QRectF(x, RULER_H, 4000, h), QColor(0, 0, 0, 50))
        p.setPen(QPen(QColor("#7A8194"), 1, Qt.DashLine))
        p.drawLine(QPointF(x, RULER_H), QPointF(x, h))

    def _paint_block(self, p, b: Block, ghost: bool = False, invalid: bool = False):
        r = self.block_rect(b)
        if r.right() < HEADER_W or r.left() > self.viewport().width():
            return
        sel = b.id in self.doc.selection
        base = QColor(b.color)
        bad = invalid or (self._drag is not None and sel and self.p.has_overlap(b))
        grad = QLinearGradient(r.topLeft(), r.bottomLeft())
        grad.setColorAt(0, base.lighter(112))
        grad.setColorAt(1, base.darker(118))
        p.save()
        if ghost:
            p.setOpacity(.55)
        path = QPainterPath()
        path.addRoundedRect(r, 6, 6)
        p.fillPath(path, QBrush(grad))
        p.setClipPath(path)
        # image thumbnails
        img = b.thumbnail_path
        if img and r.width() > 40:
            th = thumbnail(img, int(r.height() - 22))
            if th is not None:
                x = r.left() + 6
                while x < r.right() - 4:
                    p.setOpacity((.55 if ghost else 1) * .9)
                    p.drawPixmap(QPointF(x, r.top() + 18), th)
                    x += th.width() + 2
                    if x > r.left() + 6 and b.kind == "rating":
                        break
                p.setOpacity(.55 if ghost else 1)
                p.fillRect(QRectF(r.left(), r.top(), r.width(), 18), base.darker(115))
        # response window: hatching for "recording"
        if b.kind == "response":
            p.setPen(QPen(QColor(255, 255, 255, 30), 2))
            step = 10
            x = r.left() - r.height()
            while x < r.right():
                p.drawLine(QPointF(x, r.bottom()), QPointF(x + r.height(), r.top()))
                x += step
        # label
        if r.width() > 22:
            theme.draw_kind_glyph(p, b.kind, QRectF(r.left() + 4, r.top() + 1, 16, 16), QColor("white"))
        if r.width() > 40:
            p.setPen(QColor("white"))
            p.setFont(theme.ui_font(8.5, True))
            p.drawText(QRectF(r.left() + 22, r.top() + 1, r.width() - 26, 16), Qt.AlignLeft | Qt.AlignVCenter,
                       p.fontMetrics().elidedText(b.display_label, Qt.ElideRight, int(r.width() - 26)))
        info_right = r.left() + 4
        if r.width() > 50 and not (img and r.width() > 40):
            p.setPen(QColor(255, 255, 255, 190))
            p.setFont(theme.ui_font(8))
            info = f"{b.duration:g}s"
            if self.p.tr > 0:
                info += f" · {b.duration / self.p.tr:.3g} TR"
            info = p.fontMetrics().elidedText(info, Qt.ElideRight, int(r.width() - 12))
            p.drawText(QRectF(r.left() + 7, r.bottom() - 20, r.width() - 10, 16), Qt.AlignLeft | Qt.AlignVCenter, info)
            info_right = r.left() + 10 + p.fontMetrics().horizontalAdvance(info)
        if b.condition and r.width() > 60 and b.condition not in b.display_label.lower():
            p.setFont(theme.ui_font(7.5, True))
            tw = p.fontMetrics().horizontalAdvance(b.condition) + 12
            tag = QRectF(r.right() - tw - 5, r.bottom() - 19, tw, 15)
            if tag.left() > info_right:
                p.setBrush(QColor(0, 0, 0, 120))
                p.setPen(Qt.NoPen)
                p.drawRoundedRect(tag, 7, 7)
                p.setPen(QColor("white"))
                p.drawText(tag, Qt.AlignCenter, b.condition)
        p.setClipping(False)
        if bad:
            p.setPen(QPen(QColor(theme.DANGER), 2.5))
        elif sel:
            p.setPen(QPen(QColor(theme.ACCENT), 2.5))
        else:
            p.setPen(QPen(base.darker(150), 1))
        p.setBrush(Qt.NoBrush)
        p.drawRoundedRect(r.adjusted(.5, .5, -.5, -.5), 6, 6)
        if sel and r.width() > 3 * EDGE:
            p.setBrush(QColor(theme.ACCENT))
            p.setPen(Qt.NoPen)
            for x in (r.left() + 2, r.right() - 5):
                p.drawRoundedRect(QRectF(x, r.center().y() - 10, 3, 20), 1.5, 1.5)
        p.restore()

    def _paint_ghost(self, p):
        if not self._ghost:
            return
        for b in self._ghost["blocks"]:
            self._paint_block(p, b, ghost=True)

    def _paint_empty_hint(self, p, w):
        r = QRectF(HEADER_W + 30, self.track_top(0) + 8, max(300, w - HEADER_W - 60), TRACK_H - 16)
        p.setPen(QPen(QColor("#3A404D"), 1.5, Qt.DashLine))
        p.setBrush(Qt.NoBrush)
        p.drawRoundedRect(r, 8, 8)
        p.setPen(QColor(theme.TEXT_DIM))
        p.setFont(theme.ui_font(10.5))
        p.drawText(r, Qt.AlignCenter,
                   "Drag a block from the library onto this track  ·  drop image files here  ·  "
                   "or start from  ▦ Templates  (Hariri, Go/No-Go, emotion regulation)")

    def _paint_playhead(self, p, h):
        x = self.x_of(self.playhead)
        if x < HEADER_W - 1:
            return
        col = QColor(theme.ACCENT)
        p.setPen(QPen(col, 1.5))
        p.drawLine(QPointF(x, 18), QPointF(x, h))
        head = QPainterPath(QPointF(x - 7, 2))
        head.lineTo(x + 7, 2)
        head.lineTo(x + 7, 12)
        head.lineTo(x, 19)
        head.lineTo(x - 7, 12)
        head.closeSubpath()
        p.setBrush(col)
        p.setPen(Qt.NoPen)
        p.drawPath(head)

    # ------------------------------------------------------------------ mouse
    def mousePressEvent(self, e):
        pos = e.position()
        self.setFocus()
        if e.button() == Qt.RightButton:
            return super().mousePressEvent(e)
        if e.button() != Qt.LeftButton:
            return
        if pos.y() < RULER_H and pos.x() > HEADER_W:
            self._drag = {"mode": "scrub"}
            self.set_playhead(self.snap_time(self.t_of(pos.x()), mods=e.modifiers()) if self.snap else self.t_of(pos.x()))
            return
        if pos.x() < HEADER_W:
            return
        b, edge = self.block_at(pos)
        mods = e.modifiers()
        if b is None:
            if not (mods & (Qt.ShiftModifier | Qt.ControlModifier)):
                self.doc.select([])
            self._drag = {"mode": "band", "origin": pos, "base": list(self.doc.selection)}
            return
        if mods & (Qt.ShiftModifier | Qt.ControlModifier):
            sel = list(self.doc.selection)
            if b.id in sel:
                sel.remove(b.id)
            else:
                sel.append(b.id)
            self.doc.select(sel)
        elif b.id not in self.doc.selection:
            self.doc.select([b.id])
        if b.id not in self.doc.selection:
            return
        if edge != "move":
            self.doc.select([b.id])
        self._drag = {
            "mode": edge, "block": b.id, "x": pos.x(), "y": pos.y(), "moved": False,
            "orig": {i: (self.p.block(i).start, self.p.block(i).duration, self.p.block(i).track)
                     for i in self.doc.selection},
        }

    def mouseMoveEvent(self, e):
        pos = e.position()
        d = self._drag
        if d is None:
            self._update_hover(pos)
            return
        mode = d["mode"]
        if mode == "scrub":
            t = self.t_of(max(HEADER_W, pos.x()))
            self.set_playhead(self.snap_time(t, mods=e.modifiers()))
            return
        if mode == "band":
            self._band = QRectF(d["origin"], pos).normalized()
            ids = [b.id for b in self.p.blocks if self.block_rect(b).intersects(self._band)]
            self.doc.select(list(dict.fromkeys(d["base"] + ids)))
            self.viewport().update()
            return
        if not d["moved"] and abs(pos.x() - d["x"]) < 3 and abs(pos.y() - d["y"]) < 3:
            return
        if not d["moved"]:
            d["moved"] = True
            self.doc.begin_edit()
        prim = self.p.block(d["block"])
        o_start, o_dur, o_track = d["orig"][prim.id]
        dt = (pos.x() - d["x"]) / self.pps
        ids = set(d["orig"])
        if mode == "move":
            new_start = max(0.0, o_start + dt)
            s1 = self.snap_time(new_start, ids, e.modifiers())
            m1 = self._snap_line
            s2 = self.snap_time(new_start + o_dur, ids, e.modifiers()) - o_dur
            m2 = self._snap_line
            if m2 is not None and (m1 is None or abs(s2 - new_start) < abs(s1 - new_start)):
                snapped = s2
            else:
                snapped, self._snap_line = s1, m1
            delta = snapped - o_start
            min_start = min(v[0] for v in d["orig"].values())
            delta = max(delta, -min_start)
            t_now = self.track_at(pos.y())
            dtrack = (t_now - o_track) if t_now is not None else 0
            min_tr = min(v[2] for v in d["orig"].values())
            max_tr = max(v[2] for v in d["orig"].values())
            dtrack = max(-min_tr, min(len(self.p.tracks) - 1 - max_tr, dtrack))
            for i, (s, _du, tr) in d["orig"].items():
                bb = self.p.block(i)
                bb.start = round(s + delta, 4)
                bb.track = tr + dtrack
        elif mode == "left":
            end = o_start + o_dur
            ns = min(end - MIN_DUR, max(0.0, self.snap_time(o_start + dt, ids, e.modifiers())))
            prim.start, prim.duration = round(ns, 4), round(end - ns, 4)
        elif mode == "right":
            ne = max(o_start + MIN_DUR, self.snap_time(o_start + o_dur + dt, ids, e.modifiers()))
            prim.duration = round(ne - o_start, 4)
        self._show_drag_tip(prim)
        self._update_scrollbars()
        self.viewport().update()

    def _show_drag_tip(self, b: Block):
        tr = self.p.tr
        txt = f"{b.display_label}\nstart {b.start:.2f}s · {b.duration:.2f}s"
        if tr > 0:
            txt += f"\nvolume {int(b.start // tr) + 1} · {b.duration / tr:.2f} TR"
        if self.p.has_overlap(b):
            txt += "\n⚠ overlaps – will snap back"
        QToolTip.showText(QCursor.pos() + QPoint(16, 16), txt, self)

    def mouseReleaseEvent(self, e):
        d = self._drag
        self._drag = None
        self._band = None
        self._snap_line = None
        QToolTip.hideText()
        if d and d["mode"] in ("move", "left", "right") and d["moved"]:
            moved = [self.p.block(i) for i in d["orig"]]
            if any(self.p.has_overlap(b) for b in moved):
                self.doc.cancel_edit()
                self.window().statusBar().showMessage("Blocks can't overlap on the same track – move undone.", 4000)
            else:
                self.doc.commit_edit()
        self.viewport().update()

    def mouseDoubleClickEvent(self, e):
        pos = e.position()
        if pos.y() < RULER_H or pos.x() < HEADER_W:
            return
        b, _ = self.block_at(pos)
        if b is not None:
            self.doc.select([b.id])
            self.openInspector.emit()
            return
        track = self.track_at(pos.y())
        if track is not None:
            self._add_menu(self.snap_time(self.t_of(pos.x())), track).exec(e.globalPosition().toPoint())

    def _update_hover(self, pos):
        if pos.y() < RULER_H and pos.x() > HEADER_W:
            self.viewport().setCursor(Qt.PointingHandCursor)
            return
        b, edge = self.block_at(pos)
        if b is None:
            self.viewport().setCursor(Qt.ArrowCursor)
            self.viewport().setToolTip("")
        else:
            self.viewport().setCursor(Qt.SizeHorCursor if edge in ("left", "right") else Qt.OpenHandCursor)
            tip = f"<b>{b.display_label}</b><br>{b.type.title} · {b.start:.2f}–{b.end:.2f}s"
            if b.condition:
                tip += f"<br>condition: {b.condition}"
            tip += "<br><span style='color:#8C93A3'>drag to move · drag edges to trim · double-click to edit</span>"
            self.viewport().setToolTip(tip)

    def wheelEvent(self, e):
        dy = e.angleDelta().y()
        if e.modifiers() & Qt.ControlModifier:
            self.set_zoom(self.pps * (1.15 if dy > 0 else 1 / 1.15), e.position().x())
        else:
            dx = e.angleDelta().x() or dy
            self.horizontalScrollBar().setValue(self.horizontalScrollBar().value() - dx)

    # ------------------------------------------------------------------ keyboard
    def keyPressEvent(self, e):
        k = e.key()
        if k in (Qt.Key_Delete, Qt.Key_Backspace):
            self.delete_selected(ripple=bool(e.modifiers() & Qt.ShiftModifier))
        elif k in (Qt.Key_Left, Qt.Key_Right) and self.doc.selection:
            step = (self.p.tr if self.p.tr > 0 else 1.0) if e.modifiers() & Qt.ShiftModifier else self.grid_step()
            sign = -1 if k == Qt.Key_Left else 1
            self.doc.begin_edit()
            for b in self.doc.selected_blocks():
                b.start = max(0.0, round(b.start + sign * step, 4))
            if any(self.p.has_overlap(b) for b in self.doc.selected_blocks()):
                self.doc.cancel_edit()
            else:
                self.doc.commit_edit()
        elif k == Qt.Key_A and e.modifiers() & Qt.ControlModifier:
            self.doc.select([b.id for b in self.p.blocks])
        else:
            super().keyPressEvent(e)

    # ------------------------------------------------------------------ actions
    def delete_selected(self, ripple: bool = False):
        ids = set(self.doc.selection)
        if not ids:
            return

        def fn(par):
            if ripple:
                par.ripple_delete(ids)
            else:
                par.blocks = [b for b in par.blocks if b.id not in ids]
        self.doc.edit(fn)
        self.doc.select([])

    def duplicate_selected(self):
        blocks = sorted(self.doc.selected_blocks(), key=lambda b: b.start)
        if not blocks:
            return
        span = max(b.end for b in blocks) - min(b.start for b in blocks)
        new = []

        def fn(par):
            for b in blocks:
                c = b.clone()
                c.start = b.start + span
                new.append(c)
            # insert after the selection, pushing later blocks to the right
            end = max(b.end for b in blocks)
            for o in par.blocks:
                if o.start >= end - 1e-9 and o.track in {b.track for b in blocks}:
                    o.start += span
            par.blocks.extend(new)
        self.doc.edit(fn)
        self.doc.select([b.id for b in new])

    def repeat_selected(self):
        blocks = sorted(self.doc.selected_blocks(), key=lambda b: b.start)
        if not blocks:
            return
        n, ok = QInputDialog.getInt(self, "Repeat", "How many extra copies of the selection?", 3, 1, 500)
        if not ok:
            return
        span = max(b.end for b in blocks) - min(b.start for b in blocks)
        end = max(b.end for b in blocks)
        tracks = {b.track for b in blocks}

        def fn(par):
            for o in par.blocks:
                if o.start >= end - 1e-9 and o.track in tracks:
                    o.start += span * n
            for k in range(1, n + 1):
                for b in blocks:
                    c = b.clone()
                    c.start = b.start + span * k
                    par.blocks.append(c)
        self.doc.edit(fn)

    def split_at_playhead(self):
        t = self.playhead
        targets = [b for b in (self.doc.selected_blocks() or self.p.active_blocks(t)) if b.start < t < b.end]
        if not targets:
            return

        def fn(par):
            for b in targets:
                bb = par.block(b.id)
                c = bb.clone()
                c.start = t
                c.duration = round(bb.end - t, 4)
                bb.duration = round(t - bb.start, 4)
                par.blocks.append(c)
        self.doc.edit(fn)

    def add_block(self, kind: str, start: float | None = None, track: int | None = None, **params) -> Block:
        bt = BLOCK_TYPES[kind]
        track = bt.default_track if track is None else track
        track = min(track, len(self.p.tracks) - 1)
        if start is None:
            start = self.p.track_end(track) if track == 0 else self.playhead
        b = Block.create(kind, start, track=track, **params)
        if self.p.tr > 0 and kind != "response" and abs(bt.default_duration % self.p.tr) > 1e-6 \
                and bt.default_duration > self.p.tr:
            b.duration = round(bt.default_duration / self.p.tr) * self.p.tr
        self.doc.edit(lambda par: par.ripple_insert(b))
        self.doc.select([b.id])
        return b

    def _add_menu(self, t: float, track: int) -> QMenu:
        m = QMenu(self)
        m.addSection(f"Add at {fmt_time(t)}")
        for kind, bt in BLOCK_TYPES.items():
            a = QAction(theme.kind_icon(kind), bt.title, m)
            a.triggered.connect(lambda _=False, k=kind: self.add_block(k, t, track))
            m.addAction(a)
        return m

    def contextMenuEvent(self, e):
        pos = QPointF(e.pos())
        if pos.x() < HEADER_W or pos.y() < RULER_H:
            return
        b, _ = self.block_at(pos)
        if b is not None and b.id not in self.doc.selection:
            self.doc.select([b.id])
        m = QMenu(self)
        if b is not None:
            m.addAction("Edit properties…\tdouble-click", self.openInspector.emit)
            m.addAction("Duplicate\tCtrl+D", self.duplicate_selected)
            m.addAction("Repeat ×N…", self.repeat_selected)
            m.addAction("Split at playhead\tS", self.split_at_playhead)
            m.addSeparator()
            m.addAction("Delete\tDel", self.delete_selected)
            m.addAction("Delete and close gap\tShift+Del", lambda: self.delete_selected(True))
        else:
            track = self.track_at(pos.y())
            if track is not None:
                sub = self._add_menu(self.snap_time(self.t_of(pos.x())), track)
                sub.setTitle("Add block here")
                m.addMenu(sub)
                m.addSeparator()
                m.addAction("Close all gaps on this track", lambda: self.doc.edit(lambda par: par.close_gaps(track)))
        m.exec(e.globalPos())

    # ------------------------------------------------------------------ drag & drop
    def _drop_payload(self, mime: QMimeData):
        if mime.hasFormat(MIME_KIND):
            return ("kind", bytes(mime.data(MIME_KIND)).decode())
        if mime.hasUrls():
            files = [u.toLocalFile() for u in mime.urls() if u.isLocalFile()]
            imgs = []
            for f in files:
                if os.path.isdir(f):
                    imgs += sorted(os.path.join(f, n) for n in os.listdir(f) if is_image(n))
                elif is_image(f):
                    imgs.append(f)
            if imgs:
                return ("images", imgs)
        return None

    def _ghost_blocks(self, payload, pos) -> list[Block]:
        kind, data = payload
        track = self.track_at(pos.y())
        t = max(0.0, self.snap_time(self.t_of(pos.x())))
        if kind == "kind":
            bt = BLOCK_TYPES[data]
            b = Block.create(data, t, track=bt.default_track if track is None else track)
            return [b]
        blocks, cur = [], t
        for f in data:
            b = Block.create("image", cur, track=0 if track is None else track, path=f)
            blocks.append(b)
            cur += b.duration
        return blocks

    def dragEnterEvent(self, e):
        payload = self._drop_payload(e.mimeData())
        if payload:
            e.acceptProposedAction()
            self._ghost = {"payload": payload, "blocks": self._ghost_blocks(payload, e.position())}
            self.viewport().update()
        else:
            e.ignore()

    def dragMoveEvent(self, e):
        if self._ghost:
            self._ghost["blocks"] = self._ghost_blocks(self._ghost["payload"], e.position())
            e.acceptProposedAction()
            x = e.position().x()
            sb = self.horizontalScrollBar()
            if x > self.viewport().width() - 30:
                sb.setValue(sb.value() + 15)
            elif HEADER_W < x < HEADER_W + 20:
                sb.setValue(sb.value() - 15)
            self.viewport().update()

    def dragLeaveEvent(self, e):
        self._ghost = None
        self._snap_line = None
        self.viewport().update()

    def dropEvent(self, e):
        if not self._ghost:
            return
        blocks = self._ghost_blocks(self._ghost["payload"], e.position())
        self._ghost = None
        self._snap_line = None
        if self.track_at(e.position().y()) is None and e.position().y() < RULER_H:
            return

        def fn(par):
            for b in blocks:
                par.ripple_insert(b)
        self.doc.edit(fn)
        self.doc.select([b.id for b in blocks])
        e.acceptProposedAction()
        self.setFocus()


def start_kind_drag(widget, kind: str):
    drag = QDrag(widget)
    mime = QMimeData()
    mime.setData(MIME_KIND, kind.encode())
    drag.setMimeData(mime)
    drag.setPixmap(theme.kind_icon(kind, size=36).pixmap(36, 36))
    drag.exec(Qt.CopyAction)
