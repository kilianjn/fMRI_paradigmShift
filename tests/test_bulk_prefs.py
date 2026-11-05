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
    par.block(ratings[0]).params["mode"] = "cursor"           # make them differ
    win.doc.select(ratings)
    app.processEvents()
    lab, combo = _combo_for(win.inspector, "Answer with")
    assert isinstance(combo, QComboBox) and lab.text().endswith("≠") and combo.currentIndex() == -1
    combo.activated.emit(combo.findData("cursor"))
    app.processEvents()
    assert all(win.doc.paradigm.block(i).params["mode"] == "cursor" for i in ratings)
    win.doc.undo()                                            # one undo step reverts all
    assert sum(win.doc.paradigm.block(i).params["mode"] == "cursor" for i in ratings) == 1


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
    win._set_pref("rating_mode", "cursor")
    win._set_pref("new_tr", 1.5)
    b = win.timeline.add_block("rating")
    assert b.params["mode"] == "cursor"
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
    dlg.w["rating_mode"].setCurrentIndex(dlg.w["rating_mode"].findData("cursor"))
    dlg.accept()
    assert win.prefs.get("watchdog_trials") == 8 and win.prefs.get("rating_mode") == "cursor"


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
