import os

from paradigmshift.model import (Block, Paradigm, generate_block_design, generate_event_design)


def make(*specs, track=0):
    p = Paradigm()
    for kind, start, dur in specs:
        p.blocks.append(Block.create(kind, start, dur, track=track))
    return p


def test_duration_and_volumes():
    p = make(("fixation", 0, 10), ("image", 10, 5))
    assert p.duration == 15
    p.tr = 2.0
    assert p.volumes == 8


def test_ripple_insert_pushes_later_blocks():
    p = make(("fixation", 0, 10), ("image", 10, 4))
    new = Block.create("rest", 10, 6)
    p.ripple_insert(new)
    starts = sorted((b.start, b.kind) for b in p.blocks)
    assert starts == [(0, "fixation"), (10, "rest"), (16, "image")]
    assert not any(p.has_overlap(b) for b in p.blocks)


def test_ripple_insert_into_middle_goes_behind_block():
    p = make(("fixation", 0, 10), ("image", 10, 4))
    new = Block.create("rest", 3, 2)
    p.ripple_insert(new)
    assert new.start == 10
    assert p.block(new.id).end == 12
    assert [b.kind for b in p.blocks_on(0)] == ["fixation", "rest", "image"]
    assert not any(p.has_overlap(b) for b in p.blocks)


def test_ripple_insert_into_free_space_keeps_position():
    p = make(("fixation", 0, 10), ("image", 20, 4))
    new = Block.create("rest", 12, 3)
    p.ripple_insert(new)
    assert new.start == 12 and p.blocks[1].start == 20


def test_ripple_delete_closes_gap():
    p = make(("fixation", 0, 10), ("image", 10, 4), ("rest", 14, 6))
    p.ripple_delete({p.blocks[1].id})
    assert [(b.kind, b.start) for b in p.blocks_on(0)] == [("fixation", 0), ("rest", 10)]


def test_stimulus_at_prefers_stimulus_track():
    p = make(("image", 0, 4))
    p.blocks.append(Block.create("response", 0, 4, track=1))
    assert p.stimulus_at(1).kind == "image"
    assert p.stimulus_at(5) is None


def test_save_load_roundtrip_relative_paths(tmp_path):
    img = tmp_path / "pics" / "a.png"
    img.parent.mkdir()
    img.write_bytes(b"x")
    p = Paradigm(name="T", tr=1.5)
    b = Block.create("image", 0, 2, path=str(img))
    b.condition = "faces"
    p.blocks.append(b)
    p.hardware.trigger_key = "5"
    f = tmp_path / "t.paradigm.json"
    p.save(str(f))
    assert '"pics/a.png"' in f.read_text() or '"pics\\\\a.png"' in f.read_text()
    q = Paradigm.load(str(f))
    assert q.tr == 1.5 and q.hardware.trigger_key == "5"
    assert os.path.samefile(q.blocks[0].params["path"], img)
    assert q.blocks[0].condition == "faces"
    assert q.validate() == []


def test_validate_reports_missing_image_and_overlap():
    p = make(("image", 0, 4), ("fixation", 2, 4))
    msgs = " ".join(p.validate())
    assert "no image" in msgs and "overlaps" in msgs


def test_block_design_generator():
    conds = [{"name": "A", "kind": "fixation"}, {"name": "B", "kind": "text"}]
    blocks = generate_block_design(conds, 20, 10, 3, lead_in=10)
    stim = [b for b in blocks if b.condition]
    assert [b.condition for b in stim] == ["A", "B"] * 3
    assert max(b.end for b in blocks) == 10 + 6 * 30
    # contiguous, no overlaps
    blocks.sort(key=lambda b: b.start)
    for a, b in zip(blocks, blocks[1:]):
        assert abs(a.end - b.start) < 1e-9


def test_block_design_splits_pictures():
    conds = [{"name": "faces", "images": ["a.png", "b.png", "c.png", "d.png"]}]
    blocks = generate_block_design(conds, 20, 0, 1)
    assert len(blocks) == 4 and all(b.duration == 5 for b in blocks)


def test_event_design_jitter_on_quarter_tr():
    blocks = generate_event_design([f"{i}.png" for i in range(10)], 3, 2, 6, "pic", tr=2.0, seed=3)
    itis = [b for b in blocks if b.label == "ITI"]
    assert len(itis) == 10
    for b in itis:
        assert 0.5 <= b.duration <= 6.0
        assert abs(b.duration / 0.5 - round(b.duration / 0.5)) < 1e-6
    resp = [b for b in blocks if b.kind == "response"]
    assert len(resp) == 10 and all(r.track == 1 for r in resp)
