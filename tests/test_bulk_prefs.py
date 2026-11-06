"""Bulk editing in the inspector, find & replace, and preferences."""
import os

import pytest

QtWidgets = pytest.importorskip("PySide6.QtWidgets")
from PySide6.QtWidgets import QComboBox, QLineEdit  # noqa: E402

from paradigmshift import model  # noqa: E402
from paradigmshift.mainwindow import MainWindow  # noqa: E402
from paradigmshift.model import Block, Paradigm, find_text, replace_text  # noqa: E402

EX = os.path.join(os.path.dirname(__file__), "..", "examples")


@pytest.fixture(scope="module")
def app():
    return QtWidgets.QApplication.instance() or QtWidgets.QApplication([])


@pytest.fixture
def win(app):
    w = MainWindow()
    w.prefs.reset()
    w.apply_prefs()
    w.show()
    w.open_file(os.path.join(EX, "emotional_rating.paradigm.json"))
    app.processEvents()
    yield w
    w.doc.dirty = False
    w.prefs.reset()
    model.PARAM_OVERRIDES.clear()
    w.close()


def _combo_for(inspector, label_text):
    form = inspector.scroll.widget().layout()
    for row in range(form.rowCount()):
        lab = form.itemAt(row, form.ItemRole.LabelRole)
        fld = form.itemAt(row, form.ItemRole.FieldRole)
        if lab and fld and lab.widget() and lab.widget().text().startswith(label_text):
            return lab.widget(), fld.widget()
    raise AssertionError(f"no row {label_text!r}")


def test_change_rating_mode_of_all_ratings_at_once(win, app):
    par = win.doc.paradigm
    ratings = [b.id for b in par.blocks if b.kind == "rating"]
    par.block(ratings[0]).params["mode"] = "direct"           # make them differ (default is cursor)
    win.doc.select(ratings)
    app.processEvents()
    lab, combo = _combo_for(win.inspector, "Answer with")
    assert isinstance(combo, QComboBox) and lab.text().endswith("≠") and combo.currentIndex() == -1
    combo.activated.emit(combo.findData("direct"))
    app.processEvents()
    assert all(win.doc.paradigm.block(i).params["mode"] == "direct" for i in ratings)
    win.doc.undo()                                            # one undo step reverts all
    assert sum(win.doc.paradigm.block(i).params["mode"] == "direct" for i in ratings) == 1


def test_untouched_mixed_text_is_not_overwritten(win, app):
    par = win.doc.paradigm
    ratings = [b.id for b in par.blocks if b.kind == "rating"]
    par.block(ratings[0]).params["question"] = "Something else?"
    win.doc.select(ratings)
    app.processEvents()
    _lab, line = _combo_for(win.inspector, "Question")
    assert isinstance(line, QLineEdit) and line.text() == ""
    line.editingFinished.emit()                               # focus left without typing
    assert par.block(ratings[0]).params["question"] == "Something else?"
    line.setText("How do you feel now?")
    line.editingFinished.emit()
    assert {win.doc.paradigm.block(i).params["question"] for i in ratings} == {"How do you feel now?"}


def test_mixed_selection_offers_type_filter(win, app):
    win.doc.select([b.id for b in win.doc.paradigm.blocks])
    app.processEvents()
    texts = [b.text() for b in win.inspector.scroll.widget().findChildren(QtWidgets.QPushButton)]
    assert any("× Rating" in t for t in texts)


def test_find_and_replace_text():
    par = Paradigm()
    par.blocks += [Block.create("text", 0, 2, text="Press the BUTTON"),
                   Block.create("rating", 2, 2, question="Which button?"),
                   Block.create("fixation", 4, 2, label="button rest")]
    assert len(find_text(par, "button")) == 3
    assert len(find_text(par, "button", case=True)) == 2
    assert replace_text(par, "button", "grip", fields=["text", "question"]) == 2
    assert par.blocks[0].params["text"] == "Press the grip" and par.blocks[1].params["question"] == "Which grip?"
    assert par.blocks[2].label == "button rest"


def test_preferences_drive_new_blocks_and_paradigms(win, app):
    assert win.timeline.add_block("rating").params["mode"] == "cursor"   # the default
    win._set_pref("rating_mode", "direct")
    win._set_pref("new_tr", 1.5)
    b = win.timeline.add_block("rating")
    assert b.params["mode"] == "direct"
    win.doc.dirty = False
    win.new_file()
    assert win.doc.paradigm.tr == 1.5
    win._set_pref("show_hints", False)
    assert not win.tl_hint.isVisible()
    win._set_pref("snap_px", 25)
    assert win.timeline.snap_px == 25


def test_preferences_dialog_roundtrip(win, app):
    from paradigmshift.dialogs import PreferencesDialog
    dlg = PreferencesDialog(win.prefs, win)
    dlg.w["watchdog_trials"].setValue(8)
    dlg.w["rating_mode"].setCurrentIndex(dlg.w["rating_mode"].findData("direct"))
    dlg.accept()
    assert win.prefs.get("watchdog_trials") == 8 and win.prefs.get("rating_mode") == "direct"


def test_abort_without_confirmation_and_paradigm_copy(app, tmp_path):
    from paradigmshift.controlroom import open_run
    par = Paradigm(tr=2.0, dummy_scans=0)
    par.blocks.append(Block.create("fixation", 0, 60))
    c, pw, cr = open_run(par, {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": False,
                               "fullscreen": False, "screen": 0, "confirm_abort": False,
                               "save_paradigm_with_data": True})
    c.router.feed(par.hardware.trigger_key)
    c.request_abort()
    assert c.phase == "done" and c.aborted
    assert any(f.endswith("_paradigm.json") for f in c.files)
    assert Paradigm.load([f for f in c.files if f.endswith("_paradigm.json")][0]).blocks
    cr.close()


def test_mixed_conditions_survive_focus_out(win, app):
    par = win.doc.paradigm
    ratings = [b.id for b in par.blocks if b.kind == "rating"]
    before = [par.block(i).condition for i in ratings]
    assert len(set(before)) > 1
    win.doc.select(ratings)
    app.processEvents()
    _lab, cond = _combo_for(win.inspector, "Condition")
    cond.lineEdit().editingFinished.emit()
    assert [win.doc.paradigm.block(i).condition for i in ratings] == before


def test_blur_previews(win, app, tmp_path):
    from PySide6.QtGui import QColor, QImage

    from paradigmshift import stimulus
    img = QImage(200, 100, QImage.Format_RGB32)
    for x in range(200):
        for y in range(100):
            img.setPixelColor(x, y, QColor("#000000") if (x // 10 + y // 10) % 2 else QColor("#FFFFFF"))
    f = str(tmp_path / "check.png")
    img.save(f)
    sharp = stimulus.thumbnail(f, 100, blur=False).toImage()
    soft = stimulus.thumbnail(f, 100, blur=True).toImage()
    spread = lambda im: max(QColor(im.pixel(x, 50)).lightness() for x in range(20, 180)) - \
        min(QColor(im.pixel(x, 50)).lightness() for x in range(20, 180))  # noqa: E731
    assert spread(sharp) > 200 and spread(soft) < 120
    win._set_pref("blur_previews", True)
    assert stimulus.EDITOR_BLUR and win.a_blur.isChecked()
    win._set_pref("blur_previews", False)
    assert not stimulus.EDITOR_BLUR


def test_participant_screen_is_never_blurred(app, tmp_path):
    from PySide6.QtCore import QRectF
    from PySide6.QtGui import QColor, QImage, QPainter

    from paradigmshift import stimulus
    from paradigmshift.runner import RunController
    img = QImage(400, 225, QImage.Format_RGB32)
    for x in range(400):
        for y in range(225):
            img.setPixelColor(x, y, QColor("#000000") if (x // 20) % 2 else QColor("#FFFFFF"))
    f = str(tmp_path / "stripes.png")
    img.save(f)
    par = Paradigm(dummy_scans=0)
    par.blocks.append(Block.create("image", 0, 10, path=f, scale=1.0))
    stimulus.EDITOR_BLUR = True
    try:
        c = RunController(par, {"out_dir": str(tmp_path)})
        c.state.force_start(0.0)
        c.current = par.blocks[0]

        def render(blur):
            out = QImage(400, 225, QImage.Format_RGB32)
            p = QPainter(out)
            c.paint_participant(p, QRectF(0, 0, 400, 225), blur=blur)
            p.end()
            return {QColor(out.pixel(x, 112)).lightness() for x in range(40, 360)}
        assert min(render(False)) < 20 and max(render(False)) > 235      # crisp stripes
        soft = render(True)                                               # the control-room mirror can blur
        assert max(soft) - min(soft) < 150
    finally:
        stimulus.EDITOR_BLUR = False


def test_control_room_sections_fold_and_remember(app, tmp_path):
    from paradigmshift.controlroom import ControlRoomWindow
    from paradigmshift.runner import RunController
    par = Paradigm(dummy_scans=0)
    par.blocks.append(Block.create("fixation", 0, 10))
    c = RunController(par, {"out_dir": str(tmp_path)})
    cr = ControlRoomWindow(c)
    cr.show()
    assert set(cr.sections) == {"status", "progress", "mirror", "nownext", "perf", "answers"}
    cr.sections["mirror"].set_folded(True)
    cr.sections["status"].set_folded(True)
    cr.refresh()
    app.processEvents()
    assert not cr.mirror.isVisible() and cr.sections["mirror"].summary.isVisible()
    assert "triggers" in cr.sections["status"].summary.text()
    cr.timer.stop()
    cr.hide()
    cr2 = ControlRoomWindow(RunController(par, {"out_dir": str(tmp_path)}))
    assert cr2.sections["mirror"].folded and cr2.sections["status"].folded and not cr2.sections["answers"].folded
    for sec in cr2.sections.values():
        sec.set_folded(False)
    cr2.timer.stop()
