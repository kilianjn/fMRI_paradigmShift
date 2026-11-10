//! Presents a paradigm in the scanner and logs triggers, events and grip responses.
//!
//! Timing: the paradigm clock starts at the first SyncBox trigger after the dummy scans (t = 0).
//! Planned onsets are relative to that trigger; actual onsets are measured when the run clock
//! (a 1 ms thread, see `controlroom.rs`) switches the stimulus. All triggers are logged too, so the timing
//! can be verified volume by volume afterwards.
//!
//! A run has one [`RunController`] (clock, hardware input, logging); the participant screen and
//! the control room are two views on it.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::hardware::{now, Input, InputRouter, Waker};
use crate::model::{basename, Block, Paradigm, ParamExt};

pub fn bids_label(text: &str) -> String {
    let s: String = text.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if s.is_empty() { "task".into() } else { s }
}

#[derive(Clone, Debug)]
pub struct RunSettings {
    pub sub: String,
    pub ses: String,
    pub run: u32,
    pub out_dir: String,
    pub screen: usize,
    pub fullscreen: bool,
    pub simulate: bool,
    pub dummy_scans: u32,
    pub waiting_screen: String,
    pub waiting_text: String,
    pub end_text: String,
    pub confirm_abort: bool,
    pub save_paradigm_with_data: bool,
    pub watchdog_trials: u32,
    pub trigger_warning_trs: f64,
    pub control_refresh_ms: u32,
    pub blur_control_mirror: bool,
    /// size of the participant window when not full screen (default 1280 × 720)
    pub window_size: Option<(f32, f32)>,
}

impl Default for RunSettings {
    fn default() -> Self {
        RunSettings {
            sub: "01".into(),
            ses: String::new(),
            run: 1,
            out_dir: String::new(),
            screen: 0,
            fullscreen: true,
            simulate: false,
            dummy_scans: 0,
            waiting_screen: "fixation".into(),
            waiting_text: String::new(),
            end_text: String::new(),
            confirm_abort: true,
            save_paradigm_with_data: true,
            watchdog_trials: 5,
            trigger_warning_trs: 2.5,
            control_refresh_ms: 100,
            blur_control_mirror: false,
            window_size: None,
        }
    }
}

pub fn output_stem(p: &Paradigm, s: &RunSettings) -> String {
    let mut parts = vec![format!("sub-{}", bids_label(if s.sub.is_empty() { "01" } else { &s.sub }))];
    if !s.ses.is_empty() {
        parts.push(format!("ses-{}", bids_label(&s.ses)));
    }
    parts.push(format!("task-{}", bids_label(&p.name)));
    parts.push(format!("run-{:02}", s.run));
    parts.join("_")
}

/// Python-style float text ("4.0", "1.25") so the TSV files look like those of the original app.
pub fn py_float(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 { format!("{x:.1}") } else { format!("{x}") }
}

fn na_f(x: Option<f64>) -> String {
    x.map(py_float).unwrap_or_else(|| "n/a".into())
}

fn tsv(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rating {
    NA,
    Value(i64),
    Unconfirmed(i64),
}

impl Rating {
    pub fn text(&self) -> String {
        match self {
            Rating::NA => "n/a".into(),
            Rating::Value(v) => v.to_string(),
            Rating::Unconfirmed(v) => format!("{v} (unconfirmed)"),
        }
    }
    pub fn value(&self) -> Option<i64> {
        match self {
            Rating::Value(v) => Some(*v),
            _ => None,
        }
    }
    pub fn is_na(&self) -> bool {
        *self == Rating::NA
    }
}

/// One row of the events file (one block that was shown).
#[derive(Clone, Debug)]
pub struct EventRow {
    pub id: String,
    pub onset: f64,
    pub duration: f64,
    pub planned_onset: f64,
    pub trial_type: String,
    pub block: String,
    pub label: String,
    pub stim_file: String,
    pub response: Option<String>,
    pub response_time: Option<f64>,
    pub rating: Rating,
    pub correct: Option<i32>,
    pub outcome: Option<String>,
}

impl EventRow {
    pub fn answered(&self) -> bool {
        self.response.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RatingState {
    pub value: Option<i64>,
    pub confirmed: bool,
    pub rt: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct LogRow {
    pub time: Option<f64>,
    pub kind: String,
    pub value: String,
    pub info: String,
}

/// One row of the button-press backup log.
#[derive(Clone, Debug)]
pub struct PressRow {
    pub time: Option<f64>,
    pub time_since_open: f64,
    pub volume: u32,
    pub wall_clock: String,
    pub key: String,
    pub button: String,
    pub block: String,
    pub block_kind: String,
    pub condition: String,
}

pub const PRESS_COLUMNS: [&str; 9] =
    ["time", "time_since_open", "volume", "wall_clock", "key", "button", "block", "block_kind", "condition"];

impl PressRow {
    fn tsv(&self) -> String {
        [
            na_f(self.time),
            py_float(self.time_since_open),
            self.volume.to_string(),
            self.wall_clock.clone(),
            self.key.clone(),
            if self.button.is_empty() { "unassigned".into() } else { self.button.clone() },
            self.block.clone(),
            self.block_kind.clone(),
            self.condition.clone(),
        ]
        .iter()
        .map(|s| tsv(s))
        .collect::<Vec<_>>()
        .join("\t")
    }
}

/// Paint state of the current block (rating cursor, chosen picture, feedback outcome).
#[derive(Clone, Debug, Default)]
pub struct PaintState {
    pub t: f64,
    pub value: Option<i64>,
    pub confirmed: bool,
    pub chosen: Option<String>,
    pub success: bool,
}

/// Was a finished trial done right? (Unanswered trials count as over – used for feedback blocks.)
pub fn trial_success(b: &Block, ev: &EventRow) -> bool {
    let answered = ev.answered();
    match b.kind.as_str() {
        "gonogo" => answered == (b.params.s("trial", "go") == "go"),
        "choice" => {
            let corr = b.params.s("correct", "");
            if corr.is_empty() { !answered } else { answered && ev.correct == Some(1) }
        }
        "match" => ev.correct == Some(1),
        _ => false,
    }
}

/// Bookkeeping of a run (kept separate from the windows for testing).
pub struct RunState {
    pub p: Paradigm,
    pub triggers: Vec<f64>,
    pub t0: Option<f64>,
    pub log: Vec<LogRow>,
    pub events: Vec<EventRow>,
    index: HashMap<String, usize>,
    pub ratings: HashMap<String, RatingState>,
    pub feedback: HashMap<String, bool>,
    pub presses: Vec<PressRow>,
    /// bumped on every change, lets views refresh cheaply
    pub version: u64,
}

impl RunState {
    pub fn new(p: Paradigm) -> Self {
        RunState {
            p,
            triggers: vec![],
            t0: None,
            log: vec![],
            events: vec![],
            index: HashMap::new(),
            ratings: HashMap::new(),
            feedback: HashMap::new(),
            presses: vec![],
            version: 0,
        }
    }

    pub fn event(&self, id: &str) -> Option<&EventRow> {
        self.index.get(id).map(|&i| &self.events[i])
    }

    fn event_mut(&mut self, id: &str) -> Option<&mut EventRow> {
        let i = *self.index.get(id)?;
        Some(&mut self.events[i])
    }

    pub fn rel(&self, t: f64) -> Option<f64> {
        self.t0.map(|t0| round4(t - t0))
    }

    /// Returns true when this trigger starts the paradigm.
    pub fn on_trigger(&mut self, t: f64) -> bool {
        self.triggers.push(t);
        let n = self.triggers.len() as u32;
        let mut started = false;
        if self.t0.is_none() && n == self.p.dummy_scans + 1 {
            self.t0 = Some(t);
            started = true;
        }
        let info = if self.t0.is_none() || n <= self.p.dummy_scans {
            "dummy".to_string()
        } else {
            format!("volume {}", n - self.p.dummy_scans)
        };
        let time = self.rel(t);
        self.log.push(LogRow { time, kind: "trigger".into(), value: n.to_string(), info });
        self.version += 1;
        started
    }

    pub fn record_press(&mut self, key: &str, name: &str, t: f64, t_open: f64) -> PressRow {
        let tt = self.t0.map(|t0| t - t0);
        let blk = tt.and_then(|x| self.p.stimulus_at(x));
        let row = PressRow {
            time: tt.map(round4),
            time_since_open: round4(t - t_open),
            volume: self.volumes_seen(),
            wall_clock: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
            key: key.to_string(),
            button: name.to_string(),
            block: match (blk, tt) {
                (Some(b), _) => b.display_label(),
                (None, None) => "before start".into(),
                (None, Some(_)) => "–".into(),
            },
            block_kind: blk.map(|b| b.kind.clone()).unwrap_or_default(),
            condition: blk.map(|b| b.condition.clone()).unwrap_or_default(),
        };
        self.presses.push(row.clone());
        self.version += 1;
        row
    }

    /// Start without waiting for the scanner (operator override, logged as such).
    pub fn force_start(&mut self, t: f64) {
        if self.t0.is_none() {
            self.t0 = Some(t);
            self.log.push(LogRow {
                time: Some(0.0),
                kind: "manual_start".into(),
                value: String::new(),
                info: "started without trigger".into(),
            });
            self.version += 1;
        }
    }

    pub fn dummies_seen(&self) -> u32 {
        (self.triggers.len() as u32).min(self.p.dummy_scans)
    }

    pub fn volumes_seen(&self) -> u32 {
        (self.triggers.len() as u32).saturating_sub(self.p.dummy_scans)
    }

    pub fn measured_tr(&self) -> Option<f64> {
        let n = self.triggers.len();
        let ts = &self.triggers[n.saturating_sub(8)..];
        if ts.len() < 2 {
            return None;
        }
        Some((ts[ts.len() - 1] - ts[0]) / (ts.len() - 1) as f64)
    }

    pub fn onset(&mut self, b: &Block, t: f64) {
        if self.index.contains_key(&b.id) || self.t0.is_none() {
            return;
        }
        let t0 = self.t0.unwrap();
        let mut ev = EventRow {
            id: b.id.clone(),
            onset: round4(t - t0),
            duration: b.duration,
            planned_onset: b.start,
            trial_type: if b.condition.is_empty() { b.kind.clone() } else { b.condition.clone() },
            block: b.kind.clone(),
            label: b.display_label(),
            stim_file: b.params.nonempty("path").map(|p| basename(&p)).unwrap_or_else(|| "n/a".into()),
            response: None,
            response_time: None,
            rating: Rating::NA,
            correct: None,
            outcome: None,
        };
        match b.kind.as_str() {
            "match" => {
                let top = basename(&b.params.s("top", ""));
                ev.stim_file = if top.is_empty() { "n/a".into() } else { top };
            }
            "gonogo" => {
                if b.condition.is_empty() {
                    ev.trial_type = b.params.s("trial", "go");
                }
            }
            "feedback" => {
                let ok = self
                    .p
                    .previous_scored(b)
                    .and_then(|prev| self.event(&prev.id).map(|pev| trial_success(prev, pev)))
                    .unwrap_or(false);
                ev.outcome = Some(if ok { "hit" } else { "miss" }.into());
                ev.label = b.params.s(if ok { "hit_text" } else { "miss_text" }, "");
                self.feedback.insert(b.id.clone(), ok);
            }
            _ => {}
        }
        self.log.push(LogRow { time: self.rel(t), kind: "onset".into(), value: b.kind.clone(), info: b.display_label() });
        if b.kind == "rating" {
            let start = if b.params.s("mode", "direct") == "cursor" { Some(b.rating_start()) } else { None };
            self.ratings.insert(b.id.clone(), RatingState { value: start, confirmed: false, rt: None });
        }
        self.index.insert(b.id.clone(), self.events.len());
        self.events.push(ev);
        self.version += 1;
    }

    /// What the renderer needs to draw `b` at paradigm time `t`.
    pub fn paint_state(&self, b: Option<&Block>, t: f64) -> PaintState {
        let Some(b) = b else { return PaintState::default() };
        let rs = self.ratings.get(&b.id).cloned().unwrap_or_default();
        let mut st = PaintState { t: t - b.start, value: rs.value, confirmed: rs.confirmed, ..Default::default() };
        if let Some(ev) = self.event(&b.id) {
            if b.kind == "match" {
                if let Some(r) = &ev.response {
                    if r == "left" || r == "right" {
                        st.chosen = Some(r.clone());
                    }
                }
            }
        }
        if b.kind == "feedback" {
            st.success = self.feedback.get(&b.id).copied().unwrap_or(false);
        }
        st
    }

    /// Apply a grip press to whatever is active. Returns ids of blocks that changed.
    pub fn on_button(&mut self, name: &str, t: f64) -> Vec<String> {
        self.log.push(LogRow { time: self.rel(t), kind: "button".into(), value: name.to_string(), info: String::new() });
        let Some(t0) = self.t0 else { return vec![] };
        let tt = t - t0;
        let hw = self.p.hardware.clone();
        let active: Vec<Block> = self.p.active_blocks(tt).into_iter().cloned().collect();
        let mut changed = vec![];
        for b in active {
            let Some(onset) = self.event(&b.id).map(|e| e.onset) else { continue };
            let rt = round4(tt - onset);
            match b.kind.as_str() {
                "match" => {
                    let ev = self.event_mut(&b.id).unwrap();
                    if ev.answered() {
                        continue;
                    }
                    let side = if name == b.params.s("left_button", "Left index") {
                        "left"
                    } else if name == b.params.s("right_button", "Right index") {
                        "right"
                    } else {
                        continue;
                    };
                    ev.response = Some(side.into());
                    ev.response_time = Some(rt);
                    ev.correct = Some((side == b.params.s("correct", "left")) as i32);
                    changed.push(b.id.clone());
                }
                "gonogo" => {
                    let want = b.params.s("button", "any");
                    let ev = self.event_mut(&b.id).unwrap();
                    if (want != "any" && want != name) || ev.answered() {
                        continue;
                    }
                    let go = b.params.s("trial", "go") == "go";
                    ev.response = Some(name.into());
                    ev.response_time = Some(rt);
                    ev.outcome = Some(if go { "hit" } else { "false_alarm" }.into());
                    ev.correct = Some(go as i32);
                    changed.push(b.id.clone());
                }
                "choice" => {
                    let want = b.params.s("buttons", "any");
                    let corr = b.params.s("correct", "");
                    let ev = self.event_mut(&b.id).unwrap();
                    if (want != "any" && want != name) || ev.answered() {
                        continue;
                    }
                    ev.response = Some(name.into());
                    ev.response_time = Some(rt);
                    let (outcome, correct) = if corr.is_empty() {
                        ("false_alarm", 0)
                    } else if corr == "any" {
                        ("hit", 1)
                    } else if name == corr {
                        ("correct", 1)
                    } else {
                        ("incorrect", 0)
                    };
                    ev.outcome = Some(outcome.into());
                    ev.correct = Some(correct);
                    changed.push(b.id.clone());
                }
                "rating" => {
                    let mut st = self.ratings.get(&b.id).cloned().unwrap_or_default();
                    if st.confirmed {
                        continue;
                    }
                    let (lo, hi) = (b.params.i("min", 1), b.params.i("max", 4));
                    if b.params.s("mode", "direct") == "direct" {
                        match b.rating_value_for(&hw, name) {
                            Some(v) => st = RatingState { value: Some(v), confirmed: true, rt: Some(tt - onset) },
                            None => continue,
                        }
                    } else {
                        let [left, right, confirm] = b.rating_cursor_buttons(&hw);
                        if name == left {
                            st.value = Some((st.value.unwrap_or(lo) - 1).max(lo));
                        } else if name == right {
                            st.value = Some((st.value.unwrap_or(lo) + 1).min(hi));
                        } else if name == confirm {
                            st.confirmed = true;
                            st.rt = Some(tt - onset);
                        } else {
                            continue;
                        }
                    }
                    if st.confirmed {
                        let ev = self.event_mut(&b.id).unwrap();
                        ev.rating = st.value.map(Rating::Value).unwrap_or(Rating::NA);
                        ev.response = Some(name.into());
                        ev.response_time = st.rt.map(round4);
                    }
                    self.ratings.insert(b.id.clone(), st);
                    changed.push(b.id.clone());
                }
                _ => {}
            }
        }
        if !changed.is_empty() {
            self.version += 1;
        }
        changed
    }

    /// Fill in the outcomes of trials that were never answered (called at the end of a run).
    pub fn finalize(&mut self) {
        for (id, st) in self.ratings.clone() {
            if let Some(ev) = self.event_mut(&id) {
                if !st.confirmed {
                    if let Some(v) = st.value {
                        ev.rating = Rating::Unconfirmed(v); // last cursor position counts as an unconfirmed answer
                    }
                }
            }
        }
        let blocks: HashMap<String, Block> = self.p.blocks.iter().map(|b| (b.id.clone(), b.clone())).collect();
        for ev in self.events.iter_mut() {
            let Some(b) = blocks.get(&ev.id) else { continue };
            if ev.answered() {
                continue;
            }
            match b.kind.as_str() {
                "gonogo" => {
                    let go = b.params.s("trial", "go") == "go";
                    ev.outcome = Some(if go { "miss" } else { "correct_rejection" }.into());
                    ev.correct = Some((!go) as i32);
                }
                "choice" => {
                    let expect = !b.params.s("correct", "").is_empty();
                    ev.outcome = Some(if expect { "miss" } else { "correct_rejection" }.into());
                    ev.correct = Some((!expect) as i32);
                }
                "match" => {
                    ev.outcome = Some("miss".into());
                    ev.correct = Some(0);
                }
                _ => {}
            }
        }
    }

    pub fn write(&mut self, out_dir: &Path, stem: &str) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        self.finalize();
        let ev_path = out_dir.join(format!("{stem}_events.tsv"));
        let cols = [
            "onset", "duration", "trial_type", "block", "label", "stim_file", "response", "response_time", "rating",
            "correct", "outcome", "planned_onset",
        ];
        let mut s = cols.join("\t") + "\n";
        let mut rows: Vec<&EventRow> = self.events.iter().collect();
        rows.sort_by(|a, b| a.onset.total_cmp(&b.onset));
        for r in rows {
            let vals = [
                py_float(r.onset),
                py_float(r.duration),
                r.trial_type.clone(),
                r.block.clone(),
                r.label.clone(),
                r.stim_file.clone(),
                r.response.clone().unwrap_or_else(|| "n/a".into()),
                na_f(r.response_time),
                r.rating.text(),
                r.correct.map(|c| c.to_string()).unwrap_or_else(|| "n/a".into()),
                r.outcome.clone().unwrap_or_else(|| "n/a".into()),
                py_float(r.planned_onset),
            ];
            s.push_str(&vals.iter().map(|v| tsv(v)).collect::<Vec<_>>().join("\t"));
            s.push('\n');
        }
        std::fs::write(&ev_path, s).map_err(|e| format!("{}: {e}", ev_path.display()))?;
        let log_path = out_dir.join(format!("{stem}_log.tsv"));
        let mut s = String::from("time\ttype\tvalue\tinfo\n");
        for r in &self.log {
            s.push_str(&format!("{}\t{}\t{}\t{}\n", na_f(r.time), r.kind, tsv(&r.value), tsv(&r.info)));
        }
        std::fs::write(&log_path, s).map_err(|e| format!("{}: {e}", log_path.display()))?;
        Ok(vec![ev_path, log_path])
    }
}

// =============================================================================
// Live evaluation – used by the control room
// =============================================================================
pub const SCORED_KINDS: &[&str] = &["match", "gonogo", "choice", "rating"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Ok,
    Bad,
    Warn,
    Pending,
    Info,
}

/// (text, tone) describing a trial's result so far. `t = None` means the run is over.
pub fn live_result(b: &Block, ev: &EventRow, t: Option<f64>) -> (String, Tone) {
    let answered = ev.answered();
    let over = t.map(|t| t >= b.end()).unwrap_or(true);
    let resp = ev.response.clone().unwrap_or_default();
    match b.kind.as_str() {
        "gonogo" => {
            let go = b.params.s("trial", "go") == "go";
            if answered {
                return if go { ("hit".into(), Tone::Ok) } else { ("false alarm".into(), Tone::Bad) };
            }
            if !over {
                return ("…".into(), Tone::Pending);
            }
            if go { ("miss".into(), Tone::Bad) } else { ("correct rejection".into(), Tone::Ok) }
        }
        "choice" => {
            let corr = b.params.s("correct", "");
            if answered {
                if corr.is_empty() {
                    return ("false alarm".into(), Tone::Bad);
                }
                if corr == "any" {
                    return ("hit".into(), Tone::Ok);
                }
                return if ev.correct == Some(1) { (format!("{resp} ✓"), Tone::Ok) } else { (format!("{resp} ✗"), Tone::Bad) };
            }
            if !over {
                return ("…".into(), Tone::Pending);
            }
            if corr.is_empty() { ("correct rejection".into(), Tone::Ok) } else { ("miss".into(), Tone::Bad) }
        }
        "match" => {
            if answered {
                let ok = ev.correct == Some(1);
                return (format!("{resp} {}", if ok { "✓" } else { "✗" }), if ok { Tone::Ok } else { Tone::Bad });
            }
            if !over { ("…".into(), Tone::Pending) } else { ("no answer".into(), Tone::Warn) }
        }
        "rating" => {
            if !ev.rating.is_na() {
                return (format!("rated {}", ev.rating.text()), Tone::Info);
            }
            if !over { ("…".into(), Tone::Pending) } else { ("no rating".into(), Tone::Warn) }
        }
        _ => {
            if answered { (resp, Tone::Info) } else { ("…".into(), Tone::Pending) }
        }
    }
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = xs.collect();
    if v.is_empty() { None } else { Some(v.iter().sum::<f64>() / v.len() as f64) }
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 { "0%".into() } else { format!("{:.0}%", 100.0 * a as f64 / b as f64) }
}

fn uniq<'a>(it: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out: Vec<&str> = vec![];
    for x in it {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// Performance so far as `(title, text, tone)` lines. `t = None` means the run is over.
pub fn live_stats(state: &RunState, t: Option<f64>, watchdog: u32) -> Vec<(String, String, Tone)> {
    let mut out: Vec<(String, String, Tone)> = vec![];
    let rows: Vec<(&Block, &EventRow)> =
        state.events.iter().filter_map(|ev| state.p.block(&ev.id).map(|b| (b, ev))).collect();
    let done = |b: &Block, ev: &EventRow| t.map(|t| t >= b.end()).unwrap_or(true) || ev.answered();

    let matching: Vec<_> = rows.iter().filter(|(b, ev)| b.kind == "match" && done(b, ev)).collect();
    if !matching.is_empty() {
        let ok = matching.iter().filter(|(_, ev)| ev.correct == Some(1)).count();
        let miss = matching.iter().filter(|(_, ev)| !ev.answered()).count();
        let rt = mean(matching.iter().filter_map(|(_, ev)| ev.response_time));
        let per: Vec<String> = uniq(matching.iter().map(|(b, _)| b.condition.as_str()))
            .into_iter()
            .map(|c| {
                let sub: Vec<_> = matching.iter().filter(|(b, _)| b.condition == c).collect();
                let good = sub.iter().filter(|(_, ev)| ev.correct == Some(1)).count();
                format!("{} {}/{}", if c.is_empty() { "trials" } else { c }, good, sub.len())
            })
            .collect();
        let acc = ok as f64 / matching.len() as f64;
        out.push((
            "Matching".into(),
            format!(
                "{ok}/{} correct ({})  ·  {miss} no answer{}\n{}",
                matching.len(),
                pct(ok, matching.len()),
                rt.map(|r| format!("  ·  RT {r:.2}s")).unwrap_or_default(),
                per.join("  ·  ")
            ),
            if acc >= 0.8 { Tone::Ok } else { Tone::Warn },
        ));
    }

    let gng: Vec<_> = rows.iter().filter(|(b, ev)| b.kind == "gonogo" && done(b, ev)).collect();
    if !gng.is_empty() {
        let res: Vec<String> = gng.iter().map(|(b, ev)| live_result(b, ev, t).0).collect();
        let n = |k: &str| res.iter().filter(|r| *r == k).count();
        let (hit, miss, fa, cr) = (n("hit"), n("miss"), n("false alarm"), n("correct rejection"));
        let go = hit + miss;
        let nogo = fa + cr;
        let rt = mean(gng.iter().filter(|(b, _)| b.params.s("trial", "go") == "go").filter_map(|(_, ev)| ev.response_time));
        let mut rates = vec![];
        if go > 0 {
            rates.push(format!("hit rate {}", pct(hit, go)));
        }
        if nogo > 0 {
            rates.push(format!("false alarm rate {}", pct(fa, nogo)));
        }
        if let Some(r) = rt {
            rates.push(format!("Go RT {r:.2}s"));
        }
        out.push((
            "Go / No-Go".into(),
            format!("{hit} hits · {miss} misses · {fa} false alarms · {cr} correct rejections\n{}", rates.join("  ·  ")),
            if go == 0 || hit as f64 / go as f64 >= 0.8 { Tone::Ok } else { Tone::Warn },
        ));
    }

    let ch: Vec<_> = rows.iter().filter(|(b, ev)| b.kind == "choice" && done(b, ev)).collect();
    if !ch.is_empty() {
        let ok = ch.iter().filter(|(b, ev)| trial_success(b, ev)).count();
        let acc = ok as f64 / ch.len() as f64;
        let per: Vec<String> = uniq(ch.iter().map(|(b, _)| b.condition.as_str()))
            .into_iter()
            .map(|c| {
                let sub: Vec<_> = ch.iter().filter(|(b, _)| b.condition == c).collect();
                let good: Vec<_> = sub.iter().filter(|(b, ev)| trial_success(b, ev)).collect();
                let rt = mean(good.iter().filter_map(|(_, ev)| ev.response_time));
                format!(
                    "{} {}/{}{}",
                    if c.is_empty() { "trials" } else { c },
                    good.len(),
                    sub.len(),
                    rt.map(|r| format!(" RT {r:.2}s")).unwrap_or_default()
                )
            })
            .collect();
        let fa = ch.iter().filter(|(_, ev)| ev.outcome.as_deref() == Some("false_alarm")).count();
        let miss = ch.iter().filter(|(b, ev)| !b.params.s("correct", "").is_empty() && !ev.answered()).count();
        out.push((
            "Choice trials".into(),
            format!("{ok}/{} correct ({})  ·  {miss} misses  ·  {fa} false alarms\n{}", ch.len(), pct(ok, ch.len()), per.join("  ·  ")),
            if acc >= 0.8 { Tone::Ok } else { Tone::Warn },
        ));
    }

    let rat: Vec<_> = rows.iter().filter(|(b, ev)| b.kind == "rating" && done(b, ev)).collect();
    if !rat.is_empty() {
        let given: Vec<_> = rat.iter().filter(|(_, ev)| !ev.rating.is_na()).collect();
        // rating blocks often share one condition ("rating"); group by the stimulus shown before instead
        let mut groups: Vec<(String, Vec<f64>)> = vec![];
        for (b, ev) in &given {
            let key = if b.condition.is_empty() || b.condition == "rating" {
                state
                    .p
                    .blocks_on(b.track)
                    .into_iter()
                    .filter(|o| {
                        o.end() <= b.start + 1e-6
                            && !o.condition.is_empty()
                            && ["image", "match", "text"].contains(&o.kind.as_str())
                            && o.condition != "cue"
                            && o.condition != "instruction"
                    })
                    .last()
                    .map(|o| o.condition.clone())
                    .unwrap_or_default()
            } else {
                b.condition.clone()
            };
            let key = if key.is_empty() { "all".to_string() } else { key };
            if let Some(v) = ev.rating.value() {
                match groups.iter_mut().find(|g| g.0 == key) {
                    Some(g) => g.1.push(v as f64),
                    None => groups.push((key, vec![v as f64])),
                }
            }
        }
        let mut text = format!("{}/{} rated", given.len(), rat.len());
        if !groups.is_empty() {
            let g: Vec<String> =
                groups.iter().map(|(k, v)| format!("{k} ⌀ {:.1}", v.iter().sum::<f64>() / v.len() as f64)).collect();
            text.push('\n');
            text.push_str(&g.join("  ·  "));
        }
        out.push(("Ratings".into(), text, if given.len() == rat.len() { Tone::Ok } else { Tone::Warn }));
    }

    if !state.presses.is_empty() {
        let mut per: Vec<(String, usize)> = vec![];
        for pr in &state.presses {
            let k = if pr.button.is_empty() { format!("key {}", pr.key) } else { pr.button.clone() };
            match per.iter_mut().find(|x| x.0 == k) {
                Some(x) => x.1 += 1,
                None => per.push((k, 1)),
            }
        }
        out.push((
            "Grip presses".into(),
            format!(
                "{} in total  ·  {}",
                state.presses.len(),
                per.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("  ·  ")
            ),
            Tone::Info,
        ));
    }

    // watchdog: participant stopped answering?
    if let (Some(t), true) = (t, watchdog > 0) {
        let mut recent: Vec<_> = rows
            .iter()
            .filter(|(b, _)| {
                SCORED_KINDS.contains(&b.kind.as_str())
                    && t >= b.end()
                    && !(b.kind == "gonogo" && b.params.s("trial", "go") == "nogo")
                    && !(b.kind == "choice" && b.params.s("correct", "").is_empty())
            })
            .collect();
        recent.sort_by(|a, b| a.0.start.total_cmp(&b.0.start));
        let w = watchdog as usize;
        let recent = &recent[recent.len().saturating_sub(w)..];
        if recent.len() == w && recent.iter().all(|(_, ev)| !ev.answered()) {
            out.insert(
                0,
                (
                    "Attention".into(),
                    format!("No response in the last {watchdog} trials – check the grips and the participant."),
                    Tone::Bad,
                ),
            );
        }
    }
    out
}

// =============================================================================
// Run controller – one per run, shared by participant screen and control room
// =============================================================================
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Waiting,
    Dummy,
    Running,
    Done,
}

pub struct RunController {
    pub state: RunState,
    pub settings: RunSettings,
    pub current: Option<String>,
    pub files: Vec<PathBuf>,
    pub aborted: bool,
    pub error: String,
    pub hw_status: String,
    done: bool,
    t_end: Option<f64>,
    abort_armed_at: Option<f64>,
    router: InputRouter,
    press_file: Option<File>,
    press_path: Option<PathBuf>,
    t_open: f64,
    sim_start_at: Option<f64>,
    visual_key: Option<(String, bool, i64)>,
    /// bumped whenever the participant screen must be repainted
    pub stimulus_version: u64,
    /// size of the participant screen (set by its window) – the control-room mirror uses its shape
    pub screen_size: Option<(f32, f32)>,
}

impl RunController {
    pub fn new(paradigm: Paradigm, settings: RunSettings, waker: Waker) -> Self {
        // a controlled SyncBox is told how many volumes to expect (+ a margin, so it never stops early)
        let volumes = paradigm.volumes() + paradigm.dummy_scans + 20;
        let router = InputRouter::new(paradigm.hardware.clone(), settings.simulate, paradigm.tr, volumes, waker);
        RunController {
            state: RunState::new(paradigm),
            settings,
            current: None,
            files: vec![],
            aborted: false,
            error: String::new(),
            hw_status: String::new(),
            done: false,
            t_end: None,
            abort_armed_at: None,
            router,
            press_file: None,
            press_path: None,
            t_open: now(),
            sim_start_at: None,
            visual_key: None,
            stimulus_version: 0,
            screen_size: None,
        }
    }

    pub fn p(&self) -> &Paradigm {
        &self.state.p
    }

    /// Open the crash-safe press log and start listening to the hardware.
    pub fn start(&mut self) {
        self.t_open = now();
        if !self.settings.out_dir.is_empty() {
            let dir = PathBuf::from(&self.settings.out_dir);
            let path = dir.join(format!("{}_buttons.tsv", output_stem(&self.state.p, &self.settings)));
            let r = std::fs::create_dir_all(&dir).and_then(|_| File::create(&path)).and_then(|mut f| {
                f.write_all((PRESS_COLUMNS.join("\t") + "\n").as_bytes())?;
                f.flush()?;
                Ok(f)
            });
            match r {
                Ok(f) => {
                    self.press_file = Some(f);
                    self.press_path = Some(path);
                }
                Err(e) => self.error = format!("Could not create the button-press log: {e}"),
            }
        }
        self.router.start();
        if self.settings.simulate {
            self.sim_start_at = Some(now() + 1.5);
        }
    }

    pub fn phase(&self) -> Phase {
        if self.done {
            Phase::Done
        } else if self.state.t0.is_some() {
            Phase::Running
        } else if !self.state.triggers.is_empty() {
            Phase::Dummy
        } else {
            Phase::Waiting
        }
    }

    /// Paradigm time. Stops at the end of the run – also when it was aborted.
    pub fn elapsed(&self) -> f64 {
        match (self.state.t0, self.t_end) {
            (None, _) => 0.0,
            (_, Some(e)) => e,
            (Some(t0), None) => now() - t0,
        }
    }

    /// Operator override: start the paradigm clock without a scanner trigger.
    pub fn start_now(&mut self) {
        if self.state.t0.is_none() && !self.done {
            self.state.force_start(now());
            self.tick();
        }
    }

    /// First call arms, a second call within 3 s aborts (so a stray key press can't end a run).
    pub fn request_abort(&mut self) {
        if self.done {
            return;
        }
        let armed = self.abort_armed();
        if !self.settings.confirm_abort || armed {
            self.finish(true);
        } else {
            self.abort_armed_at = Some(now());
        }
    }

    pub fn abort_armed(&self) -> bool {
        !self.done && self.abort_armed_at.map(|t| now() - t < 3.0).unwrap_or(false)
    }

    pub fn finish(&mut self, aborted: bool) {
        if self.done {
            return;
        }
        let dur = self.state.p.duration();
        if let Some(t0) = self.state.t0 {
            self.t_end = Some(if aborted { (now() - t0).min(dur) } else { dur });
        }
        self.done = true;
        self.aborted = aborted;
        self.router.stop();
        if self.state.t0.is_some() {
            if aborted {
                self.state.log.push(LogRow {
                    time: self.t_end.map(round4),
                    kind: "abort".into(),
                    value: String::new(),
                    info: "run aborted by operator".into(),
                });
            }
            let stem = output_stem(&self.state.p, &self.settings) + if aborted { "_ABORTED" } else { "" };
            let out = PathBuf::from(&self.settings.out_dir);
            match self.state.write(&out, &stem) {
                Ok(f) => {
                    self.files = f;
                    if self.settings.save_paradigm_with_data {
                        let path = out.join(format!("{stem}_paradigm.json"));
                        match self.state.p.save(&path) {
                            Ok(()) => self.files.push(path),
                            Err(e) => self.error = format!("Could not save the paradigm copy: {e}"),
                        }
                    }
                }
                Err(e) => self.error = format!("Could not save the logs: {e}"),
            }
        }
        // close the press log; never started and nothing pressed: no empty file left behind
        if let Some(f) = self.press_file.take() {
            drop(f);
            if let Some(p) = self.press_path.take() {
                if !self.state.presses.is_empty() || self.state.t0.is_some() {
                    self.files.push(p);
                } else {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        self.stimulus_version += 1;
        self.state.version += 1;
    }

    /// A key typed while the run windows have focus (USB keyboard SyncBox, grips). Ignored in
    /// serial mode – then only the SyncBox counts. (Esc is handled separately.)
    pub fn key(&mut self, text: &str, t: f64) {
        if !self.done && self.router.accepts_keyboard() {
            self.router.feed(text, t);
        }
    }

    fn apply(&mut self, input: Input) {
        match input {
            Input::Trigger(t) => {
                if !self.done && self.state.on_trigger(t) {
                    self.visual_key = None;
                }
            }
            Input::Button(name, t) => {
                if !self.done && !self.state.on_button(&name, t).is_empty() {
                    self.stimulus_version += 1;
                }
            }
            Input::Pressed(key, name, t) => {
                if self.done {
                    return;
                }
                let row = self.state.record_press(&key, &name, t, self.t_open);
                if let Some(f) = self.press_file.as_mut() {
                    let r = f.write_all((row.tsv() + "\n").as_bytes()).and_then(|_| f.flush());
                    if let Err(e) = r {
                        self.error = format!("Button-press log: {e}");
                    }
                }
            }
            Input::Other(..) => {}
            Input::Error(m) => {
                self.error = m;
                self.state.version += 1;
            }
            Input::Status(m) => {
                self.hw_status = m;
                self.state.version += 1;
            }
        }
    }

    /// Process input and advance the clock. Call once per frame (and whenever input arrives).
    pub fn tick(&mut self) {
        if let Some(at) = self.sim_start_at {
            if now() >= at {
                self.sim_start_at = None;
                self.router.start_simulation();
            }
        }
        for input in self.router.poll() {
            self.apply(input);
        }
        if self.done || self.state.t0.is_none() {
            return;
        }
        let t = self.elapsed();
        if t >= self.state.p.duration() {
            self.finish(false);
            return;
        }
        let (cur, key) = match self.state.p.stimulus_at(t) {
            Some(b) => {
                let tb = t - b.start;
                let key = (b.id.clone(), crate::stimulus::stim_visible(b, tb), crate::stimulus::flicker_phase(b, tb) as i64);
                (Some(b.id.clone()), Some(key))
            }
            None => (None, None),
        };
        self.current = cur;
        if key != self.visual_key {
            self.visual_key = key;
            self.stimulus_version += 1;
        }
        let active: Vec<Block> = self.state.p.active_blocks(t).into_iter().cloned().collect();
        let tnow = now();
        for b in active {
            if self.state.event(&b.id).is_none() {
                self.state.onset(&b, tnow);
                self.stimulus_version += 1;
            }
        }
    }

    pub fn current_block(&self) -> Option<&Block> {
        self.current.as_ref().and_then(|id| self.state.p.block(id))
    }

    /// Seconds since the last trigger (None before the first).
    pub fn since_last_trigger(&self) -> Option<f64> {
        self.state.triggers.last().map(|t| now() - t)
    }
}

impl Drop for RunController {
    fn drop(&mut self) {
        if !self.done {
            self.finish(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn paradigm() -> Paradigm {
        let mut p = Paradigm { name: "Emo rating!".into(), tr: 2.0, dummy_scans: 2, ..Default::default() };
        p.blocks.push(Block::create("image", 0.0, Some(4.0)).with(json!({"path": "x.png"})).lab("picture"));
        p.blocks.push(Block::create("rating", 4.0, Some(4.0)).with(json!({"path": "x.png", "mode": "direct"})).cond("pos"));
        p.blocks.push(Block::create("rating", 8.0, Some(4.0)).with(json!({"mode": "cursor"})));
        p
    }

    #[test]
    fn keyboard_counts_only_in_keyboard_mode() {
        for (mode, expected) in [("keyboard", 1), ("serial", 0)] {
            let mut p = paradigm();
            p.hardware.sync_mode = mode.into(); // serial without a port: nothing arrives from the SyncBox
            let dir = std::env::temp_dir().join(format!("ps_kb_{}", crate::model::new_id()));
            let settings = RunSettings { out_dir: dir.to_string_lossy().to_string(), ..Default::default() };
            let mut c = RunController::new(p, settings, crate::hardware::no_waker());
            c.start();
            c.key("s", now()); // the trigger key, typed on the keyboard
            c.tick();
            assert_eq!(c.state.triggers.len(), expected, "{mode}");
            c.finish(true);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn dummy_scans_and_t0() {
        let mut s = RunState::new(paradigm());
        assert!(!s.on_trigger(100.0));
        assert!(!s.on_trigger(102.0));
        assert!(s.on_trigger(104.0));
        assert_eq!(s.t0, Some(104.0));
        assert!(!s.on_trigger(106.0));
    }

    #[test]
    fn every_press_is_recorded_with_context() {
        let mut s = RunState::new(paradigm());
        let early = s.record_press("a", "Left thumb", 1.0, 0.5);
        assert!(early.time.is_none() && early.block == "before start" && early.time_since_open == 0.5);
        s.on_trigger(0.0);
        s.on_trigger(2.0);
        s.on_trigger(4.0); // t0
        let row = s.record_press("b", "Left index", 5.25, 0.5);
        assert_eq!(row.time, Some(1.25));
        assert_eq!(row.block, "picture");
        assert_eq!(row.block_kind, "image");
        assert_eq!(row.volume, 1);
        let odd = s.record_press("x", "", 9.0, 0.5);
        assert!(odd.tsv().contains("\tunassigned\t"));
        assert_eq!(s.presses.len(), 3);
    }

    #[test]
    fn direct_rating_maps_buttons_to_values() {
        let p = paradigm();
        let rating = p.blocks[1].clone();
        let mut s = RunState::new(p);
        for _ in 0..3 {
            s.on_trigger(0.0);
        }
        s.onset(&rating, 4.0);
        s.on_button("Right index", 5.0); // 3rd button -> value 3
        assert_eq!(s.event(&rating.id).unwrap().rating, Rating::Value(3));
        s.on_button("Left thumb", 5.5); // already confirmed, ignored
        assert_eq!(s.event(&rating.id).unwrap().rating, Rating::Value(3));
    }

    #[test]
    fn cursor_rating_moves_and_confirms() {
        let p = paradigm();
        let cur = p.blocks[2].clone();
        let mut s = RunState::new(p.clone());
        for _ in 0..3 {
            s.on_trigger(0.0);
        }
        s.onset(&cur, 8.0);
        assert_eq!(s.ratings[&cur.id].value, Some(3)); // starts in the middle
        s.on_button("Left index", 8.5);
        s.on_button("Left index", 8.7);
        s.on_button("Left index", 8.9); // clamps at 1
        s.on_button("Right index", 9.0);
        s.on_button("Right thumb", 9.5); // confirm
        let ev = s.event(&cur.id).unwrap();
        assert_eq!(ev.rating, Rating::Value(2));
        assert_eq!(ev.response_time, Some(1.5));
        let dir = std::env::temp_dir().join(format!("ps_run_{}", crate::model::new_id()));
        let settings = RunSettings { sub: "07".into(), run: 2, ..Default::default() };
        let files = s.write(&dir, &output_stem(&p, &settings)).unwrap();
        assert!(files[0].to_string_lossy().ends_with("sub-07_task-Emorating_run-02_events.tsv"));
        let text = std::fs::read_to_string(&files[0]).unwrap();
        let header: Vec<&str> = text.lines().next().unwrap().split('\t').collect();
        let row: Vec<&str> = text.lines().nth(1).unwrap().split('\t').collect();
        let col = header.iter().position(|h| *h == "rating").unwrap();
        assert_eq!(row[col], "2");
        assert_eq!(row[header.iter().position(|h| *h == "planned_onset").unwrap()], "8.0");
        let log = std::fs::read_to_string(&files[1]).unwrap();
        assert_eq!(log.lines().filter(|l| l.split('\t').nth(1) == Some("trigger")).count(), 3);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rating_buttons_can_be_chosen_per_block() {
        let mut p = Paradigm { dummy_scans: 0, ..Default::default() };
        // direct: value 1 = Right thumb, value 2 = Left thumb (value 3, 4 keep the hardware order)
        p.blocks.push(Block::create("rating", 0.0, Some(4.0)).with(json!({
            "mode": "direct", "value_buttons": ["Right thumb", "Left thumb"]})));
        // cursor: thumbs move, right index confirms
        p.blocks.push(Block::create("rating", 4.0, Some(4.0)).with(json!({
            "mode": "cursor", "cursor_left": "Left thumb", "cursor_right": "Right thumb", "cursor_confirm": "Right index"})));
        let b = p.blocks.clone();
        assert_eq!(b[0].rating_value_buttons(&p.hardware), vec!["Right thumb", "Left thumb", "Right index", "Right thumb"]);
        let mut s = RunState::new(p);
        s.on_trigger(0.0);
        s.onset(&b[0], 0.0);
        s.on_button("Left thumb", 1.0);
        assert_eq!(s.event(&b[0].id).unwrap().rating, Rating::Value(2));
        s.onset(&b[1], 4.0);
        s.on_button("Left index", 4.2); // the hardware default for "left" – not used by this block
        assert_eq!(s.ratings[&b[1].id].value, Some(3));
        s.on_button("Right thumb", 4.5);
        s.on_button("Right index", 5.0);
        assert_eq!(s.event(&b[1].id).unwrap().rating, Rating::Value(4));
    }

    #[test]
    fn scoring_and_live_stats() {
        let mut p = Paradigm { tr: 1.0, ..Default::default() };
        p.blocks.push(Block::create("gonogo", 0.0, Some(1.0)).with(json!({"trial": "go"})));
        p.blocks.push(Block::create("gonogo", 1.0, Some(1.0)).with(json!({"trial": "nogo"})));
        p.blocks.push(Block::create("choice", 2.0, Some(1.0)).with(json!({"text": "A", "correct": "Left index"})));
        p.blocks.push(Block::create("feedback", 3.0, Some(1.0)));
        p.blocks.push(Block::create("match", 4.0, Some(1.0)).with(json!({"correct": "right"})));
        let b = p.blocks.clone();
        let mut s = RunState::new(p);
        s.on_trigger(10.0);
        s.onset(&b[0], 10.0);
        s.on_button("Left index", 10.3);
        s.onset(&b[1], 11.0);
        s.onset(&b[2], 12.0);
        s.on_button("Right index", 12.4);
        s.onset(&b[3], 13.0);
        assert_eq!(s.feedback[&b[3].id], false); // wrong answer in the choice trial
        s.onset(&b[4], 14.0);
        s.on_button("Right index", 14.5);
        assert_eq!(s.event(&b[4].id).unwrap().correct, Some(1));
        let stats = live_stats(&s, None, 5);
        let titles: Vec<&str> = stats.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(titles, vec!["Matching", "Go / No-Go", "Choice trials"]);
        assert!(stats[1].1.starts_with("1 hits · 0 misses · 0 false alarms · 1 correct rejections"));
        s.finalize();
        assert_eq!(s.event(&b[1].id).unwrap().outcome.as_deref(), Some("correct_rejection"));
        assert_eq!(live_result(&b[2], s.event(&b[2].id).unwrap(), None).1, Tone::Bad);
    }

    #[test]
    fn controller_runs_with_simulated_scanner() {
        let mut p = Paradigm { name: "sim".into(), tr: 0.1, dummy_scans: 1, ..Default::default() };
        p.blocks.push(Block::create("fixation", 0.0, Some(0.3)));
        p.blocks.push(Block::create("choice", 0.3, Some(0.2)).with(json!({"text": "X", "correct": "any"})));
        let dir = std::env::temp_dir().join(format!("ps_ctl_{}", crate::model::new_id()));
        let settings = RunSettings { out_dir: dir.to_string_lossy().to_string(), simulate: true, ..Default::default() };
        let mut c = RunController::new(p, settings, crate::hardware::no_waker());
        c.start();
        c.sim_start_at = Some(now());
        let t_end = now() + 5.0;
        let mut pressed = false;
        while c.phase() != Phase::Done && now() < t_end {
            c.tick();
            if c.phase() == Phase::Running && c.elapsed() > 0.35 && !pressed {
                c.key("c", now());
                pressed = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(c.phase(), Phase::Done);
        assert!(!c.aborted);
        assert_eq!(c.files.len(), 4, "{:?} {}", c.files, c.error); // events, log, paradigm, buttons
        let ev = std::fs::read_to_string(&c.files[0]).unwrap();
        assert!(ev.contains("\thit\t"), "{ev}");
        let buttons = std::fs::read_to_string(c.files.last().unwrap()).unwrap();
        assert!(buttons.lines().count() == 2 && buttons.contains("Right index"));
        std::fs::remove_dir_all(dir).ok();
    }
}
