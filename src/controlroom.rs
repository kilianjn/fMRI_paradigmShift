//! Control room: what the operator sees while a run is going (shown in the main window), plus
//! the participant screen (a separate, usually full-screen window on the projector).
//!
//! Mirror of the participant screen, scanner/trigger status, progress, the current and next
//! block, a live feed of answers and a running performance summary.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use egui::epaint::CornerRadius;
use egui::{Align, Align2, Color32, Context, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportBuilder, ViewportId};

use crate::hardware::now;
use crate::model::{fmt_g, fmt_time, Paradigm};
use crate::runner::{live_result, live_stats, Phase, RunController, Tone, SCORED_KINDS};
use crate::stimulus;
use crate::theme::{self, hex, ACCENT, BG, BORDER, DANGER, OK, PANEL, PANEL_2, TEXT, TEXT_DIM};

pub type SharedRun = Arc<Mutex<RunController>>;

fn tone(t: Tone) -> Color32 {
    match t {
        Tone::Ok => OK,
        Tone::Bad => DANGER,
        Tone::Warn => ACCENT,
        Tone::Pending => TEXT_DIM,
        Tone::Info => TEXT,
    }
}

/// Exactly what the participant sees – used by their screen and the control-room mirror.
pub fn paint_participant(p: &egui::Painter, r: Rect, c: &RunController, blur: bool) {
    // the same reference height everywhere: the mirror is an exact, scaled-down copy
    let ref_h = stimulus::REF_HEIGHT;
    match c.phase() {
        Phase::Running => {
            let t = c.elapsed();
            let b = c.current_block();
            let st = c.state.paint_state(b, t);
            stimulus::paint_stimulus(p, r, c.p(), b, &st, ref_h, blur);
        }
        // aborted: neutral empty screen instead of the "thank you" text
        Phase::Done => {
            let text = if c.aborted { String::new() } else { c.p().end_text.clone() };
            stimulus::paint_waiting(p, r, c.p(), Some(&text), ref_h);
        }
        _ => stimulus::paint_waiting(p, r, c.p(), None, ref_h),
    }
}

/// Route keyboard input (USB SyncBox, grips, Esc) from any of the run's windows to the run.
pub fn route_keys(ctx: &Context, c: &mut RunController) {
    let events = ctx.input(|i| i.events.clone());
    for ev in events {
        match ev {
            egui::Event::Text(t) => c.key(&t, now()),
            egui::Event::Key { key: egui::Key::Escape, pressed: true, repeat: false, .. } => c.request_abort(),
            _ => {}
        }
    }
}

pub const PARTICIPANT_ID: &str = "participant_screen";

/// The participant's window: full screen on the chosen monitor, or a normal window (for tests).
pub fn screen_builder(title: String, fullscreen: bool, screen: usize, window_size: Option<(f32, f32)>) -> ViewportBuilder {
    let builder = ViewportBuilder::default().with_title(title).with_icon(Arc::new(theme::app_icon()));
    if fullscreen {
        builder.with_monitor(screen).with_fullscreen(true).with_decorations(false)
    } else {
        let (w, h) = window_size.unwrap_or((1280.0, 720.0));
        builder.with_inner_size([w, h])
    }
}

pub struct RunSession {
    pub ctl: SharedRun,
    pub cr: ControlRoom,
    builder: ViewportBuilder,
    participant_closed: Arc<AtomicBool>,
    clock_stop: Arc<AtomicBool>,
    clock: Option<std::thread::JoinHandle<()>>,
}

/// The run clock: ticks the run every millisecond on its own thread – independent of which window
/// has the focus or is being repainted – and wakes the windows whenever they have to show something
/// new (participant screen: on every stimulus change; control room: at its refresh rate).
fn clock_thread(ctx: Context, ctl: SharedRun, stop: Arc<AtomicBool>) -> Option<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("run-clock".into())
        .spawn(move || {
            let pid = ViewportId::from_hash_of(PARTICIPANT_ID);
            let mut last_stim = u64::MAX;
            let mut last_version = u64::MAX;
            let (mut last_cr, mut last_part) = (0.0, 0.0);
            while !stop.load(Ordering::Relaxed) {
                let (done, stim, version, refresh) = {
                    let Ok(mut c) = ctl.lock() else { return };
                    c.tick();
                    (c.phase() == Phase::Done, c.stimulus_version, c.state.version, c.settings.control_refresh_ms)
                };
                let t = now();
                // the participant screen is drawn in the control room's pass (immediate viewport),
                // so waking the main window updates both
                if stim != last_stim {
                    last_stim = stim;
                    last_part = t;
                    ctx.request_repaint_of(ViewportId::ROOT);
                    ctx.request_repaint_of(pid);
                    last_cr = t;
                }
                if version != last_version && t - last_cr >= 0.03 {
                    last_version = version;
                    last_cr = t;
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
                if t - last_cr >= refresh.max(30) as f64 / 1000.0 {
                    last_cr = t;
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
                if t - last_part >= 0.25 {
                    last_part = t;
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
                if done {
                    ctx.request_repaint_of(pid);
                    ctx.request_repaint_of(ViewportId::ROOT);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        })
        .ok()
}

impl RunSession {
    pub fn new(ctx: &Context, ctl: RunController, folded: Vec<String>) -> Self {
        let s = ctl.settings.clone();
        let builder = screen_builder(format!("{} – participant screen", ctl.p().name), s.fullscreen, s.screen, s.window_size);
        let ctl = Arc::new(Mutex::new(ctl));
        let clock_stop = Arc::new(AtomicBool::new(false));
        let clock = clock_thread(ctx.clone(), ctl.clone(), clock_stop.clone());
        RunSession {
            ctl,
            cr: ControlRoom::new(folded),
            builder,
            participant_closed: Arc::new(AtomicBool::new(false)),
            clock_stop,
            clock,
        }
    }

    /// Show the participant window (call every frame while the session exists).
    ///
    /// It is an *immediate* viewport: drawn in the same pass as the control room, so both windows
    /// always update together, whichever of them has the focus. (With a separately scheduled
    /// "deferred" viewport, egui ignores repaint requests for the window that is not active on some
    /// systems – the background window froze until it was clicked: egui issue #4945.)
    pub fn show_participant(&self, ctx: &Context) {
        if self.participant_closed.load(Ordering::Relaxed) {
            return;
        }
        let ctl = &self.ctl;
        let closed = &self.participant_closed;
        ctx.show_viewport_immediate(ViewportId::from_hash_of(PARTICIPANT_ID), self.builder.clone(), |ui, _class| {
            let ctx = ui.ctx().clone();
            stimulus::poll(&ctx);
            let Ok(mut c) = ctl.lock() else { return };
            route_keys(&ctx, &mut c);
            c.tick();
            if ctx.input(|i| i.viewport().close_requested()) {
                if c.phase() != Phase::Done {
                    c.finish(true);
                }
                closed.store(true, Ordering::Relaxed);
            }
            let r = ui.max_rect();
            c.screen_size = Some((r.width(), r.height()));
            paint_participant(ui.painter(), r, &c, false);
            ctx.set_cursor_icon(egui::CursorIcon::None);
        });
    }
}

impl Drop for RunSession {
    fn drop(&mut self) {
        self.clock_stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.clock.take() {
            let _ = h.join();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum CrAction {
    None,
    Close,
}

pub struct ControlRoom {
    pub folded: HashSet<String>,
    confirm_start: bool,
    confirm_close: bool,
    shown: (u64, usize, bool),
    perf: Vec<(String, String, Tone)>,
    rows: Vec<[String; 6]>,
    row_tones: Vec<Tone>,
}

const SECTIONS: [(&str, &str); 6] = [
    ("status", "Status  ·  time, volumes, triggers"),
    ("progress", "Run progress"),
    ("mirror", "Participant screen  (live mirror)"),
    ("nownext", "Now & next"),
    ("perf", "Performance so far"),
    ("answers", "Answers  (newest first)"),
];

fn sec_title(key: &str) -> &'static str {
    SECTIONS.iter().find(|s| s.0 == key).map(|s| s.1).unwrap_or("")
}

impl ControlRoom {
    pub fn new(folded: Vec<String>) -> Self {
        ControlRoom {
            folded: folded.into_iter().collect(),
            confirm_start: false,
            confirm_close: false,
            shown: (u64::MAX, usize::MAX, false),
            perf: vec![],
            rows: vec![],
            row_tones: vec![],
        }
    }

    fn refresh_feed(&mut self, c: &RunController, t: Option<f64>) {
        self.perf = live_stats(&c.state, t, c.settings.watchdog_trials);
        let par = c.p();
        let mut rows: Vec<(f64, [String; 6], Tone)> = vec![];
        for ev in &c.state.events {
            let Some(b) = par.block(&ev.id) else { continue };
            if !SCORED_KINDS.contains(&b.kind.as_str()) {
                continue;
            }
            let (res, tn) = live_result(b, ev, t);
            let ans = if b.kind == "rating" && !ev.rating.is_na() { ev.rating.text() } else { ev.response.clone().unwrap_or_default() };
            rows.push((
                ev.onset,
                [
                    fmt_time(ev.onset, 1),
                    format!("{}: {}", b.title(), b.display_label()),
                    b.condition.clone(),
                    ans,
                    ev.response_time.map(|x| format!("{x:.3}")).unwrap_or_default(),
                    res,
                ],
                tn,
            ));
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        rows.truncate(300);
        self.row_tones = rows.iter().map(|r| r.2).collect();
        self.rows = rows.into_iter().map(|r| r.1).collect();
    }

    /// Draw the control room into the main window. Returns `Close` when the operator is done.
    pub fn ui(&mut self, ui: &mut Ui, session: &SharedRun, app_focused: bool) -> CrAction {
        let ctx = ui.ctx().clone();
        let Ok(mut c) = session.lock() else { return CrAction::None };
        route_keys(&ctx, &mut c);
        c.tick();
        let ph = c.phase();
        let par = c.p().clone();
        let tr = par.tr;
        let t = if matches!(ph, Phase::Running | Phase::Done) { c.elapsed() } else { 0.0 };
        let dur = par.duration();
        let mut action = CrAction::None;

        // refresh the slow parts only when something changed
        let rows_done = c
            .state
            .events
            .iter()
            .filter(|ev| par.block(&ev.id).map(|b| SCORED_KINDS.contains(&b.kind.as_str()) && t >= b.end()).unwrap_or(false))
            .count();
        let key = (c.state.version, rows_done, ph == Phase::Done);
        if key != self.shown || ph == Phase::Done && self.perf.is_empty() && !c.state.events.is_empty() {
            self.shown = key;
            self.refresh_feed(&c, if ph == Phase::Done { None } else { Some(t) });
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(14, 12))).show(ui, |ui| {
            // ---------------------------------------------------------------- header
            ui.horizontal(|ui| {
                let (pill, pill_col) = match ph {
                    Phase::Waiting => ("WAITING FOR SCANNER".to_string(), ACCENT),
                    Phase::Dummy => (format!("DUMMY SCANS  {} / {}", c.state.dummies_seen(), par.dummy_scans), ACCENT),
                    Phase::Running => ("● RUNNING".to_string(), OK),
                    Phase::Done if c.aborted => (format!("■ ABORTED at {}", fmt_time(t, 1)), DANGER),
                    Phase::Done => ("✓ FINISHED".to_string(), hex("#7FB2FF")),
                };
                let g = ui.painter().layout_no_wrap(pill.clone(), theme::bold(12.5), hex("#111111"));
                let (r, _) = ui.allocate_exact_size(g.size() + Vec2::new(24.0, 8.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(11), pill_col);
                ui.painter().galley(r.center() - g.size() / 2.0, g, hex("#111111"));
                let s = &c.settings;
                let who = format!(
                    "sub-{}{} · run-{:02}",
                    if s.sub.is_empty() { "?" } else { &s.sub },
                    if s.ses.is_empty() { String::new() } else { format!(" · ses-{}", s.ses) },
                    s.run
                );
                ui.label(RichText::new(&par.name).font(theme::bold(15.5)));
                ui.label(RichText::new(who).color(TEXT_DIM).size(15.0));
                if s.simulate {
                    egui::Frame::new().stroke(Stroke::new(1.0, ACCENT)).fill(PANEL_2).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
                        ui.label(RichText::new("SIMULATED SCANNER").color(ACCENT).font(theme::bold(11.0)));
                    });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ph == Phase::Done {
                        if theme::primary_button(ui, "Close").clicked() {
                            action = CrAction::Close;
                        }
                        if !c.files.is_empty() && ui.button("Open data folder").clicked() {
                            let _ = open::that(&c.settings.out_dir);
                        }
                    } else {
                        let armed = c.abort_armed();
                        let txt = if armed { "■  Click again to abort" } else { "■  Abort run" };
                        if ui.add(egui::Button::new(RichText::new(txt).color(DANGER).font(theme::bold(13.5)))).clicked() {
                            c.request_abort();
                        }
                        if armed {
                            ctx.request_repaint_after(std::time::Duration::from_millis(200));
                        }
                    }
                    if matches!(ph, Phase::Waiting | Phase::Dummy)
                        && ui
                            .button("Start now without trigger")
                            .on_hover_text("Start the paradigm clock immediately (logged as a manual start). Only for tests or if the trigger cable fails.")
                            .clicked()
                    {
                        self.confirm_start = true;
                    }
                    ui.menu_button("☰ Sections", |ui| {
                        for (k, title) in SECTIONS {
                            let mut open = !self.folded.contains(k);
                            if ui.checkbox(&mut open, title).changed() {
                                if open {
                                    self.folded.remove(k);
                                } else {
                                    self.folded.insert(k.to_string());
                                }
                            }
                        }
                        ui.separator();
                        if ui.button("Unfold all").clicked() {
                            self.folded.clear();
                        }
                        if ui.button("Fold all").clicked() {
                            self.folded = SECTIONS.iter().map(|s| s.0.to_string()).collect();
                        }
                    });
                });
            });
            ui.add_space(6.0);

            // ---------------------------------------------------------------- banner
            if let Some((msg, col)) = self.banner(&c, ph, t, app_focused) {
                let fr = egui::Frame::new()
                    .fill(PANEL_2)
                    .stroke(Stroke::new(1.0, col))
                    .corner_radius(6)
                    .inner_margin(egui::Margin { left: 16, right: 12, top: 8, bottom: 8 })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(msg).size(13.5)).wrap());
                    });
                let r = fr.response.rect;
                ui.painter().rect_filled(
                    Rect::from_min_size(r.min, Vec2::new(4.0, r.height())),
                    CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 },
                    col,
                );
                ui.add_space(6.0);
            }

            // ---------------------------------------------------------------- status cards
            let shown_t = t.min(dur);
            let total_vol = par.volumes();
            let cur_vol = if tr > 0.0 && ph == Phase::Running { ((shown_t / tr).floor() as u32 + 1).min(total_vol) } else { c.state.volumes_seen() };
            let pill_summary = format!(
                "{}  ·  {} / {}  ·  volume {cur_vol} / {total_vol}  ·  {} triggers",
                match ph {
                    Phase::Waiting => "waiting",
                    Phase::Dummy => "dummy scans",
                    Phase::Running => "running",
                    Phase::Done => "done",
                },
                fmt_time(shown_t, 1),
                fmt_time(dur, 1),
                c.state.triggers.len()
            );
            self.section(ui, "status", &pill_summary, |ui| {
                ui.columns(5, |cols| {
                    let aborted = ph == Phase::Done && c.aborted;
                    if aborted {
                        card(&mut cols[0], "Elapsed", &fmt_time(shown_t, 1), &format!("stopped – of {}", fmt_time(dur, 1)), Some(DANGER));
                        card(
                            &mut cols[1],
                            "Remaining",
                            "—",
                            &if dur > 0.0 { format!("aborted after {:.0}%", 100.0 * shown_t / dur) } else { "aborted".into() },
                            Some(DANGER),
                        );
                    } else {
                        card(&mut cols[0], "Elapsed", &fmt_time(shown_t, 1), &format!("of {}", fmt_time(dur, 1)), None);
                        card(
                            &mut cols[1],
                            "Remaining",
                            &fmt_time((dur - shown_t).max(0.0), 1),
                            &if dur > 0.0 { format!("{:.0}% done", 100.0 * shown_t / dur) } else { String::new() },
                            None,
                        );
                    }
                    card(&mut cols[2], "Volume", &format!("{cur_vol} / {total_vol}"), &format!("{} counted by trigger", c.state.volumes_seen()), None);
                    if matches!(ph, Phase::Waiting | Phase::Dummy) {
                        card(
                            &mut cols[3],
                            "Triggers",
                            &format!("{} / {}", c.state.triggers.len(), par.dummy_scans + 1),
                            &format!("start after {} dummy scans", par.dummy_scans),
                            Some(ACCENT),
                        );
                    } else {
                        card(&mut cols[3], "Triggers", &c.state.triggers.len().to_string(), &format!("incl. {} dummy", c.state.dummies_seen()), None);
                    }
                    match c.state.measured_tr() {
                        None => card(&mut cols[4], "Measured TR", "–", &format!("expected {} s", fmt_g(tr)), None),
                        Some(m) => {
                            let ok = (m - tr).abs() < 0.05;
                            card(
                                &mut cols[4],
                                "Measured TR",
                                &format!("{m:.3} s"),
                                &format!("expected {} s  {}", fmt_g(tr), if ok { "✓" } else { "⚠ differs" }),
                                if ok { None } else { Some(DANGER) },
                            );
                        }
                    }
                });
            });
            ui.add_space(8.0);
            let prog = if dur > 0.0 { format!("{:.0}% done", 100.0 * shown_t / dur) } else { String::new() };
            self.section(ui, "progress", &prog, |ui| progress_strip(ui, &par, ph, t, c.aborted));
            ui.add_space(8.0);

            // ---------------------------------------------------------------- two columns
            let (now_txt, next_txt) = now_next(&c, &par, ph, t);
            let mirror_sum = match (ph, c.current_block()) {
                (Phase::Running, Some(b)) => format!("showing: {} – {}", b.title(), b.display_label()),
                (Phase::Waiting | Phase::Dummy, _) => "waiting screen".into(),
                (Phase::Running, None) => "background".into(),
                _ => "end screen".into(),
            };
            let perf_sum = {
                let s: Vec<String> = self.perf.iter().take(2).map(|(t, x, _)| format!("{t}: {}", x.lines().next().unwrap_or(""))).collect();
                if s.is_empty() { "nothing scored yet".to_string() } else { s.join("  ·  ") }
            };
            let ans_sum = format!(
                "{} answers{}",
                self.rows.len(),
                self.rows.first().map(|r| format!("  ·  latest: {}", r[5])).unwrap_or_default()
            );
            let left_open = !self.folded.contains("mirror");
            let right_open = !self.folded.contains("answers");
            let avail = ui.available_size();
            let left_w = if left_open == right_open { avail.x * 0.56 } else if left_open { avail.x - 440.0 } else { 440.0 };
            let blur = c.settings.blur_control_mirror;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(left_w - 6.0, avail.y), Layout::top_down(Align::Min), |ui| {
                    let nn_h = if self.folded.contains("nownext") { 34.0 } else { 86.0 };
                    let mirror_h = (avail.y - nn_h - 14.0).max(120.0);
                    self.section_sized(ui, "mirror", &mirror_sum, Some(mirror_h), |ui| {
                        let r = ui.available_rect_before_wrap();
                        ui.allocate_rect(r, Sense::hover());
                        ui.painter().rect_filled(r, CornerRadius::ZERO, PANEL);
                        // same shape as the participant screen (16:9 until its window has opened)
                        let aspect = c.screen_size.map(|(sw, sh)| sw / sh.max(1.0)).unwrap_or(16.0 / 9.0);
                        let w = r.width().min(r.height() * aspect);
                        let bx = Rect::from_center_size(r.center(), Vec2::new(w, w / aspect));
                        paint_participant(ui.painter(), bx, &c, blur);
                        ui.painter().rect_stroke(bx, CornerRadius::ZERO, Stroke::new(1.0, BORDER), StrokeKind::Outside);
                    });
                    ui.add_space(8.0);
                    let plain_now = now_txt.replace("**", "");
                    self.section(ui, "nownext", &plain_now, |ui| {
                        ui.label(RichText::new(&plain_now).size(14.0));
                        if !next_txt.is_empty() {
                            ui.label(RichText::new(&next_txt).color(TEXT_DIM));
                        }
                    });
                });
                ui.add_space(6.0);
                ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), avail.y), Layout::top_down(Align::Min), |ui| {
                    let perf = self.perf.clone();
                    self.section(ui, "perf", &perf_sum, |ui| {
                        if perf.is_empty() {
                            theme::hint(ui, "Nothing to score yet – answers appear here as soon as trials end.");
                        }
                        for (title, text, tn) in &perf {
                            ui.label(RichText::new(format!("● {title}")).color(tone(*tn)).font(theme::bold(13.0)));
                            ui.add(egui::Label::new(RichText::new(text).size(13.0)).wrap());
                        }
                    });
                    ui.add_space(8.0);
                    let h = ui.available_height().max(60.0);
                    let rows = self.rows.clone();
                    let tones = self.row_tones.clone();
                    self.section_sized(ui, "answers", &ans_sum, Some(h - 4.0), |ui| answers_table(ui, &rows, &tones));
                });
            });
        });

        // ---------------------------------------------------------------- confirmations
        if self.confirm_start {
            let mut d = crate::dialogs::Confirm {
                title: "Start without trigger?".into(),
                text: "Start the paradigm now without waiting for the scanner?\nThe onsets will then not be locked to the scan.".into(),
                yes: "Start now".into(),
            };
            match d.ui(&ctx) {
                crate::dialogs::Outcome::Accept => {
                    c.start_now();
                    self.confirm_start = false;
                }
                crate::dialogs::Outcome::Cancel => self.confirm_start = false,
                _ => {}
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && c.phase() != Phase::Done {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            let mut d = crate::dialogs::Confirm {
                title: "Abort the run?".into(),
                text: "The run is still going. Abort it and save the data recorded so far?".into(),
                yes: "Abort run".into(),
            };
            match d.ui(&ctx) {
                crate::dialogs::Outcome::Accept => {
                    c.finish(true);
                    self.confirm_close = false;
                }
                crate::dialogs::Outcome::Cancel => self.confirm_close = false,
                _ => {}
            }
        }
        if c.phase() != Phase::Done {
            ctx.request_repaint_after(std::time::Duration::from_millis(c.settings.control_refresh_ms.max(30) as u64));
        }
        action
    }

    fn banner(&self, c: &RunController, ph: Phase, t: f64, app_focused: bool) -> Option<(String, Color32)> {
        let par = c.p();
        let tr = par.tr;
        if ph == Phase::Done {
            let mut msg = if c.aborted && c.state.t0.is_none() {
                "Run aborted before the scanner started.".to_string()
            } else if c.aborted {
                let vol = if tr > 0.0 { (t / tr).floor() as i64 + 1 } else { 0 };
                format!(
                    "Run aborted at {} (volume {vol} of {}). The clock has stopped and the data recorded until then was saved \
                     (file names end in _ABORTED).",
                    fmt_time(t, 1),
                    par.volumes()
                )
            } else {
                "Run complete.".to_string()
            };
            if !c.files.is_empty() {
                let names: Vec<String> = c.files.iter().map(|f| f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).collect();
                msg.push_str(&format!("\nSaved: {}", names.join(", ")));
            } else if c.error.is_empty() {
                msg.push_str("\nNo data was recorded (the scanner never started).");
            }
            if !c.error.is_empty() {
                msg.push_str(&format!("\n⚠ {}", c.error));
            }
            return Some((msg, if c.aborted || !c.error.is_empty() { DANGER } else { OK }));
        }
        if c.abort_armed() {
            return Some(("Press Esc again or click “Click again to abort” to stop the run.".into(), DANGER));
        }
        if !c.error.is_empty() {
            return Some((format!("⚠ Hardware: {}", c.error), DANGER));
        }
        if par.hardware.sync_mode == "keyboard" && !app_focused {
            return Some((
                "⚠ ParadigmShift is not the active application – SyncBox triggers and grip presses arrive as key presses and \
                 will be lost. Click this window or the participant screen."
                    .into(),
                DANGER,
            ));
        }
        if let Some(last) = c.since_last_trigger() {
            if ph == Phase::Running && tr > 0.0 && t < par.duration() && last > c.settings.trigger_warning_trs * tr {
                return Some((format!("⚠ No scanner trigger for {last:.1} s – has the scanner stopped?"), ACCENT));
            }
        }
        match ph {
            Phase::Waiting => {
                let hw = &par.hardware;
                let mut src = if hw.sync_mode == "keyboard" {
                    format!("key “{}” (USB keyboard mode)", hw.trigger_key)
                } else {
                    format!("serial port {}, trigger “{}”", hw.serial_port, hw.trigger_key)
                };
                if !c.hw_status.is_empty() {
                    src.push_str(&format!("  ·  {}", c.hw_status));
                }
                Some((
                    format!(
                        "Waiting for the first trigger from the SyncBox via {src}. {} dummy scan(s) will be skipped, the paradigm \
                         starts with trigger {}.",
                        par.dummy_scans,
                        par.dummy_scans + 1
                    ),
                    ACCENT,
                ))
            }
            Phase::Dummy => {
                let left = (par.dummy_scans + 1) as i64 - c.state.triggers.len() as i64;
                Some((
                    format!(
                        "Scanner is running – dummy scan {} of {} received. The paradigm starts with trigger {} (in about {:.0} s). \
                         The participant still sees the waiting screen.",
                        c.state.dummies_seen(),
                        par.dummy_scans,
                        par.dummy_scans + 1,
                        left as f64 * tr
                    ),
                    ACCENT,
                ))
            }
            _ => None,
        }
    }

    fn section(&mut self, ui: &mut Ui, key: &str, summary: &str, body: impl FnOnce(&mut Ui)) {
        self.section_sized(ui, key, summary, None, body)
    }

    /// A control-room panel that folds to its title bar (with a one-line summary).
    fn section_sized(&mut self, ui: &mut Ui, key: &str, summary: &str, height: Option<f32>, body: impl FnOnce(&mut Ui)) {
        let folded = self.folded.contains(key);
        theme::panel_frame().inner_margin(egui::Margin { left: 12, right: 12, top: 6, bottom: 10 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let (Some(h), false) = (height, folded) {
                ui.set_height((h - 18.0).max(40.0));
            }
            ui.horizontal(|ui| {
                let arrow = if folded { "▸  " } else { "▾  " };
                let r = ui
                    .add(egui::Button::new(RichText::new(format!("{arrow}{}", sec_title(key).to_uppercase())).font(theme::bold(11.0)).color(TEXT_DIM)).frame(false))
                    .on_hover_text("Click to fold / unfold");
                if r.clicked() {
                    if folded {
                        self.folded.remove(key);
                    } else {
                        self.folded.insert(key.to_string());
                    }
                }
                if folded {
                    ui.add(egui::Label::new(RichText::new(summary).color(TEXT_DIM).size(12.0)).truncate());
                }
            });
            if !folded {
                body(ui);
            }
        });
    }
}

fn card(ui: &mut Ui, title: &str, value: &str, sub: &str, color: Option<Color32>) {
    theme::panel_frame().fill(PANEL).show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::panel_title(ui, title);
        ui.label(RichText::new(value).font(theme::mono(22.0)).color(color.unwrap_or(TEXT)));
        ui.add(egui::Label::new(RichText::new(sub).color(TEXT_DIM).size(12.0)).truncate());
    });
}

fn now_next(c: &RunController, par: &Paradigm, ph: Phase, t: f64) -> (String, String) {
    match ph {
        Phase::Running => {
            let now = match c.current_block() {
                None => "NOW  background (gap)".to_string(),
                Some(b) => format!(
                    "NOW  ■ {}: {}{}  —  {:.1} s left",
                    b.title(),
                    b.display_label(),
                    if b.condition.is_empty() { String::new() } else { format!(" · {}", b.condition) },
                    (b.end() - t).max(0.0)
                ),
            };
            let next = match par.blocks_on(0).into_iter().find(|x| x.start > t + 1e-6) {
                Some(n) => format!(
                    "NEXT  {}: {}{}  in {:.1} s",
                    n.title(),
                    n.display_label(),
                    if n.condition.is_empty() { String::new() } else { format!(" · {}", n.condition) },
                    n.start - t
                ),
                None => "NEXT  end of run".into(),
            };
            (now, next)
        }
        Phase::Done if c.aborted => ("Participant sees: an empty screen (run aborted)".into(), String::new()),
        Phase::Done => (
            if par.end_text.is_empty() {
                "Participant sees: empty background".into()
            } else {
                format!("Participant sees: end screen “{}”", par.end_text)
            },
            String::new(),
        ),
        _ => {
            let what = match par.waiting_screen.as_str() {
                "fixation" => "a fixation cross".to_string(),
                "blank" => "an empty screen".to_string(),
                "text" => format!("“{}”", par.waiting_text),
                _ => String::new(),
            };
            let first = par.blocks_on(0).first().map(|b| format!("FIRST  {}: {}", b.title(), b.display_label())).unwrap_or_default();
            (format!("Participant sees: {what}  (no scanner information is shown to them)"), first)
        }
    }
}

/// The whole run as one strip: blocks in their colours, what's done dimmed, a playhead.
fn progress_strip(ui: &mut Ui, par: &Paradigm, ph: Phase, t: f64, aborted: bool) {
    let w = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(Vec2::new(w, 54.0), Sense::hover());
    let r = Rect::from_min_max(outer.min + Vec2::new(1.0, 4.0), outer.max - Vec2::new(1.0, 16.0));
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::ZERO, hex("#1B1E24"));
    let dur = par.duration().max(1e-6);
    let sx = r.width() as f64 / dur;
    // block edges on whole pixels, and a 1-pt gap at the end of every block that is wide enough,
    // so that neighbouring blocks of the same type (e.g. Hariri trials) stay distinguishable
    let ppp = ui.ctx().pixels_per_point();
    let snap = |x: f32| (x * ppp).round() / ppp;
    for b in par.blocks_on(0) {
        let x0 = snap(r.left() + (b.start * sx) as f32);
        let x1 = snap(r.left() + (b.end() * sx) as f32);
        let gap = if x1 - x0 >= 4.0 { 1.0 } else { 0.0 };
        let br = Rect::from_min_max(Pos2::new(x0, r.top()), Pos2::new((x1 - gap).max(x0 + 1.0 / ppp), r.bottom()));
        p.rect_filled(br, CornerRadius::ZERO, hex(b.color()));
    }
    let tt = if matches!(ph, Phase::Running | Phase::Done) { t.min(dur) } else { 0.0 };
    let x = r.left() + (tt * sx) as f32;
    p.rect_filled(Rect::from_min_max(r.min, Pos2::new(x, r.bottom())), CornerRadius::ZERO, Color32::from_black_alpha(140));
    let ab = ph == Phase::Done && aborted;
    if ab {
        // the part that never ran: greyed out and hatched
        let rest = Rect::from_min_max(Pos2::new(x, r.top()), r.max);
        p.rect_filled(rest, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(20, 22, 27, 200));
        stimulus::hatch(p, rest, Color32::from_rgba_unmultiplied(240, 80, 110, 90));
    }
    p.line_segment(
        [Pos2::new(x, r.top() - 3.0), Pos2::new(x, r.bottom() + 3.0)],
        Stroke::new(if ab { 3.0 } else { 2.0 }, if ab { DANGER } else { ACCENT }),
    );
    if ab {
        p.text(Pos2::new(x + 4.0, r.bottom() + 2.0), Align2::LEFT_TOP, format!("ABORTED at {}", fmt_time(tt, 1)), theme::bold(10.5), DANGER);
    }
    p.text(Pos2::new(r.left(), r.bottom() + 2.0), Align2::LEFT_TOP, "00:00", theme::font(10.5), TEXT_DIM);
    p.text(Pos2::new(r.right(), r.bottom() + 2.0), Align2::RIGHT_TOP, fmt_time(par.duration(), 0), theme::font(10.5), TEXT_DIM);
}

fn answers_table(ui: &mut Ui, rows: &[[String; 6]], tones: &[Tone]) {
    let widths = [64.0, 0.0, 90.0, 80.0, 56.0, 120.0];
    let avail = ui.available_width();
    let fixed: f32 = widths.iter().sum();
    let gap = ui.spacing().item_spacing.x;
    let trial_w = (avail - fixed - 6.0 * gap - 24.0).max(120.0);
    let header = ["Time", "Trial", "Condition", "Answer", "RT", "Result"];
    let cell = |ui: &mut Ui, i: usize, text: RichText| {
        let w = if i == 1 { trial_w } else { widths[i] };
        ui.allocate_ui_with_layout(Vec2::new(w, 20.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_width(w);
            ui.add(egui::Label::new(text).truncate());
        });
    };
    egui::Frame::new().fill(BG).stroke(Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(ui.available_height());
        ui.horizontal(|ui| {
            for (i, h) in header.iter().enumerate() {
                cell(ui, i, RichText::new(*h).color(TEXT_DIM));
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("answers").auto_shrink([false, false]).show_rows(ui, 22.0, rows.len(), |ui, range| {
            for k in range {
                let r = &rows[k];
                ui.horizontal(|ui| {
                    for (i, v) in r.iter().enumerate() {
                        let mut t = RichText::new(v.as_str()).size(13.0);
                        if i == 5 {
                            t = t.color(tone(tones[k]));
                        }
                        cell(ui, i, t);
                    }
                });
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Paradigm};
    use crate::runner::RunSettings;
    use serde_json::json;

    /// The run must advance, score and save its data even when no window is ever repainted
    /// (e.g. an unfocused or covered window).
    #[test]
    fn clock_runs_the_paradigm_without_any_repaint() {
        let mut p = Paradigm { name: "clock".into(), tr: 0.1, dummy_scans: 1, ..Default::default() };
        p.blocks.push(Block::create("fixation", 0.0, Some(0.2)));
        p.blocks.push(Block::create("choice", 0.2, Some(0.3)).with(json!({"text": "X", "correct": "any"})));
        let dir = std::env::temp_dir().join(format!("ps_clock_{}", crate::model::new_id()));
        let settings = RunSettings { out_dir: dir.to_string_lossy().to_string(), simulate: true, ..Default::default() };
        let ctx = Context::default();
        let mut ctl = RunController::new(p, settings, crate::hardware::no_waker());
        ctl.start();
        let session = RunSession::new(&ctx, ctl, vec![]);
        let end = now() + 6.0;
        let mut pressed = false;
        loop {
            let (phase, t) = {
                let c = session.ctl.lock().unwrap();
                (c.phase(), c.elapsed())
            };
            if phase == Phase::Done || now() > end {
                break;
            }
            if phase == Phase::Running && t > 0.3 && !pressed {
                session.ctl.lock().unwrap().key("c", now());
                pressed = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let c = session.ctl.lock().unwrap();
        assert_eq!(c.phase(), Phase::Done);
        assert!(!c.aborted);
        let ev = std::fs::read_to_string(&c.files[0]).unwrap();
        assert!(ev.contains("\thit\t"), "{ev}");
        drop(c);
        drop(session);
        std::fs::remove_dir_all(dir).ok();
    }
}
