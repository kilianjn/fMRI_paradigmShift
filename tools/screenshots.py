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
from paradigmshift.model import Paradigm  # noqa: E402
from paradigmshift.controlroom import ControlRoomWindow  # noqa: E402
from paradigmshift.runner import PresentationWindow, RunController  # noqa: E402

SETTINGS = {"sub": "01", "run": 1, "out_dir": "/tmp/paradigmshift-shots", "simulate": False,
            "fullscreen": False, "screen": 0}


def fake_run(par, at: float, answer, seed=3):
    """A controller frozen *at* seconds into the run, with simulated triggers and grip answers."""
    import random
    rng = random.Random(seed)
    c = RunController(par, dict(SETTINGS))
    t0 = now() - at
    tr = par.tr
    for i in range(par.dummy_scans):
        c.state.on_trigger(t0 - (par.dummy_scans - i) * tr)
    k = 0
    while k * tr <= at:
        c.state.on_trigger(t0 + k * tr + rng.uniform(-.001, .001))
        k += 1
    for b in sorted(par.blocks, key=lambda b: b.start):
        if b.start <= at:
            c.state.onset(b, t0 + b.start + .002)
            press = answer(b, rng)
            if press and b.start + press[1] < min(at, b.end):
                c.state.on_button(press[0], t0 + b.start + press[1])
    c.current = par.stimulus_at(at)
    return c


def hariri_answer(b, rng):
    if b.kind != "match":
        return None
    if rng.random() < .08:
        return None
    side = b.params["correct"] if rng.random() < .9 else ("left" if b.params["correct"] == "right" else "right")
    return (b.params["left_button"] if side == "left" else b.params["right_button"]), rng.uniform(.7, 1.6)


def gonogo_answer(b, rng):
    if b.kind != "gonogo":
        return None
    if b.params["trial"] == "go":
        return ("Right index", rng.uniform(.3, .55)) if rng.random() < .95 else None
    return ("Right index", rng.uniform(.3, .45)) if rng.random() < .2 else None

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
    win.prefs.reset()
    win.apply_prefs()
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

    # 9 – participant screen during dummy scans: only a fixation cross, no technical text
    wp = par.copy()
    wp.dummy_scans = 4
    c = RunController(wp, dict(SETTINGS))
    c.state.on_trigger(now() - 2)
    c.state.on_trigger(now())
    pw = PresentationWindow(c)
    pw.resize(1280, 720)
    pw.show()
    settle(app)
    shot(pw, "09_participant_waiting.png")
    cr = ControlRoomWindow(c, None)
    cr.show()
    cr.refresh()
    settle(app)
    shot(cr, "17_control_room_dummy_scans.png")
    cr.timer.stop()
    cr.hide()
    pw.hide()

    # 10 – participant screen: rating trial
    rating = [b for b in par.blocks if b.kind == "rating"][0]
    c = fake_run(par.copy(), rating.start + 1, lambda b, r: None)
    c.state.ratings[c.current.id]["value"] = 3
    pw = PresentationWindow(c)
    pw.resize(1280, 720)
    pw.show()
    settle(app)
    shot(pw, "10_participant_rating.png")
    pw.hide()

    # 11 – template dialog
    from paradigmshift.dialogs import TemplateDialog
    td = TemplateDialog(par.hardware, os.path.join(ROOT, "examples", "stimuli"), win)
    td.show()
    td.select("hariri")
    settle(app)
    shot(td, "11_templates_hariri.png")
    td.select("emoreg")
    settle(app)
    shot(td, "12_templates_emoreg.png")
    td.reject()

    ex = os.path.join(ROOT, "examples")

    # 13 – Hariri in the editor
    win.doc.dirty = False
    win.open_file(os.path.join(ex, "hariri_face_matching.paradigm.json"))
    settle(app)
    m = [b for b in win.doc.paradigm.blocks if b.kind == "match" and b.condition == "faces"][1]
    win.doc.select([m.id])
    win.timeline.set_zoom(14)
    win.timeline.horizontalScrollBar().setValue(0)
    win.timeline.set_playhead(m.start + 2)
    settle(app)
    shot(win, "13_hariri_editor.png")

    # 14 – Go/No-Go, zoomed in on a No-Go trial
    win.doc.dirty = False
    win.open_file(os.path.join(ex, "go_nogo.paradigm.json"))
    settle(app)
    ng = [b for b in win.doc.paradigm.blocks if b.kind == "gonogo" and b.params["trial"] == "nogo"][1]
    win.doc.select([ng.id])
    win.timeline.set_zoom(55)
    win.timeline.horizontalScrollBar().setValue(int((ng.start - 8) * 55))
    win.timeline.set_playhead(ng.start + 0.2)
    settle(app)
    shot(win, "14_gonogo_editor.png")

    # 15 – emotion regulation
    win.doc.dirty = False
    win.open_file(os.path.join(ex, "emotion_regulation.paradigm.json"))
    settle(app)
    pic = [b for b in win.doc.paradigm.blocks if b.condition == "decrease_negative"][0]
    win.doc.select([pic.id])
    win.timeline.set_zoom(16)
    win.timeline.horizontalScrollBar().setValue(int(max(0, pic.start - 30) * 16))
    win.timeline.set_playhead(pic.start + 2)
    settle(app)
    shot(win, "15_emoreg_editor.png")

    # 16 – participant screen: Hariri faces trial, the chosen face is framed
    hp = Paradigm.load(os.path.join(ex, "hariri_face_matching.paradigm.json"))
    trial = [b for b in hp.blocks if b.kind == "match" and b.condition == "faces"][0]
    c = fake_run(hp, trial.start + 1.5, lambda b, r: (b.params["left_button"], 1.1) if b is trial else None)
    pw = PresentationWindow(c)
    pw.resize(1280, 720)
    pw.show()
    settle(app)
    shot(pw, "16_participant_hariri.png")
    pw.hide()

    # 18/19 – control room in the middle of a run
    hp = Paradigm.load(os.path.join(ex, "hariri_face_matching.paradigm.json"))
    c = fake_run(hp, 101.3, hariri_answer)
    cr = ControlRoomWindow(c, None)
    cr.show()
    cr.refresh()
    settle(app)
    shot(cr, "18_control_room_hariri.png")
    cr.timer.stop()
    cr.hide()
    gp = Paradigm.load(os.path.join(ex, "go_nogo.paradigm.json"))
    c = fake_run(gp, 143.2, gonogo_answer)
    cr = ControlRoomWindow(c, None)
    cr.show()
    cr.refresh()
    settle(app)
    shot(cr, "19_control_room_gonogo.png")
    cr.timer.stop()
    cr.hide()

    # 20 – bulk editing: all rating blocks selected, answer mode differs
    win.doc.dirty = False
    win.open_file(EXAMPLE)
    settle(app)
    par = win.doc.paradigm
    ratings = [b.id for b in par.blocks if b.kind == "rating"]
    for i in ratings[:3]:
        par.block(i).params["mode"] = "cursor"
    win.doc.select(ratings)
    win.timeline.zoom_fit()
    settle(app)
    shot(win, "20_bulk_edit_ratings.png")

    # 21 – find and replace
    from paradigmshift.dialogs import FindReplaceDialog, PreferencesDialog
    fr = FindReplaceDialog(win.doc, win)
    fr.show()
    fr.find.setText("feel")
    fr.repl.setText("feel right now")
    settle(app)
    shot(fr, "21_find_replace.png")
    fr.reject()

    # 22 – preferences
    pd = PreferencesDialog(win.prefs, win)
    pd.show()
    from PySide6.QtWidgets import QTabWidget
    pd.findChild(QTabWidget).setCurrentIndex(1)
    settle(app)
    shot(pd, "22_preferences.png")
    pd.reject()

    # 23 – control room after an abort: the clock is frozen
    hp = Paradigm.load(os.path.join(ex, "hariri_face_matching.paradigm.json"))
    c = fake_run(hp, 117.6, hariri_answer)
    c._begin = lambda: None
    c.settings["out_dir"] = "/tmp/paradigmshift-shots"
    cr = ControlRoomWindow(c, None)
    cr.show()
    c.finish(aborted=True)
    settle(app)
    shot(cr, "23_control_room_aborted.png")
    cr.hide()

    win.doc.dirty = False
    QTimer.singleShot(0, app.quit)
    app.exec()


if __name__ == "__main__":
    main()
