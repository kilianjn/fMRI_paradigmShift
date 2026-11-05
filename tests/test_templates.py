import os
import random

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
QtGui = pytest.importorskip("PySide6.QtGui")

from paradigmshift import templates as T  # noqa: E402
from paradigmshift.model import Block, Paradigm  # noqa: E402
from paradigmshift.runner import RunState, performance_summary  # noqa: E402


@pytest.fixture(scope="module", autouse=True)
def app():
    return QtGui.QGuiApplication.instance() or QtGui.QGuiApplication([])


@pytest.fixture(scope="module")
def stim(tmp_path_factory):
    return str(tmp_path_factory.mktemp("stim"))


def no_overlaps(par):
    return not any(par.has_overlap(b) for b in par.blocks)


def test_hariri_structure(stim):
    par = T.build("hariri", T.HaririOptions(), stim)
    assert par.duration == 9 * (2 + 6 * 5) and no_overlaps(par) and par.validate() == []
    match = [b for b in par.blocks if b.kind == "match"]
    assert len(match) == 54
    # blocks alternate shapes / faces, starting and ending with shapes
    order = [b.condition for b in match[::6]]
    assert order == ["shapes", "faces"] * 4 + ["shapes"]
    for b in match:
        pr = b.params
        correct, foil = (pr["left"], pr["right"]) if pr["correct"] == "left" else (pr["right"], pr["left"])
        if b.condition == "faces":
            emo = lambda f: "angry" if "angry" in f else "fearful"  # noqa: E731
            assert emo(correct) == emo(pr["top"]) != emo(foil)
            assert correct != pr["top"]          # same emotion, different person
        else:
            assert correct == pr["top"] != foil
    sides = [b.params["correct"] for b in match]
    assert sides.count("left") == sides.count("right")


def test_hariri_user_faces_folder(tmp_path, stim):
    for name in ("01F_AN_C.png", "02M_AN_O.png", "01F_FE_C.png", "02M_FE_O.png", "01F_HA_C.png"):
        (tmp_path / name).write_bytes(b"x")
    faces = T.faces_from_folder(str(tmp_path))
    assert len(faces["angry"]) == 2 and len(faces["fearful"]) == 2
    with pytest.raises(ValueError):
        T.faces_from_folder(str(stim))   # placeholder dir root holds no faces


def test_gonogo_sequence_constraints():
    rng = random.Random(4)
    seq = T.gonogo_sequence(200, 50, rng)
    assert len(seq) == 200 and seq.count("nogo") == 50
    assert seq[:3] == ["go"] * 3
    assert all(not (a == b == "nogo") for a, b in zip(seq, seq[1:]))
    with pytest.raises(ValueError):
        T.gonogo_sequence(10, 8, rng)


def test_gonogo_template():
    par = T.build_gonogo(T.GoNoGoOptions(n_trials=80, rest_every=40))
    trials = [b for b in par.blocks if b.kind == "gonogo"]
    assert len(trials) == 80 and sum(b.params["trial"] == "nogo" for b in trials) == 20
    assert all(b.params["text"] == "X" for b in trials if b.params["trial"] == "nogo")
    assert all(b.params["text"] != "X" for b in trials if b.params["trial"] == "go")
    assert no_overlaps(par) and par.validate() == []


def test_emoreg_template(stim):
    par = T.build("emoreg", T.EmoRegOptions(trials_per_condition=6, include_increase=True), stim)
    pics = [b for b in par.blocks if b.kind == "image"]
    conds = [b.condition for b in pics]
    assert {c: conds.count(c) for c in set(conds)} == {
        "look_neutral": 6, "look_negative": 6, "decrease_negative": 6, "increase_negative": 6}
    # no condition three times in a row
    assert all(len(set(conds[i:i + 3])) > 1 for i in range(len(conds) - 2))
    # every picture is preceded by its cue and followed by a rating
    for b in pics:
        i = par.blocks.index(b)
        assert par.blocks[i - 1].params["text"] == T.EMOREG_CUES[b.condition]
        assert par.blocks[i + 1].kind == "rating"
    neg = [b.params["path"] for b in pics if b.condition != "look_neutral"]
    assert len(set(neg)) == len(neg)       # no negative picture is repeated
    assert no_overlaps(par) and par.validate() == []


def _started(par):
    s = RunState(par)
    for _ in range(par.dummy_scans + 1):
        s.on_trigger(0.0)
    return s


def test_runner_scores_matching_and_gonogo(tmp_path):
    par = Paradigm(dummy_scans=0)
    m = Block.create("match", 0, 5, top="a", left="b", right="c", correct="right")
    go = Block.create("gonogo", 5, 1.5, trial="go")
    nogo = Block.create("gonogo", 6.5, 1.5, trial="nogo")
    go2 = Block.create("gonogo", 8, 1.5, trial="go")
    nogo2 = Block.create("gonogo", 9.5, 1.5, trial="nogo")
    par.blocks += [m, go, nogo, go2, nogo2]
    s = _started(par)
    for b in par.blocks:
        s.onset(b, b.start)
    s.on_button("Left thumb", 1.0)        # not a left/right answer button -> ignored
    s.on_button("Right index", 1.2)
    s.on_button("Left index", 1.4)        # only the first answer counts
    s.on_button("Right thumb", 5.4)       # go -> hit
    s.on_button("Right thumb", 6.9)       # nogo -> false alarm
    s.write(str(tmp_path), "x")
    e = s.events
    assert e[m.id]["response"] == "right" and e[m.id]["correct"] == 1 and e[m.id]["response_time"] == 1.2
    assert e[go.id]["outcome"] == "hit" and e[go.id]["response_time"] == 0.4
    assert e[nogo.id]["outcome"] == "false_alarm" and e[nogo.id]["correct"] == 0
    assert e[go2.id]["outcome"] == "miss"
    assert e[nogo2.id]["outcome"] == "correct_rejection" and e[nogo2.id]["correct"] == 1
    summary = " ".join(performance_summary(s))
    assert "1 hits · 1 misses · 1 false alarms · 1 correct rejections" in summary and "1/1 correct" in summary


def test_example_projects_load():
    ex = os.path.join(os.path.dirname(__file__), "..", "examples")
    for name in ("hariri_face_matching", "go_nogo", "emotion_regulation", "monetary_incentive_delay", "stroop",
                 "flanker", "n_back", "motor_localizer", "visual_checkerboard", "resting_state"):
        par = Paradigm.load(os.path.join(ex, name + ".paradigm.json"))
        assert par.blocks and par.validate() == [], name


# ---------------------------------------------------------------- more standard paradigms
def test_every_template_builds_cleanly(stim):
    for t in T.TEMPLATES:
        par = T.build(t.key, t.options(), stim)
        assert par.blocks and no_overlaps(par) and par.validate() == [], t.key
        assert t.category in T.CATEGORIES, t.key


def test_nback_targets_match_n_back():
    rng = random.Random(2)
    for n in (1, 2, 3):
        seq = T.nback_sequence(n, 20, 6, "BCDFG", rng)
        for i, (letter, target) in enumerate(seq):
            if i >= n:
                assert target == (letter == seq[i - n][0])
        assert sum(t for _, t in seq) == 6
    zero = T.nback_sequence(0, 15, 5, "BCDF", rng, "X")
    assert all(t == (letter == "X") for letter, t in zero)
    par = T.build_nback(T.NBackOptions(levels="0, 1, 2", blocks_per_level=2, trials_per_block=12))
    trials = [b for b in par.blocks if b.kind == "choice"]
    assert {b.condition for b in trials} == {"0back", "1back", "2back"}
    assert all((b.params["correct"] != "") == b.label.endswith("(target)") for b in trials)


def test_stroop_and_flanker_correct_answers():
    o = T.StroopOptions(n_trials=40, words="ROT, GRÜN, BLAU, GELB")
    par = T.build_stroop(o)
    btn = {"red": o.red_button, "green": o.green_button, "blue": o.blue_button, "yellow": o.yellow_button}
    word = dict(zip(["red", "green", "blue", "yellow"], ["ROT", "GRÜN", "BLAU", "GELB"]))
    for b in (x for x in par.blocks if x.kind == "choice"):
        ink = next(c for c, h in T.STROOP_INK.items() if h == b.params["color"])
        assert b.params["correct"] == btn[ink]
        assert (b.params["text"] == word[ink]) == (b.condition == "congruent")
    fl = T.build_flanker(T.FlankerOptions(n_trials=40))
    for b in (x for x in fl.blocks if x.kind == "choice"):
        middle = b.params["text"][2]
        assert b.params["correct"] == ("Left index" if middle == "←" else "Right index")
        assert (len(set(b.params["text"])) == 1) == (b.condition == "congruent")


def test_mid_structure_and_feedback_scoring():
    par = T.build("mid", T.MIDOptions(trials_per_condition=4), str(os.path.join(os.path.dirname(__file__),
                                                                               "..", "examples", "stimuli")))
    fbs = [b for b in par.blocks if b.kind == "feedback"]
    assert len(fbs) == 12
    for fb in fbs:
        prev = par.previous_scored(fb)
        assert prev.kind == "choice" and prev.condition.replace("target", "feedback") == fb.condition
    s = _started(par)
    tgt = [b for b in par.blocks if b.kind == "choice"][:2]
    for b in par.blocks:
        if b.start <= tgt[1].end + 3:
            s.onset(b, b.start) if b.kind != "feedback" else None
    s.on_button("Right index", tgt[0].start + 0.15)          # hit on the first target, none on the second
    for fb in fbs[:2]:
        s.onset(fb, fb.start)
    assert s.paint_state(fbs[0], fbs[0].start)["success"] is True
    assert s.paint_state(fbs[1], fbs[1].start)["success"] is False
    assert s.events[fbs[0].id]["outcome"] == "hit" and s.events[fbs[1].id]["outcome"] == "miss"


def test_choice_scoring_outcomes(tmp_path):
    par = Paradigm(dummy_scans=0)
    right = Block.create("choice", 0, 2, text="A", correct="Left index")
    wrong = Block.create("choice", 2, 2, text="B", correct="Left index")
    fa = Block.create("choice", 4, 2, text="C", correct="")
    cr = Block.create("choice", 6, 2, text="D", correct="")
    miss = Block.create("choice", 8, 2, text="E", correct="Right thumb")
    par.blocks += [right, wrong, fa, cr, miss]
    s = _started(par)
    for b in par.blocks:
        s.onset(b, b.start)
    s.on_button("Left index", 0.5)
    s.on_button("Right index", 2.5)
    s.on_button("Right index", 4.4)
    s.write(str(tmp_path), "x")
    e = s.events
    assert (e[right.id]["outcome"], e[right.id]["correct"]) == ("correct", 1)
    assert (e[wrong.id]["outcome"], e[wrong.id]["correct"]) == ("incorrect", 0)
    assert e[fa.id]["outcome"] == "false_alarm" and e[cr.id]["outcome"] == "correct_rejection"
    assert e[miss.id]["outcome"] == "miss"
    assert "Choice trials" in " ".join(performance_summary(s))


def test_flicker_changes_visual_key():
    from paradigmshift.stimulus import flicker_phase, visual_key
    b = Block.create("image", 0, 10, path="a.png", flicker_path="b.png", flicker_hz=8)
    assert [flicker_phase(b, t) for t in (0.0, 0.13, 0.26)] == [0, 1, 0]
    assert visual_key(b, 0.0) != visual_key(b, 0.13)
    still = Block.create("image", 0, 10, path="a.png")
    assert visual_key(still, 0.0) == visual_key(still, 5.0)
