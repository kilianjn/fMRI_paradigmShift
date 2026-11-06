//! Task blocks: ready-made blocks for the standard paradigms (library tab “Task blocks”).
//!
//! * **Task trials** (Stroop, Flanker, N-back letter) are single Choice trials with a few task
//!   settings (e.g. word + ink colour). Changing them in the inspector re-derives text, colour,
//!   correct button and condition.
//! * **Task sequences** (MID trial, emotion-regulation trial, N-back block, Hariri block, tapping
//!   block, checkerboard, resting state) insert several blocks at once. Most have variants that
//!   are picked on drop.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::model::{round4, Block, ParamExt};
use crate::placeholders as ph;
use crate::rng::Rng;
use crate::templates::nback_sequence;

pub const STROOP_COLORS: [&str; 4] = ["red", "green", "blue", "yellow"];

pub fn stroop_ink(c: &str) -> &'static str {
    match c {
        "red" => "#E53935",
        "green" => "#43A047",
        "blue" => "#1E88E5",
        "yellow" => "#FDD835",
        _ => "#FFFFFF",
    }
}

pub fn stroop_default_word(c: &str) -> &'static str {
    match c {
        "red" => "RED",
        "green" => "GREEN",
        "blue" => "BLUE",
        "yellow" => "YELLOW",
        _ => "",
    }
}

pub fn stroop_default_button(c: &str) -> &'static str {
    match c {
        "red" => "Left thumb",
        "green" => "Left index",
        "blue" => "Right index",
        "yellow" => "Right thumb",
        _ => "",
    }
}

fn arrow(side: &str) -> &'static str {
    if side == "left" { "←" } else { "→" }
}

pub fn default_stroop_words() -> Value {
    json!({"red": "RED", "green": "GREEN", "blue": "BLUE", "yellow": "YELLOW"})
}

pub fn default_stroop_buttons() -> Value {
    json!({"red": "Left thumb", "green": "Left index", "blue": "Right index", "yellow": "Right thumb"})
}

/// Where generated placeholder pictures go (per user).
pub fn default_stim_dir() -> PathBuf {
    crate::prefs::data_dir().join("placeholder-stimuli")
}

// =============================================================================
// Task trials: settings → derived choice-trial parameters
// =============================================================================
/// Recompute text / colour / correct button / condition of a task trial from its task settings.
pub fn derive(b: &mut Block) {
    let kind = b.params.s("preset", "");
    match kind.as_str() {
        "stroop" => {
            let ink = b.params.s("ink", "red");
            let word = b.params.s("word", "red");
            let lookup = |key: &str, c: &str| -> Option<String> {
                b.params.get(key).and_then(|m| m.get(c)).and_then(|v| v.as_str()).map(String::from)
            };
            let text = if word == "neutral" {
                "XXXX".to_string()
            } else {
                lookup("words", &word).unwrap_or_else(|| {
                    let d = stroop_default_word(&word);
                    if d.is_empty() { word.to_uppercase() } else { d.to_string() }
                })
            };
            let correct = lookup("ink_buttons", &ink).unwrap_or_else(|| stroop_default_button(&ink).to_string());
            b.params.insert("text".into(), json!(text));
            b.params.insert("color".into(), json!(stroop_ink(&ink)));
            b.params.insert("correct".into(), json!(correct));
            b.condition = if word == "neutral" {
                "neutral".into()
            } else if word == ink {
                "congruent".into()
            } else {
                "incongruent".into()
            };
            b.label = format!("{text} in {ink}");
        }
        "flanker" => {
            let side = b.params.s("direction", "left");
            let cong = b.params.s("congruency", "congruent");
            let other = if side == "left" { "right" } else { "left" };
            let flank = match cong.as_str() {
                "congruent" => arrow(&side),
                "incongruent" => arrow(other),
                _ => "–",
            };
            let text = format!("{f}{f}{m}{f}{f}", f = flank, m = arrow(&side));
            let correct = if side == "left" {
                b.params.s("left_button", "Left index")
            } else {
                b.params.s("right_button", "Right index")
            };
            b.params.insert("text".into(), json!(text));
            b.params.insert("correct".into(), json!(correct));
            b.condition = cong.clone();
            b.label = format!("{text} ({side})");
        }
        "nback" => {
            let target = b.params.b("target", false);
            let letter: String = b.params.s("letter", "A").chars().take(3).collect::<String>().to_uppercase();
            let correct = if target { b.params.s("target_button", "Right index") } else { String::new() };
            b.params.insert("text".into(), json!(letter));
            b.params.insert("correct".into(), json!(correct));
            b.condition = format!("{}back", b.params.i("level", 2));
            b.label = if target { format!("{letter} (target)") } else { String::new() };
        }
        _ => {}
    }
}

pub enum FieldSpec {
    Text,
    Int,
    Choices(Vec<(&'static str, Value)>),
}

/// Settings shown in the inspector: (param, label, spec).
pub fn trial_fields(preset: &str) -> Vec<(&'static str, &'static str, FieldSpec)> {
    match preset {
        "stroop" => vec![
            (
                "word",
                "Word",
                FieldSpec::Choices(vec![
                    ("RED", json!("red")),
                    ("GREEN", json!("green")),
                    ("BLUE", json!("blue")),
                    ("YELLOW", json!("yellow")),
                    ("XXXX (neutral)", json!("neutral")),
                ]),
            ),
            (
                "ink",
                "Ink colour",
                FieldSpec::Choices(vec![
                    ("red", json!("red")),
                    ("green", json!("green")),
                    ("blue", json!("blue")),
                    ("yellow", json!("yellow")),
                ]),
            ),
        ],
        "flanker" => vec![
            ("direction", "Middle arrow", FieldSpec::Choices(vec![("← left", json!("left")), ("→ right", json!("right"))])),
            (
                "congruency",
                "Flankers",
                FieldSpec::Choices(vec![
                    ("same direction (congruent)", json!("congruent")),
                    ("opposite (incongruent)", json!("incongruent")),
                    ("dashes (neutral)", json!("neutral")),
                ]),
            ),
        ],
        "nback" => vec![
            ("letter", "Letter", FieldSpec::Text),
            (
                "target",
                "Target?",
                FieldSpec::Choices(vec![("yes – participant presses", json!(true)), ("no – no press", json!(false))]),
            ),
            ("level", "N-back level", FieldSpec::Int),
        ],
        _ => vec![],
    }
}

pub fn trial_title(preset: &str) -> Option<&'static str> {
    match preset {
        "stroop" => Some("Stroop trial"),
        "flanker" => Some("Flanker trial"),
        "nback" => Some("N-back letter"),
        _ => None,
    }
}

// =============================================================================
// Library entries
// =============================================================================
#[derive(Clone, Default)]
pub struct PresetCtx {
    pub stim_dir: Option<PathBuf>,
    pub seed: Option<u64>,
    pub amount: Option<String>,
}

impl PresetCtx {
    fn dir(&self) -> PathBuf {
        self.stim_dir.clone().unwrap_or_else(default_stim_dir)
    }
}

type MakeFn = fn(f64, &Value, &PresetCtx) -> Vec<Block>;

pub struct Preset {
    pub key: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub description: &'static str,
    pub icon_kind: &'static str,
    pub color: &'static str,
    pub category: &'static str,
    make: MakeFn,
    /// (label, value); the first one is the default
    pub variants: fn() -> Vec<(&'static str, Value)>,
}

impl Preset {
    pub fn build(&self, start: f64, variant: Option<&Value>, ctx: &PresetCtx) -> Vec<Block> {
        let vars = (self.variants)();
        let v = match variant {
            Some(v) => v.clone(),
            None => vars.first().map(|x| x.1.clone()).unwrap_or(Value::Null),
        };
        let mut blocks = (self.make)(start, &v, ctx);
        for b in blocks.iter_mut() {
            b.params.entry("preset_group").or_insert_with(|| json!(self.key));
        }
        blocks
    }
}

fn choice(start: f64, dur: f64, params: Value) -> Block {
    let mut b = Block::create("choice", start, Some(dur)).with(params);
    derive(&mut b);
    b
}

fn mk_stroop(start: f64, _v: &Value, _c: &PresetCtx) -> Vec<Block> {
    vec![choice(
        start,
        2.0,
        json!({"preset": "stroop", "word": "red", "ink": "blue", "size": 110,
               "words": default_stroop_words(), "ink_buttons": default_stroop_buttons()}),
    )]
}

fn mk_flanker(start: f64, _v: &Value, _c: &PresetCtx) -> Vec<Block> {
    vec![choice(
        start,
        1.5,
        json!({"preset": "flanker", "direction": "left", "congruency": "incongruent", "size": 110,
               "stim_time": 0.5, "left_button": "Left index", "right_button": "Right index"}),
    )]
}

fn mk_nback_letter(start: f64, _v: &Value, _c: &PresetCtx) -> Vec<Block> {
    vec![choice(
        start,
        2.0,
        json!({"preset": "nback", "letter": "K", "target": false, "level": 2, "stim_time": 0.5, "size": 140,
               "target_button": "Right index"}),
    )]
}

fn mk_nback_block(start: f64, v: &Value, ctx: &PresetCtx) -> Vec<Block> {
    let level = v.as_i64().unwrap_or(2);
    let mut rng = Rng::new(ctx.seed);
    let rule = match level {
        0 => "press for X".to_string(),
        1 => "same as the one before".to_string(),
        n => format!("same as {n} before"),
    };
    let mut out = vec![Block::create("text", start, Some(4.0))
        .cond("cue")
        .lab(&format!("{level}-BACK"))
        .with(json!({"text": format!("{level}-BACK\n\n{rule}"), "size": 56}))];
    let mut t = start + 4.0;
    let seq = nback_sequence(level as usize, 15, 5, "BCDFGHJKLMNPQRSTVWZ", &mut rng, "X").unwrap_or_default();
    for (letter, target) in seq {
        out.push(choice(
            t,
            2.0,
            json!({"preset": "nback", "letter": letter, "target": target, "level": level, "stim_time": 0.5,
                   "size": 140, "target_button": "Right index"}),
        ));
        t += 2.0;
    }
    out
}

fn mk_mid_trial(start: f64, v: &Value, ctx: &PresetCtx) -> Vec<Block> {
    let incentive = v.as_str().unwrap_or("gain").to_string();
    let cues = ph::mid_cues(&ctx.dir().join("mid"));
    let cue = cues.get(&incentive).cloned().unwrap_or_default();
    let amt = ctx.amount.clone().unwrap_or_else(|| "1 €".into());
    let (hit, miss) = match incentive.as_str() {
        "gain" => (format!("+{amt}"), "+0".to_string()),
        "loss" => ("−0".to_string(), format!("−{amt}")),
        _ => ("0".to_string(), "0".to_string()),
    };
    let grey = incentive == "neutral";
    let spec: Vec<(&str, f64, String, &str, Value)> = vec![
        ("image", 2.0, format!("cue_{incentive}"), "cue", json!({"path": cue, "scale": 0.45})),
        ("fixation", 2.2, format!("anticipation_{incentive}"), "anticipation", json!({})),
        (
            "choice",
            0.3,
            format!("target_{incentive}"),
            "target",
            json!({"text": "■", "size": 220, "stim_time": 0.0, "correct": "any", "buttons": "any", "isi_symbol": ""}),
        ),
        (
            "feedback",
            2.0,
            format!("feedback_{incentive}"),
            "",
            json!({"hit_text": hit, "miss_text": miss, "size": 96,
                   "hit_color": if grey {"#CCCCCC"} else {"#2FB37A"},
                   "miss_color": if grey {"#CCCCCC"} else {"#F0506E"}}),
        ),
        ("fixation", 2.0, String::new(), "ITI", json!({})),
    ];
    let mut t = start;
    let mut out = vec![];
    for (kind, dur, cond, label, params) in spec {
        let label = if label == "cue" { format!("cue {incentive}") } else { label.to_string() };
        out.push(Block::create(kind, t, Some(dur)).cond(&cond).lab(&label).with(params));
        t = round4(t + dur);
    }
    out
}

pub fn emoreg_variant(v: &str) -> (&'static str, &'static str) {
    match v {
        "look_neutral" => ("LOOK", "neutral"),
        "look_negative" => ("LOOK", "negative"),
        "increase_negative" => ("INCREASE", "negative"),
        _ => ("DECREASE", "negative"),
    }
}

fn mk_emoreg_trial(start: f64, v: &Value, ctx: &PresetCtx) -> Vec<Block> {
    let cond = v.as_str().unwrap_or("decrease_negative").to_string();
    let (cue, valence) = emoreg_variant(&cond);
    let pic = ph::scenes(&ctx.dir().join(valence), valence, 1).remove(0);
    vec![
        Block::create("text", start, Some(2.0)).cond("cue").lab(cue).with(json!({"text": cue, "size": 72})),
        Block::create("image", start + 2.0, Some(7.0)).cond(&cond).with(json!({"path": pic, "scale": 0.85})),
        Block::create("rating", start + 9.0, Some(4.0)).cond("rating").with(json!({
            "question": "How negative do you feel?", "path": "", "min": 1, "max": 4,
            "left_label": "not at all", "right_label": "very"})),
        Block::create("fixation", start + 13.0, Some(4.0)).lab("Relax"),
    ]
}

fn mk_hariri_block(start: f64, v: &Value, ctx: &PresetCtx) -> Vec<Block> {
    let cond = v.as_str().unwrap_or("faces").to_string();
    let mut rng = Rng::new(ctx.seed);
    let sd = ctx.dir();
    let faces = ph::hariri_faces(&sd.join("faces"), 6);
    let shapes = ph::hariri_shapes(&sd.join("shapes"));
    let shape_keys: Vec<String> = shapes.keys().cloned().collect();
    let mut out = vec![Block::create("text", start, Some(2.0))
        .cond("instruction")
        .lab(&format!("Match {cond}"))
        .with(json!({"text": if cond == "faces" {"Match Faces"} else {"Match Shapes"}, "size": 64}))];
    let mut sides = vec!["left", "right", "left", "right", "left", "right"];
    rng.shuffle(&mut sides);
    let mut emos = vec!["angry", "fearful", "angry", "fearful", "angry", "fearful"];
    rng.shuffle(&mut emos);
    let mut t = start + 2.0;
    for k in 0..6 {
        let (top, matching, foil) = if cond == "faces" {
            let emo = emos[k];
            let other = if emo == "angry" { "fearful" } else { "angry" };
            let pair = rng.sample(&faces[emo], 2);
            let foil = rng.choice(&faces[other]).clone();
            (pair[0].clone(), pair[1].clone(), foil)
        } else {
            let s = rng.choice(&shape_keys).clone();
            let others: Vec<String> = shape_keys.iter().filter(|x| **x != s).cloned().collect();
            let o = rng.choice(&others).clone();
            (shapes[&s].clone(), shapes[&s].clone(), shapes[&o].clone())
        };
        let (left, right) = if sides[k] == "left" { (matching, foil) } else { (foil, matching) };
        out.push(Block::create("match", t, Some(5.0)).cond(&cond).with(json!({
            "top": top, "left": left, "right": right, "correct": sides[k]})));
        t += 5.0;
    }
    out
}

fn mk_tapping(start: f64, v: &Value, _c: &PresetCtx) -> Vec<Block> {
    let hand = v.as_str().unwrap_or("left");
    let text = match hand {
        "right" => "TAP\nRIGHT HAND",
        "both" => "TAP\nBOTH HANDS",
        _ => "TAP\nLEFT HAND",
    };
    vec![Block::create("text", start, Some(20.0))
        .cond(&format!("tap_{hand}"))
        .lab(&text.replace('\n', " "))
        .with(json!({"text": text, "size": 72}))]
}

fn mk_checkerboard(start: f64, _v: &Value, ctx: &PresetCtx) -> Vec<Block> {
    let (a, b) = ph::checkerboards(&ctx.dir().join("checkerboard"));
    vec![Block::create("image", start, Some(20.0))
        .cond("checkerboard")
        .lab("Checkerboard")
        .with(json!({"path": a, "flicker_path": b, "flicker_hz": 8.0, "scale": 1.0}))]
}

fn mk_resting(start: f64, v: &Value, _c: &PresetCtx) -> Vec<Block> {
    let minutes = v.as_f64().unwrap_or(8.0);
    vec![Block::create("fixation", start, Some(minutes * 60.0)).cond("rest").lab("Rest (eyes open)")]
}

pub static PRESETS: &[Preset] = &[
    Preset {
        key: "stroop",
        title: "Stroop trial",
        subtitle: "word in coloured ink",
        description: "Colour word – press the button for the INK colour. Pick word and ink in the inspector; \
                      correct button and condition follow automatically.",
        icon_kind: "choice",
        color: "#E5B53B",
        category: "Task trials",
        make: mk_stroop,
        variants: Vec::new,
    },
    Preset {
        key: "flanker",
        title: "Flanker trial",
        subtitle: "arrows, middle one counts",
        description: "Five arrows – the middle one gives the answer. Pick direction and flankers in the inspector.",
        icon_kind: "choice",
        color: "#E59A3B",
        category: "Task trials",
        make: mk_flanker,
        variants: Vec::new,
    },
    Preset {
        key: "nback_letter",
        title: "N-back letter",
        subtitle: "target or not",
        description: "One N-back letter. Mark it as target in the inspector; the correct answer follows.",
        icon_kind: "choice",
        color: "#3BA7E5",
        category: "Task trials",
        make: mk_nback_letter,
        variants: Vec::new,
    },
    Preset {
        key: "nback_block",
        title: "N-back block",
        subtitle: "cue + 15 letters",
        description: "A cue and 15 letters (5 targets).",
        icon_kind: "choice",
        color: "#3BA7E5",
        category: "Task sequences",
        make: mk_nback_block,
        variants: || {
            vec![("2-back", json!(2)), ("0-back (press for X)", json!(0)), ("1-back", json!(1)), ("3-back", json!(3))]
        },
    },
    Preset {
        key: "mid_trial",
        title: "MID trial",
        subtitle: "cue → wait → target → feedback",
        description: "One monetary-incentive-delay trial: incentive cue, anticipation, brief target (press!), \
                      feedback and ITI.",
        icon_kind: "feedback",
        color: "#B565D8",
        category: "Task sequences",
        make: mk_mid_trial,
        variants: || vec![("Win", json!("gain")), ("Avoid losing", json!("loss")), ("Nothing at stake", json!("neutral"))],
    },
    Preset {
        key: "emoreg_trial",
        title: "Emotion-regulation trial",
        subtitle: "cue → picture → rating → relax",
        description: "LOOK / DECREASE cue, a picture, a negative-affect rating and a relax period. Replace the \
                      placeholder picture with yours.",
        icon_kind: "rating",
        color: "#D45C9E",
        category: "Task sequences",
        make: mk_emoreg_trial,
        variants: || {
            vec![
                ("DECREASE · negative picture", json!("decrease_negative")),
                ("LOOK · negative picture", json!("look_negative")),
                ("LOOK · neutral picture", json!("look_neutral")),
                ("INCREASE · negative picture", json!("increase_negative")),
            ]
        },
    },
    Preset {
        key: "hariri_block",
        title: "Hariri block",
        subtitle: "instruction + 6 matching trials",
        description: "One block of the Hariri task (2 s instruction, 6 × 5 s trials).",
        icon_kind: "match",
        color: "#1FA8B8",
        category: "Task sequences",
        make: mk_hariri_block,
        variants: || vec![("Faces", json!("faces")), ("Shapes", json!("shapes"))],
    },
    Preset {
        key: "tapping",
        title: "Tapping block",
        subtitle: "20 s finger tapping",
        description: "Finger-tapping instruction for 20 s. Taps are logged as grip presses and counted live in the \
                      control room.",
        icon_kind: "response",
        color: "#2FB37A",
        category: "Task sequences",
        make: mk_tapping,
        variants: || vec![("Left hand", json!("left")), ("Right hand", json!("right")), ("Both hands", json!("both"))],
    },
    Preset {
        key: "checkerboard",
        title: "Checkerboard",
        subtitle: "20 s, 8 Hz flicker",
        description: "Flickering radial checkerboard with a red fixation dot. Set a grey background in the paradigm \
                      settings for the OFF periods.",
        icon_kind: "image",
        color: "#9AA3AD",
        category: "Task sequences",
        make: mk_checkerboard,
        variants: Vec::new,
    },
    Preset {
        key: "resting",
        title: "Resting state",
        subtitle: "fixation, eyes open",
        description: "One long fixation period for resting-state fMRI.",
        icon_kind: "fixation",
        color: "#4C8DF6",
        category: "Task sequences",
        make: mk_resting,
        variants: || {
            vec![("8 minutes", json!(8.0)), ("5 minutes", json!(5.0)), ("10 minutes", json!(10.0)), ("6 minutes", json!(6.0))]
        },
    },
];

pub fn preset(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PresetCtx {
        PresetCtx {
            stim_dir: Some(std::env::temp_dir().join(format!("ps_pre_{}", crate::model::new_id()))),
            seed: Some(1),
            amount: None,
        }
    }

    #[test]
    fn stroop_derives_everything() {
        let mut b = preset("stroop").unwrap().build(0.0, None, &ctx()).remove(0);
        assert_eq!(b.params.s("text", ""), "RED");
        assert_eq!(b.params.s("color", ""), stroop_ink("blue"));
        assert_eq!(b.params.s("correct", ""), "Right index");
        assert_eq!(b.condition, "incongruent");
        b.params.insert("ink".into(), json!("red"));
        derive(&mut b);
        assert_eq!(b.condition, "congruent");
        assert_eq!(b.params.s("correct", ""), "Left thumb");
        b.params.insert("word".into(), json!("neutral"));
        derive(&mut b);
        assert_eq!(b.params.s("text", ""), "XXXX");
        assert_eq!(b.condition, "neutral");
    }

    #[test]
    fn flanker_and_nback() {
        let mut b = preset("flanker").unwrap().build(0.0, None, &ctx()).remove(0);
        assert_eq!(b.params.s("text", ""), "→→←→→");
        assert_eq!(b.params.s("correct", ""), "Left index");
        b.params.insert("congruency".into(), json!("neutral"));
        b.params.insert("direction".into(), json!("right"));
        derive(&mut b);
        assert_eq!(b.params.s("text", ""), "––→––");
        assert_eq!(b.params.s("correct", ""), "Right index");
        let mut n = preset("nback_letter").unwrap().build(0.0, None, &ctx()).remove(0);
        assert_eq!(n.params.s("correct", "x"), "");
        n.params.insert("target".into(), json!(true));
        derive(&mut n);
        assert_eq!(n.params.s("correct", ""), "Right index");
        assert_eq!(n.condition, "2back");
    }

    #[test]
    fn sequences_build() {
        let c = ctx();
        for p in PRESETS {
            for (_, v) in (p.variants)().iter().chain([("", Value::Null)].iter()).take(2) {
                let blocks = p.build(10.0, if v.is_null() { None } else { Some(v) }, &c);
                assert!(!blocks.is_empty(), "{}", p.key);
                assert!(blocks.iter().all(|b| b.start >= 10.0 - 1e-9));
                assert!(blocks.iter().all(|b| b.params.s("preset_group", "") == p.key));
            }
        }
        let mid = preset("mid_trial").unwrap().build(0.0, Some(&json!("loss")), &c);
        assert_eq!(mid.len(), 5);
        assert_eq!(mid[3].params.s("miss_text", ""), "−1 €");
        let nb = preset("nback_block").unwrap().build(0.0, Some(&json!(2)), &c);
        assert_eq!(nb.len(), 16);
        assert_eq!(nb.iter().filter(|b| b.params.b("target", false)).count(), 5);
        if let Some(d) = c.stim_dir {
            std::fs::remove_dir_all(d).ok();
        }
    }
}
