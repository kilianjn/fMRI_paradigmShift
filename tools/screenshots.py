"""Render screenshots of the app into docs/screenshots (works headless).

    QT_QPA_PLATFORM=offscreen python tools/screenshots.py
"""
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, ROOT)

from PySide6.QtCore import QTimer  # noqa: E402
from PySide6.QtWidgets import QApplication  # noqa: E402

from paradigmshift import theme  # noqa: E402
from paradigmshift.dialogs import HardwareDialog, QuickDesignDialog, RunDialog  # noqa: E402
from paradigmshift.hardware import now  # noqa: E402
from paradigmshift.mainwindow import MainWindow  # noqa: E402
from paradigmshift.runner import PresentationWindow  # noqa: E402

OUT = os.path.join(ROOT, "docs", "screenshots")
EXAMPLE = os.path.join(ROOT, "examples", "emotional_rating.paradigm.json")


def settle(app, n=5):
    for _ in range(n):
        app.processEvents()


def shot(widget, name):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, name)
    widget.grab().save(path)
    print("saved", path)


def main():
    app = QApplication(sys.argv)
    app.setStyle("Fusion")
    app.setStyleSheet(theme.STYLESHEET)

    # 1 – empty editor, first launch
    win = MainWindow()
    win.resize(1600, 960)
    win.show()
    settle(app)
    shot(win, "01_empty_start.png")

    # 2 – example paradigm, rating block selected, playhead on the rating
    win.doc.dirty = False
    win.open_file(EXAMPLE)
    settle(app)
    win.timeline.zoom_fit()
    par = win.doc.paradigm
    rating = [b for b in par.blocks if b.kind == "rating"][1]
    win.doc.select([rating.id])
    win.timeline.set_playhead(rating.start + 1.3)
    settle(app)
    shot(win, "02_editor_rating_selected.png")

    # 3 – image block selected, zoomed in
    img = [b for b in par.blocks if b.kind == "image"][2]
    win.doc.select([img.id])
    win.timeline.set_zoom(40)
    win.timeline.horizontalScrollBar().setValue(int((img.start - 14) * 40))
    win.timeline.set_playhead(img.start + 2)
    settle(app)
    shot(win, "03_editor_image_zoomed.png")

    # 4 – nothing selected: paradigm settings in the inspector
    win.doc.select([])
    win.timeline.zoom_fit()
    win.timeline.set_playhead(3)
    settle(app)
    shot(win, "04_editor_paradigm_settings.png")

    # 5 – hardware dialog with some live activity
    hw = HardwareDialog(par.hardware, par.tr, win)
    hw.show()
    settle(app)
    t = now()
    for i in range(6):
        hw._on_trigger(t + i * 2.001)
    hw.trig_lamp.level = 1.0
    settle(app)
    shot(hw, "05_hardware_syncbox.png")
    from PySide6.QtWidgets import QTabWidget
    tabs = hw.findChild(QTabWidget)
    tabs.setCurrentIndex(1)
    hw._on_button("Right index", now())
    hw._rows[2][0].level = 1.0
    settle(app)
    shot(hw, "06_hardware_grips.png")
    hw.reject()

    # 7 – quick design wizard
    qd = QuickDesignDialog(par, win.library.media.files(), win)
    qd.show()
    pos = [f for f in win.library.media.files() if "pos_" in f]
    neg = [f for f in win.library.media.files() if "neg_" in f]
    qd.cond_table.setRowCount(0)
    qd._conds.clear()
    qd._add_condition({"name": "positive", "kind": "image", "images": pos})
    qd._add_condition({"name": "negative", "kind": "image", "images": neg})
    settle(app)
    shot(qd, "07_quick_design.png")
    qd.reject()

    # 8 – run dialog
    rd = RunDialog(par, os.path.expanduser("~/ParadigmShift-data"), win)
    rd.show()
    settle(app)
    shot(rd, "08_run_dialog.png")
    rd.reject()

    # 9/10 – participant screen: waiting for scanner, then a rating trial
    pw = PresentationWindow(par.copy(), {"sub": "01", "run": 1, "out_dir": "/tmp", "simulate": False,
                                         "fullscreen": False, "screen": 0})
    pw.resize(1280, 720)
    pw.show()
    pw.state.triggers = [0.0, 2.0]
    settle(app)
    shot(pw, "09_participant_waiting.png")
    pw.phase = "running"
    pw.current = [b for b in pw.p.blocks if b.kind == "rating"][0]
    pw.state.t0 = now() - pw.current.start - 1
    pw.state.onset(pw.current, now())
    pw.state.ratings[pw.current.id]["value"] = 3
    settle(app)
    shot(pw, "10_participant_rating.png")
    pw.phase = "done"
    pw.close()
    win.doc.dirty = False
    QTimer.singleShot(0, app.quit)
    app.exec()


if __name__ == "__main__":
    main()
