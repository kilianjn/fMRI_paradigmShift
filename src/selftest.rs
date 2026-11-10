//! Function test: a screen for the participant – mirrored in the main window for the operator –
//! that shows at a glance whether everything works before a session: each grip button lights up
//! when it is pressed, a lamp flashes with every scanner pulse (with the measured TR), corner marks
//! show whether the projector shows the whole screen, and a slowly drifting logo shows that the
//! display runs smoothly. Nothing is logged to disk.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use egui::epaint::CornerRadius;
use egui::{Align, Align2, Color32, Context, FontId, Layout, Pos2, Rect, RichText, Stroke, TextureHandle, Ui, Vec2, ViewportBuilder, ViewportId};

use crate::controlroom::screen_builder;
use crate::hardware::{now, Input, InputRouter};
use crate::model::Hardware;
use crate::theme::{self, hex, ACCENT, BG, BORDER, DANGER, OK, TEXT, TEXT_DIM};

pub const TEST_SCREEN_ID: &str = "function_test_screen";
/// Pulse lamp colour (the same light blue as the scanner pulses in the hardware dialog).
const PULSE: Color32 = Color32::from_rgb(0x7F, 0xD3, 0xFF);

#[derive(Clone, Debug)]
pub struct TestSettings {
    pub screen: usize,
    pub fullscreen: bool,
    /// generate scanner pulses at the TR (no scanner / SyncBox needed)
    pub simulate: bool,
    pub tr: f64,
    pub background: String,
    pub window_size: Option<(f32, f32)>,
}

impl Default for TestSettings {
    fn default() -> Self {
        TestSettings { screen: 0, fullscreen: true, simulate: false, tr: 2.0, background: "#000000".into(), window_size: None }
    }
}

pub struct TestState {
    pub hw: Hardware,
    pub settings: TestSettings,
    router: InputRouter,
    pub t0: f64,
    pub pulses: u32,
    pub last_pulse: Option<f64>,
    intervals: VecDeque<f64>,
    /// per grip button (same order as `hw.buttons`): last press, number of presses
    pub presses: Vec<(Option<f64>, u32)>,
    /// the last key that is not assigned to any button
    pub other: Option<(String, f64)>,
    pub log: VecDeque<(f64, String, Color32)>,
    pub error: String,
    pub status: String,
    pub screen_size: Option<(f32, f32)>,
    frames: VecDeque<f64>,
    pub stop_requested: bool,
}

impl TestState {
    pub fn new(hw: Hardware, settings: TestSettings, waker: crate::hardware::Waker) -> Self {
        let mut router = InputRouter::new(hw.clone(), settings.simulate, settings.tr, 9999, waker);
        router.start();
        router.start_simulation();
        let n = hw.buttons.len();
        TestState {
            hw,
            settings,
            router,
            t0: now(),
            pulses: 0,
            last_pulse: None,
            intervals: VecDeque::new(),
            presses: vec![(None, 0); n],
            other: None,
            log: VecDeque::new(),
            error: String::new(),
            status: String::new(),
            screen_size: None,
            frames: VecDeque::new(),
            stop_requested: false,
        }
    }

    fn push_log(&mut self, t: f64, text: String, color: Color32) {
        self.log.push_front((t, text, color));
        self.log.truncate(200);
    }

    /// A key from the keyboard (USB-keyboard SyncBox, grips) – from either window.
    pub fn key(&mut self, text: &str, t: f64) {
        self.router.feed(text, t);
    }

    /// Take in everything that arrived (keys, serial data, simulated pulses).
    pub fn poll(&mut self) {
        for input in self.router.poll() {
            match input {
                Input::Trigger(t) => {
                    self.pulses += 1;
                    if let Some(l) = self.last_pulse {
                        self.intervals.push_back(t - l);
                        while self.intervals.len() > 8 {
                            self.intervals.pop_front();
                        }
                    }
                    self.last_pulse = Some(t);
                    let n = self.pulses;
                    self.push_log(t, format!("Scanner pulse #{n}"), PULSE);
                }
                Input::Button(name, t) => {
                    if let Some(i) = self.hw.buttons.iter().position(|b| b.name == name) {
                        self.presses[i].0 = Some(t);
                        self.presses[i].1 += 1;
                        let key = self.hw.buttons[i].key.clone();
                        self.push_log(t, format!("{name} pressed  (key “{key}”)"), ACCENT);
                    }
                }
                Input::Other(key, t) => {
                    self.other = Some((key.clone(), t));
                    self.push_log(t, format!("Key “{key}” – not assigned to a grip button"), TEXT_DIM);
                }
                Input::Pressed(..) => {}
                Input::Error(m) => {
                    self.push_log(now(), m.clone(), DANGER);
                    self.error = m;
                }
                Input::Status(m) => self.status = m,
            }
        }
    }

    /// Mean of the last pulse intervals.
    pub fn measured_tr(&self) -> Option<f64> {
        (!self.intervals.is_empty()).then(|| self.intervals.iter().sum::<f64>() / self.intervals.len() as f64)
    }

    /// The participant screen drew a frame (for the frame-rate display).
    pub fn frame_drawn(&mut self) {
        let t = now();
        self.frames.push_back(t);
        while self.frames.front().map(|f| t - f > 1.0).unwrap_or(false) {
            self.frames.pop_front();
        }
    }

    /// Frames per second of the participant screen over the last second.
    pub fn fps(&self) -> Option<f64> {
        let (a, b) = (self.frames.front()?, self.frames.back()?);
        (self.frames.len() > 5 && b > a).then(|| (self.frames.len() - 1) as f64 / (b - a))
    }

    pub fn reset(&mut self) {
        self.pulses = 0;
        self.last_pulse = None;
        self.intervals.clear();
        self.presses = vec![(None, 0); self.hw.buttons.len()];
        self.other = None;
        self.log.clear();
        self.error.clear();
        self.t0 = now();
    }

    pub fn buttons_tested(&self) -> usize {
        self.presses.iter().filter(|p| p.1 > 0).count()
    }
}

/// How lit a lamp is `dt` seconds after its event (1 → 0 over `hold` seconds).
fn glow(dt: Option<f64>, hold: f64) -> f32 {
    match dt {
        Some(d) if (0.0..hold).contains(&d) => (1.0 - d / hold).powf(0.6) as f32,
        _ => 0.0,
    }
}

fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Text centred at `pos`, shrunk to `max_w` if needed.
fn label(p: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, size: f32, color: Color32, max_w: f32, bold: bool) {
    let mut size = size.max(4.0);
    let mk = |s: f32| if bold { theme::bold(s) } else { theme::font(s) };
    let mut g = p.layout_no_wrap(text.to_string(), mk(size), color);
    if g.size().x > max_w && max_w > 1.0 {
        size = (size * max_w / g.size().x).floor().max(4.0);
        g = p.layout_no_wrap(text.to_string(), mk(size), color);
    }
    let r = anchor.anchor_size(pos, g.size());
    p.galley(r.min, g, color);
}

/// The test screen – exactly the same on the participant's screen and in the operator's mirror.
pub fn paint_test(p: &egui::Painter, r: Rect, s: &TestState, logo: Option<&TextureHandle>) {
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    let (w, h) = (r.width(), r.height());
    let t = now();
    p.rect_filled(r, CornerRadius::ZERO, hex(&s.settings.background));

    // a drifting logo that bounces off the edges (and changes its tint on every bounce) – it shows
    // at once whether the screen is live and moving smoothly
    let size = h * 0.16;
    let el = (t - s.t0) as f32;
    let (vx, vy) = (h * 0.09, h * 0.065);
    let bounce = |v: f32, range: f32| -> (f32, i64) {
        if range <= 1.0 {
            return (0.0, 0);
        }
        let d = (v * el).rem_euclid(2.0 * range);
        let n = (v * el / range).floor() as i64;
        (if d > range { 2.0 * range - d } else { d }, n)
    };
    let (bx, nx) = bounce(vx, w - size);
    let (by, ny) = bounce(vy, h - size);
    let tints = [hex("#7FD3FF"), hex("#F5A524"), hex("#9B8CFF"), hex("#5BD69A"), hex("#FF7FA8")];
    let tint = tints[((nx + ny).rem_euclid(tints.len() as i64)) as usize];
    let lr = Rect::from_min_size(Pos2::new(r.left() + bx, r.top() + by), Vec2::splat(size));
    match logo {
        Some(tex) => {
            p.image(tex.id(), lr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), theme::alpha(mix(Color32::WHITE, tint, 0.5), 70));
        }
        None => {
            p.circle_stroke(lr.center(), size * 0.4, Stroke::new(size * 0.06, theme::alpha(tint, 70)));
        }
    }

    // corner marks: all four must be visible on the projection
    let (cl, cw) = (h * 0.05, (h * 0.006).max(1.5));
    let cc = Color32::from_gray(0x8A);
    for (corner, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        let c = corner + Vec2::new(dx * h * 0.02, dy * h * 0.02);
        p.line_segment([c, c + Vec2::new(dx * cl, 0.0)], Stroke::new(cw, cc));
        p.line_segment([c, c + Vec2::new(0.0, dy * cl)], Stroke::new(cw, cc));
    }

    let maxw = w * 0.9;
    label(&p, Pos2::new(r.center().x, r.top() + h * 0.09), Align2::CENTER_CENTER, "FUNCTION TEST", h * 0.032, TEXT_DIM, maxw, true);
    label(&p, Pos2::new(r.center().x, r.top() + h * 0.16), Align2::CENTER_CENTER, "Please press each button once", h * 0.045, Color32::WHITE, maxw, false);

    // grip buttons: a lamp each – lights up when pressed, green ring once it has worked
    let n = s.hw.buttons.len().max(1);
    let gap_mid = if n % 2 == 0 && n >= 2 { 1.0 } else { 0.0 }; // left hand | right hand
    let slots = n as f32 + gap_mid * 0.6;
    let slot = (w * 0.86 / slots).min(h * 0.3);
    let rad = (slot * 0.3).min(h * 0.075);
    let x0 = r.center().x - slot * slots / 2.0 + slot / 2.0;
    let y = r.top() + h * 0.44;
    for (i, b) in s.hw.buttons.iter().enumerate() {
        let extra = if gap_mid > 0.0 && i >= n / 2 { slot * 0.6 } else { 0.0 };
        let c = Pos2::new(x0 + i as f32 * slot + extra, y);
        let (last, count) = s.presses.get(i).copied().unwrap_or((None, 0));
        let k = glow(last.map(|l| t - l), 0.45);
        let idle = Color32::from_gray(0x1E);
        let fill = mix(idle, ACCENT, k);
        if k > 0.0 {
            p.circle_filled(c, rad * (1.0 + 0.35 * k), theme::alpha(ACCENT, (60.0 * k) as u8));
        }
        p.circle_filled(c, rad, fill);
        let ring = if count > 0 { OK } else { Color32::from_gray(0x70) };
        p.circle_stroke(c, rad, Stroke::new((h * 0.006).max(1.5), ring));
        label(&p, Pos2::new(c.x, c.y + rad + h * 0.025), Align2::CENTER_TOP, &b.name, h * 0.032, Color32::WHITE, slot * 0.95, false);
        let (txt, col) = if count > 0 { (format!("✓ {count}×"), OK) } else { ("–".to_string(), Color32::from_gray(0x80)) };
        label(&p, Pos2::new(c.x, c.y + rad + h * 0.07), Align2::CENTER_TOP, &txt, h * 0.03, col, slot * 0.95, true);
    }

    // scanner pulse lamp
    let pc = Pos2::new(r.center().x - w * 0.12, r.top() + h * 0.8);
    let pr = h * 0.045;
    let hold = (s.measured_tr().unwrap_or(s.settings.tr) * 0.35).clamp(0.08, 0.3);
    let k = glow(s.last_pulse.map(|l| t - l), hold);
    if k > 0.0 {
        p.circle_filled(pc, pr * (1.0 + 0.5 * k), theme::alpha(PULSE, (70.0 * k) as u8));
    }
    p.circle_filled(pc, pr, mix(Color32::from_gray(0x1E), PULSE, k));
    p.circle_stroke(pc, pr, Stroke::new((h * 0.005).max(1.0), if s.pulses > 0 { PULSE } else { Color32::from_gray(0x70) }));
    let tx = pc.x + pr + h * 0.035;
    let tw = (r.right() - tx - w * 0.04).max(10.0);
    let head = if s.settings.simulate { "Scanner pulses (simulated)" } else { "Scanner pulses" };
    label(&p, Pos2::new(tx, pc.y - h * 0.005), Align2::LEFT_BOTTOM, head, h * 0.034, Color32::WHITE, tw, false);
    let sub = match (s.pulses, s.measured_tr()) {
        (0, _) => "waiting for the first pulse …".to_string(),
        (n, Some(tr)) => format!("{n} received  ·  TR {tr:.2} s"),
        (n, None) => format!("{n} received"),
    };
    label(&p, Pos2::new(tx, pc.y + h * 0.005), Align2::LEFT_TOP, &sub, h * 0.03, Color32::from_gray(0xB0), tw, false);
}

pub struct TestSession {
    pub state: Arc<Mutex<TestState>>,
    builder: ViewportBuilder,
    closed: Arc<AtomicBool>,
    logo: Option<TextureHandle>,
    ticker_stop: Arc<AtomicBool>,
    ticker: Option<std::thread::JoinHandle<()>>,
}

/// About 60 times a second: take in the inputs and wake *both* windows. A window's own repaint
/// request is not enough – on some systems the window in the background is then only redrawn when
/// it is clicked (the same reason the run has its own clock thread).
fn ticker(ctx: Context, state: Arc<Mutex<TestState>>, stop: Arc<AtomicBool>) -> Option<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("function-test-ticker".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok(mut s) = state.lock() {
                    s.poll();
                }
                // the test screen is drawn in the main window's pass, so this updates both
                ctx.request_repaint_of(ViewportId::ROOT);
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        })
        .ok()
}

impl Drop for TestSession {
    fn drop(&mut self) {
        self.ticker_stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.ticker.take() {
            let _ = h.join();
        }
    }
}

/// Keys from a window: text goes to the test, Esc ends it.
fn route(ctx: &Context, s: &mut TestState) {
    let events = ctx.input(|i| i.events.clone());
    for ev in events {
        match ev {
            egui::Event::Text(t) => s.key(&t, now()),
            egui::Event::Key { key: egui::Key::Escape, pressed: true, repeat: false, .. } => s.stop_requested = true,
            _ => {}
        }
    }
}

impl TestSession {
    pub fn new(ctx: &Context, hw: Hardware, settings: TestSettings) -> Self {
        let builder = screen_builder("Function test – participant screen".into(), settings.fullscreen, settings.screen, settings.window_size);
        let c = ctx.clone();
        let wake = Arc::new(move || c.request_repaint_of(ViewportId::ROOT));
        let state = Arc::new(Mutex::new(TestState::new(hw, settings, wake)));
        let size = 128;
        let img = egui::ColorImage::from_rgba_unmultiplied([size, size], &crate::placeholders::app_icon_rgba(size as u32));
        let logo = Some(ctx.load_texture("function_test_logo", img, egui::TextureOptions::LINEAR));
        let ticker_stop = Arc::new(AtomicBool::new(false));
        let ticker = ticker(ctx.clone(), state.clone(), ticker_stop.clone());
        TestSession { state, builder, closed: Arc::new(AtomicBool::new(false)), logo, ticker_stop, ticker }
    }

    /// The participant's test screen (call every frame while the test runs). An *immediate*
    /// viewport: drawn in the same pass as the main window, so both always update together,
    /// whichever has the focus (separately scheduled "deferred" viewports froze in the background
    /// on some systems – egui issue #4945).
    pub fn show_screen(&self, ctx: &Context) {
        if self.closed.load(Ordering::Relaxed) {
            return;
        }
        let state = &self.state;
        let closed = &self.closed;
        let logo = self.logo.clone();
        ctx.show_viewport_immediate(ViewportId::from_hash_of(TEST_SCREEN_ID), self.builder.clone(), |ui, _class| {
            let ctx = ui.ctx().clone();
            let Ok(mut s) = state.lock() else { return };
            route(&ctx, &mut s);
            s.poll();
            if ctx.input(|i| i.viewport().close_requested()) {
                closed.store(true, Ordering::Relaxed);
                s.stop_requested = true;
            }
            let r = ui.max_rect();
            s.screen_size = Some((r.width(), r.height()));
            s.frame_drawn();
            paint_test(ui.painter(), r, &s, logo.as_ref());
            ctx.set_cursor_icon(egui::CursorIcon::None);
        });
    }

    /// The participant's test screen painted into `rect` (used for documentation screenshots).
    pub fn paint_full(&self, p: &egui::Painter, rect: Rect) {
        if let Ok(s) = self.state.lock() {
            paint_test(p, rect, &s, self.logo.as_ref());
        }
    }

    /// The operator's view in the main window. Returns true when the test should end.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut stop = false;
        let Ok(mut s) = self.state.lock() else { return true };
        route(&ctx, &mut s);
        s.poll();
        if s.stop_requested {
            stop = true;
        }
        let frame = egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(14, 10));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            // header
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(130.0, 24.0), egui::Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(12), ACCENT);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "● FUNCTION TEST", theme::bold(12.0), Color32::from_rgb(0x1B, 0x1B, 0x1B));
                let hw = &s.hw;
                let sync = if hw.sync_mode == "serial" {
                    format!("SyncBox on serial port {}", if hw.serial_port.is_empty() { "(not set)" } else { &hw.serial_port })
                } else {
                    format!("SyncBox as USB keyboard, trigger key “{}”", hw.trigger_key)
                };
                let sim = if s.settings.simulate { "  ·  simulated scanner pulses" } else { "" };
                ui.label(RichText::new(format!("{sync}{sim}")).color(TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::colored_button(ui, "■  End test", DANGER).on_hover_text("Close the test screen (Esc)").clicked() {
                        stop = true;
                    }
                    if ui.button("Reset").on_hover_text("Clear the counters and the list").clicked() {
                        s.reset();
                    }
                });
            });
            ui.add_space(8.0);
            let avail = ui.available_size();
            let side_w = (avail.x * 0.34).clamp(300.0, 460.0);
            ui.horizontal_top(|ui| {
                // mirror: exactly what the participant sees, in the shape of their screen
                let mw = (avail.x - side_w - 12.0).max(200.0);
                ui.allocate_ui_with_layout(Vec2::new(mw, avail.y), Layout::top_down(Align::Min), |ui| {
                    theme::panel_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height((ui.available_height()).max(0.0));
                        theme::panel_title(ui, "Participant screen  (live mirror)");
                        let r = ui.available_rect_before_wrap().shrink(4.0);
                        let aspect = s.screen_size.map(|(w, h)| w / h.max(1.0)).unwrap_or(16.0 / 9.0);
                        let w = r.width().min(r.height() * aspect);
                        let bx = Rect::from_center_size(r.center(), Vec2::new(w, w / aspect));
                        paint_test(ui.painter(), bx, &s, self.logo.as_ref());
                        ui.painter().rect_stroke(bx, CornerRadius::ZERO, Stroke::new(1.0, BORDER), egui::StrokeKind::Outside);
                    });
                });
                ui.allocate_ui_with_layout(Vec2::new(side_w, avail.y), Layout::top_down(Align::Min), |ui| {
                    theme::panel_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(ui.available_height().max(0.0));
                        theme::panel_title(ui, "Checklist");
                        let check = |ui: &mut Ui, ok: bool, text: String| {
                            ui.horizontal(|ui| {
                                let (c, t) = if ok { (OK, "✓") } else { (TEXT_DIM, "○") };
                                ui.label(RichText::new(t).color(c).font(theme::bold(14.0)));
                                ui.label(RichText::new(text).color(if ok { TEXT } else { TEXT_DIM }));
                            });
                        };
                        let nb = s.hw.buttons.len();
                        check(ui, nb > 0 && s.buttons_tested() == nb, format!("Grip buttons: {} of {nb} pressed", s.buttons_tested()));
                        for (i, b) in s.hw.buttons.iter().enumerate() {
                            let c = s.presses.get(i).map(|p| p.1).unwrap_or(0);
                            ui.label(RichText::new(format!("      {}  (key “{}”)  –  {}", b.name, b.key, if c > 0 { format!("{c}×") } else { "not yet".into() })).color(if c > 0 { TEXT } else { TEXT_DIM }).size(12.5));
                        }
                        let tr_txt = match s.measured_tr() {
                            Some(tr) => format!("  ·  measured TR {tr:.3} s (paradigm {} s)", crate::model::fmt_g(s.settings.tr)),
                            None => String::new(),
                        };
                        check(ui, s.pulses >= 2, format!("Scanner pulses: {}{tr_txt}", s.pulses));
                        let screen = match (s.screen_size, s.fps()) {
                            (Some((w, h)), Some(f)) => format!("Participant screen: {w:.0}×{h:.0}  ·  {f:.0} frames/s"),
                            (Some((w, h)), None) => format!("Participant screen: {w:.0}×{h:.0}"),
                            _ => "Participant screen: opening …".into(),
                        };
                        check(ui, s.fps().map(|f| f >= 20.0).unwrap_or(false), screen);
                        theme::hint(ui, "Ask the participant whether all four corner marks are visible and the logo moves smoothly.");
                        if !s.error.is_empty() {
                            ui.add(egui::Label::new(RichText::new(format!("⚠ {}", s.error)).color(DANGER)).wrap());
                        }
                        if let Some((k, _)) = &s.other {
                            theme::hint(ui, format!("Last unassigned key: “{k}” – set the keys in ⚙ Hardware."));
                        }
                        ui.add_space(6.0);
                        theme::panel_title(ui, "Inputs  (newest first)");
                        egui::Frame::new().fill(BG).stroke(Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(6).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(ui.available_height());
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                if s.log.is_empty() {
                                    ui.label(RichText::new("Press a grip button or wait for a scanner pulse.").color(TEXT_DIM));
                                }
                                let t0 = s.t0;
                                for (t, text, col) in s.log.iter() {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("{:>7.2} s", t - t0)).font(FontId::monospace(12.0)).color(TEXT_DIM));
                                        ui.label(RichText::new(text).color(*col));
                                    });
                                }
                            });
                        });
                    });
                });
            });
        });
        drop(s);
        stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::no_waker;

    #[test]
    fn ticker_wakes_both_windows_and_takes_in_inputs() {
        // no window ever repaints here – the ticker alone must keep both windows and the inputs going
        let ctx = Context::default();
        let session = TestSession::new(&ctx, Hardware::default(), TestSettings::default());
        session.state.lock().unwrap().key("a", now());
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert!(ctx.has_requested_repaint_for(&ViewportId::ROOT));
        assert_eq!(session.state.lock().unwrap().presses[0].1, 1);
        drop(session); // stops and joins the ticker
    }

    #[test]
    fn keys_light_buttons_and_count_pulses() {
        let hw = Hardware::default(); // USB keyboard: trigger "s", grips a–d
        let mut s = TestState::new(hw, TestSettings::default(), no_waker());
        for k in ["a", "b", "b", "x", "s"] {
            s.key(k, now());
        }
        s.poll();
        assert_eq!(s.presses[0].1, 1);
        assert_eq!(s.presses[1].1, 2);
        assert_eq!(s.buttons_tested(), 2);
        assert_eq!(s.pulses, 1);
        assert_eq!(s.other.as_ref().map(|o| o.0.as_str()), Some("x"));
        s.key("s", s.last_pulse.unwrap() + 2.0);
        s.poll();
        assert!((s.measured_tr().unwrap() - 2.0).abs() < 1e-9);
        s.reset();
        assert_eq!((s.pulses, s.buttons_tested(), s.log.len()), (0, 0, 0));
    }
}
