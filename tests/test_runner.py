import csv

from paradigmshift.model import Block, Paradigm
from paradigmshift.runner import RunState, output_stem


def paradigm():
    p = Paradigm(name="Emo rating!", tr=2.0, dummy_scans=2)
    p.blocks.append(Block.create("image", 0, 4, path="x.png", label="picture"))
    rating = Block.create("rating", 4, 4, path="x.png")
    rating.condition = "pos"
    p.blocks.append(rating)
    cur = Block.create("rating", 8, 4, mode="cursor")
    p.blocks.append(cur)
    return p


def test_dummy_scans_and_t0():
    s = RunState(paradigm())
    assert not s.on_trigger(100.0)
    assert not s.on_trigger(102.0)
    assert s.on_trigger(104.0)
    assert s.t0 == 104.0
    assert not s.on_trigger(106.0)


def test_every_press_is_recorded_with_context():
    p = paradigm()
    s = RunState(p)
    early = s.record_press("a", "Left thumb", 1.0, t_open=0.5)       # before the paradigm started
    assert early["time"] == "n/a" and early["block"] == "before start" and early["time_since_open"] == 0.5
    s.on_trigger(0.0)
    s.on_trigger(2.0)
    s.on_trigger(4.0)  # t0
    row = s.record_press("b", "Left index", 5.25, t_open=0.5)
    assert row["time"] == 1.25 and row["block"] == "picture" and row["block_kind"] == "image" and row["volume"] == 1
    odd = s.record_press("x", "", 9.0, t_open=0.5)
    assert odd["button"] == "unassigned" and odd["key"] == "x"
    assert len(s.presses) == 3


def test_direct_rating_maps_buttons_to_values():
    p = paradigm()
    s = RunState(p)
    s.on_trigger(0.0)
    s.on_trigger(0.0)
    s.on_trigger(0.0)
    rating = p.blocks[1]
    s.onset(rating, 4.0)
    s.on_button("Right index", 5.0)       # 3rd button -> value 3
    assert s.events[rating.id]["rating"] == 3
    s.on_button("Left thumb", 5.5)        # already confirmed, ignored
    assert s.events[rating.id]["rating"] == 3


def test_cursor_rating_moves_and_confirms(tmp_path):
    p = paradigm()
    s = RunState(p)
    for _ in range(3):
        s.on_trigger(0.0)
    cur = p.blocks[2]
    s.onset(cur, 8.0)
    assert s.ratings[cur.id]["value"] == 3   # starts in the middle
    s.on_button("Left index", 8.5)
    s.on_button("Left index", 8.7)
    s.on_button("Left index", 8.9)           # clamps at 1
    s.on_button("Right index", 9.0)
    s.on_button("Right thumb", 9.5)          # confirm
    ev = s.events[cur.id]
    assert ev["rating"] == 2 and ev["response_time"] == 1.5
    files = s.write(str(tmp_path), output_stem(p, {"sub": "07", "run": 2}))
    assert files[0].endswith("sub-07_task-Emorating_run-02_events.tsv")
    rows = list(csv.DictReader(open(files[0]), delimiter="\t"))
    assert rows[0]["rating"] == "2"
    log = list(csv.DictReader(open(files[1]), delimiter="\t"))
    assert sum(r["type"] == "trigger" for r in log) == 3
