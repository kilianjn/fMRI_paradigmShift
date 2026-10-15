"""Ready-made paradigm modules: Hariri face matching, Go/No-Go, emotion regulation.

Each ``build_*`` function returns a complete :class:`Paradigm` that can be
edited further in the timeline. The defaults follow the commonly published
versions of the tasks; all of them are parameters, so adapt them to your
protocol. Without your own stimulus folders, generated placeholder pictures
are used (see :mod:`paradigmshift.placeholders`).
"""
from __future__ import annotations

import os
import random
import re
from dataclasses import dataclass, field

from .model import Block, Paradigm

IMAGE_RE = re.compile(r"\.(png|jpe?g|bmp|gif|tiff?|webp)$", re.I)


def list_images(folder: str) -> list[str]:
    if not folder or not os.path.isdir(folder):
        return []
    return sorted(os.path.join(folder, f) for f in os.listdir(folder) if IMAGE_RE.search(f))


def _q(x: float, step: float) -> float:
    return round(round(x / step) * step, 4) if step > 0 else round(x, 4)


class _Seq:
    """Appends blocks back to back on the stimulus track."""

    def __init__(self, par: Paradigm):
        self.par, self.t = par, 0.0

    def add(self, kind: str, dur: float, condition: str = "", label: str = "", **params) -> Block:
        b = Block.create(kind, self.t, dur, track=0, label=label, condition=condition, **params)
        self.par.blocks.append(b)
        self.t = round(self.t + dur, 4)
        return b


def _balanced(items: list, n: int, rng: random.Random) -> list:
    """*n* items, as evenly balanced as possible, shuffled."""
    out = (items * (n // len(items) + 1))[:n]
    rng.shuffle(out)
    return out


# =============================================================================
# Hariri emotional face matching
# =============================================================================
_EMO_KEYS = {
    "angry": ("angry", "anger", "_an_", "_an.", "-an-", "-an."),
    "fearful": ("fear", "afraid", "_fe_", "_fe.", "-fe-", "-fe."),
}


def faces_from_folder(folder: str) -> dict[str, list[str]]:
    """Sort a face folder into emotions by file name (e.g. NimStim ``01F_AN_C``, KDEF ``AF01AFS``)."""
    out: dict[str, list[str]] = {"angry": [], "fearful": []}
    for f in list_images(folder):
        name = os.path.basename(f).lower()
        for emo, keys in _EMO_KEYS.items():
            if any(k in name for k in keys) or (emo == "angry" and re.search(r"[a-z]{2}\d\dan", name)) \
                    or (emo == "fearful" and re.search(r"[a-z]{2}\d\daf", name)):
                out[emo].append(f)
                break
    if min(len(v) for v in out.values()) < 2:
        raise ValueError("The face folder needs at least two angry and two fearful faces. File names must "
                         "contain e.g. 'angry'/'fear' or the NimStim codes '_AN_'/'_FE_'.")
    return out


@dataclass
class HaririOptions:
    tr: float = 2.0
    face_blocks: int = 4             # shape blocks = face blocks + 1 (start and end with shapes)
    trials_per_block: int = 6
    trial_dur: float = 5.0
    instruction_dur: float = 2.0
    faces_dir: str = ""              # empty → placeholder faces
    shapes_dir: str = ""             # empty → placeholder shapes
    seed: int = 1


def build_hariri(o: HaririOptions, stim_dir: str) -> Paradigm:
    """Hariri et al. (2002) amygdala paradigm.

    Faces: pick which of the two bottom faces shows the same emotion as the top
    face (angry / fearful). Shapes (sensorimotor control): pick the matching
    geometric form. Blocks alternate S F S F … S, each preceded by an
    instruction. Responses: left / right grip button.
    """
    from . import placeholders as ph
    rng = random.Random(o.seed)
    faces = faces_from_folder(o.faces_dir) if o.faces_dir else ph.hariri_faces(os.path.join(stim_dir, "faces"))
    if o.shapes_dir and len(list_images(o.shapes_dir)) >= 2:
        shapes = {os.path.basename(f): f for f in list_images(o.shapes_dir)}
    else:
        shapes = ph.hariri_shapes(os.path.join(stim_dir, "shapes"))

    par = Paradigm(name="Hariri face matching", tr=o.tr, dummy_scans=3, background="#000000")
    seq = _Seq(par)
    n_blocks = 2 * o.face_blocks + 1
    for i in range(n_blocks):
        is_face = i % 2 == 1
        cond = "faces" if is_face else "shapes"
        seq.add("text", o.instruction_dur, "instruction", "Match " + cond,
                text="Match Faces" if is_face else "Match Shapes", size=64)
        sides = _balanced(["left", "right"], o.trials_per_block, rng)
        emos = _balanced(list(faces), o.trials_per_block, rng)
        for k in range(o.trials_per_block):
            if is_face:
                emo = emos[k]
                other = [e for e in faces if e != emo][0]
                same = rng.sample(faces[emo], 2)         # two different people, same emotion
                foil = rng.choice([f for f in faces[other]])
                top, match = same
            else:
                kinds = list(shapes)
                s = rng.choice(kinds)
                top = match = shapes[s]
                foil = shapes[rng.choice([x for x in kinds if x != s])]
            left, right = (match, foil) if sides[k] == "left" else (foil, match)
            seq.add("match", o.trial_dur, cond, top=top, left=left, right=right, correct=sides[k])
    return par


# =============================================================================
# Go / No-Go
# =============================================================================
@dataclass
class GoNoGoOptions:
    tr: float = 2.0
    n_trials: int = 160
    nogo_percent: float = 25.0
    stim_time: float = 0.5
    trial_dur: float = 1.5           # stimulus onset asynchrony
    go_letters: str = "ABCDEFGHJKLMNPRSTUVWYZ"
    nogo_letter: str = "X"
    rest_every: int = 40             # fixation block after every N trials (0 = none)
    rest_dur: float = 16.0
    jitter_max: float = 0.0          # extra random fixation after each trial (0 = none)
    button: str = "any"
    seed: int = 1


def gonogo_sequence(n: int, n_nogo: int, rng: random.Random, lead_go: int = 3) -> list[str]:
    """Go/No-Go order: the first *lead_go* trials are Go, and No-Go never repeats back to back."""
    n_go = n - n_nogo
    slots = max(0, n_go - lead_go + 1)   # gaps after the lead-in Go trials
    if n_nogo > slots:
        raise ValueError("Too many No-Go trials to keep them apart – lower the No-Go percentage.")
    chosen = set(rng.sample(range(slots), n_nogo))
    seq = []
    for i in range(n_go):
        seq.append("go")
        if i >= lead_go - 1 and (i - lead_go + 1) in chosen:
            seq.append("nogo")
    return seq


def build_gonogo(o: GoNoGoOptions) -> Paradigm:
    """Letter Go/No-Go: press for every letter, withhold for the No-Go letter (rapid event-related)."""
    rng = random.Random(o.seed)
    par = Paradigm(name="Go-NoGo", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    seq.add("text", 8.0, "instruction", "Instructions",
            text=f"Press the button as fast as you can for every letter.\n\n"
                 f"Do NOT press for  {o.nogo_letter}", size=44)
    seq.add("fixation", 4.0, label="Get ready")
    n_nogo = round(o.n_trials * o.nogo_percent / 100)
    trials = gonogo_sequence(o.n_trials, n_nogo, rng)
    go_letters = [c for c in o.go_letters.upper() if c.strip() and c != o.nogo_letter.upper()] or ["O"]
    for i, kind in enumerate(trials):
        letter = o.nogo_letter if kind == "nogo" else rng.choice(go_letters)
        seq.add("gonogo", o.trial_dur, kind, trial=kind, text=letter, stim_time=o.stim_time, button=o.button)
        if o.jitter_max > 0:
            j = _q(rng.uniform(0, o.jitter_max), 0.25)
            if j > 0:
                seq.add("fixation", j, label="jitter")
        if o.rest_every and (i + 1) % o.rest_every == 0 and i + 1 < len(trials):
            seq.add("fixation", o.rest_dur, label="Rest")
    seq.add("fixation", o.rest_dur, label="Rest")
    return par


# =============================================================================
# Emotion regulation (cognitive reappraisal)
# =============================================================================
@dataclass
class EmoRegOptions:
    tr: float = 2.0
    trials_per_condition: int = 8
    include_increase: bool = False
    cue_dur: float = 2.0
    picture_dur: float = 7.0
    rating_dur: float = 4.0
    iti_min: float = 2.0
    iti_max: float = 6.0
    rating_max: int = 4
    neutral_dir: str = ""            # empty → placeholder scenes
    negative_dir: str = ""
    seed: int = 1


EMOREG_CUES = {"look_neutral": "LOOK", "look_negative": "LOOK",
               "decrease_negative": "DECREASE", "increase_negative": "INCREASE"}


def _no_long_runs(items: list[str], rng: random.Random, max_run: int = 2) -> list[str]:
    for _ in range(2000):
        rng.shuffle(items)
        if all(len(set(items[i:i + max_run + 1])) > 1 for i in range(len(items) - max_run)):
            return items
    return items


def build_emoreg(o: EmoRegOptions, stim_dir: str) -> Paradigm:
    """Ochsner-style reappraisal: cue (LOOK / DECREASE [/ INCREASE]) → picture → affect rating → relax."""
    from . import placeholders as ph
    rng = random.Random(o.seed)
    conds = ["look_neutral", "look_negative", "decrease_negative"] + (["increase_negative"] if o.include_increase else [])
    n = o.trials_per_condition
    n_neg = n * (len(conds) - 1)
    neutral = list_images(o.neutral_dir) or ph.scenes(os.path.join(stim_dir, "neutral"), "neutral", n)
    negative = list_images(o.negative_dir) or ph.scenes(os.path.join(stim_dir, "negative"), "negative", n_neg)
    rng.shuffle(neutral)
    rng.shuffle(negative)
    pools = {"neutral": iter(neutral * (n // max(1, len(neutral)) + 1)),
             "negative": iter(negative * (n_neg // max(1, len(negative)) + 1))}

    par = Paradigm(name="Emotion regulation", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    instr = ("LOOK: just look at the picture and let yourself feel whatever it evokes.\n"
             "DECREASE: reinterpret the picture so it feels less negative.")
    if o.include_increase:
        instr += "\nINCREASE: reinterpret it so it feels more negative."
    seq.add("text", 10.0, "instruction", "Instructions", text=instr + "\n\nAfterwards rate how negative you feel.",
            size=32)
    seq.add("fixation", 10.0, label="Baseline")
    order = _no_long_runs([c for c in conds for _ in range(n)], rng)
    for cond in order:
        seq.add("text", o.cue_dur, "cue", EMOREG_CUES[cond], text=EMOREG_CUES[cond], size=72)
        pool = "neutral" if cond == "look_neutral" else "negative"
        seq.add("image", o.picture_dur, cond, path=next(pools[pool]), scale=0.85)
        seq.add("rating", o.rating_dur, "rating", question="How negative do you feel?", path="",
                min=1, max=o.rating_max, left_label="not at all", right_label="very")
        lo, hi = sorted((o.iti_min, o.iti_max))
        seq.add("fixation", max(0.5, _q(rng.uniform(lo, hi), 0.5)), label="Relax")
    seq.add("fixation", 10.0, label="Baseline")
    return par


# =============================================================================
# Registry used by the template dialog
# =============================================================================
@dataclass
class Template:
    key: str
    title: str
    subtitle: str
    description: str
    reference: str
    options: type
    build: object
    needs_stim_dir: bool = True
    color: str = "#1FA8B8"
    kinds: list = field(default_factory=list)


TEMPLATES = [
    Template("hariri", "Hariri face matching", "amygdala · blocked",
             "Participants pick which of two bottom faces shows the same emotion (angry / fearful) as the top "
             "face. Shape-matching blocks are the sensorimotor control. Blocks alternate Shapes / Faces, each "
             "starting with a short instruction. Answers: left / right grip button.",
             "Hariri AR et al. (2002) Science 297:400–403",
             HaririOptions, build_hariri, True, "#1FA8B8", ["match"]),
    Template("gonogo", "Go / No-Go", "response inhibition · event-related",
             "Letters flash briefly. Press for every letter except the No-Go letter. No-Go trials never follow "
             "each other and the run starts with Go trials. Logged as hits, misses, false alarms and correct "
             "rejections.",
             "e.g. Garavan H et al. (1999) PNAS 96:8301–8306; Simmonds DJ et al. (2008) Neuropsychologia 46:224",
             GoNoGoOptions, build_gonogo, False, "#6BBF3A", ["gonogo"]),
    Template("emoreg", "Emotion regulation", "cognitive reappraisal · event-related",
             "Each trial: a cue (LOOK or DECREASE, optionally INCREASE), a neutral or negative picture, a rating "
             "of how negative the participant feels, and a jittered relax period. Conditions: look_neutral, "
             "look_negative, decrease_negative.",
             "Ochsner KN et al. (2002) J Cogn Neurosci 14:1215–1229; Ochsner KN et al. (2004) NeuroImage 23:483",
             EmoRegOptions, build_emoreg, True, "#D45C9E", ["text", "image", "rating"]),
]


def build(key: str, options, stim_dir: str) -> Paradigm:
    t = next(t for t in TEMPLATES if t.key == key)
    return t.build(options, stim_dir) if t.needs_stim_dir else t.build(options)
