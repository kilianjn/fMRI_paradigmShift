//! Ready-made paradigm modules (Hariri, Go/No-Go, emotion regulation, MID, Stroop, flanker,
//! N-back, motor localizer, checkerboard, resting state).
//!
//! Each builder returns a complete [`Paradigm`] that can be edited further in the timeline. The
//! defaults follow the commonly published versions of the tasks; all of them are options, so adapt
//! them to your protocol. Without your own stimulus folders, generated placeholder pictures are used.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::model::{basename, list_images, round4, Block, Paradigm};
use crate::placeholders as ph;
use crate::presets::derive;
use crate::rng::Rng;

// --------------------------------------------------------------------------
// Options (shown as a form in the template dialog)
// --------------------------------------------------------------------------
#[derive(Clone, Debug, PartialEq)]
pub enum OptVal {
    F(f64),
    I(i64),
    B(bool),
    S(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opts(pub Vec<(&'static str, OptVal)>);

impl Opts {
    fn get(&self, k: &str) -> Option<&OptVal> {
        self.0.iter().find(|(n, _)| *n == k).map(|(_, v)| v)
    }
    pub fn f(&self, k: &str) -> f64 {
        match self.get(k) {
            Some(OptVal::F(x)) => *x,
            Some(OptVal::I(x)) => *x as f64,
            _ => 0.0,
        }
    }
    pub fn i(&self, k: &str) -> i64 {
        match self.get(k) {
            Some(OptVal::I(x)) => *x,
            Some(OptVal::F(x)) => *x as i64,
            _ => 0,
        }
    }
    pub fn u(&self, k: &str) -> usize {
        self.i(k).max(0) as usize
    }
    pub fn b(&self, k: &str) -> bool {
        matches!(self.get(k), Some(OptVal::B(true)))
    }
    pub fn s(&self, k: &str) -> String {
        match self.get(k) {
            Some(OptVal::S(x)) => x.clone(),
            _ => String::new(),
        }
    }
    pub fn set(&mut self, k: &str, v: OptVal) {
        if let Some(e) = self.0.iter_mut().find(|(n, _)| *n == k) {
            e.1 = v;
        }
    }
}

fn f(n: &'static str, v: f64) -> (&'static str, OptVal) {
    (n, OptVal::F(v))
}
fn i(n: &'static str, v: i64) -> (&'static str, OptVal) {
    (n, OptVal::I(v))
}
fn b(n: &'static str, v: bool) -> (&'static str, OptVal) {
    (n, OptVal::B(v))
}
fn s(n: &'static str, v: &str) -> (&'static str, OptVal) {
    (n, OptVal::S(v.to_string()))
}

pub fn opt_label(name: &str) -> (&'static str, &'static str) {
    match name {
        "tr" => ("TR", " s"),
        "face_blocks" => ("Face blocks", ""),
        "trials_per_block" => ("Trials per block", ""),
        "trial_dur" => ("Trial length", " s"),
        "instruction_dur" => ("Instruction", " s"),
        "faces_dir" => ("Faces folder", ""),
        "shapes_dir" => ("Shapes folder", ""),
        "seed" => ("Random seed", ""),
        "n_trials" => ("Trials", ""),
        "nogo_percent" => ("No-Go trials", " %"),
        "stim_time" => ("Stimulus visible", " s"),
        "go_letters" => ("Go letters", ""),
        "nogo_letter" => ("No-Go letter", ""),
        "rest_every" => ("Rest after every", " trials"),
        "rest_dur" => ("Rest length", " s"),
        "jitter_max" => ("Extra jitter up to", " s"),
        "button" => ("Go button", ""),
        "trials_per_condition" => ("Trials per condition", ""),
        "include_increase" => ("Add INCREASE condition", ""),
        "cue_dur" => ("Cue", " s"),
        "picture_dur" => ("Picture", " s"),
        "rating_dur" => ("Rating", " s"),
        "iti_min" => ("Pause between, from", " s"),
        "iti_max" => ("Pause between, up to", " s"),
        "rating_max" => ("Rating scale 1 to", ""),
        "neutral_dir" => ("Neutral pictures", ""),
        "negative_dir" => ("Negative pictures", ""),
        "minutes" => ("Length", " min"),
        "fixation" => ("Fixation cross (eyes open)", ""),
        "instruction" => ("Instruction", ""),
        "include_both" => ("Add “both hands” blocks", ""),
        "block_dur" => ("Block length", " s"),
        "cycles" => ("Cycles", ""),
        "random_order" => ("Random order per cycle", ""),
        "on_dur" => ("Checkerboard ON", " s"),
        "off_dur" => ("Grey OFF", " s"),
        "flicker_hz" => ("Flicker", " Hz"),
        "levels" => ("N-back levels", ""),
        "blocks_per_level" => ("Blocks per level", ""),
        "target_percent" => ("Targets", " %"),
        "letters" => ("Letters", ""),
        "zero_back_target" => ("0-back target", ""),
        "target_button" => ("Target button", ""),
        "incongruent_percent" => ("Incongruent", " %"),
        "neutral_percent" => ("Neutral", " %"),
        "use_yellow" => ("4 colours (incl. yellow)", ""),
        "words" => ("Colour words", ""),
        "red_button" => ("Red ink →", ""),
        "green_button" => ("Green ink →", ""),
        "blue_button" => ("Blue ink →", ""),
        "yellow_button" => ("Yellow ink →", ""),
        "left_button" => ("Left answer", ""),
        "right_button" => ("Right answer", ""),
        "amount" => ("Amount", ""),
        "anticipation_min" => ("Anticipation from", " s"),
        "anticipation_max" => ("Anticipation up to", " s"),
        "target_dur" => ("Target visible", " s"),
        "feedback_dur" => ("Feedback", " s"),
        _ => ("", ""),
    }
}

pub fn opt_tip(name: &str) -> Option<&'static str> {
    Some(match name {
        "faces_dir" => "Folder with angry and fearful faces (e.g. NimStim or KDEF). Empty = built-in placeholder faces.",
        "shapes_dir" => "Folder with shape pictures. Empty = built-in circles and ellipses.",
        "neutral_dir" => "Folder with neutral pictures (e.g. IAPS/OASIS). Empty = built-in placeholders.",
        "negative_dir" => "Folder with negative pictures (e.g. IAPS/OASIS). Empty = built-in placeholders.",
        "nogo_percent" => "Share of No-Go trials. They never follow each other directly.",
        "trial_dur" => "Time per trial (stimulus onset to next onset).",
        "seed" => "Change to get a different random order. Same seed = same paradigm.",
        "levels" => "Comma-separated memory loads, e.g. “0, 2” or “0, 1, 2”.",
        "words" => "One word per colour in the order red, green, blue, yellow – translate as needed.",
        "target_dur" => "Pressing while the target is visible counts as a hit. Typical: 0.2–0.4 s.",
        "stim_time" => "How long the stimulus stays on screen (0 = whole trial).",
        _ => return None,
    })
}

// --------------------------------------------------------------------------
// helpers
// --------------------------------------------------------------------------
fn q(x: f64, step: f64) -> f64 {
    if step > 0.0 { round4((x / step).round() * step) } else { round4(x) }
}

/// Appends blocks back to back on the stimulus track.
struct Seq {
    par: Paradigm,
    t: f64,
}

impl Seq {
    fn new(par: Paradigm) -> Self {
        Seq { par, t: 0.0 }
    }
    fn add(&mut self, kind: &str, dur: f64, condition: &str, label: &str, params: serde_json::Value) -> usize {
        let mut blk = Block::create(kind, self.t, Some(dur)).cond(condition).lab(label).with(params);
        blk.track = 0;
        self.par.blocks.push(blk);
        self.t = round4(self.t + dur);
        self.par.blocks.len() - 1
    }
    fn derive_last(&mut self) {
        if let Some(b) = self.par.blocks.last_mut() {
            derive(b);
        }
    }
}

/// `n` items, as evenly balanced as possible, shuffled.
fn balanced<T: Clone>(items: &[T], n: usize, rng: &mut Rng) -> Vec<T> {
    let mut out: Vec<T> = items.iter().cycle().take(n).cloned().collect();
    rng.shuffle(&mut out);
    out
}

fn no_long_runs(mut items: Vec<String>, rng: &mut Rng, max_run: usize) -> Vec<String> {
    for _ in 0..2000 {
        rng.shuffle(&mut items);
        let ok = items.len() <= max_run
            || (0..items.len() - max_run).all(|k| items[k..k + max_run + 1].iter().any(|x| *x != items[k]));
        if ok {
            return items;
        }
    }
    items
}

fn pyround(x: f64) -> usize {
    x.round().max(0.0) as usize
}

// =============================================================================
// Hariri emotional face matching
// =============================================================================
/// Sort a face folder into emotions by file name (e.g. NimStim `01F_AN_C`, KDEF `AF01AFS`).
pub fn faces_from_folder(folder: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    let keys: [(&str, &[&str]); 2] = [
        ("angry", &["angry", "anger", "_an_", "_an.", "-an-", "-an."]),
        ("fearful", &["fear", "afraid", "_fe_", "_fe.", "-fe-", "-fe."]),
    ];
    let kdef_an = regex::Regex::new(r"[a-z]{2}\d\dan").unwrap();
    let kdef_af = regex::Regex::new(r"[a-z]{2}\d\daf").unwrap();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    out.insert("angry".into(), vec![]);
    out.insert("fearful".into(), vec![]);
    for fp in list_images(folder) {
        let name = basename(&fp).to_lowercase();
        for (emo, ks) in keys.iter() {
            let hit = ks.iter().any(|k| name.contains(k))
                || (*emo == "angry" && kdef_an.is_match(&name))
                || (*emo == "fearful" && kdef_af.is_match(&name));
            if hit {
                out.get_mut(*emo).unwrap().push(fp.clone());
                break;
            }
        }
    }
    if out.values().map(|v| v.len()).min().unwrap_or(0) < 2 {
        return Err("The face folder needs at least two angry and two fearful faces. File names must contain \
                    e.g. 'angry'/'fear' or the NimStim codes '_AN_'/'_FE_'."
            .into());
    }
    Ok(out)
}

fn hariri_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        i("face_blocks", 4),
        i("trials_per_block", 6),
        f("trial_dur", 5.0),
        f("instruction_dur", 2.0),
        s("faces_dir", ""),
        s("shapes_dir", ""),
        i("seed", 1),
    ])
}

/// Hariri et al. (2002): match the emotion of the top face (angry / fearful), or the shape.
/// Blocks alternate S F S F … S, each preceded by an instruction. Responses: left / right grip.
pub fn build_hariri(o: &Opts, stim_dir: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let faces = if o.s("faces_dir").is_empty() {
        ph::hariri_faces(&stim_dir.join("faces"), 6)
    } else {
        faces_from_folder(&o.s("faces_dir"))?
    };
    let shape_files = list_images(&o.s("shapes_dir"));
    let shapes: BTreeMap<String, String> = if shape_files.len() >= 2 {
        shape_files.iter().map(|f| (basename(f), f.clone())).collect()
    } else {
        ph::hariri_shapes(&stim_dir.join("shapes"))
    };
    let shape_keys: Vec<String> = shapes.keys().cloned().collect();
    let emos_all: Vec<String> = faces.keys().cloned().collect();
    let mut seq = Seq::new(Paradigm::named("Hariri face matching", o.f("tr"), 3));
    let n_blocks = 2 * o.u("face_blocks") + 1;
    let tpb = o.u("trials_per_block");
    for bi in 0..n_blocks {
        let is_face = bi % 2 == 1;
        let cond = if is_face { "faces" } else { "shapes" };
        seq.add(
            "text",
            o.f("instruction_dur"),
            "instruction",
            &format!("Match {cond}"),
            json!({"text": if is_face {"Match Faces"} else {"Match Shapes"}, "size": 64}),
        );
        let sides = balanced(&["left", "right"], tpb, &mut rng);
        let emos = balanced(&emos_all, tpb, &mut rng);
        for k in 0..tpb {
            let (top, matching, foil) = if is_face {
                let emo = &emos[k];
                let other = emos_all.iter().find(|e| *e != emo).unwrap();
                let same = rng.sample(&faces[emo], 2); // two different people, same emotion
                let foil = rng.choice(&faces[other]).clone();
                (same[0].clone(), same.get(1).cloned().unwrap_or_else(|| same[0].clone()), foil)
            } else {
                let sk = rng.choice(&shape_keys).clone();
                let others: Vec<String> = shape_keys.iter().filter(|x| **x != sk).cloned().collect();
                let ok = rng.choice(&others).clone();
                (shapes[&sk].clone(), shapes[&sk].clone(), shapes[&ok].clone())
            };
            let (left, right) = if sides[k] == "left" { (matching, foil) } else { (foil, matching) };
            seq.add(
                "match",
                o.f("trial_dur"),
                cond,
                "",
                json!({"top": top, "left": left, "right": right, "correct": sides[k]}),
            );
        }
    }
    Ok(seq.par)
}

// =============================================================================
// Go / No-Go
// =============================================================================
fn gonogo_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        i("n_trials", 160),
        f("nogo_percent", 25.0),
        f("stim_time", 0.5),
        f("trial_dur", 1.5),
        s("go_letters", "ABCDEFGHJKLMNPRSTUVWYZ"),
        s("nogo_letter", "X"),
        i("rest_every", 40),
        f("rest_dur", 16.0),
        f("jitter_max", 0.0),
        s("button", "any"),
        i("seed", 1),
    ])
}

/// Go/No-Go order: the first `lead_go` trials are Go, and No-Go never repeats back to back.
pub fn gonogo_sequence(n: usize, n_nogo: usize, rng: &mut Rng, lead_go: usize) -> Result<Vec<&'static str>, String> {
    let n_go = n.saturating_sub(n_nogo);
    let slots = (n_go + 1).saturating_sub(lead_go);
    if n_nogo > slots {
        return Err("Too many No-Go trials to keep them apart – lower the No-Go percentage.".into());
    }
    let chosen = rng.sample_range(0, slots, n_nogo);
    let mut seq = vec![];
    for k in 0..n_go {
        seq.push("go");
        if k + 1 >= lead_go && chosen.contains(&(k + 1 - lead_go)) {
            seq.push("nogo");
        }
    }
    Ok(seq)
}

/// Letter Go/No-Go: press for every letter, withhold for the No-Go letter (rapid event-related).
pub fn build_gonogo(o: &Opts, _stim: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let mut seq = Seq::new(Paradigm::named("Go-NoGo", o.f("tr"), 3));
    let nogo_letter = o.s("nogo_letter");
    seq.add(
        "text",
        8.0,
        "instruction",
        "Instructions",
        json!({"text": format!("Press the button as fast as you can for every letter.\n\nDo NOT press for  {nogo_letter}"),
               "size": 44}),
    );
    seq.add("fixation", 4.0, "", "Get ready", json!({}));
    let n_trials = o.u("n_trials");
    let n_nogo = pyround(n_trials as f64 * o.f("nogo_percent") / 100.0);
    let trials = gonogo_sequence(n_trials, n_nogo, &mut rng, 3)?;
    let nogo_up = nogo_letter.to_uppercase();
    let mut go_letters: Vec<String> = o
        .s("go_letters")
        .to_uppercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_string())
        .filter(|c| *c != nogo_up)
        .collect();
    if go_letters.is_empty() {
        go_letters.push("O".into());
    }
    let rest_every = o.u("rest_every");
    for (k, kind) in trials.iter().enumerate() {
        let letter = if *kind == "nogo" { nogo_letter.clone() } else { rng.choice(&go_letters).clone() };
        seq.add(
            "gonogo",
            o.f("trial_dur"),
            kind,
            "",
            json!({"trial": kind, "text": letter, "stim_time": o.f("stim_time"), "button": o.s("button")}),
        );
        if o.f("jitter_max") > 0.0 {
            let j = q(rng.uniform(0.0, o.f("jitter_max")), 0.25);
            if j > 0.0 {
                seq.add("fixation", j, "", "jitter", json!({}));
            }
        }
        if rest_every > 0 && (k + 1) % rest_every == 0 && k + 1 < trials.len() {
            seq.add("fixation", o.f("rest_dur"), "", "Rest", json!({}));
        }
    }
    seq.add("fixation", o.f("rest_dur"), "", "Rest", json!({}));
    Ok(seq.par)
}

// =============================================================================
// Emotion regulation (cognitive reappraisal)
// =============================================================================
fn emoreg_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        i("trials_per_condition", 8),
        b("include_increase", false),
        f("cue_dur", 2.0),
        f("picture_dur", 7.0),
        f("rating_dur", 4.0),
        f("iti_min", 2.0),
        f("iti_max", 6.0),
        i("rating_max", 4),
        s("neutral_dir", ""),
        s("negative_dir", ""),
        i("seed", 1),
    ])
}

fn emoreg_cue(c: &str) -> &'static str {
    match c {
        "decrease_negative" => "DECREASE",
        "increase_negative" => "INCREASE",
        _ => "LOOK",
    }
}

/// Ochsner-style reappraisal: cue (LOOK / DECREASE [/ INCREASE]) → picture → affect rating → relax.
pub fn build_emoreg(o: &Opts, stim_dir: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let mut conds = vec!["look_neutral", "look_negative", "decrease_negative"];
    if o.b("include_increase") {
        conds.push("increase_negative");
    }
    let n = o.u("trials_per_condition");
    let n_neg = n * (conds.len() - 1);
    let mut neutral = list_images(&o.s("neutral_dir"));
    if neutral.is_empty() {
        neutral = ph::scenes(&stim_dir.join("neutral"), "neutral", n.max(1));
    }
    let mut negative = list_images(&o.s("negative_dir"));
    if negative.is_empty() {
        negative = ph::scenes(&stim_dir.join("negative"), "negative", n_neg.max(1));
    }
    rng.shuffle(&mut neutral);
    rng.shuffle(&mut negative);
    let mut neutral_it = neutral.iter().cycle();
    let mut negative_it = negative.iter().cycle();

    let mut seq = Seq::new(Paradigm::named("Emotion regulation", o.f("tr"), 3));
    let mut instr = "LOOK: just look at the picture and let yourself feel whatever it evokes.\n\
                     DECREASE: reinterpret the picture so it feels less negative."
        .to_string();
    if o.b("include_increase") {
        instr.push_str("\nINCREASE: reinterpret it so it feels more negative.");
    }
    seq.add(
        "text",
        10.0,
        "instruction",
        "Instructions",
        json!({"text": format!("{instr}\n\nAfterwards rate how negative you feel."), "size": 32}),
    );
    seq.add("fixation", 10.0, "", "Baseline", json!({}));
    let all: Vec<String> = conds.iter().flat_map(|c| std::iter::repeat_n(c.to_string(), n)).collect();
    let order = no_long_runs(all, &mut rng, 2);
    let (lo, hi) = if o.f("iti_min") <= o.f("iti_max") { (o.f("iti_min"), o.f("iti_max")) } else { (o.f("iti_max"), o.f("iti_min")) };
    for cond in order {
        let cue = emoreg_cue(&cond);
        seq.add("text", o.f("cue_dur"), "cue", cue, json!({"text": cue, "size": 72}));
        let pic = if cond == "look_neutral" { neutral_it.next() } else { negative_it.next() }.cloned().unwrap_or_default();
        seq.add("image", o.f("picture_dur"), &cond, "", json!({"path": pic, "scale": 0.85}));
        seq.add(
            "rating",
            o.f("rating_dur"),
            "rating",
            "",
            json!({"question": "How negative do you feel?", "path": "", "min": 1, "max": o.i("rating_max"),
                   "left_label": "not at all", "right_label": "very"}),
        );
        seq.add("fixation", q(rng.uniform(lo, hi), 0.5).max(0.5), "", "Relax", json!({}));
    }
    seq.add("fixation", 10.0, "", "Baseline", json!({}));
    Ok(seq.par)
}

// =============================================================================
// Resting state
// =============================================================================
fn resting_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        f("minutes", 8.0),
        b("fixation", true),
        s("instruction", "Please keep your eyes open and look at the cross.\nLie still, relax and let your thoughts wander."),
        f("instruction_dur", 8.0),
    ])
}

/// Resting-state run: optional instruction, then one long fixation (eyes open) or empty screen.
pub fn build_resting(o: &Opts, _stim: &Path) -> Result<Paradigm, String> {
    let mut par = Paradigm::named("Resting state", o.f("tr"), 3);
    par.waiting_screen = if o.b("fixation") { "fixation" } else { "blank" }.into();
    let mut seq = Seq::new(par);
    if !o.s("instruction").trim().is_empty() && o.f("instruction_dur") > 0.0 {
        seq.add(
            "text",
            o.f("instruction_dur"),
            "instruction",
            "Instructions",
            json!({"text": o.s("instruction"), "size": 36}),
        );
    }
    let mut dur = o.f("minutes") * 60.0;
    if o.f("tr") > 0.0 {
        dur = (dur / o.f("tr")).round() * o.f("tr");
    }
    if o.b("fixation") {
        seq.add("fixation", dur, "rest", "Rest (eyes open)", json!({}));
    } else {
        seq.add("rest", dur, "rest", "Rest", json!({}));
    }
    Ok(seq.par)
}

// =============================================================================
// Motor localizer (finger tapping)
// =============================================================================
fn motor_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        b("include_both", false),
        f("block_dur", 20.0),
        f("rest_dur", 20.0),
        i("cycles", 5),
        b("random_order", false),
        i("seed", 1),
    ])
}

/// Blocked finger tapping (left / right [/ both]) alternating with rest.
pub fn build_motor(o: &Opts, _stim: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let mut conds = vec![("tap_left", "TAP\nLEFT HAND"), ("tap_right", "TAP\nRIGHT HAND")];
    if o.b("include_both") {
        conds.push(("tap_both", "TAP\nBOTH HANDS"));
    }
    let mut seq = Seq::new(Paradigm::named("Motor localizer", o.f("tr"), 3));
    seq.add(
        "text",
        8.0,
        "instruction",
        "Instructions",
        json!({"text": "When a hand is shown, tap your fingers against the thumb\nof that hand – quickly and steadily.\n\nWhen you see the cross, rest.",
               "size": 34}),
    );
    seq.add("fixation", o.f("rest_dur"), "", "Rest", json!({}));
    for _ in 0..o.u("cycles") {
        let mut order = conds.clone();
        if o.b("random_order") {
            rng.shuffle(&mut order);
        }
        for (cond, text) in order {
            seq.add("text", o.f("block_dur"), cond, &text.replace('\n', " "), json!({"text": text, "size": 72}));
            seq.add("fixation", o.f("rest_dur"), "", "Rest", json!({}));
        }
    }
    Ok(seq.par)
}

// =============================================================================
// Visual localizer (flickering checkerboard)
// =============================================================================
fn checkerboard_defaults() -> Opts {
    Opts(vec![f("tr", 2.0), f("on_dur", 20.0), f("off_dur", 20.0), i("cycles", 6), f("flicker_hz", 8.0), i("seed", 1)])
}

/// Flickering radial checkerboard ON vs. grey screen OFF, red fixation dot throughout.
pub fn build_checkerboard(o: &Opts, stim_dir: &Path) -> Result<Paradigm, String> {
    let (a, bb) = ph::checkerboards(&stim_dir.join("checkerboard"));
    let mut par = Paradigm::named("Visual checkerboard", o.f("tr"), 3);
    par.background = "#808080".into();
    let mut seq = Seq::new(par);
    let dot = json!({"symbol": "•", "color": "#E53935", "size": 40});
    seq.add("fixation", o.f("off_dur"), "", "Off", dot.clone());
    for _ in 0..o.u("cycles") {
        seq.add(
            "image",
            o.f("on_dur"),
            "checkerboard",
            "Checkerboard",
            json!({"path": a, "flicker_path": bb, "flicker_hz": o.f("flicker_hz"), "scale": 1.0}),
        );
        seq.add("fixation", o.f("off_dur"), "", "Off", dot.clone());
    }
    Ok(seq.par)
}

// =============================================================================
// N-back working memory
// =============================================================================
fn nback_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        s("levels", "0, 2"),
        i("blocks_per_level", 4),
        i("trials_per_block", 15),
        f("target_percent", 33.0),
        f("stim_time", 0.5),
        f("trial_dur", 2.0),
        f("instruction_dur", 4.0),
        f("rest_dur", 14.0),
        s("letters", "BCDFGHJKLMNPQRSTVWZ"),
        s("zero_back_target", "X"),
        s("target_button", "Right index"),
        i("seed", 1),
    ])
}

/// `[(letter, is_target), …]` for one block. n = 0: the target is `zero_target`.
pub fn nback_sequence(
    n: usize,
    count: usize,
    n_targets: usize,
    letters: &str,
    rng: &mut Rng,
    zero_target: &str,
) -> Result<Vec<(String, bool)>, String> {
    let zt = zero_target.to_uppercase();
    let pool: Vec<String> = letters
        .to_uppercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_string())
        .filter(|c| *c != zt)
        .collect();
    if pool.len() < 2 {
        return Err("Give at least two letters.".into());
    }
    if n == 0 {
        let tpos = rng.sample_range(0, count, n_targets.min(count));
        return Ok((0..count)
            .map(|k| if tpos.contains(&k) { (zt.clone(), true) } else { (rng.choice(&pool).clone(), false) })
            .collect());
    }
    if n_targets + n > count {
        return Err(format!("Too many targets for a {n}-back block of {count} trials."));
    }
    let tpos = rng.sample_range(n, count, n_targets);
    let mut seq: Vec<(String, bool)> = vec![];
    for k in 0..count {
        if tpos.contains(&k) {
            let l = seq[k - n].0.clone();
            seq.push((l, true));
        } else {
            let avoid = if k >= n { Some(seq[k - n].0.clone()) } else { None };
            let cands: Vec<String> = pool.iter().filter(|c| Some(*c) != avoid.as_ref()).cloned().collect();
            seq.push((rng.choice(&cands).clone(), false));
        }
    }
    Ok(seq)
}

/// Blocked N-back: a cue names the level, letters follow; press only for targets.
pub fn build_nback(o: &Opts, _stim: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let mut levels = vec![];
    for x in o.s("levels").replace(';', ",").split(',') {
        let x = x.trim();
        if x.is_empty() {
            continue;
        }
        levels.push(x.parse::<usize>().map_err(|_| "Levels must be numbers, e.g. “0, 2”.".to_string())?);
    }
    if levels.is_empty() {
        return Err("Give at least one N-back level, e.g. “0, 2”.".into());
    }
    let zt = o.s("zero_back_target");
    let mut seq = Seq::new(Paradigm::named("N-back", o.f("tr"), 3));
    seq.add(
        "text",
        10.0,
        "instruction",
        "Instructions",
        json!({"text": format!("Letters will appear one after another.\n\n0-BACK: press when you see {zt}.\n2-BACK: press when the letter is the same as two letters before.\n\nOtherwise do not press."),
               "size": 32}),
    );
    seq.add("fixation", o.f("rest_dur"), "", "Rest", json!({}));
    let tpb = o.u("trials_per_block");
    let n_targets = pyround(tpb as f64 * o.f("target_percent") / 100.0);
    for _ in 0..o.u("blocks_per_level") {
        let mut order = levels.clone();
        rng.shuffle(&mut order);
        for n in order {
            let cond = format!("{n}back");
            let rule = match n {
                0 => format!("press for {zt}"),
                1 => "same as the one before".to_string(),
                _ => format!("same as {n} before"),
            };
            seq.add("text", o.f("instruction_dur"), "cue", &format!("{n}-BACK"), json!({"text": format!("{n}-BACK\n\n{rule}"), "size": 56}));
            for (letter, target) in nback_sequence(n, tpb, n_targets, &o.s("letters"), &mut rng, &zt)? {
                seq.add(
                    "choice",
                    o.f("trial_dur"),
                    &cond,
                    "",
                    json!({"preset": "nback", "letter": letter, "target": target, "level": n,
                           "target_button": o.s("target_button"), "stim_time": o.f("stim_time"),
                           "buttons": "any", "size": 140}),
                );
                seq.derive_last();
            }
            seq.add("fixation", o.f("rest_dur"), "", "Rest", json!({}));
        }
    }
    Ok(seq.par)
}

// =============================================================================
// Stroop (colour-word)
// =============================================================================
fn stroop_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        i("n_trials", 96),
        f("incongruent_percent", 50.0),
        f("neutral_percent", 0.0),
        b("use_yellow", true),
        s("words", "RED, GREEN, BLUE, YELLOW"),
        f("stim_time", 0.0),
        f("trial_dur", 2.0),
        f("iti_min", 1.0),
        f("iti_max", 4.0),
        s("red_button", "Left thumb"),
        s("green_button", "Left index"),
        s("blue_button", "Right index"),
        s("yellow_button", "Right thumb"),
        i("seed", 1),
    ])
}

fn condition_mix(o: &Opts, rng: &mut Rng) -> Result<Vec<String>, String> {
    let n = o.u("n_trials");
    let n_inc = pyround(n as f64 * o.f("incongruent_percent") / 100.0);
    let n_neu = pyround(n as f64 * o.f("neutral_percent") / 100.0);
    if n_inc + n_neu > n {
        return Err("Incongruent + neutral percentages exceed 100 %.".into());
    }
    let n_con = n - n_inc - n_neu;
    let mut all = vec!["congruent".to_string(); n_con];
    all.extend(vec!["incongruent".to_string(); n_inc]);
    all.extend(vec!["neutral".to_string(); n_neu]);
    Ok(no_long_runs(all, rng, 3))
}

/// Colour-word Stroop: name the INK colour with the grips; congruent vs incongruent (vs neutral).
pub fn build_stroop(o: &Opts, _stim: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let mut colors = vec!["red", "green", "blue"];
    if o.b("use_yellow") {
        colors.push("yellow");
    }
    let words: Vec<String> = o.s("words").split(',').map(|w| w.trim().to_string()).collect();
    if words.len() < colors.len() || words.iter().take(colors.len()).any(|w| w.is_empty()) {
        return Err(format!("Give one word per colour ({}).", colors.join(", ")));
    }
    let mut word = serde_json::Map::new();
    for (c, w) in ["red", "green", "blue", "yellow"].iter().zip(words.iter()) {
        word.insert(c.to_string(), json!(w));
    }
    let button = json!({"red": o.s("red_button"), "green": o.s("green_button"),
                        "blue": o.s("blue_button"), "yellow": o.s("yellow_button")});
    let conds = condition_mix(o, &mut rng)?;
    let mut seq = Seq::new(Paradigm::named("Stroop", o.f("tr"), 3));
    let mapping: Vec<String> = colors
        .iter()
        .map(|c| format!("{} ink  →  {}", word[*c].as_str().unwrap_or(""), button[*c].as_str().unwrap_or("")))
        .collect();
    seq.add(
        "text",
        12.0,
        "instruction",
        "Instructions",
        json!({"text": format!("Press the button for the INK COLOUR of the word –\nnot for what the word says.\n\n{}", mapping.join("\n")),
               "size": 32}),
    );
    seq.add("fixation", 6.0, "", "Get ready", json!({}));
    let inks = balanced(&colors, conds.len(), &mut rng);
    let (lo, hi) = if o.f("iti_min") <= o.f("iti_max") { (o.f("iti_min"), o.f("iti_max")) } else { (o.f("iti_max"), o.f("iti_min")) };
    for (cond, ink) in conds.iter().zip(inks.iter()) {
        let wk = match cond.as_str() {
            "congruent" => ink.to_string(),
            "neutral" => "neutral".to_string(),
            _ => {
                let others: Vec<&str> = colors.iter().filter(|c| *c != ink).copied().collect();
                rng.choice(&others).to_string()
            }
        };
        seq.add(
            "choice",
            o.f("trial_dur"),
            cond,
            "",
            json!({"preset": "stroop", "word": wk, "ink": ink, "words": word.clone(), "ink_buttons": button.clone(),
                   "size": 110, "stim_time": o.f("stim_time"), "buttons": "any"}),
        );
        seq.derive_last();
        let iti = q(rng.uniform(lo, hi), 0.25);
        if iti > 0.0 {
            seq.add("fixation", iti, "", "ITI", json!({}));
        }
    }
    seq.add("fixation", 10.0, "", "End", json!({}));
    Ok(seq.par)
}

// =============================================================================
// Eriksen flanker
// =============================================================================
fn flanker_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        i("n_trials", 96),
        f("incongruent_percent", 50.0),
        f("neutral_percent", 0.0),
        f("stim_time", 0.5),
        f("trial_dur", 1.5),
        f("iti_min", 1.0),
        f("iti_max", 4.0),
        s("left_button", "Left index"),
        s("right_button", "Right index"),
        i("seed", 1),
    ])
}

/// Arrow flanker: respond to the direction of the middle arrow.
pub fn build_flanker(o: &Opts, _stim: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let conds = condition_mix(o, &mut rng)?;
    let mut seq = Seq::new(Paradigm::named("Flanker", o.f("tr"), 3));
    seq.add(
        "text",
        10.0,
        "instruction",
        "Instructions",
        json!({"text": format!("Look at the MIDDLE arrow only.\n\n←  points left:  {}\n→  points right:  {}\n\nAnswer as fast and as accurately as you can.",
                               o.s("left_button"), o.s("right_button")),
               "size": 32}),
    );
    seq.add("fixation", 6.0, "", "Get ready", json!({}));
    let sides = balanced(&["left", "right"], conds.len(), &mut rng);
    let (lo, hi) = if o.f("iti_min") <= o.f("iti_max") { (o.f("iti_min"), o.f("iti_max")) } else { (o.f("iti_max"), o.f("iti_min")) };
    for (cond, side) in conds.iter().zip(sides.iter()) {
        seq.add(
            "choice",
            o.f("trial_dur"),
            cond,
            "",
            json!({"preset": "flanker", "direction": side, "congruency": cond, "size": 110,
                   "stim_time": o.f("stim_time"), "left_button": o.s("left_button"),
                   "right_button": o.s("right_button"), "buttons": "any"}),
        );
        seq.derive_last();
        let iti = q(rng.uniform(lo, hi), 0.25);
        if iti > 0.0 {
            seq.add("fixation", iti, "", "ITI", json!({}));
        }
    }
    seq.add("fixation", 10.0, "", "End", json!({}));
    Ok(seq.par)
}

// =============================================================================
// Monetary incentive delay (MID)
// =============================================================================
fn mid_defaults() -> Opts {
    Opts(vec![
        f("tr", 2.0),
        i("trials_per_condition", 15),
        s("amount", "1 €"),
        f("cue_dur", 2.0),
        f("anticipation_min", 2.0),
        f("anticipation_max", 2.5),
        f("target_dur", 0.3),
        f("feedback_dur", 2.0),
        f("iti_min", 1.5),
        f("iti_max", 4.0),
        i("seed", 1),
    ])
}

/// Knutson-style MID: cue (win / lose / nothing) → anticipation → brief target → feedback.
pub fn build_mid(o: &Opts, stim_dir: &Path) -> Result<Paradigm, String> {
    let mut rng = Rng::seeded(o.i("seed") as u64);
    let cues = ph::mid_cues(&stim_dir.join("mid"));
    let amt = o.s("amount").trim().to_string();
    let mut seq = Seq::new(Paradigm::named("Monetary incentive delay", o.f("tr"), 3));
    seq.add(
        "text",
        12.0,
        "instruction",
        "Instructions",
        json!({"text": format!("A shape tells you what is at stake:\ncircle = you can WIN {amt}   ·   square = you can avoid LOSING {amt}   ·   triangle = no money\n\nThen press a button as fast as you can when the white square appears."),
               "size": 30}),
    );
    seq.add("fixation", 6.0, "", "Get ready", json!({}));
    let all: Vec<String> = (0..o.u("trials_per_condition"))
        .flat_map(|_| ["gain", "loss", "neutral"].map(String::from))
        .collect();
    let order = no_long_runs(all, &mut rng, 2);
    let sorted = |a: f64, b: f64| if a <= b { (a, b) } else { (b, a) };
    for c in order {
        let (hit, miss) = match c.as_str() {
            "gain" => (format!("+{amt}"), "+0".to_string()),
            "loss" => ("−0".to_string(), format!("−{amt}")),
            _ => ("0".to_string(), "0".to_string()),
        };
        seq.add("image", o.f("cue_dur"), &format!("cue_{c}"), &format!("cue {c}"), json!({"path": cues[&c], "scale": 0.45}));
        let (lo, hi) = sorted(o.f("anticipation_min"), o.f("anticipation_max"));
        seq.add("fixation", q(rng.uniform(lo, hi), 0.1), &format!("anticipation_{c}"), "anticipation", json!({}));
        seq.add(
            "choice",
            o.f("target_dur"),
            &format!("target_{c}"),
            "target",
            json!({"text": "■", "size": 220, "stim_time": 0.0, "correct": "any", "buttons": "any", "isi_symbol": ""}),
        );
        let neutral = c == "neutral";
        seq.add(
            "feedback",
            o.f("feedback_dur"),
            &format!("feedback_{c}"),
            "",
            json!({"hit_text": hit, "miss_text": miss, "size": 96,
                   "hit_color": if neutral {"#CCCCCC"} else {"#2FB37A"},
                   "miss_color": if neutral {"#CCCCCC"} else {"#F0506E"}}),
        );
        let (lo, hi) = sorted(o.f("iti_min"), o.f("iti_max"));
        seq.add("fixation", q(rng.uniform(lo, hi), 0.25), "", "ITI", json!({}));
    }
    Ok(seq.par)
}

// =============================================================================
// Registry used by the template dialog
// =============================================================================
pub struct Template {
    pub key: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub description: &'static str,
    pub reference: &'static str,
    pub color: &'static str,
    pub icon_kind: &'static str,
    pub category: &'static str,
    pub defaults: fn() -> Opts,
    pub build: fn(&Opts, &Path) -> Result<Paradigm, String>,
}

pub static TEMPLATES: &[Template] = &[
    Template {
        key: "hariri",
        title: "Hariri face matching",
        subtitle: "amygdala · blocked",
        description: "Participants pick which of two bottom faces shows the same emotion (angry / fearful) as the top \
                      face. Shape-matching blocks are the sensorimotor control. Blocks alternate Shapes / Faces, each \
                      starting with a short instruction. Answers: left / right grip button.",
        reference: "Hariri AR et al. (2002) Science 297:400–403",
        color: "#1FA8B8",
        icon_kind: "match",
        category: "Emotion & reward",
        defaults: hariri_defaults,
        build: build_hariri,
    },
    Template {
        key: "gonogo",
        title: "Go / No-Go",
        subtitle: "response inhibition · event-related",
        description: "Letters flash briefly. Press for every letter except the No-Go letter. No-Go trials never follow \
                      each other and the run starts with Go trials. Logged as hits, misses, false alarms and correct \
                      rejections.",
        reference: "e.g. Garavan H et al. (1999) PNAS 96:8301–8306; Simmonds DJ et al. (2008) Neuropsychologia 46:224",
        color: "#6BBF3A",
        icon_kind: "gonogo",
        category: "Cognitive control",
        defaults: gonogo_defaults,
        build: build_gonogo,
    },
    Template {
        key: "emoreg",
        title: "Emotion regulation",
        subtitle: "cognitive reappraisal · event-related",
        description: "Each trial: a cue (LOOK or DECREASE, optionally INCREASE), a neutral or negative picture, a rating \
                      of how negative the participant feels, and a jittered relax period. Conditions: look_neutral, \
                      look_negative, decrease_negative.",
        reference: "Ochsner KN et al. (2002) J Cogn Neurosci 14:1215–1229; Ochsner KN et al. (2004) NeuroImage 23:483",
        color: "#D45C9E",
        icon_kind: "text",
        category: "Emotion & reward",
        defaults: emoreg_defaults,
        build: build_emoreg,
    },
    Template {
        key: "mid",
        title: "Monetary incentive delay",
        subtitle: "reward anticipation · event-related",
        description: "A cue shows what is at stake (circle = win, square = avoid a loss, triangle = nothing). After a \
                      jittered anticipation phase a white square flashes briefly – pressing while it is visible is a \
                      hit. Feedback shows the outcome. Separate conditions for cue, anticipation, target and feedback. \
                      Note: the target duration is fixed (not adapted to performance).",
        reference: "Knutson B et al. (2000) NeuroImage 12:20–27; Knutson B et al. (2001) J Neurosci 21:RC159",
        color: "#B565D8",
        icon_kind: "feedback",
        category: "Emotion & reward",
        defaults: mid_defaults,
        build: build_mid,
    },
    Template {
        key: "stroop",
        title: "Stroop",
        subtitle: "interference · event-related",
        description: "Colour words in coloured ink. Press the button for the INK colour. Congruent (RED in red) vs. \
                      incongruent (RED in blue), optionally neutral (XXXX). Jittered fixation between trials. \
                      Words can be translated, e.g. “ROT, GRÜN, BLAU, GELB”.",
        reference: "Stroop JR (1935) J Exp Psychol 18:643; e.g. Leung HC et al. (2000) Cereb Cortex 10:552",
        color: "#E5B53B",
        icon_kind: "choice",
        category: "Cognitive control",
        defaults: stroop_defaults,
        build: build_stroop,
    },
    Template {
        key: "flanker",
        title: "Eriksen flanker",
        subtitle: "conflict monitoring · event-related",
        description: "Five arrows; the middle one gives the answer (left / right grip). Flankers point the same way \
                      (congruent) or the other way (incongruent), optionally neutral dashes. Jittered fixation.",
        reference: "Eriksen BA & Eriksen CW (1974) Percept Psychophys 16:143; e.g. Hazeltine E et al. (2000) Brain 123:1929",
        color: "#E59A3B",
        icon_kind: "choice",
        category: "Cognitive control",
        defaults: flanker_defaults,
        build: build_flanker,
    },
    Template {
        key: "nback",
        title: "N-back",
        subtitle: "working memory · blocked",
        description: "Blocks of letters at different memory loads (default 0-back and 2-back), each starting with a cue. \
                      Press only for targets: the letter X (0-back) or a letter that matches the one N positions back. \
                      Rest blocks in between. Logged as hits, misses, false alarms and correct rejections.",
        reference: "Braver TS et al. (1997) NeuroImage 5:49–62; Owen AM et al. (2005) Hum Brain Mapp 25:46",
        color: "#3BA7E5",
        icon_kind: "choice",
        category: "Working memory",
        defaults: nback_defaults,
        build: build_nback,
    },
    Template {
        key: "motor",
        title: "Motor localizer",
        subtitle: "finger tapping · blocked",
        description: "Blocks of finger tapping with the left or right hand (optionally both) alternate with rest. \
                      Taps arrive as grip presses: they are logged and counted live in the control room.",
        reference: "e.g. Rao SM et al. (1993) Neurology 43:2311",
        color: "#2FB37A",
        icon_kind: "response",
        category: "Localizers & rest",
        defaults: motor_defaults,
        build: build_motor,
    },
    Template {
        key: "checkerboard",
        title: "Visual checkerboard",
        subtitle: "visual cortex localizer · blocked",
        description: "A radial checkerboard flickers (default 8 Hz contrast reversal) in ON blocks and alternates with \
                      a grey screen. A red fixation dot stays in the centre throughout.",
        reference: "e.g. Fox PT & Raichle ME (1984) J Neurophysiol 51:1109",
        color: "#9AA3AD",
        icon_kind: "image",
        category: "Localizers & rest",
        defaults: checkerboard_defaults,
        build: build_checkerboard,
    },
    Template {
        key: "resting",
        title: "Resting state",
        subtitle: "intrinsic connectivity · continuous",
        description: "A short instruction, then a fixation cross (eyes open) or an empty screen for the whole run. \
                      Default 8 minutes.",
        reference: "Biswal B et al. (1995) Magn Reson Med 34:537",
        color: "#4C8DF6",
        icon_kind: "fixation",
        category: "Localizers & rest",
        defaults: resting_defaults,
        build: build_resting,
    },
];

pub const CATEGORIES: &[&str] = &["Emotion & reward", "Cognitive control", "Working memory", "Localizers & rest"];

pub fn template(key: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.key == key)
}

pub fn build(key: &str, o: &Opts, stim_dir: &Path) -> Result<Paradigm, String> {
    let t = template(key).ok_or("unknown template")?;
    (t.build)(o, stim_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParamExt;

    fn stim() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ps_tpl_{}", crate::model::new_id()))
    }

    fn no_overlaps(p: &Paradigm) {
        for b in &p.blocks {
            assert!(!p.has_overlap(b), "{} overlaps", b.display_label());
        }
    }

    #[test]
    fn every_template_builds_and_validates() {
        let sd = stim();
        for t in TEMPLATES {
            let p = (t.build)(&(t.defaults)(), &sd).unwrap();
            assert!(!p.blocks.is_empty(), "{}", t.key);
            no_overlaps(&p);
            assert!(p.validate().is_empty(), "{}: {:?}", t.key, p.validate());
        }
        std::fs::remove_dir_all(sd).ok();
    }

    #[test]
    fn hariri_structure() {
        let sd = stim();
        let p = build_hariri(&hariri_defaults(), &sd).unwrap();
        let m: Vec<&Block> = p.blocks.iter().filter(|b| b.kind == "match").collect();
        assert_eq!(m.len(), 9 * 6);
        assert_eq!(p.duration(), 9.0 * (2.0 + 30.0));
        assert_eq!(m.iter().filter(|b| b.condition == "faces").count(), 24);
        for b in m.iter().filter(|b| b.condition == "faces") {
            let side = b.params.s("correct", "");
            let other = if side == "left" { "right" } else { "left" };
            let emo = |f: &str| if f.contains("angry") { "angry" } else { "fearful" };
            assert_eq!(emo(&b.params.s("top", "")), emo(&b.params.s(&side, "")));
            assert_ne!(emo(&b.params.s("top", "")), emo(&b.params.s(other, "")));
            assert_ne!(b.params.s("top", ""), b.params.s(&side, ""));
        }
        std::fs::remove_dir_all(sd).ok();
    }

    #[test]
    fn gonogo_never_two_nogo_in_a_row() {
        let mut rng = Rng::seeded(4);
        let seq = gonogo_sequence(160, 40, &mut rng, 3).unwrap();
        assert_eq!(seq.len(), 160);
        assert_eq!(seq.iter().filter(|x| **x == "nogo").count(), 40);
        assert!(seq[..3].iter().all(|x| *x == "go"));
        for w in seq.windows(2) {
            assert!(!(w[0] == "nogo" && w[1] == "nogo"));
        }
        assert!(gonogo_sequence(10, 9, &mut rng, 3).is_err());
    }

    #[test]
    fn nback_targets_match() {
        let mut rng = Rng::seeded(2);
        let seq = nback_sequence(2, 15, 5, "BCDFG", &mut rng, "X").unwrap();
        assert_eq!(seq.iter().filter(|x| x.1).count(), 5);
        for (k, (l, t)) in seq.iter().enumerate() {
            if k >= 2 {
                assert_eq!(*t, *l == seq[k - 2].0);
            }
        }
        let z = nback_sequence(0, 15, 5, "BCDFG", &mut rng, "X").unwrap();
        assert!(z.iter().all(|(l, t)| *t == (l == "X")));
    }

    #[test]
    fn stroop_and_flanker_mix() {
        let sd = stim();
        let p = build_stroop(&stroop_defaults(), &sd).unwrap();
        let ch: Vec<&Block> = p.blocks.iter().filter(|b| b.kind == "choice").collect();
        assert_eq!(ch.len(), 96);
        assert_eq!(ch.iter().filter(|b| b.condition == "incongruent").count(), 48);
        let mut o = stroop_defaults();
        o.set("words", OptVal::S("ROT, GRÜN".into()));
        assert!(build_stroop(&o, &sd).is_err());
        let fl = build_flanker(&flanker_defaults(), &sd).unwrap();
        assert_eq!(fl.blocks.iter().filter(|b| b.kind == "choice").count(), 96);
        let mut o = flanker_defaults();
        o.set("neutral_percent", OptVal::F(60.0));
        assert!(build_flanker(&o, &sd).is_err());
    }

    #[test]
    fn mid_feedback_follows_target() {
        let sd = stim();
        let p = build_mid(&mid_defaults(), &sd).unwrap();
        for b in p.blocks.iter().filter(|b| b.kind == "feedback") {
            let prev = p.previous_scored(b).unwrap();
            assert_eq!(prev.kind, "choice");
            assert!(prev.condition.starts_with("target_"));
        }
        std::fs::remove_dir_all(sd).ok();
    }

    #[test]
    fn faces_folder_sorting() {
        let d = stim();
        std::fs::create_dir_all(&d).unwrap();
        for n in ["01F_AN_C.png", "02M_AN_O.png", "03F_FE_C.png", "AF04AFS.jpg", "other.png"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        let f = faces_from_folder(&d.to_string_lossy()).unwrap();
        assert_eq!(f["angry"].len(), 2);
        assert_eq!(f["fearful"].len(), 2);
        std::fs::remove_dir_all(d).ok();
    }
}
