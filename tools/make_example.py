"""Create the example pictures and example paradigm in ./examples.

    python tools/make_example.py
"""
import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from PySide6.QtCore import QPointF, QRectF, Qt  # noqa: E402
from PySide6.QtGui import QColor, QGuiApplication, QImage, QLinearGradient, QPainter, QPainterPath, QRadialGradient  # noqa: E402

from paradigmshift.model import Block, Paradigm  # noqa: E402

ROOT = os.path.join(os.path.dirname(__file__), "..", "examples")
IMG = os.path.join(ROOT, "images")


def landscape(out, sky_top, sky_bottom, sun, hills, seed):
    rng = random.Random(seed)
    w, h = 800, 600
    img = QImage(w, h, QImage.Format_RGB32)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    g = QLinearGradient(0, 0, 0, h)
    g.setColorAt(0, QColor(sky_top))
    g.setColorAt(1, QColor(sky_bottom))
    p.fillRect(0, 0, w, h, g)
    if sun:
        rg = QRadialGradient(QPointF(w * rng.uniform(.25, .75), h * .3), 140)
        rg.setColorAt(0, QColor(sun))
        rg.setColorAt(.35, QColor(sun))
        c = QColor(sun)
        c.setAlpha(0)
        rg.setColorAt(1, c)
        p.fillRect(0, 0, w, h, rg)
    for i, col in enumerate(hills):
        path = QPainterPath(QPointF(0, h))
        base = h * (.45 + .14 * i)
        amp = rng.uniform(30, 80)
        ph = rng.uniform(0, 6)
        fr = rng.uniform(1.2, 2.8)
        for x in range(0, w + 10, 10):
            path.lineTo(x, base - amp * math.sin(ph + fr * x / w * math.pi) - 15 * math.sin(x / 37))
        path.lineTo(w, h)
        path.closeSubpath()
        p.fillPath(path, QColor(col))
    p.end()
    img.save(out)


def main():
    app = QGuiApplication.instance() or QGuiApplication(sys.argv)  # noqa: F841
    os.makedirs(IMG, exist_ok=True)
    pos = [
        ("sunrise", "#FF9A62", "#FFE3A3", "#FFF6D5", ["#7A5A8C", "#4E3B66", "#2B2140"]),
        ("meadow", "#6EC6FF", "#D8F1FF", "#FFFDE8", ["#7BC86C", "#4FA05A", "#2F6E3E"]),
        ("beach", "#4FC3F7", "#E1F5FE", "#FFFFFF", ["#1E88E5", "#F6D7A7", "#E8C28E"]),
        ("lavender", "#B39DDB", "#FCE4EC", "#FFF3E0", ["#9575CD", "#7E57C2", "#5E35B1"]),
    ]
    neg = [
        ("storm", "#263238", "#546E7A", None, ["#37474F", "#263238", "#101418"]),
        ("ashes", "#3E2723", "#795548", "#BF360C", ["#4E342E", "#3E2723", "#1B0F0C"]),
        ("fog", "#455A64", "#90A4AE", None, ["#607D8B", "#455A64", "#263238"]),
        ("night", "#0D1B2A", "#1B263B", "#E0E1DD", ["#22303F", "#152230", "#0A121B"]),
    ]
    paths = {"positive": [], "negative": []}
    for i, (name, *args) in enumerate(pos):
        f = os.path.join(IMG, f"pos_{name}.png")
        landscape(f, *args, seed=i)
        paths["positive"].append(f)
    for i, (name, *args) in enumerate(neg):
        f = os.path.join(IMG, f"neg_{name}.png")
        landscape(f, *args, seed=10 + i)
        paths["negative"].append(f)

    # An emotional picture rating paradigm: instructions, baseline, then
    # picture -> rating trials alternating between conditions, with fixation ITIs.
    par = Paradigm(name="Emotional picture rating", tr=2.0, dummy_scans=3)
    t = 0.0

    def add(kind, dur, track=0, cond="", **kw):
        nonlocal t
        b = Block.create(kind, t, dur, track=track, **kw)
        b.condition = cond
        par.blocks.append(b)
        t += dur
        return b

    add("text", 6.0, text="Rate how each picture makes you feel.\n\nUse the grip buttons 1 – 4.")
    add("fixation", 10.0, label="Baseline")
    order = [("positive", 0), ("negative", 0), ("positive", 1), ("negative", 1),
             ("negative", 2), ("positive", 2), ("negative", 3), ("positive", 3)]
    itis = [4.0, 6.0, 4.0, 8.0, 6.0, 4.0, 6.0, 8.0]
    for (cond, i), iti in zip(order, itis):
        img = paths[cond][i]
        b = add("image", 4.0, cond=cond, path=img)
        resp = Block.create("response", b.start, 4.0, track=1)
        resp.condition = cond
        par.blocks.append(resp)
        add("rating", 4.0, cond=cond, path=img, question="How do you feel?",
            left_label="very bad", right_label="very good")
        add("fixation", iti, label="ITI")
    add("rest", 10.0, label="End baseline")
    out = os.path.join(ROOT, "emotional_rating.paradigm.json")
    par.save(out)
    print("wrote", out)


if __name__ == "__main__":
    main()
