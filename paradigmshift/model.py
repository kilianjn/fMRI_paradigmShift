"""Data model for an fMRI paradigm: tracks, blocks and hardware settings.

Everything is plain Python (no Qt) so the model can be tested and scripted
without a display. A paradigm serialises to a single JSON file.
"""
from __future__ import annotations

import copy
import json
import math
import os
import uuid
from dataclasses import asdict, dataclass, field

FILE_SUFFIX = ".paradigm.json"
FORMAT_VERSION = 1


# --------------------------------------------------------------------------
# Block types
# --------------------------------------------------------------------------
@dataclass(frozen=True)
class BlockType:
    kind: str
    title: str
    color: str
    description: str
    default_duration: float
    default_track: int
    defaults: dict


BLOCK_TYPES: dict[str, BlockType] = {
    t.kind: t
    for t in [
        BlockType(
            "fixation", "Fixation", "#4C8DF6",
            "A fixation cross – the classic baseline.",
            10.0, 0, {"symbol": "+", "color": "#FFFFFF", "size": 72},
        ),
        BlockType(
            "image", "Image", "#E0913A",
            "Shows a picture. Drop image files straight onto the timeline.",
            4.0, 0, {"path": "", "scale": 0.8},
        ),
        BlockType(
            "rating", "Rating", "#D45C9E",
            "Shows an image + a rating scale the participant answers with the grips.",
            6.0, 0,
            {
                "path": "", "question": "How pleasant is this image?",
                "min": 1, "max": 4, "left_label": "not at all", "right_label": "very",
                "mode": "direct",  # direct: one button per value | cursor: move + confirm
                "scale": 0.9,
            },
        ),
        BlockType(
            "text", "Instruction", "#8B6CEF",
            "Shows a text, e.g. instructions or a task cue.",
            8.0, 0, {"text": "Please keep still and look at the cross.", "size": 40, "color": "#FFFFFF"},
        ),
        BlockType(
            "rest", "Pause", "#5B6577",
            "Blank screen in the background colour.",
            12.0, 0, {},
        ),
        BlockType(
            "response", "Response window", "#2FB37A",
            "Records grip presses while it runs (place it under a stimulus).",
            4.0, 1, {"buttons": "any", "correct": ""},
        ),
    ]
}

STIMULUS_KINDS = ("fixation", "image", "rating", "text", "rest")


# --------------------------------------------------------------------------
# Blocks & tracks
# --------------------------------------------------------------------------
def _new_id() -> str:
    return uuid.uuid4().hex[:10]


@dataclass
class Block:
    kind: str
    start: float
    duration: float
    track: int = 0
    label: str = ""
    condition: str = ""
    params: dict = field(default_factory=dict)
    id: str = field(default_factory=_new_id)

    @property
    def end(self) -> float:
        return self.start + self.duration

    @property
    def type(self) -> BlockType:
        return BLOCK_TYPES[self.kind]

    @property
    def display_label(self) -> str:
        if self.label:
            return self.label
        path = self.params.get("path")
        if path:
            return os.path.basename(path)
        if self.kind == "text":
            return self.params.get("text", "").split("\n")[0][:40] or self.type.title
        return self.type.title

    @classmethod
    def create(cls, kind: str, start: float, duration: float | None = None,
               track: int | None = None, label: str = "", condition: str = "", **params) -> "Block":
        bt = BLOCK_TYPES[kind]
        merged = copy.deepcopy(bt.defaults)
        merged.update(params)
        return cls(
            kind=kind,
            start=float(start),
            duration=float(bt.default_duration if duration is None else duration),
            track=bt.default_track if track is None else track,
            label=label,
            condition=condition,
            params=merged,
        )

    def clone(self) -> "Block":
        b = copy.deepcopy(self)
        b.id = _new_id()
        return b

    def overlaps(self, other: "Block", eps: float = 1e-6) -> bool:
        return (self.track == other.track and self.id != other.id
                and self.start < other.end - eps and other.start < self.end - eps)


DEFAULT_TRACKS = ["Stimuli", "Responses"]


# --------------------------------------------------------------------------
# Hardware
# --------------------------------------------------------------------------
@dataclass
class GripButton:
    name: str
    key: str


@dataclass
class Hardware:
    # NordicNeuroLab SyncBox – forwards the scanner TTL pulse to the PC either as
    # a USB keyboard key press or as a character on a serial (COM) port.
    sync_mode: str = "keyboard"  # "keyboard" | "serial"
    trigger_key: str = "s"
    serial_port: str = ""
    baud_rate: int = 57600
    # NNL ResponseGrips – their buttons reach the PC through the SyncBox the same way.
    buttons: list = field(default_factory=lambda: [
        GripButton("Left thumb", "1"),
        GripButton("Left index", "2"),
        GripButton("Right index", "3"),
        GripButton("Right thumb", "4"),
    ])
    # Rating scales in "cursor" mode
    move_left: str = "Left index"
    move_right: str = "Right index"
    confirm: str = "Right thumb"

    def button_names(self) -> list[str]:
        return [b.name for b in self.buttons]

    def button_for_key(self, key: str) -> str | None:
        for b in self.buttons:
            if b.key and b.key.lower() == key.lower():
                return b.name
        return None


# --------------------------------------------------------------------------
# Paradigm
# --------------------------------------------------------------------------
@dataclass
class Paradigm:
    name: str = "Untitled paradigm"
    tr: float = 2.0
    dummy_scans: int = 0
    background: str = "#000000"
    tracks: list = field(default_factory=lambda: list(DEFAULT_TRACKS))
    blocks: list = field(default_factory=list)
    hardware: Hardware = field(default_factory=Hardware)

    # ---- derived -----------------------------------------------------------
    @property
    def duration(self) -> float:
        return max((b.end for b in self.blocks), default=0.0)

    @property
    def volumes(self) -> int:
        if self.tr <= 0:
            return 0
        return int(math.ceil(self.duration / self.tr - 1e-9))

    def block(self, block_id: str) -> Block | None:
        for b in self.blocks:
            if b.id == block_id:
                return b
        return None

    def blocks_on(self, track: int) -> list[Block]:
        return sorted((b for b in self.blocks if b.track == track), key=lambda b: b.start)

    def active_blocks(self, t: float) -> list[Block]:
        return [b for b in self.blocks if b.start <= t < b.end]

    def stimulus_at(self, t: float) -> Block | None:
        for b in sorted(self.active_blocks(t), key=lambda b: b.track):
            if b.kind in STIMULUS_KINDS:
                return b
        return None

    def conditions(self) -> list[str]:
        seen = []
        for b in sorted(self.blocks, key=lambda b: b.start):
            if b.condition and b.condition not in seen:
                seen.append(b.condition)
        return seen

    def has_overlap(self, block: Block) -> bool:
        return any(block.overlaps(o) for o in self.blocks)

    def track_end(self, track: int) -> float:
        return max((b.end for b in self.blocks if b.track == track), default=0.0)

    # ---- editing helpers -----------------------------------------------------
    def ripple_insert(self, block: Block) -> None:
        """Insert *block*; later blocks on the same track move right to make room."""
        clash = [o for o in self.blocks if block.overlaps(o)]
        if clash:
            first = min(clash, key=lambda o: o.start)
            if first.start < block.start:
                # Dropped onto the middle of a block: go right behind it.
                block.start = first.end
            shift = block.end - min(
                (o.start for o in self.blocks
                 if o.track == block.track and o.end > block.start + 1e-9),
                default=block.end)
            if shift > 0:
                for o in self.blocks:
                    if o.track == block.track and o.start >= block.start - 1e-9:
                        o.start += shift
        self.blocks.append(block)

    def ripple_delete(self, ids: set[str]) -> None:
        removed = [b for b in self.blocks if b.id in ids]
        self.blocks = [b for b in self.blocks if b.id not in ids]
        for r in sorted(removed, key=lambda b: -b.start):
            for o in self.blocks:
                if o.track == r.track and o.start >= r.end - 1e-9:
                    o.start -= r.duration

    def close_gaps(self, track: int = 0) -> None:
        t = 0.0
        for b in self.blocks_on(track):
            b.start = t
            t = b.end

    def validate(self) -> list[str]:
        problems = []
        for b in sorted(self.blocks, key=lambda b: b.start):
            where = f"{b.display_label} @ {b.start:.2f}s"
            path = b.params.get("path")
            if b.kind == "image" and not path:
                problems.append(f"{where}: no image chosen")
            if path and not os.path.exists(path):
                problems.append(f"{where}: image file not found")
            if self.has_overlap(b):
                problems.append(f"{where}: overlaps another block")
        return problems

    # ---- (de)serialisation ---------------------------------------------------
    def to_dict(self) -> dict:
        d = asdict(self)
        d["format"] = FORMAT_VERSION
        return d

    @classmethod
    def from_dict(cls, d: dict) -> "Paradigm":
        hw = dict(d.get("hardware", {}))
        hw["buttons"] = [GripButton(**b) for b in hw.get("buttons", [])] or Hardware().buttons
        return cls(
            name=d.get("name", "Untitled paradigm"),
            tr=float(d.get("tr", 2.0)),
            dummy_scans=int(d.get("dummy_scans", 0)),
            background=d.get("background", "#000000"),
            tracks=list(d.get("tracks", DEFAULT_TRACKS)),
            blocks=[Block(**b) for b in d.get("blocks", [])],
            hardware=Hardware(**hw),
        )

    def save(self, path: str) -> None:
        base = os.path.dirname(os.path.abspath(path))
        d = self.to_dict()
        # store images relative to the paradigm file so projects can be moved
        for b in d["blocks"]:
            p = b["params"].get("path")
            if p and os.path.isabs(p):
                try:
                    b["params"]["path"] = os.path.relpath(p, base)
                except ValueError:  # different drive on Windows
                    pass
        with open(path, "w", encoding="utf-8") as fh:
            json.dump(d, fh, indent=2)

    @classmethod
    def load(cls, path: str) -> "Paradigm":
        with open(path, encoding="utf-8") as fh:
            p = cls.from_dict(json.load(fh))
        base = os.path.dirname(os.path.abspath(path))
        for b in p.blocks:
            rel = b.params.get("path")
            if rel and not os.path.isabs(rel):
                b.params["path"] = os.path.normpath(os.path.join(base, rel))
        return p

    def copy(self) -> "Paradigm":
        return Paradigm.from_dict(copy.deepcopy(asdict(self)))


# --------------------------------------------------------------------------
# Design generators
# --------------------------------------------------------------------------
def generate_block_design(conditions: list[dict], block_dur: float, rest_dur: float,
                          repetitions: int, order: str = "alternate", start: float = 0.0,
                          lead_in: float = 0.0, seed: int | None = None) -> list[Block]:
    """Classic ON/OFF block design.

    *conditions* is a list of ``{"name", "kind", "path"/"images"}`` dicts. Each
    block of a condition with several images is split into equal image events.
    """
    import random
    rng = random.Random(seed)
    seq = []
    for _ in range(repetitions):
        cyc = list(range(len(conditions)))
        if order == "random":
            rng.shuffle(cyc)
        seq.extend(cyc)

    blocks: list[Block] = []
    t = start
    if lead_in > 0:
        blocks.append(Block.create("fixation", t, lead_in, label="Baseline"))
        t += lead_in
    for i, ci in enumerate(seq):
        cond = conditions[ci]
        images = cond.get("images") or []
        if images:
            n = len(images)
            step = block_dur / n
            if order == "random":
                images = images[:]
                rng.shuffle(images)
            for img in images:
                b = Block.create("image", t, step, path=img)
                b.condition = cond["name"]
                blocks.append(b)
                t += step
        else:
            b = Block.create(cond.get("kind", "fixation"), t, block_dur)
            b.condition = cond["name"]
            b.label = cond["name"]
            blocks.append(b)
            t += block_dur
        if rest_dur > 0:
            blocks.append(Block.create("fixation", t, rest_dur, label="Rest"))
            t += rest_dur
    return blocks


def generate_event_design(images: list[str], stim_dur: float, iti_min: float, iti_max: float,
                          condition: str = "", rating: bool = False, shuffle: bool = True,
                          start: float = 0.0, tr: float = 0.0, seed: int | None = None) -> list[Block]:
    """Event-related design: one stimulus per image with a jittered fixation ITI."""
    import random
    rng = random.Random(seed)
    images = images[:]
    if shuffle:
        rng.shuffle(images)
    blocks: list[Block] = []
    t = start
    for img in images:
        kind = "rating" if rating else "image"
        b = Block.create(kind, t, stim_dur, path=img)
        b.condition = condition
        blocks.append(b)
        if kind == "image":
            blocks.append(Block.create("response", t, stim_dur))
        t += stim_dur
        iti = rng.uniform(iti_min, iti_max)
        if tr > 0:  # keep the jitter on a 1/4-TR grid – easier for modelling
            q = tr / 4
            iti = max(q, round(iti / q) * q)
        blocks.append(Block.create("fixation", t, round(iti, 3), label="ITI"))
        t += iti
    return blocks
