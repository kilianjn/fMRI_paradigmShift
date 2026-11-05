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
from .presets import STROOP_INK, derive  # noqa: F401  (STROOP_INK is re-exported for callers/tests)

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
# Resting state
# =============================================================================
@dataclass
class RestingOptions:
    tr: float = 2.0
    minutes: float = 8.0
    fixation: bool = True            # fixation cross (eyes open); off = empty screen
    instruction: str = ("Please keep your eyes open and look at the cross.\n"
                        "Lie still, relax and let your thoughts wander.")
    instruction_dur: float = 8.0


def build_resting(o: RestingOptions) -> Paradigm:
    """Resting-state run: optional instruction, then one long fixation (eyes open) or empty screen."""
    par = Paradigm(name="Resting state", tr=o.tr, dummy_scans=3,
                   waiting_screen="fixation" if o.fixation else "blank")
    seq = _Seq(par)
    if o.instruction.strip() and o.instruction_dur > 0:
        seq.add("text", o.instruction_dur, "instruction", "Instructions", text=o.instruction, size=36)
    dur = o.minutes * 60
    if o.tr > 0:
        dur = round(dur / o.tr) * o.tr
    if o.fixation:
        seq.add("fixation", dur, "rest", "Rest (eyes open)")
    else:
        seq.add("rest", dur, "rest", "Rest")
    return par


# =============================================================================
# Motor localizer (finger tapping)
# =============================================================================
@dataclass
class MotorOptions:
    tr: float = 2.0
    include_both: bool = False       # add a "both hands" condition
    block_dur: float = 20.0
    rest_dur: float = 20.0
    cycles: int = 5
    random_order: bool = False
    record_taps: bool = True         # response windows check that the participant is tapping
    seed: int = 1


def build_motor(o: MotorOptions) -> Paradigm:
    """Blocked finger tapping (left / right [/ both]) alternating with rest."""
    rng = random.Random(o.seed)
    conds = [("tap_left", "TAP\nLEFT HAND"), ("tap_right", "TAP\nRIGHT HAND")]
    if o.include_both:
        conds.append(("tap_both", "TAP\nBOTH HANDS"))
    par = Paradigm(name="Motor localizer", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    seq.add("text", 8.0, "instruction", "Instructions",
            text="When a hand is shown, tap your fingers against the thumb\nof that hand – quickly and steadily.\n\n"
                 "When you see the cross, rest.", size=34)
    seq.add("fixation", o.rest_dur, label="Rest")
    for _ in range(o.cycles):
        order = list(conds)
        if o.random_order:
            rng.shuffle(order)
        for cond, text in order:
            b = seq.add("text", o.block_dur, cond, text.replace("\n", " "), text=text, size=72)
            if o.record_taps:   # 2-s windows: shows in the control room whether taps keep coming
                t, step = b.start, 2.0
                while t < b.end - 1e-6:
                    w = Block.create("response", t, min(step, b.end - t), track=1, condition=cond)
                    par.blocks.append(w)
                    t = round(t + step, 4)
            seq.add("fixation", o.rest_dur, label="Rest")
    return par


# =============================================================================
# Visual localizer (flickering checkerboard)
# =============================================================================
@dataclass
class CheckerboardOptions:
    tr: float = 2.0
    on_dur: float = 20.0
    off_dur: float = 20.0
    cycles: int = 6
    flicker_hz: float = 8.0          # contrast reversals per second
    seed: int = 1


def build_checkerboard(o: CheckerboardOptions, stim_dir: str) -> Paradigm:
    """Flickering radial checkerboard ON vs. grey screen OFF, red fixation dot throughout."""
    from . import placeholders as ph
    a, b = ph.checkerboards(os.path.join(stim_dir, "checkerboard"))
    par = Paradigm(name="Visual checkerboard", tr=o.tr, dummy_scans=3, background="#808080")
    seq = _Seq(par)
    dot = dict(symbol="•", color="#E53935", size=40)
    seq.add("fixation", o.off_dur, label="Off", **dot)
    for _ in range(o.cycles):
        seq.add("image", o.on_dur, "checkerboard", "Checkerboard", path=a, flicker_path=b,
                flicker_hz=o.flicker_hz, scale=1.0)
        seq.add("fixation", o.off_dur, label="Off", **dot)
    return par


# =============================================================================
# N-back working memory
# =============================================================================
@dataclass
class NBackOptions:
    tr: float = 2.0
    levels: str = "0, 2"             # e.g. "0, 1, 2"
    blocks_per_level: int = 4
    trials_per_block: int = 15
    target_percent: float = 33.0
    stim_time: float = 0.5
    trial_dur: float = 2.0
    instruction_dur: float = 4.0
    rest_dur: float = 14.0
    letters: str = "BCDFGHJKLMNPQRSTVWZ"
    zero_back_target: str = "X"
    target_button: str = "Right index"
    seed: int = 1


def nback_sequence(n: int, count: int, n_targets: int, letters: str, rng: random.Random,
                   zero_target: str = "X") -> list[tuple[str, bool]]:
    """``[(letter, is_target), …]`` for one block. n = 0: target is *zero_target*."""
    pool = [c for c in letters.upper() if c.strip() and c != zero_target.upper()]
    if n == 0:
        tpos = set(rng.sample(range(count), min(n_targets, count)))
        return [(zero_target.upper() if i in tpos else rng.choice(pool), i in tpos) for i in range(count)]
    if n_targets > count - n:
        raise ValueError(f"Too many targets for a {n}-back block of {count} trials.")
    tpos = set(rng.sample(range(n, count), n_targets))
    seq: list[tuple[str, bool]] = []
    for i in range(count):
        if i in tpos:
            seq.append((seq[i - n][0], True))
        else:
            avoid = {seq[i - n][0]} if i >= n else set()
            seq.append((rng.choice([c for c in pool if c not in avoid]), False))
    return seq


def build_nback(o: NBackOptions) -> Paradigm:
    """Blocked N-back: a cue names the level, letters follow; press only for targets."""
    rng = random.Random(o.seed)
    try:
        levels = [int(x) for x in o.levels.replace(";", ",").split(",") if x.strip()]
    except ValueError:
        raise ValueError("Levels must be numbers, e.g. “0, 2”.") from None
    if not levels:
        raise ValueError("Give at least one N-back level, e.g. “0, 2”.")
    par = Paradigm(name="N-back", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    seq.add("text", 10.0, "instruction", "Instructions",
            text="Letters will appear one after another.\n\n"
                 f"0-BACK: press when you see {o.zero_back_target}.\n"
                 "2-BACK: press when the letter is the same as two letters before.\n\n"
                 "Otherwise do not press.", size=32)
    seq.add("fixation", o.rest_dur, label="Rest")
    n_targets = round(o.trials_per_block * o.target_percent / 100)
    for _ in range(o.blocks_per_level):
        order = list(levels)
        rng.shuffle(order)
        for n in order:
            cond = f"{n}back"
            rule = f"press for {o.zero_back_target}" if n == 0 else \
                f"same as {n} before" if n > 1 else "same as the one before"
            seq.add("text", o.instruction_dur, "cue", f"{n}-BACK", text=f"{n}-BACK\n\n{rule}", size=56)
            for letter, target in nback_sequence(n, o.trials_per_block, n_targets, o.letters, rng,
                                                 o.zero_back_target):
                b = seq.add("choice", o.trial_dur, cond, preset="nback", letter=letter, target=target, level=n,
                            target_button=o.target_button, stim_time=o.stim_time, buttons="any", size=140)
                derive(b)
            seq.add("fixation", o.rest_dur, label="Rest")
    return par


# =============================================================================
# Stroop (colour-word)
# =============================================================================
@dataclass
class StroopOptions:
    tr: float = 2.0
    n_trials: int = 96
    incongruent_percent: float = 50.0
    neutral_percent: float = 0.0     # "XXXX" printed in colour
    use_yellow: bool = True          # 4 colours (else red, green, blue)
    words: str = "RED, GREEN, BLUE, YELLOW"
    stim_time: float = 0.0           # 0 = word stays for the whole trial
    trial_dur: float = 2.0
    iti_min: float = 1.0
    iti_max: float = 4.0
    red_button: str = "Left thumb"
    green_button: str = "Left index"
    blue_button: str = "Right index"
    yellow_button: str = "Right thumb"
    seed: int = 1


def build_stroop(o: StroopOptions) -> Paradigm:
    """Colour-word Stroop: name the INK colour with the grips; congruent vs incongruent (vs neutral)."""
    rng = random.Random(o.seed)
    colors = ["red", "green", "blue"] + (["yellow"] if o.use_yellow else [])
    words = [w.strip() for w in o.words.split(",")]
    if len(words) < len(colors):
        raise ValueError(f"Give one word per colour ({', '.join(colors)}).")
    word = dict(zip(["red", "green", "blue", "yellow"], words))
    button = {"red": o.red_button, "green": o.green_button, "blue": o.blue_button, "yellow": o.yellow_button}
    n_inc = round(o.n_trials * o.incongruent_percent / 100)
    n_neu = round(o.n_trials * o.neutral_percent / 100)
    n_con = o.n_trials - n_inc - n_neu
    if n_con < 0:
        raise ValueError("Incongruent + neutral percentages exceed 100 %.")
    conds = _no_long_runs(["congruent"] * n_con + ["incongruent"] * n_inc + ["neutral"] * n_neu, rng, 3)
    par = Paradigm(name="Stroop", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    mapping = "\n".join(f"{word[c]} ink  →  {button[c]}" for c in colors)
    seq.add("text", 12.0, "instruction", "Instructions",
            text="Press the button for the INK COLOUR of the word –\nnot for what the word says.\n\n" + mapping,
            size=32)
    seq.add("fixation", 6.0, label="Get ready")
    inks = _balanced(colors, len(conds), rng)
    for cond, ink in zip(conds, inks):
        wk = ink if cond == "congruent" else "neutral" if cond == "neutral" else \
            rng.choice([c for c in colors if c != ink])
        b = seq.add("choice", o.trial_dur, cond, preset="stroop", word=wk, ink=ink, words=dict(word),
                    ink_buttons=dict(button), size=110, stim_time=o.stim_time, buttons="any")
        derive(b)
        lo, hi = sorted((o.iti_min, o.iti_max))
        iti = _q(rng.uniform(lo, hi), 0.25)
        if iti > 0:
            seq.add("fixation", iti, label="ITI")
    seq.add("fixation", 10.0, label="End")
    return par


# =============================================================================
# Eriksen flanker
# =============================================================================
@dataclass
class FlankerOptions:
    tr: float = 2.0
    n_trials: int = 96
    incongruent_percent: float = 50.0
    neutral_percent: float = 0.0     # "– – ← – –"
    stim_time: float = 0.5
    trial_dur: float = 1.5
    iti_min: float = 1.0
    iti_max: float = 4.0
    left_button: str = "Left index"
    right_button: str = "Right index"
    seed: int = 1


def build_flanker(o: FlankerOptions) -> Paradigm:
    """Arrow flanker: respond to the direction of the middle arrow."""
    rng = random.Random(o.seed)
    n_inc = round(o.n_trials * o.incongruent_percent / 100)
    n_neu = round(o.n_trials * o.neutral_percent / 100)
    n_con = o.n_trials - n_inc - n_neu
    if n_con < 0:
        raise ValueError("Incongruent + neutral percentages exceed 100 %.")
    conds = _no_long_runs(["congruent"] * n_con + ["incongruent"] * n_inc + ["neutral"] * n_neu, rng, 3)
    par = Paradigm(name="Flanker", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    seq.add("text", 10.0, "instruction", "Instructions",
            text="Look at the MIDDLE arrow only.\n\n"
                 f"←  points left:  {o.left_button}\n→  points right:  {o.right_button}\n\n"
                 "Answer as fast and as accurately as you can.", size=32)
    seq.add("fixation", 6.0, label="Get ready")
    sides = _balanced(["left", "right"], len(conds), rng)
    for cond, side in zip(conds, sides):
        b = seq.add("choice", o.trial_dur, cond, preset="flanker", direction=side, congruency=cond, size=110,
                    stim_time=o.stim_time, left_button=o.left_button, right_button=o.right_button, buttons="any")
        derive(b)
        lo, hi = sorted((o.iti_min, o.iti_max))
        iti = _q(rng.uniform(lo, hi), 0.25)
        if iti > 0:
            seq.add("fixation", iti, label="ITI")
    seq.add("fixation", 10.0, label="End")
    return par


# =============================================================================
# Monetary incentive delay (MID)
# =============================================================================
@dataclass
class MIDOptions:
    tr: float = 2.0
    trials_per_condition: int = 15
    amount: str = "1 €"
    cue_dur: float = 2.0
    anticipation_min: float = 2.0
    anticipation_max: float = 2.5
    target_dur: float = 0.3          # press while the target is visible = hit (fixed, not adaptive)
    feedback_dur: float = 2.0
    iti_min: float = 1.5
    iti_max: float = 4.0
    seed: int = 1


def build_mid(o: MIDOptions, stim_dir: str) -> Paradigm:
    """Knutson-style MID: cue (win / lose / nothing) → anticipation → brief target → feedback."""
    from . import placeholders as ph
    rng = random.Random(o.seed)
    cues = ph.mid_cues(os.path.join(stim_dir, "mid"))
    amt = o.amount.strip()
    fb = {"gain": (f"+{amt}", "+0"), "loss": ("−0", f"−{amt}"), "neutral": ("0", "0")}
    par = Paradigm(name="Monetary incentive delay", tr=o.tr, dummy_scans=3)
    seq = _Seq(par)
    seq.add("text", 12.0, "instruction", "Instructions",
            text="A shape tells you what is at stake:\n"
                 f"circle = you can WIN {amt}   ·   square = you can avoid LOSING {amt}   ·   triangle = no money\n\n"
                 "Then press a button as fast as you can when the white square appears.", size=30)
    seq.add("fixation", 6.0, label="Get ready")
    order = _no_long_runs(["gain", "loss", "neutral"] * o.trials_per_condition, rng, 2)
    for c in order:
        seq.add("image", o.cue_dur, f"cue_{c}", f"cue {c}", path=cues[c], scale=0.45)
        lo, hi = sorted((o.anticipation_min, o.anticipation_max))
        seq.add("fixation", _q(rng.uniform(lo, hi), 0.1), f"anticipation_{c}", "anticipation")
        seq.add("choice", o.target_dur, f"target_{c}", "target", text="■", size=220, stim_time=0.0,
                correct="any", buttons="any", isi_symbol="")
        hit, miss = fb[c]
        seq.add("feedback", o.feedback_dur, f"feedback_{c}", "", hit_text=hit, miss_text=miss,
                hit_color="#2FB37A" if c != "neutral" else "#CCCCCC",
                miss_color="#F0506E" if c != "neutral" else "#CCCCCC", size=96)
        lo, hi = sorted((o.iti_min, o.iti_max))
        seq.add("fixation", _q(rng.uniform(lo, hi), 0.25), label="ITI")
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
    category: str = ""


TEMPLATES = [
    Template("hariri", "Hariri face matching", "amygdala · blocked",
             "Participants pick which of two bottom faces shows the same emotion (angry / fearful) as the top "
             "face. Shape-matching blocks are the sensorimotor control. Blocks alternate Shapes / Faces, each "
             "starting with a short instruction. Answers: left / right grip button.",
             "Hariri AR et al. (2002) Science 297:400–403",
             HaririOptions, build_hariri, True, "#1FA8B8", ["match"], "Emotion & reward"),
    Template("gonogo", "Go / No-Go", "response inhibition · event-related",
             "Letters flash briefly. Press for every letter except the No-Go letter. No-Go trials never follow "
             "each other and the run starts with Go trials. Logged as hits, misses, false alarms and correct "
             "rejections.",
             "e.g. Garavan H et al. (1999) PNAS 96:8301–8306; Simmonds DJ et al. (2008) Neuropsychologia 46:224",
             GoNoGoOptions, build_gonogo, False, "#6BBF3A", ["gonogo"], "Cognitive control"),
    Template("emoreg", "Emotion regulation", "cognitive reappraisal · event-related",
             "Each trial: a cue (LOOK or DECREASE, optionally INCREASE), a neutral or negative picture, a rating "
             "of how negative the participant feels, and a jittered relax period. Conditions: look_neutral, "
             "look_negative, decrease_negative.",
             "Ochsner KN et al. (2002) J Cogn Neurosci 14:1215–1229; Ochsner KN et al. (2004) NeuroImage 23:483",
             EmoRegOptions, build_emoreg, True, "#D45C9E", ["text", "image", "rating"], "Emotion & reward"),
    Template("mid", "Monetary incentive delay", "reward anticipation · event-related",
             "A cue shows what is at stake (circle = win, square = avoid a loss, triangle = nothing). After a "
             "jittered anticipation phase a white square flashes briefly – pressing while it is visible is a "
             "hit. Feedback shows the outcome. Separate conditions for cue, anticipation, target and feedback. "
             "Note: the target duration is fixed (not adapted to performance).",
             "Knutson B et al. (2000) NeuroImage 12:20–27; Knutson B et al. (2001) J Neurosci 21:RC159",
             MIDOptions, build_mid, True, "#B565D8", ["feedback"], "Emotion & reward"),
    Template("stroop", "Stroop", "interference · event-related",
             "Colour words in coloured ink. Press the button for the INK colour. Congruent (RED in red) vs. "
             "incongruent (RED in blue), optionally neutral (XXXX). Jittered fixation between trials. "
             "Words can be translated, e.g. “ROT, GRÜN, BLAU, GELB”.",
             "Stroop JR (1935) J Exp Psychol 18:643; e.g. Leung HC et al. (2000) Cereb Cortex 10:552",
             StroopOptions, build_stroop, False, "#E5B53B", ["choice"], "Cognitive control"),
    Template("flanker", "Eriksen flanker", "conflict monitoring · event-related",
             "Five arrows; the middle one gives the answer (left / right grip). Flankers point the same way "
             "(congruent) or the other way (incongruent), optionally neutral dashes. Jittered fixation.",
             "Eriksen BA & Eriksen CW (1974) Percept Psychophys 16:143; e.g. Hazeltine E et al. (2000) Brain 123:1929",
             FlankerOptions, build_flanker, False, "#E59A3B", ["choice"], "Cognitive control"),
    Template("nback", "N-back", "working memory · blocked",
             "Blocks of letters at different memory loads (default 0-back and 2-back), each starting with a cue. "
             "Press only for targets: the letter X (0-back) or a letter that matches the one N positions back. "
             "Rest blocks in between. Logged as hits, misses, false alarms and correct rejections.",
             "Braver TS et al. (1997) NeuroImage 5:49–62; Owen AM et al. (2005) Hum Brain Mapp 25:46",
             NBackOptions, build_nback, False, "#3BA7E5", ["choice"], "Working memory"),
    Template("motor", "Motor localizer", "finger tapping · blocked",
             "Blocks of finger tapping with the left or right hand (optionally both) alternate with rest. "
             "2-s response windows record whether the participant keeps tapping, visible live in the control "
             "room.",
             "e.g. Rao SM et al. (1993) Neurology 43:2311",
             MotorOptions, build_motor, False, "#2FB37A", ["response"], "Localizers & rest"),
    Template("checkerboard", "Visual checkerboard", "visual cortex localizer · blocked",
             "A radial checkerboard flickers (default 8 Hz contrast reversal) in ON blocks and alternates with "
             "a grey screen. A red fixation dot stays in the centre throughout.",
             "e.g. Fox PT & Raichle ME (1984) J Neurophysiol 51:1109",
             CheckerboardOptions, build_checkerboard, True, "#9AA3AD", ["image"], "Localizers & rest"),
    Template("resting", "Resting state", "intrinsic connectivity · continuous",
             "A short instruction, then a fixation cross (eyes open) or an empty screen for the whole run. "
             "Default 8 minutes.",
             "Biswal B et al. (1995) Magn Reson Med 34:537",
             RestingOptions, build_resting, False, "#4C8DF6", ["fixation"], "Localizers & rest"),
]

CATEGORIES = ["Emotion & reward", "Cognitive control", "Working memory", "Localizers & rest"]


def build(key: str, options, stim_dir: str) -> Paradigm:
    t = next(t for t in TEMPLATES if t.key == key)
    return t.build(options, stim_dir) if t.needs_stim_dir else t.build(options)
