"""Task blocks: ready-made blocks for the standard paradigms, for the library's “Task blocks” tab.

Two flavours:

* **Task trials** (Stroop, Flanker, N-back letter) are single Choice trials with a
  few task settings (e.g. word + ink colour). Changing them in the inspector
  re-derives text, colour, correct button and condition, so you never have to
  keep those in sync by hand.
* **Task sequences** (MID trial, emotion-regulation trial, N-back block, Hariri
  block, tapping block, checkerboard, resting state) insert several blocks at
  once. Most have variants (e.g. win / lose / nothing) that you pick on drop.
"""
from __future__ import annotations

import os
import random
from dataclasses import dataclass, field

from .model import Block

STROOP_INK = {"red": "#E53935", "green": "#43A047", "blue": "#1E88E5", "yellow": "#FDD835"}
STROOP_WORDS = {"red": "RED", "green": "GREEN", "blue": "BLUE", "yellow": "YELLOW"}
STROOP_BUTTONS = {"red": "Left thumb", "green": "Left index", "blue": "Right index", "yellow": "Right thumb"}
ARROWS = {"left": "←", "right": "→"}


def default_stim_dir() -> str:
    """Where generated placeholder pictures go (per user)."""
    try:
        from PySide6.QtCore import QStandardPaths
        base = QStandardPaths.writableLocation(QStandardPaths.AppDataLocation)
    except ImportError:  # pragma: no cover
        base = ""
    return os.path.join(base or os.path.join(os.path.expanduser("~"), ".paradigmshift"), "placeholder-stimuli")


# =============================================================================
# Task trials: settings → derived choice-trial parameters
# =============================================================================
def derive(b: Block) -> None:
    """Recompute text / colour / correct button / condition of a task trial from its task settings."""
    p = b.params
    kind = p.get("preset")
    if kind == "stroop":
        words = p.get("words") or STROOP_WORDS
        buttons = p.get("ink_buttons") or STROOP_BUTTONS
        ink, word = p.get("ink", "red"), p.get("word", "red")
        p["text"] = "XXXX" if word == "neutral" else words.get(word, STROOP_WORDS.get(word, word.upper()))
        p["color"] = STROOP_INK.get(ink, "#FFFFFF")
        p["correct"] = buttons.get(ink, "")
        b.condition = "neutral" if word == "neutral" else ("congruent" if word == ink else "incongruent")
        b.label = f"{p['text']} in {ink}"
    elif kind == "flanker":
        side, cong = p.get("direction", "left"), p.get("congruency", "congruent")
        other = "right" if side == "left" else "left"
        flank = {"congruent": ARROWS[side], "incongruent": ARROWS[other], "neutral": "–"}.get(cong, "–")
        p["text"] = flank * 2 + ARROWS[side] + flank * 2
        p["correct"] = p.get("left_button", "Left index") if side == "left" else p.get("right_button", "Right index")
        b.condition = cong
        b.label = f"{p['text']} ({side})"
    elif kind == "nback":
        target = bool(p.get("target", False))
        p["text"] = str(p.get("letter", "A"))[:3].upper()
        p["correct"] = p.get("target_button", "Right index") if target else ""
        b.condition = f"{int(p.get('level', 2))}back"
        b.label = f"{p['text']} (target)" if target else ""


# settings shown in the inspector: (param, label, choices | "text" | "int")
TRIAL_FIELDS = {
    "stroop": [("word", "Word", [("RED", "red"), ("GREEN", "green"), ("BLUE", "blue"), ("YELLOW", "yellow"),
                                 ("XXXX (neutral)", "neutral")]),
               ("ink", "Ink colour", [("red", "red"), ("green", "green"), ("blue", "blue"), ("yellow", "yellow")])],
    "flanker": [("direction", "Middle arrow", [("← left", "left"), ("→ right", "right")]),
                ("congruency", "Flankers", [("same direction (congruent)", "congruent"),
                                            ("opposite (incongruent)", "incongruent"),
                                            ("dashes (neutral)", "neutral")])],
    "nback": [("letter", "Letter", "text"),
              ("target", "Target?", [("yes – participant presses", True), ("no – no press", False)]),
              ("level", "N-back level", "int")],
}
TRIAL_TITLES = {"stroop": "Stroop trial", "flanker": "Flanker trial", "nback": "N-back letter"}


# =============================================================================
# Library entries
# =============================================================================
@dataclass
class Preset:
    key: str
    title: str
    subtitle: str
    description: str
    icon_kind: str
    color: str
    category: str
    make: object                       # (start, variant, ctx) -> list[Block]
    variants: list = field(default_factory=list)   # [(label, value)], first = default

    def build(self, start: float, variant=None, ctx: dict | None = None) -> list[Block]:
        if variant is None and self.variants:
            variant = self.variants[0][1]
        blocks = self.make(start, variant, ctx or {})
        for b in blocks:
            b.params.setdefault("preset_group", self.key)
        return blocks


def _choice(start, dur, **params) -> Block:
    b = Block.create("choice", start, dur, **params)
    derive(b)
    return b


def _stroop(start, _v, _ctx):
    return [_choice(start, 2.0, preset="stroop", word="red", ink="blue", size=110,
                    words=dict(STROOP_WORDS), ink_buttons=dict(STROOP_BUTTONS))]


def _flanker(start, _v, _ctx):
    return [_choice(start, 1.5, preset="flanker", direction="left", congruency="incongruent", size=110,
                    stim_time=0.5, left_button="Left index", right_button="Right index")]


def _nback_letter(start, _v, _ctx):
    return [_choice(start, 2.0, preset="nback", letter="K", target=False, level=2, stim_time=0.5, size=140,
                    target_button="Right index")]


def _nback_block(start, level, ctx):
    from .templates import nback_sequence
    rng = random.Random(ctx.get("seed"))
    rule = "press for X" if level == 0 else ("same as the one before" if level == 1 else f"same as {level} before")
    out = [Block.create("text", start, 4.0, condition="cue", label=f"{level}-BACK",
                        text=f"{level}-BACK\n\n{rule}", size=56)]
    t = start + 4.0
    for letter, target in nback_sequence(level, 15, 5, "BCDFGHJKLMNPQRSTVWZ", rng, "X"):
        out.append(_choice(t, 2.0, preset="nback", letter=letter, target=target, level=level, stim_time=0.5,
                           size=140, target_button="Right index"))
        t += 2.0
    return out


def _mid_trial(start, incentive, ctx):
    from . import placeholders as ph
    cue = ph.mid_cues(os.path.join(ctx.get("stim_dir") or default_stim_dir(), "mid"))[incentive]
    amt = ctx.get("amount", "1 €")
    hit, miss = {"gain": (f"+{amt}", "+0"), "loss": ("−0", f"−{amt}"), "neutral": ("0", "0")}[incentive]
    grey = incentive == "neutral"
    t = start
    out = []
    for kind, dur, cond, label, params in (
            ("image", 2.0, f"cue_{incentive}", f"cue {incentive}", {"path": cue, "scale": 0.45}),
            ("fixation", 2.2, f"anticipation_{incentive}", "anticipation", {}),
            ("choice", 0.3, f"target_{incentive}", "target",
             {"text": "■", "size": 220, "stim_time": 0.0, "correct": "any", "buttons": "any", "isi_symbol": ""}),
            ("feedback", 2.0, f"feedback_{incentive}", "",
             {"hit_text": hit, "miss_text": miss, "size": 96, "hit_color": "#CCCCCC" if grey else "#2FB37A",
              "miss_color": "#CCCCCC" if grey else "#F0506E"}),
            ("fixation", 2.0, "", "ITI", {})):
        out.append(Block.create(kind, t, dur, condition=cond, label=label, **params))
        t = round(t + dur, 4)
    return out


EMOREG_VARIANTS = {"look_neutral": ("LOOK", "neutral"), "look_negative": ("LOOK", "negative"),
                   "decrease_negative": ("DECREASE", "negative"), "increase_negative": ("INCREASE", "negative")}


def _emoreg_trial(start, cond, ctx):
    from . import placeholders as ph
    cue, valence = EMOREG_VARIANTS[cond]
    pic = ph.scenes(os.path.join(ctx.get("stim_dir") or default_stim_dir(), valence), valence, 1)[0]
    return [Block.create("text", start, 2.0, condition="cue", label=cue, text=cue, size=72),
            Block.create("image", start + 2.0, 7.0, condition=cond, path=pic, scale=0.85),
            Block.create("rating", start + 9.0, 4.0, condition="rating", question="How negative do you feel?",
                         path="", min=1, max=4, left_label="not at all", right_label="very"),
            Block.create("fixation", start + 13.0, 4.0, label="Relax")]


def _hariri_block(start, cond, ctx):
    from . import placeholders as ph
    rng = random.Random(ctx.get("seed"))
    sd = ctx.get("stim_dir") or default_stim_dir()
    faces = ph.hariri_faces(os.path.join(sd, "faces"))
    shapes = ph.hariri_shapes(os.path.join(sd, "shapes"))
    out = [Block.create("text", start, 2.0, condition="instruction", label=f"Match {cond}",
                        text="Match Faces" if cond == "faces" else "Match Shapes", size=64)]
    sides = ["left", "right"] * 3
    rng.shuffle(sides)
    emos = ["angry", "fearful"] * 3
    rng.shuffle(emos)
    t = start + 2.0
    for k in range(6):
        if cond == "faces":
            other = "fearful" if emos[k] == "angry" else "angry"
            top, match = rng.sample(faces[emos[k]], 2)
            foil = rng.choice(faces[other])
        else:
            s = rng.choice(list(shapes))
            top = match = shapes[s]
            foil = shapes[rng.choice([x for x in shapes if x != s])]
        left, right = (match, foil) if sides[k] == "left" else (foil, match)
        out.append(Block.create("match", t, 5.0, condition=cond, top=top, left=left, right=right,
                                correct=sides[k]))
        t += 5.0
    return out


def _tapping(start, hand, _ctx):
    text = {"left": "TAP\nLEFT HAND", "right": "TAP\nRIGHT HAND", "both": "TAP\nBOTH HANDS"}[hand]
    cond = f"tap_{hand}"
    out = [Block.create("text", start, 20.0, condition=cond, label=text.replace("\n", " "), text=text, size=72)]
    t = start
    while t < start + 20.0 - 1e-6:
        out.append(Block.create("response", t, 2.0, track=1, condition=cond))
        t += 2.0
    return out


def _checkerboard(start, _v, ctx):
    from . import placeholders as ph
    a, b = ph.checkerboards(os.path.join(ctx.get("stim_dir") or default_stim_dir(), "checkerboard"))
    return [Block.create("image", start, 20.0, condition="checkerboard", label="Checkerboard", path=a,
                         flicker_path=b, flicker_hz=8.0, scale=1.0)]


def _resting(start, minutes, _ctx):
    return [Block.create("fixation", start, minutes * 60.0, condition="rest", label="Rest (eyes open)")]


PRESETS = [
    Preset("stroop", "Stroop trial", "word in coloured ink", "Colour word – press the button for the INK colour. "
           "Pick word and ink in the inspector; correct button and condition follow automatically.",
           "choice", "#E5B53B", "Task trials", _stroop),
    Preset("flanker", "Flanker trial", "arrows, middle one counts", "Five arrows – the middle one gives the "
           "answer. Pick direction and flankers in the inspector.", "choice", "#E59A3B", "Task trials", _flanker),
    Preset("nback_letter", "N-back letter", "target or not", "One N-back letter. Mark it as target in the "
           "inspector; the correct answer follows.", "choice", "#3BA7E5", "Task trials", _nback_letter),
    Preset("nback_block", "N-back block", "cue + 15 letters", "A cue and 15 letters (5 targets).",
           "choice", "#3BA7E5", "Task sequences", _nback_block,
           [("2-back", 2), ("0-back (press for X)", 0), ("1-back", 1), ("3-back", 3)]),
    Preset("mid_trial", "MID trial", "cue → wait → target → feedback", "One monetary-incentive-delay trial: "
           "incentive cue, anticipation, brief target (press!), feedback and ITI.", "feedback", "#B565D8",
           "Task sequences", _mid_trial, [("Win", "gain"), ("Avoid losing", "loss"), ("Nothing at stake", "neutral")]),
    Preset("emoreg_trial", "Emotion-regulation trial", "cue → picture → rating → relax", "LOOK / DECREASE cue, a "
           "picture, a negative-affect rating and a relax period. Replace the placeholder picture with yours.",
           "rating", "#D45C9E", "Task sequences", _emoreg_trial,
           [("DECREASE · negative picture", "decrease_negative"), ("LOOK · negative picture", "look_negative"),
            ("LOOK · neutral picture", "look_neutral"), ("INCREASE · negative picture", "increase_negative")]),
    Preset("hariri_block", "Hariri block", "instruction + 6 matching trials", "One block of the Hariri task "
           "(2 s instruction, 6 × 5 s trials).", "match", "#1FA8B8", "Task sequences", _hariri_block,
           [("Faces", "faces"), ("Shapes", "shapes")]),
    Preset("tapping", "Tapping block", "20 s finger tapping", "Finger-tapping instruction for 20 s with 2-s "
           "response windows that show in the control room whether the participant taps.", "response", "#2FB37A",
           "Task sequences", _tapping, [("Left hand", "left"), ("Right hand", "right"), ("Both hands", "both")]),
    Preset("checkerboard", "Checkerboard", "20 s, 8 Hz flicker", "Flickering radial checkerboard with a red "
           "fixation dot. Set a grey background in the paradigm settings for the OFF periods.", "image", "#9AA3AD",
           "Task sequences", _checkerboard),
    Preset("resting", "Resting state", "fixation, eyes open", "One long fixation period for resting-state fMRI.",
           "fixation", "#4C8DF6", "Task sequences", _resting, [("8 minutes", 8.0), ("5 minutes", 5.0),
                                                               ("10 minutes", 10.0), ("6 minutes", 6.0)]),
]
PRESET_BY_KEY = {p.key: p for p in PRESETS}
