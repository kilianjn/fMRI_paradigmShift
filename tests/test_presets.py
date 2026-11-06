"""Task blocks from the library (presets.py)."""
import os

import pytest

QtWidgets = pytest.importorskip("PySide6.QtWidgets")

from paradigmshift.model import Block, Paradigm  # noqa: E402
from paradigmshift.presets import PRESET_BY_KEY, PRESETS, derive  # noqa: E402


@pytest.fixture(scope="module")
def app():
    return QtWidgets.QApplication.instance() or QtWidgets.QApplication([])


def test_every_preset_and_variant_builds(app, tmp_path):
    for pr in PRESETS:
        for _label, v in (pr.variants or [(None, None)]):
            blocks = pr.build(10.0, v, {"stim_dir": str(tmp_path), "seed": 1})
            par = Paradigm()
            par.blocks += blocks
            assert blocks and min(b.start for b in blocks) == 10.0, pr.key
            assert not any(par.has_overlap(b) for b in blocks), pr.key
            assert [p for p in par.validate() if "not found" in p or "nothing" in p] == [], pr.key


def test_stroop_trial_derives_everything():
    b = PRESET_BY_KEY["stroop"].build(0)[0]
    assert (b.params["text"], b.condition, b.params["correct"]) == ("RED", "incongruent", "Right index")
    b.params["word"] = "blue"
    derive(b)
    assert b.condition == "congruent" and b.params["color"] == "#1E88E5"
    b.params["word"] = "neutral"
    derive(b)
    assert b.params["text"] == "XXXX" and b.condition == "neutral"


def test_flanker_and_nback_trials_derive():
    f = PRESET_BY_KEY["flanker"].build(0)[0]
    assert f.params["text"] == "→→←→→" and f.params["correct"] == "Left index"
    f.params["direction"], f.params["congruency"] = "right", "congruent"
    derive(f)
    assert f.params["text"] == "→→→→→" and f.params["correct"] == "Right index" and f.condition == "congruent"
    n = PRESET_BY_KEY["nback_letter"].build(0)[0]
    assert n.params["correct"] == ""
    n.params["target"] = True
    derive(n)
    assert n.params["correct"] == "Right index" and n.label.endswith("(target)") and n.condition == "2back"


def test_insert_task_sequence_in_editor(app, tmp_path):
    from paradigmshift.mainwindow import MainWindow
    win = MainWindow()
    win.timeline.stim_dir = str(tmp_path)
    win.timeline.add_block("fixation")                    # 0–10 s
    blocks = win.timeline.add_preset("mid_trial", start=4.0, variant="loss", ask=False)
    par = win.doc.paradigm
    assert [b.kind for b in blocks] == ["image", "fixation", "choice", "feedback", "fixation"]
    assert blocks[0].start == 10.0                        # moved behind the fixation it was dropped into
    assert blocks[3].params["miss_text"].startswith("−")
    assert not any(par.has_overlap(b) for b in par.blocks)
    tap = win.timeline.add_preset("tapping", variant="right", ask=False)
    assert {b.track for b in tap} == {0} and all(b.condition == "tap_right" for b in tap)
    win.doc.undo()
    assert not any(b.condition == "tap_right" for b in win.doc.paradigm.blocks)
    win.doc.dirty = False
    win.close()


def test_task_fields_in_inspector_update_trials(app, tmp_path):
    from paradigmshift.mainwindow import MainWindow
    win = MainWindow()
    win.show()
    blocks = win.timeline.add_preset("nback_block", start=0, variant=2, ask=False)
    letters = [b for b in blocks if b.kind == "choice"]
    win.doc.select([b.id for b in letters])
    app.processEvents()
    form = win.inspector.scroll.widget().layout()
    combo = None
    for row in range(form.rowCount()):
        lab = form.itemAt(row, form.ItemRole.LabelRole)
        if lab and lab.widget() and lab.widget().text().startswith("Target?"):
            combo = form.itemAt(row, form.ItemRole.FieldRole).widget()
    assert combo is not None and combo.currentIndex() == -1        # mixed: some targets, some not
    combo.activated.emit(combo.findData(False))
    assert all(win.doc.paradigm.block(b.id).params["correct"] == "" for b in letters)
    win.doc.dirty = False
    win.close()


def test_template_trials_are_task_trials(app, tmp_path):
    from paradigmshift import templates as T
    par = T.build_stroop(T.StroopOptions(n_trials=12))
    trials = [b for b in par.blocks if b.kind == "choice"]
    assert all(b.params["preset"] == "stroop" for b in trials)
    b = trials[0]
    b.params["ink"] = "yellow"
    derive(b)
    assert b.params["correct"] == T.StroopOptions().yellow_button


def test_library_has_task_tab(app):
    from paradigmshift.panels import BlockCard, LibraryPanel
    lib = LibraryPanel()
    keys = [c.kind for c in lib.findChildren(BlockCard)]
    assert all("preset:" + p.key in keys for p in PRESETS)
    assert lib.tabs.tabText(1) == "Task blocks"
