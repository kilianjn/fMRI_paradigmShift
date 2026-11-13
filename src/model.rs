//! Data model for an fMRI paradigm: tracks, blocks and hardware settings.
//!
//! Plain Rust (no GUI) so the model can be tested without a display. A paradigm
//! serialises to a single JSON file that is compatible with the files written by
//! the original Python version of ParadigmShift.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::rng::Rng;

pub const FILE_SUFFIX: &str = ".paradigm.json";
pub const FORMAT_VERSION: i64 = 1;
pub const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"];

pub type Params = Map<String, Value>;

// --------------------------------------------------------------------------
// Block types
// --------------------------------------------------------------------------
pub struct BlockType {
    pub kind: &'static str,
    pub title: &'static str,
    pub color: &'static str,
    pub description: &'static str,
    pub default_duration: f64,
    pub default_track: usize,
    pub defaults: fn() -> Value,
}

pub static BLOCK_TYPES: &[BlockType] = &[
    BlockType {
        kind: "fixation",
        title: "Fixation",
        color: "#4C8DF6",
        description: "A fixation cross – the classic baseline.",
        default_duration: 10.0,
        default_track: 0,
        defaults: || json!({"symbol": "+", "color": "#FFFFFF", "size": 72}),
    },
    BlockType {
        kind: "image",
        title: "Image",
        color: "#E0913A",
        description: "Shows a picture, optionally with a text underneath. Drop image files straight onto the timeline.",
        default_duration: 4.0,
        default_track: 0,
        defaults: || {
            json!({"path": "", "scale": 0.8, "flicker_path": "", "flicker_hz": 0.0,
                   "caption": "", "caption_size": 40, "caption_color": "#FFFFFF"})
        },
    },
    BlockType {
        kind: "video",
        title: "Video",
        color: "#E4604E",
        description: "Plays a video (MP4/MOV with H.264) – with its sound if a sound output is set (Hardware → Audio). \
                      Drop video files straight onto the timeline.",
        default_duration: 6.0,
        default_track: 0,
        defaults: || json!({"path": "", "scale": 0.9, "sound": true, "volume": 1.0, "after_end": "hold"}),
    },
    BlockType {
        kind: "audio",
        title: "Audio",
        color: "#2FA88A",
        description: "Plays a sound file (WAV, MP3, OGG, FLAC, M4A) while the screen shows a fixation cross, nothing or a \
                      text. Needs a sound output (Hardware → Audio).",
        default_duration: 4.0,
        default_track: 0,
        defaults: || {
            json!({"path": "", "volume": 1.0, "screen": "fixation", "symbol": "+", "text": "", "color": "#FFFFFF", "size": 72})
        },
    },
    BlockType {
        kind: "rating",
        title: "Rating",
        color: "#D45C9E",
        description: "Shows an image + a rating scale the participant answers with the grips.",
        default_duration: 6.0,
        default_track: 0,
        defaults: || {
            json!({
                "path": "", "question": "How pleasant is this image?",
                "min": 1, "max": 4, "left_label": "not at all", "right_label": "very",
                "mode": "cursor", "scale": 0.9
            })
        },
    },
    BlockType {
        kind: "text",
        title: "Instruction",
        color: "#8B6CEF",
        description: "Shows a text, e.g. instructions or a task cue.",
        default_duration: 8.0,
        default_track: 0,
        defaults: || json!({"text": "Please keep still and look at the cross.", "size": 40, "color": "#FFFFFF"}),
    },
    BlockType {
        kind: "rest",
        title: "Pause",
        color: "#5B6577",
        description: "Blank screen in the background colour.",
        default_duration: 12.0,
        default_track: 0,
        defaults: || json!({}),
    },
    BlockType {
        kind: "match",
        title: "Matching trial",
        color: "#1FA8B8",
        description: "Three pictures: which of the two bottom ones matches the top one? (Hariri task)",
        default_duration: 5.0,
        default_track: 0,
        defaults: || {
            json!({"top": "", "left": "", "right": "", "correct": "left",
                   "left_button": "Left index", "right_button": "Right index", "scale": 0.9,
                   "show_choice": true, "choice_color": "#FFD23F"})
        },
    },
    BlockType {
        kind: "gonogo",
        title: "Go / No-Go trial",
        color: "#6BBF3A",
        description: "Flashes a letter or picture. Go: press a button. No-Go: hold back.",
        default_duration: 1.5,
        default_track: 0,
        defaults: || {
            json!({"trial": "go", "text": "A", "path": "", "stim_time": 0.5, "button": "any",
                   "size": 140, "color": "#FFFFFF", "isi_symbol": "+"})
        },
    },
    BlockType {
        kind: "choice",
        title: "Choice trial",
        color: "#E5B53B",
        description: "A word, symbol or picture plus the button that is correct – or no press at all. \
                      Used for Stroop, Flanker, N-back, MID targets …",
        default_duration: 2.0,
        default_track: 0,
        defaults: || {
            json!({"text": "", "path": "", "color": "#FFFFFF", "size": 120, "stim_time": 0.0,
                   "correct": "Right index", "buttons": "any", "isi_symbol": "+"})
        },
    },
    BlockType {
        kind: "feedback",
        title: "Feedback",
        color: "#B565D8",
        description: "Shows one text after a successful and another after a missed previous trial (e.g. MID).",
        default_duration: 2.0,
        default_track: 0,
        defaults: || {
            json!({"hit_text": "Correct!", "miss_text": "Missed", "hit_color": "#2FB37A",
                   "miss_color": "#F0506E", "size": 72})
        },
    },
];

pub fn block_type(kind: &str) -> Option<&'static BlockType> {
    BLOCK_TYPES.iter().find(|t| t.kind == kind)
}

fn bt(kind: &str) -> &'static BlockType {
    block_type(kind).or_else(|| block_type("rest")).unwrap_or(&BLOCK_TYPES[0])
}

/// Kinds whose trials are scored (they have a right/wrong or hit/miss outcome)
pub const SCORED_TRIAL_KINDS: &[&str] = &["match", "gonogo", "choice"];
pub const STIMULUS_KINDS: &[&str] =
    &["fixation", "image", "video", "audio", "rating", "text", "rest", "match", "gonogo", "choice", "feedback"];
/// Params that hold picture files.
pub const IMAGE_PARAMS: &[&str] = &["path", "top", "left", "right", "flicker_path"];
pub const NOGO_COLOR: &str = "#E5484D";

/// Preference-driven defaults for *new* blocks, e.g. rating → {"mode": "cursor"} (see prefs.rs).
pub static PARAM_OVERRIDES: LazyLock<RwLock<HashMap<String, Params>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn set_param_override(kind: &str, key: &str, value: Value) {
    if let Ok(mut m) = PARAM_OVERRIDES.write() {
        m.entry(kind.to_string()).or_default().insert(key.to_string(), value);
    }
}

// --------------------------------------------------------------------------
// Param access helpers (params are loosely typed, like the JSON file)
// --------------------------------------------------------------------------
pub fn v_str(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        Value::Bool(b) => Some(if *b { "True".into() } else { "False".into() }),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

pub fn v_f64(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn v_bool(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_f64().unwrap_or(0.0) != 0.0),
        Value::String(s) => Some(!s.is_empty() && s != "0" && s.to_lowercase() != "false"),
        Value::Null => Some(false),
        _ => Some(true),
    }
}

pub trait ParamExt {
    fn s(&self, key: &str, default: &str) -> String;
    fn f(&self, key: &str, default: f64) -> f64;
    fn i(&self, key: &str, default: i64) -> i64;
    fn b(&self, key: &str, default: bool) -> bool;
    /// Non-empty string param ("truthy" in the Python sense).
    fn nonempty(&self, key: &str) -> Option<String>;
}

impl ParamExt for Params {
    fn s(&self, key: &str, default: &str) -> String {
        v_str(self.get(key)).unwrap_or_else(|| default.to_string())
    }
    fn f(&self, key: &str, default: f64) -> f64 {
        v_f64(self.get(key)).unwrap_or(default)
    }
    fn i(&self, key: &str, default: i64) -> i64 {
        v_f64(self.get(key)).map(|x| x as i64).unwrap_or(default)
    }
    fn b(&self, key: &str, default: bool) -> bool {
        v_bool(self.get(key)).unwrap_or(default)
    }
    fn nonempty(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }
}

pub fn basename(path: &str) -> String {
    let p = path.replace('\\', "/");
    p.rsplit('/').next().unwrap_or("").to_string()
}

pub fn stem(path: &str) -> String {
    let b = basename(path);
    match b.rfind('.') {
        Some(i) if i > 0 => b[..i].to_string(),
        _ => b,
    }
}

pub fn is_image(path: &str) -> bool {
    let lower = path.to_lowercase();
    IMAGE_EXT.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

/// Pictures, videos and sounds – what the Media panel holds.
pub fn is_media(path: &str) -> bool {
    is_image(path) || crate::media::is_video_file(path) || crate::media::is_audio_file(path)
}

/// Pictures, videos and sounds in a folder, sorted.
pub fn list_media(folder: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(folder) else { return vec![] };
    let mut out: Vec<String> =
        rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).map(|p| p.to_string_lossy().to_string()).filter(|p| is_media(p)).collect();
    out.sort();
    out
}

/// Image files in a folder, sorted.
pub fn list_images(folder: &str) -> Vec<String> {
    if folder.is_empty() {
        return vec![];
    }
    let Ok(rd) = std::fs::read_dir(folder) else { return vec![] };
    let mut out: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image(&p.to_string_lossy()))
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}

// --------------------------------------------------------------------------
// Blocks
// --------------------------------------------------------------------------
pub fn new_id() -> String {
    let mut r = Rng::new(None);
    format!("{:010x}", r.next_u64() & 0xFF_FFFF_FFFF)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub kind: String,
    pub start: f64,
    pub duration: f64,
    #[serde(default)]
    pub track: usize,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub params: Params,
    #[serde(default = "new_id")]
    pub id: String,
}

impl Block {
    /// A new block with the type's defaults (and the preference overrides).
    pub fn create(kind: &str, start: f64, duration: Option<f64>) -> Block {
        let t = bt(kind);
        let mut params = match (t.defaults)() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        if let Ok(ov) = PARAM_OVERRIDES.read() {
            if let Some(o) = ov.get(kind) {
                for (k, v) in o {
                    params.insert(k.clone(), v.clone());
                }
            }
        }
        Block {
            kind: kind.to_string(),
            start,
            duration: duration.unwrap_or(t.default_duration),
            track: t.default_track,
            label: String::new(),
            condition: String::new(),
            params,
            id: new_id(),
        }
    }

    /// Merge the entries of a JSON object into the params.
    pub fn with(mut self, params: Value) -> Block {
        if let Value::Object(m) = params {
            for (k, v) in m {
                self.params.insert(k, v);
            }
        }
        self
    }

    pub fn cond(mut self, c: &str) -> Block {
        self.condition = c.to_string();
        self
    }

    pub fn lab(mut self, l: &str) -> Block {
        self.label = l.to_string();
        self
    }

    pub fn end(&self) -> f64 {
        self.start + self.duration
    }

    pub fn btype(&self) -> &'static BlockType {
        bt(&self.kind)
    }

    pub fn title(&self) -> &'static str {
        self.btype().title
    }

    pub fn display_label(&self) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        let p = &self.params;
        if let Some(path) = p.nonempty("path") {
            return basename(&path);
        }
        match self.kind.as_str() {
            "text" => {
                let t = p.s("text", "");
                let first: String = t.split('\n').next().unwrap_or("").chars().take(40).collect();
                if first.is_empty() { self.title().to_string() } else { first }
            }
            "gonogo" => {
                let pre = if p.s("trial", "go") == "nogo" { "NO-GO " } else { "GO " };
                format!("{pre}{}", p.s("text", ""))
            }
            "match" if p.nonempty("top").is_some() => basename(&p.s("top", "")),
            "choice" if p.nonempty("text").is_some() => p.s("text", "").replace('\n', " "),
            "feedback" => format!("{} / {}", p.s("hit_text", ""), p.s("miss_text", "")),
            _ => self.title().to_string(),
        }
    }

    pub fn color(&self) -> &'static str {
        if self.kind == "gonogo" && self.params.s("trial", "go") == "nogo" {
            return NOGO_COLOR;
        }
        self.btype().color
    }

    pub fn thumbnail_path(&self) -> String {
        self.params
            .nonempty("path")
            .or_else(|| self.params.nonempty("top"))
            .unwrap_or_default()
    }

    pub fn image_files(&self) -> Vec<String> {
        if self.kind == "video" || self.kind == "audio" {
            return vec![]; // their "path" is a video / sound file
        }
        IMAGE_PARAMS.iter().filter_map(|k| self.params.nonempty(k)).collect()
    }

    /// All pictures, videos and sounds this block uses (for the Media panel).
    pub fn media_files(&self) -> Vec<String> {
        let mut v = self.image_files();
        if self.kind == "video" || self.kind == "audio" {
            v.extend(self.params.nonempty("path"));
        }
        v
    }

    /// The sound this block plays: its audio file, or the sound of its video (if switched on).
    pub fn sound_file(&self) -> Option<String> {
        match self.kind.as_str() {
            "audio" => self.params.nonempty("path"),
            "video" if self.params.b("sound", true) => self.params.nonempty("path"),
            _ => None,
        }
    }

    pub fn clone_new(&self) -> Block {
        let mut b = self.clone();
        b.id = new_id();
        b
    }

    pub fn overlaps(&self, other: &Block) -> bool {
        let eps = 1e-6;
        self.track == other.track
            && self.id != other.id
            && self.start < other.end() - eps
            && other.start < self.end() - eps
    }

    pub fn is_scored_trial(&self) -> bool {
        SCORED_TRIAL_KINDS.contains(&self.kind.as_str())
    }

    /// Rating scale, cursor mode: the value the cursor starts at – `start` (kept within the scale),
    /// or by default the middle of the scale (the upper of the two middle values when their number
    /// is even, e.g. 3 on a 1–4 scale).
    pub fn rating_start(&self) -> i64 {
        let (lo, hi) = (self.params.i("min", 1), self.params.i("max", 4));
        match v_f64(self.params.get("start")) {
            Some(v) => (v.round() as i64).clamp(lo.min(hi), lo.max(hi)),
            None => (lo + hi + 1).div_euclid(2),
        }
    }

    /// Rating scale, cursor mode: the buttons that move left / right and confirm. Each can be set per
    /// block (`cursor_left`, `cursor_right`, `cursor_confirm`); empty = the hardware setup's default.
    pub fn rating_cursor_buttons(&self, hw: &Hardware) -> [String; 3] {
        let pick = |key: &str, d: &str| self.params.nonempty(key).unwrap_or_else(|| d.to_string());
        [pick("cursor_left", &hw.move_left), pick("cursor_right", &hw.move_right), pick("cursor_confirm", &hw.confirm)]
    }

    /// Rating scale, direct mode: the button for each value `min..=max` (`value_buttons`, one entry per
    /// value; a missing or empty entry = the hardware button in that position).
    pub fn rating_value_buttons(&self, hw: &Hardware) -> Vec<String> {
        let (lo, hi) = (self.params.i("min", 1), self.params.i("max", 4));
        let n = (hi - lo + 1).max(0) as usize;
        let custom: Vec<String> = match self.params.get("value_buttons") {
            Some(Value::Array(a)) => a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect(),
            _ => vec![],
        };
        let names = hw.button_names();
        (0..n)
            .map(|i| match custom.get(i) {
                Some(c) if !c.is_empty() => c.clone(),
                _ => names.get(i).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// The rating value a button press selects in direct mode.
    pub fn rating_value_for(&self, hw: &Hardware, button: &str) -> Option<i64> {
        let lo = self.params.i("min", 1);
        self.rating_value_buttons(hw).iter().position(|b| !b.is_empty() && b == button).map(|i| lo + i as i64)
    }
}

pub fn default_tracks() -> Vec<String> {
    vec!["Stimuli".to_string()]
}

// --------------------------------------------------------------------------
// Hardware
// --------------------------------------------------------------------------
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GripButton {
    pub name: String,
    #[serde(default)]
    pub key: String,
}

impl GripButton {
    pub fn new(name: &str, key: &str) -> Self {
        GripButton { name: name.into(), key: key.into() }
    }
}

fn default_buttons() -> Vec<GripButton> {
    // NNL's keys: the SyncBox sends a / b / c / d for these buttons
    vec![
        GripButton::new("Left thumb", "a"),
        GripButton::new("Left index", "b"),
        GripButton::new("Right index", "c"),
        GripButton::new("Right thumb", "d"),
    ]
}

fn d_sync_mode() -> String {
    "keyboard".into()
}
fn d_trigger() -> String {
    "s".into()
}
fn d_baud() -> u32 {
    57600
}
fn d_true() -> bool {
    true
}
fn d_one() -> u32 {
    1
}
fn d_pulse() -> u32 {
    100
}
fn d_left() -> String {
    "Left index".into()
}
fn d_right() -> String {
    "Right index".into()
}
fn d_confirm() -> String {
    "Right thumb".into()
}

/// NordicNeuroLab SyncBox + ResponseGrips. The SyncBox forwards the scanner TTL pulse to the PC
/// either as a USB keyboard key press or as a character on a serial (COM) port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    #[serde(default = "d_sync_mode")]
    pub sync_mode: String, // "keyboard" | "serial"
    #[serde(default = "d_trigger")]
    pub trigger_key: String,
    #[serde(default)]
    pub serial_port: String,
    #[serde(default = "d_baud")]
    pub baud_rate: u32,
    /// Serial mode: put the SyncBox into computer mode, configure and start it for each run, and
    /// return it to manual mode afterwards (NNL's serial protocol). Off = only listen.
    #[serde(default = "d_true")]
    pub syncbox_control: bool,
    #[serde(default = "d_one")]
    pub syncbox_slices: u32,
    #[serde(default = "d_pulse")]
    pub syncbox_pulse_ms: u32,
    #[serde(default = "default_buttons")]
    pub buttons: Vec<GripButton>,
    #[serde(default = "d_left")]
    pub move_left: String,
    #[serde(default = "d_right")]
    pub move_right: String,
    #[serde(default = "d_confirm")]
    pub confirm: String,
    /// Sound output for audio blocks and the sound of videos: a device id, "default" (the system's
    /// default output) or "" = no sound (the default).
    #[serde(default)]
    pub audio_device: String,
    /// The chosen output's name (shown when it is not connected)
    #[serde(default)]
    pub audio_device_name: String,
}

impl Default for Hardware {
    fn default() -> Self {
        Hardware {
            sync_mode: d_sync_mode(),
            trigger_key: d_trigger(),
            serial_port: String::new(),
            baud_rate: d_baud(),
            syncbox_control: true,
            syncbox_slices: 1,
            syncbox_pulse_ms: 100,
            buttons: default_buttons(),
            move_left: d_left(),
            move_right: d_right(),
            confirm: d_confirm(),
            audio_device: String::new(),
            audio_device_name: String::new(),
        }
    }
}

impl Hardware {
    pub fn button_names(&self) -> Vec<String> {
        self.buttons.iter().map(|b| b.name.clone()).collect()
    }

    pub fn button_for_key(&self, key: &str) -> Option<String> {
        let k = key.to_lowercase();
        self.buttons
            .iter()
            .find(|b| !b.key.is_empty() && b.key.to_lowercase() == k)
            .map(|b| b.name.clone())
    }
}

// --------------------------------------------------------------------------
// Paradigm
// --------------------------------------------------------------------------
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paradigm {
    pub name: String,
    pub tr: f64,
    pub dummy_scans: u32,
    pub background: String,
    /// What the participant sees before the first counted trigger (incl. dummy scans).
    pub waiting_screen: String, // "fixation" | "blank" | "text"
    pub waiting_text: String,
    pub end_text: String,
    pub tracks: Vec<String>,
    pub blocks: Vec<Block>,
    pub hardware: Hardware,
}

pub const DEFAULT_WAITING_TEXT: &str = "The measurement will start in a moment.";
pub const DEFAULT_END_TEXT: &str = "Thank you!";

impl Default for Paradigm {
    fn default() -> Self {
        Paradigm {
            name: "Untitled paradigm".into(),
            tr: 2.0,
            dummy_scans: 0,
            background: "#000000".into(),
            waiting_screen: "fixation".into(),
            waiting_text: DEFAULT_WAITING_TEXT.into(),
            end_text: DEFAULT_END_TEXT.into(),
            tracks: default_tracks(),
            blocks: vec![],
            hardware: Hardware::default(),
        }
    }
}

impl Paradigm {
    pub fn named(name: &str, tr: f64, dummy_scans: u32) -> Paradigm {
        Paradigm { name: name.into(), tr, dummy_scans, ..Default::default() }
    }

    // ---- derived -----------------------------------------------------------
    pub fn duration(&self) -> f64 {
        self.blocks.iter().map(|b| b.end()).fold(0.0, f64::max)
    }

    pub fn volumes(&self) -> u32 {
        if self.tr <= 0.0 {
            return 0;
        }
        (self.duration() / self.tr - 1e-9).ceil().max(0.0) as u32
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn block_mut(&mut self, id: &str) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    pub fn blocks_on(&self, track: usize) -> Vec<&Block> {
        let mut v: Vec<&Block> = self.blocks.iter().filter(|b| b.track == track).collect();
        v.sort_by(|a, b| a.start.total_cmp(&b.start));
        v
    }

    pub fn active_blocks(&self, t: f64) -> Vec<&Block> {
        self.blocks.iter().filter(|b| b.start <= t && t < b.end()).collect()
    }

    pub fn stimulus_at(&self, t: f64) -> Option<&Block> {
        let mut act = self.active_blocks(t);
        act.sort_by_key(|b| b.track);
        act.into_iter().find(|b| STIMULUS_KINDS.contains(&b.kind.as_str()))
    }

    /// The scored trial that ended last before `b` starts – what a feedback block reports on.
    pub fn previous_scored(&self, b: &Block) -> Option<&Block> {
        self.blocks
            .iter()
            .filter(|o| o.is_scored_trial() && o.end() <= b.start + 1e-6)
            .max_by(|x, y| x.end().total_cmp(&y.end()).then(x.start.total_cmp(&y.start)))
    }

    pub fn conditions(&self) -> Vec<String> {
        let mut bl: Vec<&Block> = self.blocks.iter().collect();
        bl.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut seen: Vec<String> = vec![];
        for b in bl {
            if !b.condition.is_empty() && !seen.contains(&b.condition) {
                seen.push(b.condition.clone());
            }
        }
        seen
    }

    pub fn has_overlap(&self, block: &Block) -> bool {
        self.blocks.iter().any(|o| block.overlaps(o))
    }

    pub fn track_end(&self, track: usize) -> f64 {
        self.blocks.iter().filter(|b| b.track == track).map(|b| b.end()).fold(0.0, f64::max)
    }

    // ---- editing helpers -----------------------------------------------------
    /// Insert `block`; later blocks on the same track move right to make room.
    pub fn ripple_insert(&mut self, mut block: Block) -> String {
        let clash: Vec<&Block> = self.blocks.iter().filter(|o| block.overlaps(o)).collect();
        if !clash.is_empty() {
            let first = clash.iter().min_by(|a, b| a.start.total_cmp(&b.start)).unwrap();
            if first.start < block.start {
                // Dropped onto the middle of a block: go right behind it.
                block.start = first.end();
            }
            let next_start = self
                .blocks
                .iter()
                .filter(|o| o.track == block.track && o.end() > block.start + 1e-9)
                .map(|o| o.start)
                .fold(f64::INFINITY, f64::min);
            let next_start = if next_start.is_finite() { next_start } else { block.end() };
            let shift = block.end() - next_start;
            if shift > 0.0 {
                for o in self.blocks.iter_mut() {
                    if o.track == block.track && o.start >= block.start - 1e-9 {
                        o.start += shift;
                    }
                }
            }
        }
        let id = block.id.clone();
        self.blocks.push(block);
        id
    }

    /// The blocks `moved` – given with their original (start, duration, track) – form one unit
    /// that is moved so it starts at the block boundary `t` (a time in the current layout) on
    /// `track`: the blocks from there on move right by the unit's length, and when the unit stays
    /// on its track, the room it leaves behind is closed. Blocks after both places therefore keep
    /// their times. Only works for a unit on one track without foreign blocks in between.
    /// Returns the unit's length.
    pub fn move_insert(&mut self, moved: &[(String, f64, f64, usize)], t: f64, track: usize) -> Result<f64, String> {
        const EPS: f64 = 1e-6;
        let Some(&(_, _, _, src)) = moved.first() else { return Ok(0.0) };
        if moved.iter().any(|m| m.3 != src) {
            return Err("Blocks from several tracks can't be inserted between other blocks.".into());
        }
        let ids: HashSet<&str> = moved.iter().map(|m| m.0.as_str()).collect();
        let g0 = moved.iter().map(|m| m.1).fold(f64::INFINITY, f64::min);
        let g1 = moved.iter().map(|m| m.1 + m.2).fold(f64::NEG_INFINITY, f64::max);
        let len = g1 - g0;
        let foreign_inside =
            self.blocks.iter().any(|b| b.track == src && !ids.contains(b.id.as_str()) && b.start < g1 - EPS && b.end() > g0 + EPS);
        if foreign_inside {
            return Err("Only blocks that are next to each other can be inserted between other blocks – \
                        select a continuous stretch with Shift+click."
                .into());
        }
        let mut t = t.max(0.0);
        if track == src {
            if t > g0 - EPS && t < g1 + EPS {
                // its own place: nothing changes
                for (id, s, _, k) in moved {
                    if let Some(b) = self.block_mut(id) {
                        b.start = *s;
                        b.track = *k;
                    }
                }
                return Ok(0.0);
            }
            // close the room the unit leaves behind
            for b in self.blocks.iter_mut().filter(|b| b.track == src && !ids.contains(b.id.as_str()) && b.start >= g1 - EPS) {
                b.start = round4(b.start - len);
            }
            if t >= g1 - EPS {
                t -= len;
            }
        }
        // make room at the new place …
        for b in self.blocks.iter_mut().filter(|b| b.track == track && !ids.contains(b.id.as_str()) && b.start >= t - EPS) {
            b.start = round4(b.start + len);
        }
        // … and put the unit there, keeping its inner layout
        for (id, s, _, _) in moved {
            if let Some(b) = self.block_mut(id) {
                b.start = round4(t + (s - g0));
                b.track = track;
            }
        }
        Ok(len)
    }

    /// Insert a sequence of blocks as one unit. If it lands inside a stimulus block it moves behind
    /// that block; everything from the insertion point on – on all tracks – moves right.
    pub fn insert_group(&mut self, mut blocks: Vec<Block>) -> Vec<String> {
        if blocks.is_empty() {
            return vec![];
        }
        let mut t0 = blocks.iter().map(|b| b.start).fold(f64::INFINITY, f64::min);
        let inside_end = self
            .blocks
            .iter()
            .filter(|o| o.track == 0 && o.start < t0 - 1e-9 && t0 - 1e-9 < o.end() - 2e-9)
            .map(|o| o.end())
            .fold(f64::NEG_INFINITY, f64::max);
        if inside_end.is_finite() {
            let shift = inside_end - t0;
            for b in blocks.iter_mut() {
                b.start = round4(b.start + shift);
            }
            t0 += shift;
        }
        let span = blocks.iter().map(|b| b.end()).fold(f64::NEG_INFINITY, f64::max) - t0;
        if self.blocks.iter().any(|o| o.end() > t0 + 1e-9 && o.start < t0 + span - 1e-9) {
            for o in self.blocks.iter_mut() {
                if o.start >= t0 - 1e-9 {
                    o.start = round4(o.start + span);
                }
            }
        }
        let ids = blocks.iter().map(|b| b.id.clone()).collect();
        self.blocks.extend(blocks);
        ids
    }

    pub fn ripple_delete(&mut self, ids: &HashSet<String>) {
        let mut removed: Vec<Block> = self.blocks.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
        self.blocks.retain(|b| !ids.contains(&b.id));
        removed.sort_by(|a, b| b.start.total_cmp(&a.start));
        for r in removed {
            for o in self.blocks.iter_mut() {
                if o.track == r.track && o.start >= r.end() - 1e-9 {
                    o.start -= r.duration;
                }
            }
        }
    }

    pub fn close_gaps(&mut self, track: usize) {
        let mut ids: Vec<(f64, String)> =
            self.blocks.iter().filter(|b| b.track == track).map(|b| (b.start, b.id.clone())).collect();
        ids.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut t = 0.0;
        for (_, id) in ids {
            if let Some(b) = self.block_mut(&id) {
                b.start = t;
                t = b.end();
            }
        }
    }

    pub fn validate(&self) -> Vec<String> {
        let mut problems = vec![];
        let mut bl: Vec<&Block> = self.blocks.iter().collect();
        bl.sort_by(|a, b| a.start.total_cmp(&b.start));
        for b in bl {
            let place = format!("{} @ {:.2}s", b.display_label(), b.start);
            let p = &b.params;
            if b.kind == "image" && p.nonempty("path").is_none() {
                problems.push(format!("{place}: no image chosen"));
            }
            if (b.kind == "video" || b.kind == "audio") && p.nonempty("path").is_none() {
                problems.push(format!("{place}: no {} file chosen", b.kind));
            }
            if b.kind == "video" || b.kind == "audio" {
                if let Some(f) = p.nonempty("path") {
                    if !Path::new(&f).exists() {
                        problems.push(format!("{place}: {} file not found", b.kind));
                    }
                }
            }
            if b.kind == "choice" && p.nonempty("text").is_none() && p.nonempty("path").is_none() {
                problems.push(format!("{place}: choice trial shows nothing (no text or picture)"));
            }
            if b.kind == "feedback" && self.previous_scored(b).is_none() {
                problems.push(format!("{place}: feedback has no scored trial before it"));
            }
            if b.kind == "match" && !["top", "left", "right"].iter().all(|k| p.nonempty(k).is_some()) {
                problems.push(format!("{place}: needs a top, left and right picture"));
            }
            if b.image_files().iter().any(|f| !Path::new(f).exists()) {
                problems.push(format!("{place}: image file not found"));
            }
            if self.has_overlap(b) {
                problems.push(format!("{place}: overlaps another block"));
            }
        }
        let sounds = self.blocks.iter().filter(|b| b.kind == "audio").count();
        if sounds > 0 && self.hardware.audio_device.is_empty() {
            problems.push(format!(
                "Sound is off: {sounds} audio block(s) will be silent – choose a sound output in Hardware → Audio"
            ));
        }
        problems
    }

    // ---- (de)serialisation ---------------------------------------------------
    pub fn to_value(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            m.insert("format".into(), json!(FORMAT_VERSION));
        }
        v
    }

    /// Tolerant reader: unknown keys are ignored, missing ones take their defaults. Older files had
    /// "response window" blocks on a second track; those (and a track that only held them) are dropped.
    pub fn from_value(d: &Value) -> Result<Paradigm, String> {
        let obj = d.as_object().ok_or("not a paradigm file (expected a JSON object)")?;
        let def = Paradigm::default();
        let mut blocks = vec![];
        if let Some(Value::Array(arr)) = obj.get("blocks") {
            for b in arr {
                let kind = b.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                if block_type(kind).is_none() {
                    continue;
                }
                let mut blk: Block = serde_json::from_value(b.clone())
                    .map_err(|e| format!("block {}: {e}", b.get("id").map(|x| x.to_string()).unwrap_or_default()))?;
                if blk.id.is_empty() {
                    blk.id = new_id();
                }
                blocks.push(blk);
            }
        }
        let names: Vec<String> = match obj.get("tracks") {
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => default_tracks(),
        };
        let names = if names.is_empty() { default_tracks() } else { names };
        let max_track = blocks.iter().map(|b| b.track).max().unwrap_or(0);
        let mut tracks: Vec<String> = names.into_iter().take(max_track + 1).collect();
        while tracks.len() < max_track + 1 {
            tracks.push(format!("Track {}", tracks.len() + 1));
        }
        let mut hardware: Hardware = match obj.get("hardware") {
            Some(h) => serde_json::from_value(h.clone()).unwrap_or_default(),
            None => Hardware::default(),
        };
        if hardware.buttons.is_empty() {
            hardware.buttons = default_buttons();
        }
        let gs = |k: &str, d: &str| obj.get(k).and_then(|x| x.as_str()).unwrap_or(d).to_string();
        Ok(Paradigm {
            name: gs("name", &def.name),
            tr: obj.get("tr").and_then(|x| x.as_f64()).unwrap_or(2.0),
            dummy_scans: obj.get("dummy_scans").and_then(|x| x.as_f64()).unwrap_or(0.0).max(0.0) as u32,
            background: gs("background", "#000000"),
            waiting_screen: gs("waiting_screen", "fixation"),
            waiting_text: gs("waiting_text", DEFAULT_WAITING_TEXT),
            end_text: gs("end_text", DEFAULT_END_TEXT),
            tracks,
            blocks,
            hardware,
        })
    }

    /// Save as JSON; pictures are stored relative to the paradigm file so projects can be moved.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let base = absolute(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut d = self.to_value();
        if let Some(Value::Array(blocks)) = d.get_mut("blocks") {
            for b in blocks.iter_mut() {
                if let Some(Value::Object(params)) = b.get_mut("params") {
                    for key in IMAGE_PARAMS {
                        if let Some(Value::String(p)) = params.get(*key) {
                            if !p.is_empty() && Path::new(p).is_absolute() {
                                if let Some(rel) = relpath(Path::new(p), &base) {
                                    params.insert(key.to_string(), Value::String(rel));
                                }
                            }
                        }
                    }
                }
            }
        }
        let text = serde_json::to_string_pretty(&d).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
        }
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Paradigm, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut p = Paradigm::from_value(&v)?;
        let base = absolute(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for b in p.blocks.iter_mut() {
            for key in IMAGE_PARAMS {
                if let Some(Value::String(rel)) = b.params.get(*key) {
                    if !rel.is_empty() && !Path::new(rel).is_absolute() {
                        let full = normalize(&base.join(rel));
                        b.params.insert(key.to_string(), Value::String(full.to_string_lossy().to_string()));
                    }
                }
            }
        }
        Ok(p)
    }
}

pub fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

pub fn round_to(x: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (x * f).round() / f
}

/// Python-style `%g`-ish formatting for numbers like TRs and durations ("2", "1.5", "0.25").
pub fn fmt_g(x: f64) -> String {
    if !x.is_finite() {
        return x.to_string();
    }
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// Like Python's `f"{x:.3g}"` (3 significant digits).
pub fn fmt_sig3(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return fmt_g(x);
    }
    let mag = x.abs().log10().floor() as i32;
    let dec = (2 - mag).max(0);
    fmt_g(round_to(x, dec))
}

pub fn fmt_time(t: f64, decimals: usize) -> String {
    let sign = if t < 0.0 { "-" } else { "" };
    let t = round_to(t.abs(), decimals as i32);
    let m = (t / 60.0).floor();
    let s = t - m * 60.0;
    let width = if decimals > 0 { 3 + decimals } else { 2 };
    format!("{sign}{:02}:{:0width$.decimals$}", m as i64, s, width = width, decimals = decimals)
}

// --------------------------------------------------------------------------
// Paths
// --------------------------------------------------------------------------
pub fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        normalize(p)
    } else {
        normalize(&std::env::current_dir().unwrap_or_default().join(p))
    }
}

/// Lexical normalisation (like `os.path.normpath`).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `p` relative to `base` with forward slashes, or None on different drives / roots.
pub fn relpath(p: &Path, base: &Path) -> Option<String> {
    let p = normalize(p);
    let base = normalize(base);
    let pc: Vec<Component> = p.components().collect();
    let bc: Vec<Component> = base.components().collect();
    // different prefix (e.g. another drive on Windows)
    if pc.first() != bc.first() {
        return None;
    }
    let mut i = 0;
    while i < pc.len() && i < bc.len() && pc[i] == bc[i] {
        i += 1;
    }
    let mut parts: Vec<String> = vec![];
    for _ in i..bc.len() {
        parts.push("..".into());
    }
    for c in &pc[i..] {
        parts.push(c.as_os_str().to_string_lossy().to_string());
    }
    if parts.is_empty() {
        return Some(".".into());
    }
    Some(parts.join("/"))
}

// --------------------------------------------------------------------------
// Design generators
// --------------------------------------------------------------------------
#[derive(Clone, Debug, Default)]
pub struct DesignCondition {
    pub name: String,
    pub kind: String,
    pub images: Vec<String>,
}

/// Classic ON/OFF block design. Each block of a condition with several images is split into
/// equal image events.
#[allow(clippy::too_many_arguments)]
pub fn generate_block_design(
    conditions: &[DesignCondition],
    block_dur: f64,
    rest_dur: f64,
    repetitions: u32,
    random_order: bool,
    start: f64,
    lead_in: f64,
    seed: Option<u64>,
) -> Vec<Block> {
    let mut rng = Rng::new(seed);
    let mut seq: Vec<usize> = vec![];
    for _ in 0..repetitions {
        let mut cyc: Vec<usize> = (0..conditions.len()).collect();
        if random_order {
            rng.shuffle(&mut cyc);
        }
        seq.extend(cyc);
    }
    let mut blocks = vec![];
    let mut t = start;
    if lead_in > 0.0 {
        blocks.push(Block::create("fixation", t, Some(lead_in)).lab("Baseline"));
        t += lead_in;
    }
    for ci in seq {
        let cond = &conditions[ci];
        if !cond.images.is_empty() {
            let n = cond.images.len();
            let step = block_dur / n as f64;
            let mut images = cond.images.clone();
            if random_order {
                rng.shuffle(&mut images);
            }
            for img in images {
                blocks.push(Block::create("image", t, Some(step)).with(json!({"path": img})).cond(&cond.name));
                t += step;
            }
        } else {
            let kind = if cond.kind.is_empty() { "fixation" } else { cond.kind.as_str() };
            blocks.push(Block::create(kind, t, Some(block_dur)).cond(&cond.name).lab(&cond.name));
            t += block_dur;
        }
        if rest_dur > 0.0 {
            blocks.push(Block::create("fixation", t, Some(rest_dur)).lab("Rest"));
            t += rest_dur;
        }
    }
    blocks
}

/// Event-related design: every picture of every condition once, each marked with its condition,
/// followed by a fixation ITI. `itis` are the ITIs in presentation order (repeated if there are
/// fewer than pictures; an ITI of 0 leaves the fixation out). `shuffle`: all conditions mixed at
/// random; otherwise in the listed order, condition by condition.
#[allow(clippy::too_many_arguments)]
pub fn generate_event_design(
    conditions: &[DesignCondition],
    stim_dur: f64,
    itis: &[f64],
    rating: bool,
    shuffle: bool,
    start: f64,
    seed: Option<u64>,
) -> Vec<Block> {
    let mut rng = Rng::new(seed);
    let mut events: Vec<(&str, &String)> =
        conditions.iter().flat_map(|c| c.images.iter().map(move |img| (c.name.as_str(), img))).collect();
    if shuffle {
        rng.shuffle(&mut events);
    }
    let mut blocks = vec![];
    let mut t = start;
    for (i, (cond, img)) in events.into_iter().enumerate() {
        let kind = if rating { "rating" } else { "image" };
        blocks.push(Block::create(kind, t, Some(stim_dur)).with(json!({"path": img})).cond(cond));
        t = round4(t + stim_dur);
        let iti = if itis.is_empty() { 0.0 } else { itis[i % itis.len()] };
        if iti > 0.0 {
            blocks.push(Block::create("fixation", t, Some(round4(iti))).lab("ITI"));
            t = round4(t + iti);
        }
    }
    blocks
}

// --------------------------------------------------------------------------
// Jitter: the ITIs of the event-related design
// --------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JitterShape {
    /// every length between min and max equally often
    Uniform,
    /// many short, few long ITIs (truncated exponential with a chosen mean)
    Exponential,
}

/// How the ITIs are chosen: from a distribution, or from an own list of values.
#[derive(Clone, Debug, PartialEq)]
pub struct Jitter {
    pub from_list: bool,
    pub shape: JitterShape,
    pub min: f64,
    pub max: f64,
    /// exponential: the wanted mean – between min and the middle of min and max
    pub mean: f64,
    /// round the values of the distribution to multiples of this (s); 0 = not at all
    pub grid: f64,
    /// own values (s) and how often each is wanted
    pub list: Vec<(f64, u32)>,
}

impl Jitter {
    /// The `n` ITIs, sorted. A distribution is sampled at evenly spaced quantiles from min to max,
    /// so `n` values follow its shape exactly (no lucky or unlucky random draws); a list is used in
    /// the proportions of its counts.
    pub fn sorted(&self, n: usize) -> Vec<f64> {
        if n == 0 {
            return vec![];
        }
        let mut v: Vec<f64> = if self.from_list {
            list_values(&self.list, n)
        } else {
            // uniform: from "min" to "max" inclusive; exponential: the middle of n equal parts
            // (so that the mean is the one asked for, even with few values)
            let uniform = self.shape == JitterShape::Uniform || !self.mean_ok();
            let q = |i: usize| if uniform && n > 1 { i as f64 / (n - 1) as f64 } else { (i as f64 + 0.5) / n as f64 };
            (0..n).map(|i| self.snap(self.quantile(q(i)))).collect()
        };
        v.sort_by(f64::total_cmp);
        v
    }

    /// The `n` ITIs in random order.
    pub fn values(&self, n: usize, seed: Option<u64>) -> Vec<f64> {
        let mut v = self.sorted(n);
        Rng::new(seed).shuffle(&mut v);
        v
    }

    fn bounds(&self) -> (f64, f64) {
        let (a, b) = (self.min.max(0.0), self.max.max(0.0));
        if a <= b { (a, b) } else { (b, a) }
    }

    /// Round to the grid – staying between min and max when a grid value lies in between.
    fn snap(&self, x: f64) -> f64 {
        let g = self.grid;
        if g <= 0.0 {
            return round_to(x, 3);
        }
        let (lo, hi) = self.bounds();
        let mut k = (x / g).round_ties_even();
        let (k_lo, k_hi) = ((lo / g - 1e-9).ceil(), (hi / g + 1e-9).floor());
        if k_lo <= k_hi {
            k = k.clamp(k_lo, k_hi);
        }
        round4(k * g).max(0.0)
    }

    fn quantile(&self, q: f64) -> f64 {
        let (lo, hi) = self.bounds();
        match (self.shape, exp_scale(lo, hi, self.mean)) {
            (JitterShape::Exponential, Some(m)) => lo - m * (1.0 - q * (1.0 - (-(hi - lo) / m).exp())).ln(),
            _ => lo + q * (hi - lo),
        }
    }

    /// Exponential: is the mean possible (between min and the middle)?
    pub fn mean_ok(&self) -> bool {
        let (lo, hi) = self.bounds();
        exp_scale(lo, hi, self.mean).is_some()
    }

    /// Multiply all times (e.g. when the TR changes and the times are meant in TRs).
    pub fn scale(&mut self, f: f64) {
        self.min = round4(self.min * f);
        self.max = round4(self.max * f);
        self.mean = round4(self.mean * f);
        for (v, _) in self.list.iter_mut() {
            *v = round4(*v * f);
        }
    }
}

/// Scale of the exponential on [lo, hi] (cut off at hi) whose mean is `mean`.
fn exp_scale(lo: f64, hi: f64, mean: f64) -> Option<f64> {
    let l = hi - lo;
    let target = mean - lo;
    if l <= 0.0 || target <= 0.0 || target >= l / 2.0 - 1e-9 {
        return None;
    }
    // mean above lo of the cut-off exponential: grows from 0 (m → 0) to l/2 (m → ∞)
    let f = |m: f64| {
        let e = (-l / m).exp();
        m - l * e / (1.0 - e)
    };
    let (mut a, mut b) = (1e-6_f64.ln(), 1e6_f64.ln());
    for _ in 0..200 {
        let c = 0.5 * (a + b);
        if f(c.exp()) < target {
            a = c;
        } else {
            b = c;
        }
    }
    Some((0.5 * (a + b)).exp())
}

/// `n` values from a list of (value, how often), in the proportions of the counts.
pub fn list_values(list: &[(f64, u32)], n: usize) -> Vec<f64> {
    let total: u64 = list.iter().map(|(_, c)| *c as u64).sum();
    if total == 0 || n == 0 {
        return vec![];
    }
    // largest remainder: floor of each share, then one more for the biggest remainders
    let shares: Vec<f64> = list.iter().map(|(_, c)| *c as f64 * n as f64 / total as f64).collect();
    let mut counts: Vec<usize> = shares.iter().map(|s| s.floor() as usize).collect();
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by(|&a, &b| (shares[b] - shares[b].floor()).total_cmp(&(shares[a] - shares[a].floor())).then(a.cmp(&b)));
    let mut missing = n - counts.iter().sum::<usize>();
    for i in order {
        if missing == 0 {
            break;
        }
        counts[i] += 1;
        missing -= 1;
    }
    list.iter().zip(counts).flat_map(|((v, _), k)| std::iter::repeat_n(*v, k)).collect()
}

/// Sorted values → (value, how often).
pub fn compact_values(sorted: &[f64]) -> Vec<(f64, u32)> {
    let mut out: Vec<(f64, u32)> = vec![];
    for &v in sorted {
        match out.last_mut() {
            Some((w, c)) if (*w - v).abs() < 1e-6 => *c += 1,
            _ => out.push((v, 1)),
        }
    }
    out
}

/// "2, 3, 4.5×2" – values (in units of `unit` seconds: 1 = seconds, TR = in TRs) with an
/// optional "×n" for how often. Separated by commas, semicolons or spaces.
pub fn parse_jitter_list(text: &str, unit: f64) -> Result<Vec<(f64, u32)>, String> {
    static TIMES: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\s*[×xX*]\s*").unwrap());
    let norm = TIMES.replace_all(text.trim(), "×");
    let mut out = vec![];
    for tok in norm.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|t| !t.is_empty()) {
        let (v, c) = match tok.split_once('×') {
            Some((v, c)) => (v, c),
            None => (tok, "1"),
        };
        let v: f64 = v.parse().map_err(|_| format!("“{tok}” is not a number"))?;
        let c: u32 = c.parse().map_err(|_| format!("“{tok}”: after × comes how often, e.g. 3×4"))?;
        if !(0.0..=600.0).contains(&(v * unit)) {
            return Err(format!("“{tok}”: between 0 and 600 s, please"));
        }
        if c > 0 {
            out.push((round4(v * unit), c));
        }
    }
    if out.is_empty() {
        return Err("Type the ITI lengths, e.g. 2, 3, 4.5×2".into());
    }
    Ok(out)
}

/// The list as text, in units of `unit` seconds.
pub fn format_jitter_list(list: &[(f64, u32)], unit: f64) -> String {
    list.iter()
        .map(|(v, c)| {
            let x = fmt_g(round_to(v / unit, 4));
            if *c == 1 { x } else { format!("{x}×{c}") }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// --------------------------------------------------------------------------
// Bulk text editing
// --------------------------------------------------------------------------
pub const TEXT_FIELDS: &[(&str, &str)] = &[
    ("text", "Instruction / Go-No-Go text"),
    ("question", "Rating question"),
    ("left_label", "Rating left label"),
    ("right_label", "Rating right label"),
    ("caption", "Text under a picture"),
    ("label", "Block label"),
];

pub fn text_field_title(f: &str) -> &'static str {
    TEXT_FIELDS.iter().find(|(k, _)| *k == f).map(|(_, t)| *t).unwrap_or("")
}

fn needle_regex(needle: &str, case: bool) -> Option<regex::Regex> {
    regex::RegexBuilder::new(&regex::escape(needle)).case_insensitive(!case).build().ok()
}

/// All `(block id, field, value)` whose text contains `needle`.
pub fn find_text(
    par: &Paradigm,
    needle: &str,
    case: bool,
    fields: &[String],
    ids: Option<&HashSet<String>>,
) -> Vec<(String, String, String)> {
    if needle.is_empty() {
        return vec![];
    }
    let Some(rx) = needle_regex(needle, case) else { return vec![] };
    let mut bl: Vec<&Block> = par.blocks.iter().collect();
    bl.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.track.cmp(&b.track)));
    let mut out = vec![];
    for b in bl {
        if let Some(ids) = ids {
            if !ids.contains(&b.id) {
                continue;
            }
        }
        for f in fields {
            let val = if f == "label" {
                Some(b.label.clone())
            } else {
                match b.params.get(f.as_str()) {
                    Some(Value::String(s)) => Some(s.clone()),
                    _ => None,
                }
            };
            if let Some(v) = val {
                if rx.is_match(&v) {
                    out.push((b.id.clone(), f.clone(), v));
                }
            }
        }
    }
    out
}

/// Replace `needle` in all matching text fields. Returns the number of fields changed.
pub fn replace_text(
    par: &mut Paradigm,
    needle: &str,
    repl: &str,
    case: bool,
    fields: &[String],
    ids: Option<&HashSet<String>>,
) -> usize {
    let hits = find_text(par, needle, case, fields, ids);
    let Some(rx) = needle_regex(needle, case) else { return 0 };
    for (id, f, val) in &hits {
        let new = rx.replace_all(val, regex::NoExpand(repl)).to_string();
        if let Some(b) = par.block_mut(id) {
            if f == "label" {
                b.label = new;
            } else {
                b.params.insert(f.clone(), Value::String(new));
            }
        }
    }
    hits.len()
}

// --------------------------------------------------------------------------
// Export of the planned design
// --------------------------------------------------------------------------
/// Planned onsets: a BIDS-style events.tsv plus one FSL 3-column file per condition.
pub fn export_onsets(par: &Paradigm, folder: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let stem: String = par.name.chars().filter(|c| c.is_alphanumeric()).collect();
    let stem = if stem.is_empty() { "paradigm".to_string() } else { stem };
    let mut files = vec![];
    let path = folder.join(format!("{stem}_planned_events.tsv"));
    let mut s = String::from("onset\tduration\ttrial_type\tblock\tstim_file\n");
    let mut bl: Vec<&Block> = par.blocks.iter().collect();
    bl.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.track.cmp(&b.track)));
    for b in &bl {
        let tt = if b.condition.is_empty() { b.kind.clone() } else { b.condition.clone() };
        let stim = basename(&b.thumbnail_path());
        s.push_str(&format!(
            "{:.3}\t{:.3}\t{}\t{}\t{}\n",
            b.start,
            b.duration,
            tt,
            b.kind,
            if stim.is_empty() { "n/a".into() } else { stim }
        ));
    }
    std::fs::write(&path, s).map_err(|e| e.to_string())?;
    files.push(path);
    for cond in par.conditions() {
        let path = folder.join(format!("{stem}_{cond}.txt"));
        let mut s = String::new();
        for b in bl.iter().filter(|b| b.condition == cond && b.track == 0) {
            s.push_str(&format!("{:.3}\t{:.3}\t1\n", b.start, b.duration));
        }
        std::fs::write(&path, s).map_err(|e| e.to_string())?;
        files.push(path);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(specs: &[(&str, f64, f64)]) -> Paradigm {
        let mut p = Paradigm::default();
        for (k, s, d) in specs {
            p.blocks.push(Block::create(k, *s, Some(*d)));
        }
        p
    }

    /// eight contiguous 4-s blocks 0..8 on track 0; returns (paradigm, ids)
    fn run8() -> (Paradigm, Vec<String>) {
        let p = make(&[("fixation", 0.0, 4.0); 8].iter().enumerate().map(|(i, s)| (s.0, i as f64 * 4.0, s.2)).collect::<Vec<_>>());
        let ids = p.blocks.iter().map(|b| b.id.clone()).collect();
        (p, ids)
    }

    fn order(p: &Paradigm, ids: &[String]) -> Vec<(usize, f64)> {
        let mut v: Vec<(usize, f64)> = p.blocks.iter().map(|b| (ids.iter().position(|i| *i == b.id).unwrap(), b.start)).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    fn unit(p: &Paradigm, ids: &[String], which: &[usize]) -> Vec<(String, f64, f64, usize)> {
        which.iter().map(|i| p.block(&ids[*i]).map(|b| (b.id.clone(), b.start, b.duration, b.track)).unwrap()).collect()
    }

    #[test]
    fn move_insert_reorders_and_keeps_the_rest() {
        // block 4 (16–20 s) dropped between blocks 0 and 1 (t = 4)
        let (mut p, ids) = run8();
        let u = unit(&p, &ids, &[4]);
        assert_eq!(p.move_insert(&u, 4.0, 0).unwrap(), 4.0);
        let o = order(&p, &ids);
        assert_eq!(o.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 4, 1, 2, 3, 5, 6, 7]);
        assert_eq!(o.iter().map(|x| x.1).collect::<Vec<_>>(), vec![0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0]);
        // blocks 1–2 (a Shift+click range) moved behind block 5 (boundary at 24 s)
        let (mut p, ids) = run8();
        let u = unit(&p, &ids, &[1, 2]);
        assert_eq!(p.move_insert(&u, 24.0, 0).unwrap(), 8.0);
        let o = order(&p, &ids);
        assert_eq!(o.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 3, 4, 5, 1, 2, 6, 7]);
        assert_eq!(o.last().unwrap().1, 28.0); // the end of the run is unchanged
        assert!(!p.blocks.iter().any(|b| p.has_overlap(b)));
    }

    #[test]
    fn move_insert_special_cases() {
        // dropped on its own place: nothing changes
        let (mut p, ids) = run8();
        let before = p.clone();
        let u = unit(&p, &ids, &[3]);
        assert_eq!(p.move_insert(&u, 16.0, 0).unwrap(), 0.0);
        assert_eq!(p, before);
        // blocks with another block in between can't be inserted as a unit
        let u = unit(&p, &ids, &[2, 4]);
        assert!(p.move_insert(&u, 0.0, 0).is_err());
        // to another track: room is made there, the source track keeps its gap
        let (mut p, ids) = run8();
        p.tracks.push("Second".into());
        let mut b = Block::create("fixation", 0.0, Some(4.0));
        b.track = 1;
        p.blocks.push(b);
        let u = unit(&p, &ids, &[5]);
        p.move_insert(&u, 0.0, 1).unwrap();
        assert_eq!(p.block(&ids[5]).map(|b| (b.track, b.start)), Some((1, 0.0)));
        assert_eq!(p.block(&ids[6]).unwrap().start, 24.0);
        assert!(p.blocks.iter().any(|b| b.track == 1 && b.start == 4.0));
    }

    #[test]
    fn media_files_are_pictures_videos_and_sounds() {
        for f in ["a.png", "b.JPG", "c.mp4", "d.MOV", "e.wav", "f.mp3", "g.flac", "h.m4a"] {
            assert!(is_media(f), "{f}");
        }
        for f in ["a.txt", "b.json", "c.avi", ""] {
            assert!(!is_media(f), "{f}");
        }
        let v = Block::create("video", 0.0, None).with(json!({"path": "/x/clip.mp4"}));
        assert_eq!(v.media_files(), vec!["/x/clip.mp4".to_string()]);
        assert!(v.image_files().is_empty());
        let m = Block::create("match", 0.0, None).with(json!({"top": "/a.png", "left": "/b.png", "right": ""}));
        assert_eq!(m.media_files().len(), 2);
    }

    #[test]
    fn rating_cursor_starts_in_the_middle_or_where_set() {
        let r = |params: serde_json::Value| Block::create("rating", 0.0, None).with(params).rating_start();
        assert_eq!(r(json!({"min": 1, "max": 5})), 3);
        assert_eq!(r(json!({"min": 1, "max": 4})), 3); // even: the upper of the two middle values
        assert_eq!(r(json!({"min": 1, "max": 7})), 4);
        assert_eq!(r(json!({"min": 1, "max": 7, "start": "middle"})), 4);
        assert_eq!(r(json!({"min": 1, "max": 7, "start": 1})), 1);
        assert_eq!(r(json!({"min": 1, "max": 4, "start": 9})), 4); // kept within the scale
    }

    #[test]
    fn duration_and_volumes() {
        let mut p = make(&[("fixation", 0.0, 10.0), ("image", 10.0, 5.0)]);
        assert_eq!(p.duration(), 15.0);
        p.tr = 2.0;
        assert_eq!(p.volumes(), 8);
    }

    #[test]
    fn ripple_insert_pushes_later_blocks() {
        let mut p = make(&[("fixation", 0.0, 10.0), ("image", 10.0, 4.0)]);
        p.ripple_insert(Block::create("rest", 10.0, Some(6.0)));
        let mut starts: Vec<(f64, String)> = p.blocks.iter().map(|b| (b.start, b.kind.clone())).collect();
        starts.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(starts, vec![(0.0, "fixation".into()), (10.0, "rest".into()), (16.0, "image".into())]);
        assert!(!p.blocks.iter().any(|b| p.has_overlap(b)));
    }

    #[test]
    fn ripple_insert_into_middle_goes_behind_block() {
        let mut p = make(&[("fixation", 0.0, 10.0), ("image", 10.0, 4.0)]);
        let id = p.ripple_insert(Block::create("rest", 3.0, Some(2.0)));
        let b = p.block(&id).unwrap();
        assert_eq!(b.start, 10.0);
        assert_eq!(b.end(), 12.0);
        let kinds: Vec<&str> = p.blocks_on(0).iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, vec!["fixation", "rest", "image"]);
        assert!(!p.blocks.iter().any(|b| p.has_overlap(b)));
    }

    #[test]
    fn ripple_insert_into_free_space_keeps_position() {
        let mut p = make(&[("fixation", 0.0, 10.0), ("image", 20.0, 4.0)]);
        let id = p.ripple_insert(Block::create("rest", 12.0, Some(3.0)));
        assert_eq!(p.block(&id).unwrap().start, 12.0);
        assert_eq!(p.blocks[1].start, 20.0);
    }

    #[test]
    fn ripple_delete_closes_gap() {
        let mut p = make(&[("fixation", 0.0, 10.0), ("image", 10.0, 4.0), ("rest", 14.0, 6.0)]);
        let ids: HashSet<String> = [p.blocks[1].id.clone()].into_iter().collect();
        p.ripple_delete(&ids);
        let v: Vec<(String, f64)> = p.blocks_on(0).iter().map(|b| (b.kind.clone(), b.start)).collect();
        assert_eq!(v, vec![("fixation".into(), 0.0), ("rest".into(), 10.0)]);
    }

    #[test]
    fn stimulus_at() {
        let p = make(&[("image", 0.0, 4.0)]);
        assert_eq!(p.stimulus_at(1.0).unwrap().kind, "image");
        assert!(p.stimulus_at(5.0).is_none());
    }

    #[test]
    fn old_files_with_response_windows_still_load() {
        let mut d = make(&[("image", 0.0, 4.0)]).to_value();
        d["tracks"] = json!(["Stimuli", "Responses"]);
        d["blocks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind": "response", "start": 0, "duration": 4, "track": 1, "params": {"buttons": "any"}}));
        let p = Paradigm::from_value(&d).unwrap();
        assert_eq!(p.blocks.len(), 1);
        assert_eq!(p.blocks[0].kind, "image");
        assert_eq!(p.tracks, vec!["Stimuli".to_string()]);
    }

    #[test]
    fn save_load_roundtrip_relative_paths() {
        let dir = std::env::temp_dir().join(format!("ps_test_{}", new_id()));
        let img = dir.join("pics").join("a.png");
        std::fs::create_dir_all(img.parent().unwrap()).unwrap();
        std::fs::write(&img, b"x").unwrap();
        let mut p = Paradigm { name: "T".into(), tr: 1.5, ..Default::default() };
        p.blocks.push(
            Block::create("image", 0.0, Some(2.0)).with(json!({"path": img.to_string_lossy()})).cond("faces"),
        );
        p.hardware.trigger_key = "5".into();
        let f = dir.join("t.paradigm.json");
        p.save(&f).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("\"pics/a.png\""));
        assert!(text.contains("\"format\": 1"));
        let q = Paradigm::load(&f).unwrap();
        assert_eq!(q.tr, 1.5);
        assert_eq!(q.hardware.trigger_key, "5");
        assert_eq!(
            std::fs::canonicalize(q.blocks[0].params.s("path", "")).unwrap(),
            std::fs::canonicalize(&img).unwrap()
        );
        assert_eq!(q.blocks[0].condition, "faces");
        assert!(q.validate().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn loads_the_bundled_examples() {
        let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut n = 0;
        for e in std::fs::read_dir(ex).unwrap() {
            let p = e.unwrap().path();
            if p.to_string_lossy().ends_with(FILE_SUFFIX) {
                let par = Paradigm::load(&p).unwrap();
                assert!(!par.blocks.is_empty());
                assert!(par.validate().is_empty(), "{}: {:?}", p.display(), par.validate());
                n += 1;
            }
        }
        assert!(n >= 10);
    }

    #[test]
    fn validate_reports_missing_image_and_overlap() {
        let p = make(&[("image", 0.0, 4.0), ("fixation", 2.0, 4.0)]);
        let msgs = p.validate().join(" ");
        assert!(msgs.contains("no image") && msgs.contains("overlaps"));
    }

    #[test]
    fn block_design_generator() {
        let conds = vec![
            DesignCondition { name: "A".into(), kind: "fixation".into(), images: vec![] },
            DesignCondition { name: "B".into(), kind: "text".into(), images: vec![] },
        ];
        let mut blocks = generate_block_design(&conds, 20.0, 10.0, 3, false, 0.0, 10.0, None);
        let stim: Vec<String> = blocks.iter().filter(|b| !b.condition.is_empty()).map(|b| b.condition.clone()).collect();
        assert_eq!(stim, ["A", "B", "A", "B", "A", "B"]);
        let end = blocks.iter().map(|b| b.end()).fold(0.0, f64::max);
        assert_eq!(end, 10.0 + 6.0 * 30.0);
        blocks.sort_by(|a, b| a.start.total_cmp(&b.start));
        for w in blocks.windows(2) {
            assert!((w[0].end() - w[1].start).abs() < 1e-9);
        }
    }

    #[test]
    fn block_design_splits_pictures() {
        let conds = vec![DesignCondition {
            name: "faces".into(),
            kind: "image".into(),
            images: vec!["a.png".into(), "b.png".into(), "c.png".into(), "d.png".into()],
        }];
        let blocks = generate_block_design(&conds, 20.0, 0.0, 1, false, 0.0, 0.0, None);
        assert_eq!(blocks.len(), 4);
        assert!(blocks.iter().all(|b| b.duration == 5.0));
    }

    #[test]
    fn event_design_jitter_on_quarter_tr() {
        let imgs: Vec<String> = (0..10).map(|i| format!("{i}.png")).collect();
        let conds = vec![DesignCondition { name: "pic".into(), kind: "image".into(), images: imgs }];
        let jit = Jitter { from_list: false, shape: JitterShape::Uniform, min: 2.0, max: 6.0, mean: 3.0, grid: 0.5, list: vec![] };
        let blocks = generate_event_design(&conds, 3.0, &jit.values(10, Some(3)), false, true, 0.0, Some(3));
        let itis: Vec<&Block> = blocks.iter().filter(|b| b.label == "ITI").collect();
        assert_eq!(itis.len(), 10);
        for b in itis {
            assert!(b.duration >= 0.5 && b.duration <= 6.0);
            assert!((b.duration / 0.5 - (b.duration / 0.5).round()).abs() < 1e-6);
        }
        assert_eq!(blocks.iter().filter(|b| b.kind == "image").count(), 10);
    }

    #[test]
    fn event_design_with_conditions() {
        let cond = |n: &str, k: usize| DesignCondition {
            name: n.into(),
            kind: "image".into(),
            images: (0..k).map(|i| format!("{n}_{i}.png")).collect(),
        };
        let conds = vec![cond("negative", 6), cond("neutral", 4), DesignCondition { name: "empty".into(), kind: "image".into(), images: vec![] }];
        // every picture once, with its own condition
        let itis = [1.0, 2.0, 3.0];
        let blocks = generate_event_design(&conds, 2.0, &itis, true, true, 0.0, Some(7));
        let pics: Vec<&Block> = blocks.iter().filter(|b| b.kind == "rating").collect();
        assert_eq!(pics.len(), 10);
        for b in &pics {
            let path = b.params.s("path", "");
            assert!(path.starts_with(&format!("{}_", b.condition)), "{path} marked {}", b.condition);
        }
        // mixed: the conditions take turns somewhere
        let order: Vec<&str> = pics.iter().map(|b| b.condition.as_str()).collect();
        assert!(order.windows(2).any(|w| w[0] != w[1]) && order[..6] != ["negative"; 6]);
        // as listed: condition by condition, in order
        let blocks = generate_event_design(&conds, 2.0, &itis, false, false, 0.0, Some(7));
        let paths: Vec<String> = blocks.iter().filter(|b| b.kind == "image").map(|b| b.params.s("path", "")).collect();
        let want: Vec<String> = conds.iter().flat_map(|c| c.images.clone()).collect();
        assert_eq!(paths, want);
    }

    #[test]
    fn jitter_from_distribution_and_list() {
        let base = Jitter { from_list: false, shape: JitterShape::Uniform, min: 2.0, max: 6.0, mean: 3.0, grid: 0.5, list: vec![] };
        // uniform: evenly spread over the range, on the grid
        let v = base.sorted(9);
        assert_eq!(v.len(), 9);
        assert!(v.iter().all(|x| (2.0..=6.0).contains(x) && ((x / 0.5) - (x / 0.5).round()).abs() < 1e-9));
        let mean = v.iter().sum::<f64>() / 9.0;
        assert!((mean - 4.0).abs() < 1e-9, "{mean}");
        assert_eq!((v[0], v[8]), (2.0, 6.0));
        // exponential: the wanted mean, more short than long ones
        let ex = Jitter { shape: JitterShape::Exponential, min: 2.0, max: 10.0, mean: 3.5, grid: 0.0, ..base.clone() };
        assert!(ex.mean_ok());
        let v = ex.sorted(200);
        let mean = v.iter().sum::<f64>() / 200.0;
        assert!((mean - 3.5).abs() < 0.05, "{mean}");
        let few = ex.sorted(8);
        let mean8 = few.iter().sum::<f64>() / 8.0;
        assert!((mean8 - 3.5).abs() < 0.15, "{mean8}");
        assert!(v.iter().filter(|x| **x < 4.0).count() > v.iter().filter(|x| **x >= 4.0).count() * 2);
        assert!(v.iter().all(|x| (2.0..=10.0).contains(x)));
        // "from" off the grid: the values stay inside the range
        let off = Jitter { min: 2.0, max: 6.0, grid: 0.375, ..base.clone() }.sorted(8);
        assert!(off.iter().all(|x| (2.0..=6.0).contains(x)), "{off:?}");
        // impossible mean → falls back to uniform
        let bad = Jitter { mean: 7.0, ..ex.clone() };
        assert!(!bad.mean_ok());
        // own list: proportions kept for any number of events, shuffled but the same values
        let list = parse_jitter_list("2, 3 ×2; 6x1", 1.0).unwrap();
        assert_eq!(list, vec![(2.0, 1), (3.0, 2), (6.0, 1)]);
        let li = Jitter { from_list: true, list, ..base.clone() };
        assert_eq!(li.sorted(8), [2.0, 2.0, 3.0, 3.0, 3.0, 3.0, 6.0, 6.0]);
        assert_eq!(li.sorted(5).len(), 5);
        let mut sh = li.values(8, Some(4));
        sh.sort_by(f64::total_cmp);
        assert_eq!(sh, li.sorted(8));
        // lists in TRs, back to text
        let tr = parse_jitter_list("1, 1.5×3", 2.0).unwrap();
        assert_eq!(tr, vec![(2.0, 1), (3.0, 3)]);
        assert_eq!(format_jitter_list(&tr, 2.0), "1, 1.5×3");
        assert_eq!(format_jitter_list(&compact_values(&[2.0, 2.0, 3.5]), 1.0), "2×2, 3.5");
        assert!(parse_jitter_list("2, abc", 1.0).is_err());
        assert!(parse_jitter_list("", 1.0).is_err());
    }

    #[test]
    fn find_and_replace() {
        let mut p = Paradigm::default();
        p.blocks.push(Block::create("text", 0.0, None).with(json!({"text": "Press the BUTTON now"})));
        p.blocks.push(Block::create("rating", 8.0, None).with(json!({"question": "How was the button?"})));
        let fields: Vec<String> = TEXT_FIELDS.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(find_text(&p, "button", false, &fields, None).len(), 2);
        assert_eq!(find_text(&p, "button", true, &fields, None).len(), 1);
        assert_eq!(replace_text(&mut p, "button", "key ($1)", false, &fields, None), 2);
        assert_eq!(p.blocks[0].params.s("text", ""), "Press the key ($1) now");
    }

    #[test]
    fn time_formatting() {
        assert_eq!(fmt_time(65.24, 1), "01:05.2");
        assert_eq!(fmt_time(65.25, 2), "01:05.25");
        assert_eq!(fmt_time(65.0, 0), "01:05");
        assert_eq!(fmt_time(59.97, 1), "01:00.0");
        assert_eq!(fmt_g(2.0), "2");
        assert_eq!(fmt_g(0.25), "0.25");
        assert_eq!(fmt_sig3(2.0 / 3.0), "0.667");
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relpath(Path::new("/a/b/c.png"), Path::new("/a")).unwrap(), "b/c.png");
        assert_eq!(relpath(Path::new("/a/x/c.png"), Path::new("/a/b")).unwrap(), "../x/c.png");
    }
}
