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

    from paradigmshift.model import Block, Paradigm
    from paradigmshift.runner import PresentationWindow

    par = Paradigm(name="quick", tr=0.25, dummy_scans=1)
    par.blocks += [Block.create("fixation", 0, 0.5), Block.create("text", 0.5, 0.5),
                   Block.create("response", 0, 1.0)]
    win = PresentationWindow(par, {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": True,
                                   "fullscreen": False, "screen": 0})
    loop = QEventLoop()
    got = []
    win.finished.connect(lambda files: (got.extend(files), loop.quit()))
    QTimer.singleShot(10000, loop.quit)
    win.start()
    QTimer.singleShot(1900, lambda: win.router.feed("2"))   # a grip press during the response window
    loop.exec()
    win.close()
    assert got and all(os.path.exists(f) for f in got)
    rows = open(got[0]).read().splitlines()
    assert len(rows) == 4            # header + 3 blocks
    assert "Left index" in open(got[0]).read()
