"""Generated placeholder stimuli so every template runs out of the box.

Validated stimulus sets (e.g. NimStim/KDEF faces, IAPS/OASIS pictures) cannot be
shipped with the app because of their licences. These drawings stand in for
them: schematic faces with angry/fearful expressions, Hariri-style geometric
shapes, and abstract neutral/negative scenes. Replace them with a validated set
for real studies – every template accepts your own folders.
"""
from __future__ import annotations

import math
import os
import random

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import (QColor, QImage, QLinearGradient, QPainter, QPainterPath, QPen,
                           QRadialGradient)

FACE_W, FACE_H = 300, 380
EMOTIONS = ("angry", "fearful")
SHAPES = ("circle", "ellipse_wide", "ellipse_tall")


def _canvas(w: int, h: int, color: str) -> tuple[QImage, QPainter]:
    img = QImage(w, h, QImage.Format_RGB32)
    img.fill(QColor(color))
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    return img, p


# ---------------------------------------------------------------------------- faces
def draw_face(out: str, identity: int, emotion: str) -> None:
    """Greyscale schematic face. *identity* changes face shape, skin tone and hair."""
    rng = random.Random(identity * 7919)
    img, p = _canvas(FACE_W, FACE_H, "#3A3A3A")
    cx, cy = FACE_W / 2, FACE_H * .53
    fw = FACE_W * rng.uniform(.56, .68)
    fh = FACE_H * rng.uniform(.62, .70)
    skin = QColor.fromHsl(0, 0, rng.randint(150, 205))
    hair = QColor.fromHsl(0, 0, rng.randint(25, 120))
    style = identity % 3

    # hair behind the head (long style)
    if style == 2:
        p.setBrush(hair)
        p.setPen(Qt.NoPen)
        p.drawRoundedRect(QRectF(cx - fw * .62, cy - fh * .55, fw * 1.24, fh * 1.05), 60, 60)
    # neck + head
    p.setPen(Qt.NoPen)
    p.setBrush(skin.darker(115))
    p.drawRect(QRectF(cx - fw * .17, cy + fh * .35, fw * .34, fh * .3))
    p.setBrush(skin)
    p.drawEllipse(QPointF(cx, cy), fw / 2, fh / 2)
    # hair on top
    p.setBrush(hair)
    top = QPainterPath()
    top.moveTo(cx - fw * .52, cy - fh * .05)
    if style == 1:   # side parting
        top.cubicTo(cx - fw * .6, cy - fh * .75, cx + fw * .7, cy - fh * .7, cx + fw * .52, cy - fh * .08)
        top.cubicTo(cx + fw * .3, cy - fh * .32, cx - fw * .1, cy - fh * .28, cx - fw * .52, cy - fh * .05)
    else:
        top.cubicTo(cx - fw * .55, cy - fh * .72, cx + fw * .55, cy - fh * .72, cx + fw * .52, cy - fh * .05)
        top.cubicTo(cx + fw * .35, cy - fh * .36, cx - fw * .35, cy - fh * .36, cx - fw * .52, cy - fh * .05)
    p.drawPath(top)

    ink = QColor("#1E1E1E")
    eye_y = cy - fh * .05
    eye_dx = fw * .2
    fear = emotion == "fearful"
    # eyes: wide open for fear, narrowed for anger
    eye_h = fh * (.07 if fear else .035)
    for sx in (-1, 1):
        ex = cx + sx * eye_dx
        p.setBrush(QColor("#F2F2F2"))
        p.setPen(QPen(ink, 2))
        p.drawEllipse(QPointF(ex, eye_y), fw * .085, eye_h)
        p.setBrush(ink)
        p.setPen(Qt.NoPen)
        p.drawEllipse(QPointF(ex, eye_y), fw * .035, min(eye_h, fw * .035))
    # brows
    p.setPen(QPen(hair.darker(130) if hair.lightness() > 60 else ink, 6, Qt.SolidLine, Qt.RoundCap))
    for sx in (-1, 1):
        inner = QPointF(cx + sx * fw * .07, eye_y - fh * (.12 if fear else .05))
        outer = QPointF(cx + sx * fw * .32, eye_y - fh * (.09 if fear else .11))
        if fear:   # raised, inner ends pulled up
            inner.setY(eye_y - fh * .16)
        p.drawLine(inner, outer)
    # nose
    p.setPen(QPen(skin.darker(150), 3, Qt.SolidLine, Qt.RoundCap))
    nl = fh * rng.uniform(.1, .15)
    p.drawLine(QPointF(cx, eye_y + fh * .04), QPointF(cx - fw * .04, eye_y + fh * .04 + nl))
    p.drawLine(QPointF(cx - fw * .04, eye_y + fh * .04 + nl), QPointF(cx + fw * .03, eye_y + fh * .05 + nl))
    # mouth
    my = cy + fh * .25
    p.setPen(QPen(ink, 4, Qt.SolidLine, Qt.RoundCap))
    if fear:   # open, stretched
        p.setBrush(QColor("#2A1A1A"))
        p.drawEllipse(QPointF(cx, my), fw * .15, fh * .06)
    else:      # tight, down-turned
        m = QPainterPath(QPointF(cx - fw * .17, my + fh * .025))
        m.cubicTo(cx - fw * .05, my - fh * .02, cx + fw * .05, my - fh * .02, cx + fw * .17, my + fh * .025)
        p.setBrush(Qt.NoBrush)
        p.drawPath(m)
    p.end()
    img.save(out)


def draw_shape(out: str, kind: str) -> None:
    """Hariri sensorimotor control: simple geometric forms, same frame as the faces."""
    img, p = _canvas(FACE_W, FACE_H, "#3A3A3A")
    c = QPointF(FACE_W / 2, FACE_H / 2)
    p.setBrush(QColor("#D9D9D9"))
    p.setPen(QPen(QColor("#F5F5F5"), 3))
    rx, ry = {"circle": (95, 95), "ellipse_wide": (120, 65), "ellipse_tall": (65, 125)}[kind]
    p.drawEllipse(c, rx, ry)
    p.end()
    img.save(out)


# ---------------------------------------------------------------------------- scenes
def draw_scene(out: str, valence: str, seed: int) -> None:
    """Abstract landscape: calm and grey-blue (neutral) or dark, red and jagged (negative)."""
    rng = random.Random(seed)
    w, h = 800, 600
    img, p = _canvas(w, h, "#000000")
    if valence == "negative":
        sky = [("#1B0F12", "#5A1E1E"), ("#101418", "#4A2A1A"), ("#1A1010", "#6B2B1B")][seed % 3]
        layers = ["#3B1414", "#260C0C", "#120606"]
        jag = 1.0
    else:
        sky = [("#7C93A6", "#C9D3DA"), ("#8C9A8F", "#D5DAD0"), ("#9AA3AD", "#DEE2E6")][seed % 3]
        layers = ["#7D8A93", "#66727A", "#525C63"]
        jag = 0.0
    g = QLinearGradient(0, 0, 0, h)
    g.setColorAt(0, QColor(sky[0]))
    g.setColorAt(1, QColor(sky[1]))
    p.fillRect(0, 0, w, h, g)
    if valence == "negative" and rng.random() < .6:   # fire glow
        rg = QRadialGradient(QPointF(w * rng.uniform(.2, .8), h * .7), 260)
        rg.setColorAt(0, QColor(220, 70, 20, 170))
        rg.setColorAt(1, QColor(220, 70, 20, 0))
        p.fillRect(0, 0, w, h, rg)
    for i, col in enumerate(layers):
        path = QPainterPath(QPointF(0, h))
        base = h * (.5 + .13 * i)
        amp = rng.uniform(25, 70)
        ph = rng.uniform(0, 6)
        for x in range(0, w + 10, 10):
            y = base - amp * math.sin(ph + 2 * x / w * math.pi)
            if jag:
                y -= rng.uniform(0, 45) * jag
            path.lineTo(x, y)
        path.lineTo(w, h)
        path.closeSubpath()
        p.fillPath(path, QColor(col))
    if valence == "negative":   # broken diagonal strokes
        p.setPen(QPen(QColor(0, 0, 0, 160), 6))
        for _ in range(rng.randint(3, 7)):
            x = rng.uniform(0, w)
            p.drawLine(QPointF(x, h * .3), QPointF(x + rng.uniform(-120, 120), h))
    p.end()
    img.save(out)


# ---------------------------------------------------------------------------- sets
def _ensure(path: str, fn, *args) -> str:
    if not os.path.exists(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        fn(path, *args)
    return path


def hariri_faces(folder: str, identities: int = 6) -> dict[str, list[str]]:
    """``{"angry": [...], "fearful": [...]}`` – index *i* is the same identity in both lists."""
    return {e: [_ensure(os.path.join(folder, f"face{i + 1:02d}_{e}.png"), draw_face, i, e)
                for i in range(identities)] for e in EMOTIONS}


def hariri_shapes(folder: str) -> dict[str, str]:
    return {k: _ensure(os.path.join(folder, f"shape_{k}.png"), draw_shape, k) for k in SHAPES}


def scenes(folder: str, valence: str, n: int) -> list[str]:
    off = 0 if valence == "neutral" else 1000
    return [_ensure(os.path.join(folder, f"{valence}_{i + 1:02d}.png"), draw_scene, valence, off + i)
            for i in range(n)]
