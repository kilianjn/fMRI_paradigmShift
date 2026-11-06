"""Smoke tests of the editor UI (run headless with QT_QPA_PLATFORM=offscreen)."""
import os

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
QtWidgets = pytest.importorskip("PySide6.QtWidgets")
from PySide6.QtCore import QMimeData, QPoint, QPointF, Qt  # noqa: E402
from PySide6.QtGui import QDropEvent, QMouseEvent  # noqa: E402

from paradigmshift.mainwindow import MainWindow, export_onsets  # noqa: E402
from paradigmshift.timeline import HEADER_W, MIME_KIND, RULER_H, TRACK_H  # noqa: E402

EXAMPLE = os.path.join(os.path.dirname(__file__), "..", "examples", "emotional_rating.paradigm.json")


@pytest.fixture(scope="module")
def app():
    return QtWidgets.QApplication.instance() or QtWidgets.QApplication([])


@pytest.fixture
def win(app):
    w = MainWindow()
    w.resize(1400, 900)
    w.show()
    app.processEvents()
    yield w
    w.doc.dirty = False
    w.close()


def mouse(widget, kind, pos, buttons=Qt.LeftButton):
    ev = QMouseEvent(kind, QPointF(pos), widget.mapToGlobal(QPointF(pos)), Qt.LeftButton, buttons,
                     Qt.NoModifier)
    QtWidgets.QApplication.sendEvent(widget, ev)


def test_add_from_library_and_undo(win):
    win.timeline.add_block("fixation")
    win.timeline.add_block("image")
    par = win.doc.paradigm
    assert [b.kind for b in par.blocks_on(0)] == ["fixation", "image"]
    assert par.blocks_on(0)[1].start == 10
    win.doc.undo()
    assert len(win.doc.paradigm.blocks) == 1
    win.doc.redo()
    assert len(win.doc.paradigm.blocks) == 2


def test_drop_kind_on_timeline(win, app):
    tl = win.timeline
    mime = QMimeData()
    mime.setData(MIME_KIND, b"rating")
    x = HEADER_W + 5 * tl.pps
    pos = QPointF(x, RULER_H + TRACK_H / 2)
    from PySide6.QtGui import QDragEnterEvent
    enter = QDragEnterEvent(pos.toPoint(), Qt.CopyAction, mime, Qt.LeftButton, Qt.NoModifier)
    tl.dragEnterEvent(enter)
    drop = QDropEvent(pos, Qt.CopyAction, mime, Qt.LeftButton, Qt.NoModifier)
    tl.dropEvent(drop)
    blocks = win.doc.paradigm.blocks
    assert len(blocks) == 1 and blocks[0].kind == "rating"
    assert abs(blocks[0].start - 4.0) < 1.01   # snapped near the drop point (TR grid)
    assert win.doc.selection == [blocks[0].id]


def test_drag_block_moves_and_rejects_overlap(win, app):
    tl = win.timeline
    a = win.timeline.add_block("fixation")         # 0..10
    b = win.timeline.add_block("image")            # 10..14
    tl.horizontalScrollBar().setValue(0)
    vp = tl.viewport()
    y = RULER_H + TRACK_H / 2
    # drag image 10 s to the right
    start = QPointF(tl.x_of(12), y)
    mouse(vp, QMouseEvent.MouseButtonPress, start)
    mouse(vp, QMouseEvent.MouseMove, QPointF(tl.x_of(22), y))
    mouse(vp, QMouseEvent.MouseButtonRelease, QPointF(tl.x_of(22), y), Qt.NoButton)
    assert abs(win.doc.paradigm.block(b.id).start - 20) < 1e-6
    # dragging it onto the fixation snaps back
    mouse(vp, QMouseEvent.MouseButtonPress, QPointF(tl.x_of(22), y))
    mouse(vp, QMouseEvent.MouseMove, QPointF(tl.x_of(7), y))
    mouse(vp, QMouseEvent.MouseButtonRelease, QPointF(tl.x_of(7), y), Qt.NoButton)
    assert abs(win.doc.paradigm.block(b.id).start - 20) < 1e-6
    assert win.doc.paradigm.block(a.id).start == 0


def test_trim_right_edge(win, app):
    tl = win.timeline
    a = tl.add_block("fixation")
    tl.set_zoom(30)
    tl.horizontalScrollBar().setValue(0)
    vp = tl.viewport()
    y = RULER_H + TRACK_H / 2
    edge = tl.block_rect(a).right() - 2
    mouse(vp, QMouseEvent.MouseButtonPress, QPointF(edge, y))
    mouse(vp, QMouseEvent.MouseMove, QPointF(edge + 4 * 30, y))
    mouse(vp, QMouseEvent.MouseButtonRelease, QPointF(edge + 4 * 30, y), Qt.NoButton)
    assert abs(win.doc.paradigm.block(a.id).duration - 14) < 1e-6


def test_open_example_inspector_and_export(win, app, tmp_path):
    win.open_file(EXAMPLE)
    par = win.doc.paradigm
    assert par.volumes == 68 and not par.validate()
    for b in par.blocks:  # every block type builds an inspector
        win.doc.select([b.id])
        app.processEvents()
    win.doc.select([b.id for b in par.blocks[:4]])
    app.processEvents()
    files = export_onsets(par, str(tmp_path))
    assert any(f.endswith("_positive.txt") for f in files)
    lines = open([f for f in files if f.endswith("_positive.txt")][0]).read().splitlines()
    assert len(lines) == 8  # 4 pictures + 4 ratings in the stimulus track
    tl = win.timeline
    tl.doc.select([par.blocks[2].id])
    n = len(par.blocks)
    tl.duplicate_selected()
    assert len(win.doc.paradigm.blocks) == n + 1
    assert not any(win.doc.paradigm.has_overlap(b) for b in win.doc.paradigm.blocks)


def test_full_run_with_simulated_scanner(app, tmp_path):
    from PySide6.QtCore import QEventLoop, QTimer
    from PySide6.QtGui import QImage, QPainter

    from paradigmshift.controlroom import open_run
    from paradigmshift.model import Block, Paradigm

    par = Paradigm(name="quick", tr=0.25, dummy_scans=2, waiting_screen="text", waiting_text="Gleich geht's los")
    par.blocks += [Block.create("fixation", 0, 0.5), Block.create("text", 0.5, 0.5)]
    settings = {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": True, "fullscreen": False, "screen": 0}
    c, pw, cr = open_run(par, settings)
    seen = {}

    def probe():
        seen.setdefault(c.phase, (len(c.state.triggers), cr.pill.text()))
        if c.phase == "running" and "pressed" not in seen:   # a grip press 0.2 s into the fixation
            seen["pressed"] = True
            QTimer.singleShot(200, lambda: c.router.feed("b"))
        if c.phase == "dummy":   # participant screen shows only the waiting text, no trigger counts
            img = QImage(400, 225, QImage.Format_RGB32)
            p = QPainter(img)
            from PySide6.QtCore import QRectF
            c.paint_participant(p, QRectF(0, 0, 400, 225))
            p.end()
            seen["dummy_img"] = img
    poll = QTimer()
    poll.timeout.connect(probe)
    poll.start(20)
    loop = QEventLoop()
    got = []
    c.finished.connect(lambda files: (got.extend(files), loop.quit()))
    QTimer.singleShot(10000, loop.quit)
    loop.exec()
    poll.stop()
    app.processEvents()
    assert {"waiting", "dummy", "running"} <= set(seen)
    assert seen["dummy"][1].startswith("DUMMY SCANS")
    assert c.state.dummies_seen == 2 and c.state.log[0]["info"] == "dummy"
    assert got and all(os.path.exists(f) for f in got)
    rows = open(got[0]).read().splitlines()
    assert len(rows) == 3            # header + 2 blocks
    import csv
    presses = list(csv.DictReader(open([f for f in got if f.endswith("_buttons.tsv")][0]), delimiter="\t"))
    assert [(r["button"], r["block"]) for r in presses] == [("Left index", "Fixation")]
    assert cr.pill.text() == "✓ FINISHED" and cr.close_btn.isVisible()
    assert "Grip presses" in " ".join(lb.text() for lb in cr.findChildren(type(cr.pill)))
    cr.close()
    assert not pw.isVisible()


def test_control_room_abort_needs_confirmation(app, tmp_path):
    from paradigmshift.controlroom import open_run
    from paradigmshift.model import Block, Paradigm

    par = Paradigm(tr=2.0, dummy_scans=0)
    par.blocks.append(Block.create("fixation", 0, 60))
    c, pw, cr = open_run(par, {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": False,
                               "fullscreen": False, "screen": 0})
    c.router.feed(par.hardware.trigger_key)
    app.processEvents()
    assert c.phase == "running"
    c.request_abort()
    assert c.phase == "running"           # first press only arms
    cr.refresh()
    assert "again" in cr.banner.text()
    c.request_abort()
    assert c.phase == "done" and c.aborted and c.files[0].endswith("_ABORTED_events.tsv")
    cr.close()


def test_matching_choice_is_framed(app):
    from PySide6.QtCore import QRectF
    from PySide6.QtGui import QColor, QImage, QPainter

    from paradigmshift.model import Block, Paradigm
    from paradigmshift.stimulus import paint_stimulus

    par = Paradigm()
    b = Block.create("match", 0, 5, top="", left="", right="")

    def render(chosen):
        img = QImage(640, 360, QImage.Format_RGB32)
        p = QPainter(img)
        paint_stimulus(p, QRectF(0, 0, 640, 360), par, b, {"chosen": chosen} if chosen else {})
        p.end()
        return img

    frame = QColor(b.params["choice_color"]).rgb()
    count = lambda img, x0, x1: sum(img.pixel(x, y) == frame  # noqa: E731
                                    for x in range(x0, x1) for y in range(180, 360))
    plain, left = render(None), render("left")
    assert count(plain, 0, 320) == 0
    assert count(left, 0, 320) > 100 and count(left, 320, 640) == 0
    b.params["show_choice"] = False
    assert count(render("left"), 0, 320) == 0


def test_template_dialog_and_examples_in_editor(win, app, tmp_path):
    from paradigmshift.dialogs import TemplateDialog

    dlg = TemplateDialog(win.doc.paradigm.hardware, str(tmp_path), win)
    from paradigmshift.templates import TEMPLATES
    for key in [t.key for t in TEMPLATES]:
        dlg.select(key)
        app.processEvents()
        assert dlg.paradigm is not None and dlg.ok.isEnabled()
    dlg.select("gonogo")
    dlg.inputs["nogo_percent"].setValue(90)        # impossible -> error shown, cannot create
    assert dlg.paradigm is None and not dlg.ok.isEnabled()
    dlg.reject()
    ex = os.path.join(os.path.dirname(__file__), "..", "examples")
    for name in ("hariri_face_matching", "go_nogo", "emotion_regulation", "monetary_incentive_delay", "stroop",
                 "flanker", "n_back", "motor_localizer", "visual_checkerboard", "resting_state"):
        win.doc.dirty = False
        win.open_file(os.path.join(ex, name + ".paradigm.json"))
        seen = set()
        for b in win.doc.paradigm.blocks:
            if b.kind not in seen:
                seen.add(b.kind)
                win.doc.select([b.id])
                win.timeline.set_playhead(b.start + 0.1)
                app.processEvents()
        assert seen


def test_abort_freezes_clock(app, tmp_path):
    import time

    from paradigmshift.controlroom import open_run
    from paradigmshift.model import Block, Paradigm

    par = Paradigm(tr=2.0, dummy_scans=0)
    par.blocks.append(Block.create("fixation", 0, 60))
    c, pw, cr = open_run(par, {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": False,
                               "fullscreen": False, "screen": 0})
    c.router.feed(par.hardware.trigger_key)
    time.sleep(0.2)
    c.request_abort()
    c.request_abort()
    app.processEvents()
    t_abort = c.elapsed()
    assert 0.15 < t_abort < 2
    time.sleep(0.3)
    app.processEvents()
    assert c.elapsed() == t_abort                      # clock stopped
    assert not cr.timer.isActive()
    assert cr.pill.text().startswith("■ ABORTED at")
    assert "aborted" in cr.banner.text().lower() and cr.card_left.value.text() == "—"
    assert c.state.log[-1]["type"] == "abort"
    cr.close()


def test_mid_like_run_end_to_end(app, tmp_path):
    import csv

    from PySide6.QtCore import QEventLoop, QTimer

    from paradigmshift.controlroom import open_run
    from paradigmshift.model import Block, Paradigm

    par = Paradigm(tr=0.25, dummy_scans=0)
    par.blocks += [Block.create("fixation", 0, 0.5),
                   Block.create("choice", 0.5, 0.8, text="■", correct="any", stim_time=0),
                   Block.create("feedback", 1.3, 0.3, hit_text="+1", miss_text="+0"),
                   Block.create("choice", 1.6, 0.4, text="■", correct="any", stim_time=0),
                   Block.create("feedback", 2.0, 0.3, hit_text="+1", miss_text="+0")]
    c, pw, cr = open_run(par, {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": True,
                               "fullscreen": False, "screen": 0, "save_paradigm_with_data": False})
    got = []
    loop = QEventLoop()
    c.finished.connect(lambda f: (got.extend(f), loop.quit()))
    QTimer.singleShot(10000, loop.quit)
    # simulated scanner starts after 1.5 s; press during the first target only
    QTimer.singleShot(1500 + 900, lambda: c.router.feed("c"))
    loop.exec()
    rows = list(csv.DictReader(open(got[0]), delimiter="\t"))
    fb = [r for r in rows if r["block"] == "feedback"]
    assert [r["outcome"] for r in fb] == ["hit", "miss"] and [r["label"] for r in fb] == ["+1", "+0"]
    tg = [r for r in rows if r["block"] == "choice"]
    assert [r["outcome"] for r in tg] == ["hit", "miss"]
    cr.close()
