//! Dialogs: hardware setup (SyncBox + grips), quick design, run setup, templates, find & replace,
//! preferences. Each is shown as a modal window over the editor.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Align, Context, Id, Key, Layout, RichText, Ui, Vec2};

use crate::document::Document;
use crate::hardware::{
    describe_ports, nnl_button_key, now, Input, InputRouter, PortInfo, PortScanner, ScanMsg, Waker, BAUD_RATES, NNL_TRIGGER,
};
use crate::model::{
    compact_values, find_text, fmt_g, fmt_time, generate_block_design, generate_event_design, parse_jitter_list, replace_text,
    round4, round_to, text_field_title, DesignCondition, Jitter,
    GripButton, Hardware, Paradigm, TEXT_FIELDS,
};
use crate::prefs::Prefs;
use crate::runner::RunSettings;
use crate::templates::{self, OptVal, Opts, CATEGORIES, TEMPLATES};
use crate::theme::{self, hex, ACCENT, BORDER, DANGER, OK, PANEL_2, TEXT, TEXT_DIM};

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Outcome {
    Open,
    Cancel,
    Accept,
}

const LABEL_W: f32 = 170.0;

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(LABEL_W, 24.0), Layout::right_to_left(Align::Center), |ui| {
            ui.label(label);
        });
        add(ui);
    });
}

fn title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::bold(22.0)));
}

fn section(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    theme::panel_title(ui, text);
}

/// The standard modal frame. Returns the outcome chosen in `body` (Escape = cancel).
pub fn modal(ctx: &Context, id: &str, width: f32, body: impl FnOnce(&mut Ui) -> Outcome) -> Outcome {
    let mut out = Outcome::Open;
    egui::Modal::new(Id::unique(id))
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(18))
        .show(ctx, |ui| {
            ui.set_width(width);
            out = body(ui);
            if ui.input(|i| i.key_pressed(Key::Escape)) && out == Outcome::Open {
                out = Outcome::Cancel;
            }
        });
    out
}

/// OK / Cancel buttons on the right.
fn buttons(ui: &mut Ui, ok: &str, ok_enabled: bool) -> Outcome {
    let mut out = Outcome::Open;
    ui.add_space(8.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.add_enabled(ok_enabled, egui::Button::new(RichText::new(ok).font(theme::bold(13.5)).color(hex("#1B1B1B"))).fill(ACCENT)).clicked() {
            out = Outcome::Accept;
        }
        if ui.button("Cancel").clicked() {
            out = Outcome::Cancel;
        }
    });
    out
}

pub fn pick_folder(title: &str, start: &str) -> Option<String> {
    let mut d = rfd::FileDialog::new().set_title(title);
    if !start.is_empty() && std::path::Path::new(start).is_dir() {
        d = d.set_directory(start);
    }
    d.pick_folder().map(|p| p.to_string_lossy().to_string())
}

// =============================================================================
// Simple dialogs
// =============================================================================
pub struct Message {
    pub title: String,
    pub text: String,
}

impl Message {
    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "message", 520.0, |ui| {
            title(ui, &self.title);
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                ui.add(egui::Label::new(self.text.as_str()).wrap());
            });
            let mut out = Outcome::Open;
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary_button(ui, "OK").clicked() {
                    out = Outcome::Accept;
                }
            });
            out
        })
    }
}

/// Help → About: who made the app, the version and where it lives on GitHub.
pub struct AboutDialog {
    icon: Option<egui::TextureHandle>,
}

pub const REPO_URL: &str = "https://github.com/kilianjn/fMRI_paradigmShift";

impl AboutDialog {
    pub fn new(ctx: &Context) -> Self {
        let size = 128;
        let img = egui::ColorImage::from_rgba_unmultiplied([size, size], &crate::placeholders::app_icon_rgba(size as u32));
        AboutDialog { icon: Some(ctx.load_texture("about_icon", img, egui::TextureOptions::LINEAR)) }
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "about", 470.0, |ui| {
            ui.vertical_centered(|ui| {
                if let Some(t) = &self.icon {
                    ui.add(egui::Image::new((t.id(), Vec2::splat(88.0))));
                }
                ui.add_space(4.0);
                ui.label(RichText::new("ParadigmShift").font(theme::bold(26.0)));
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(TEXT_DIM));
                ui.add_space(8.0);
                ui.label("A film-editor style app for designing and running fMRI paradigms.");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let w = ui.available_width();
                    // centre the sentence
                    let text = "Made by Kilian Jain together with Claude, an AI by Anthropic.";
                    let g = ui.painter().layout_no_wrap(text.into(), theme::font(14.0), TEXT);
                    ui.add_space(((w - g.size().x) / 2.0).max(0.0));
                    ui.label(RichText::new("Made by ").size(14.0));
                    ui.label(RichText::new("Kilian Jain").size(14.0).strong());
                    ui.label(RichText::new(" together with ").size(14.0));
                    ui.label(RichText::new("Claude").size(14.0).strong());
                    ui.label(RichText::new(", an AI by Anthropic.").size(14.0));
                });
                ui.add_space(12.0);
                ui.hyperlink_to(RichText::new("github.com/kilianjn/fMRI_paradigmShift").size(14.0), REPO_URL)
                    .on_hover_text("Source code, documentation and issues");
                ui.hyperlink_to("Downloads and release notes", format!("{REPO_URL}/releases"));
                ui.add_space(12.0);
                theme::hint(
                    ui,
                    "Built with egui. Fonts: Inter (SIL Open Font License), DejaVu Sans. Video decoding: OpenH264 \
                     (BSD). Sound: symphonia, rodio.",
                );
                ui.add_space(8.0);
                theme::hint(ui, crate::hardware::NNL_DISCLAIMER);
            });
            let mut out = Outcome::Open;
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary_button(ui, "Close").clicked() {
                    out = Outcome::Accept;
                }
            });
            out
        })
    }
}

/// A yes/no question; `Accept` = yes.
pub struct Confirm {
    pub title: String,
    pub text: String,
    pub yes: String,
}

impl Confirm {
    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "confirm", 460.0, |ui| {
            title(ui, &self.title);
            ui.add_space(6.0);
            ui.add(egui::Label::new(self.text.as_str()).wrap());
            buttons(ui, &self.yes, true)
        })
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum SaveChoice {
    Open,
    Save,
    Discard,
    Cancel,
}

pub fn unsaved_ui(ctx: &Context) -> SaveChoice {
    let mut c = SaveChoice::Open;
    modal(ctx, "unsaved", 420.0, |ui| {
        title(ui, "Unsaved changes");
        ui.label("Save changes to the current paradigm first?");
        ui.add_space(8.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::primary_button(ui, "Save").clicked() {
                c = SaveChoice::Save;
            }
            if ui.button("Don't save").clicked() {
                c = SaveChoice::Discard;
            }
            if ui.button("Cancel").clicked() {
                c = SaveChoice::Cancel;
            }
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            c = SaveChoice::Cancel;
        }
        Outcome::Open
    });
    c
}

pub struct RepeatAsk {
    pub n: u32,
}

impl RepeatAsk {
    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "repeat", 360.0, |ui| {
            title(ui, "Repeat");
            row(ui, "Extra copies of the selection", |ui| {
                ui.add(egui::DragValue::new(&mut self.n).range(1..=500));
            });
            let o = buttons(ui, "Repeat", true);
            if ui.input(|i| i.key_pressed(Key::Enter)) { Outcome::Accept } else { o }
        })
    }
}

pub fn shortcuts_text() -> &'static str {
    "Space\tplay / pause preview\n\
     Home\tgo to start\n\
     Shift+click\tselect all blocks from the last clicked one\n\
     Ctrl+click\tadd / remove one block\n\
     Del\tdelete selected   (Shift+Del closes the gap)\n\
     Ctrl+D\tduplicate\n\
     Ctrl+R\trepeat ×N\n\
     S\tsplit at playhead\n\
     ← →\tnudge (Shift: by one TR)\n\
     Alt + drag\tplace without snapping\n\
     Ctrl+wheel\tzoom\n\
     Ctrl+0\tfit whole run\n\
     Ctrl+Z / Ctrl+Shift+Z\tundo / redo\n\
     Ctrl+T\ttemplates\n\
     Ctrl+H\tfind and replace\n\
     Ctrl+1 / 2 / 3\tshow / minimise the Blocks / Task blocks / Media panel\n\
     F5\trun\n\
     Ctrl+Shift+O\topen a paradigm for presenting (read-only, straight to the run)\n\
     Ctrl+Shift+F\tfunction test (buttons, scanner pulses, display)"
}

// =============================================================================
// Hardware
// =============================================================================
#[derive(Clone, Copy, PartialEq)]
enum Capture {
    Trigger,
    Button(usize),
}

pub struct HardwareDialog {
    pub hw: Hardware,
    tr: f64,
    tab: usize,
    router: Option<InputRouter>,
    waker: Waker,
    triggers: Vec<f64>,
    trig_flash: f64,
    flashes: Vec<f64>,
    last: String,
    ser_note: String,
    detect_info: String,
    ports: Vec<PortInfo>,
    port_text: String,
    capture: Option<Capture>,
    sim_on: bool,
    scanner: Option<PortScanner>,
    scan_keys: Vec<String>,
    advanced: bool,
    cursor_fold: bool,
    /// sound outputs of this computer (listed when the Audio tab is opened)
    audio_devices: Option<Vec<crate::media::OutputDevice>>,
    audio_note: String,
    /// the output a test sound is playing on, until when
    audio_test: Option<(crate::media::AudioEngine, f64)>,
}

impl HardwareDialog {
    pub fn new(ctx: &Context, hw: &Hardware, tr: f64) -> Self {
        let c = ctx.clone();
        let mut d = HardwareDialog {
            hw: hw.clone(),
            tr,
            tab: 0,
            router: None,
            waker: Arc::new(move || c.request_repaint()),
            triggers: vec![],
            trig_flash: -10.0,
            flashes: vec![-10.0; hw.buttons.len()],
            last: "Press a grip button…".into(),
            ser_note: String::new(),
            detect_info: String::new(),
            ports: vec![],
            port_text: hw.serial_port.clone(),
            capture: None,
            sim_on: false,
            scanner: None,
            scan_keys: vec![],
            advanced: false,
            cursor_fold: false,
            audio_devices: None,
            audio_note: String::new(),
            audio_test: None,
        };
        d.refresh_ports(true);
        d.restart_router(false);
        d
    }

    fn refresh_ports(&mut self, initial: bool) {
        self.ports = describe_ports();
        if self.ports.is_empty() {
            self.ser_note = "No serial ports found. Is the SyncBox connected by USB and switched on? \
                             (In USB keyboard mode it does not show up as a serial port.) \
                             If it never shows up: the FTDI driver is probably missing – see below."
                .into();
        } else if !initial {
            let likely = self.ports.iter().any(|p| p.likely);
            self.ser_note = format!("{} serial port(s) found{}", self.ports.len(), if likely { " – ★ marks a likely SyncBox." } else { "." });
        }
    }

    fn restart_router(&mut self, simulate: bool) {
        if let Some(mut r) = self.router.take() {
            r.stop();
        }
        let mut r = InputRouter::new(self.hw.clone(), simulate, self.tr, 9999, self.waker.clone());
        r.start();
        if simulate {
            r.start_simulation();
        }
        self.router = Some(r);
    }

    fn hardware_sim(&self) -> bool {
        self.hw.sync_mode == "serial" && self.hw.syncbox_control && !self.hw.serial_port.is_empty()
    }

    fn on_trigger(&mut self, t: f64) {
        self.triggers.push(t);
        self.trig_flash = now();
    }

    fn poll(&mut self) {
        let mut learned = false;
        if let Some(r) = self.router.as_mut() {
            for ev in r.poll() {
                // assigning a key: in serial mode the character comes from the SyncBox
                if let (Some(cap), Input::Pressed(key, _, _)) = (self.capture, &ev) {
                    if self.hw.sync_mode == "serial" && !key.trim().is_empty() {
                        match cap {
                            Capture::Trigger => self.hw.trigger_key = key.clone(),
                            Capture::Button(i) => {
                                if let Some(b) = self.hw.buttons.get_mut(i) {
                                    b.key = key.clone();
                                }
                            }
                        }
                        self.capture = None;
                        learned = true;
                        continue;
                    }
                }
                match ev {
                    Input::Trigger(t) => {
                        self.triggers.push(t);
                        self.trig_flash = now();
                    }
                    Input::Button(name, _) => {
                        for (i, b) in self.hw.buttons.iter().enumerate() {
                            if b.name == name {
                                if let Some(f) = self.flashes.get_mut(i) {
                                    *f = now();
                                }
                            }
                        }
                        self.last = format!("Last button: {name}");
                    }
                    Input::Other(k, _) => self.last = format!("Key “{k}” is not assigned to a button"),
                    Input::Error(m) => self.ser_note = format!("⚠ {m}"),
                    Input::Status(m) => self.ser_note = format!("✓ {m}"),
                    Input::Pressed(..) => {}
                }
            }
        }
        if learned {
            self.restart_router(self.sim_on && self.hardware_sim());
        }
        if let Some(sc) = &self.scanner {
            let mut done = None;
            while let Ok(m) = sc.rx.try_recv() {
                match m {
                    ScanMsg::Progress(p) => {
                        self.detect_info = format!("{p}\nNow: run the scanner / SyncBox simulation, or press grip buttons.")
                    }
                    ScanMsg::Done(r, e) => done = Some((r, e)),
                }
            }
            if let Some((r, e)) = done {
                self.scanner = None;
                self.scan_done(r, e);
            }
        }
    }

    fn auto_detect(&mut self) {
        if let Some(mut r) = self.router.take() {
            r.stop(); // free the port we may hold ourselves
        }
        self.scan_keys.clear();
        self.detect_info = "Looking for serial ports …".into();
        self.scanner = Some(PortScanner::start(
            None,
            (2.5 * self.tr).max(4.0),
            self.hw.trigger_key.clone(),
            BAUD_RATES.to_vec(),
            self.waker.clone(),
        ));
    }

    fn apply_nnl_keys(&mut self) {
        for (i, b) in self.hw.buttons.iter_mut().enumerate() {
            let k = nnl_button_key(&b.name).map(String::from).or_else(|| (i < 4).then(|| ["a", "b", "c", "d"][i].to_string()));
            if let Some(k) = k {
                b.key = k;
            }
        }
    }

    fn scan_done(&mut self, results: Vec<crate::hardware::ScanResult>, errors: Vec<String>) {
        if let Some(r) = results
            .iter()
            .max_by_key(|x| (x.analysis.trigger.is_some(), x.analysis.chars.values().sum::<usize>()))
            .cloned()
        {
            self.hw.sync_mode = "serial".into();
            self.hw.serial_port = r.port.clone();
            self.port_text = r.port.clone();
            self.hw.baud_rate = r.baud;
            self.hw.syncbox_control = r.nnl; // a box that answered the handshake is controlled; else just listen
            let mut msg = format!(
                "✓ SyncBox found on {}{} at {} baud.",
                r.port,
                if r.description.is_empty() { String::new() } else { format!(" ({})", r.description) },
                r.baud
            );
            if r.nnl {
                self.apply_nnl_keys();
                self.hw.trigger_key = NNL_TRIGGER.into();
                msg.push_str("\nIt will be configured and started for each run. Trigger “s”, grips a – d.");
            } else if let Some(t) = r.analysis.trigger {
                self.hw.trigger_key = t.to_string();
                msg.push_str(&format!("\nTrigger “{t}”{}.", r.analysis.tr.map(|x| format!(" every {x:.3} s")).unwrap_or_default()));
                if let Some(x) = r.analysis.tr {
                    if (x - self.tr).abs() > 0.05 {
                        msg.push_str(&format!("  ⚠ The paradigm TR is {} s.", fmt_g(self.tr)));
                    }
                }
            } else {
                msg.push_str(
                    "\nNo regular trigger yet – start the scanner or the SyncBox simulation and run auto-detect again, \
                     or click the trigger field and send one pulse.",
                );
            }
            if !r.analysis.buttons.is_empty() {
                let known: Vec<String> = r
                    .analysis
                    .buttons
                    .iter()
                    .map(|c| match self.hw.button_for_key(&c.to_string()) {
                        Some(n) => format!("“{c}” = {n}"),
                        None => format!("“{c}”"),
                    })
                    .collect();
                msg.push_str(&format!("\nAlso received: {}  (grip buttons – assign them on the ResponseGrips tab if needed).", known.join(", ")));
            }
            self.detect_info = msg;
            self.ser_note.clear();
            self.refresh_ports(true);
            self.restart_router(false);
            return;
        }
        let keys: Vec<String> = self.scan_keys.iter().filter(|k| !k.trim().is_empty()).cloned().collect();
        if !keys.is_empty() {
            self.hw.sync_mode = "keyboard".into();
            let mut u: Vec<String> = vec![];
            for k in keys {
                if !u.contains(&k) {
                    u.push(k);
                }
            }
            self.detect_info = format!(
                "No serial data, but key presses arrived via USB: {}.\nYour SyncBox is in USB keyboard mode – switched to \
                 keyboard mode. Check the trigger key above.",
                u.iter().map(|k| format!("“{k}”")).collect::<Vec<_>>().join(", ")
            );
            self.restart_router(false);
            return;
        }
        let mut m = "✗ No SyncBox data received. Check: USB cable and power · SyncBox set to serial (not keyboard) output · \
                     the scanner or SyncBox simulation is running, or press grip buttons during the scan · no other program \
                     (e.g. NNL software) has the port open."
            .to_string();
        let mut seen = HashSet::new();
        for e in errors {
            if seen.insert(e.clone()) {
                m.push('\n');
                m.push_str(&e);
            }
        }
        self.detect_info = m;
        self.restart_router(false);
    }

    fn keys(&mut self, ctx: &Context) {
        if ctx.text_edit_focused() {
            return;
        }
        let texts: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None })
                .collect()
        });
        for text in texts {
            if !text.chars().all(|c| !c.is_control()) {
                continue;
            }
            if self.scanner.is_some() {
                self.scan_keys.push(text); // tells us the SyncBox is in keyboard mode
                continue;
            }
            match self.capture.take() {
                Some(Capture::Trigger) => {
                    self.hw.trigger_key = text;
                    self.restart_router(self.sim_on && self.hardware_sim());
                }
                Some(Capture::Button(i)) => {
                    if let Some(b) = self.hw.buttons.get_mut(i) {
                        b.key = text;
                    }
                    self.restart_router(self.sim_on && self.hardware_sim());
                }
                None => {
                    // live test: in serial mode only what the SyncBox sends counts, not the keyboard
                    if self.hw.sync_mode == "keyboard" {
                        if let Some(r) = self.router.as_mut() {
                            r.feed(&text, now());
                        }
                    }
                }
            }
        }
    }

    /// Sound output for audio blocks and the sound of videos – off unless chosen here.
    fn audio_tab(&mut self, ui: &mut Ui) {
        use crate::media::{output_devices, DEFAULT_OUTPUT};
        if self.audio_devices.is_none() {
            self.audio_devices = Some(output_devices());
        }
        let devices = self.audio_devices.clone().unwrap_or_default();
        let label_of = |id: &str, name: &str| -> String {
            match id {
                "" => "Off – no sound".to_string(),
                DEFAULT_OUTPUT => "System default output".to_string(),
                _ => match devices.iter().find(|d| d.id == id) {
                    Some(d) => d.name.clone(),
                    None => format!("{}  (not connected)", if name.is_empty() { id } else { name }),
                },
            }
        };
        let current = label_of(&self.hw.audio_device, &self.hw.audio_device_name);
        let mut chosen: Option<(String, String)> = None;
        row(ui, "Sound output", |ui| {
            egui::ComboBox::from_id_salt("audio_out").width(360.0).selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(self.hw.audio_device.is_empty(), "Off – no sound").clicked() {
                    chosen = Some((String::new(), String::new()));
                }
                if ui.selectable_label(self.hw.audio_device == DEFAULT_OUTPUT, "System default output").clicked() {
                    chosen = Some((DEFAULT_OUTPUT.to_string(), String::new()));
                }
                if !devices.is_empty() {
                    ui.separator();
                }
                for d in &devices {
                    if ui.selectable_label(self.hw.audio_device == d.id, &d.name).on_hover_text(&d.id).clicked() {
                        chosen = Some((d.id.clone(), d.name.clone()));
                    }
                }
            });
            if ui.button("Refresh").on_hover_text("List the sound outputs again (e.g. after plugging in a USB sound card)").clicked() {
                self.audio_devices = Some(output_devices());
            }
        });
        if let Some((id, name)) = chosen {
            self.hw.audio_device = id;
            self.hw.audio_device_name = name;
            self.audio_note.clear();
        }
        row(ui, "", |ui| {
            let on = !self.hw.audio_device.is_empty();
            let playing = self.audio_test.is_some();
            if ui.add_enabled(on && !playing, egui::Button::new("♪  Test sound")).on_hover_text("Plays a short tone on the chosen output").clicked() {
                match crate::media::test_sound(&self.hw.audio_device) {
                    Ok(e) => {
                        self.audio_test = Some((e, now() + 1.2));
                        self.audio_note = "♪ A short tone is playing – can the participant hear it?".into();
                    }
                    Err(e) => self.audio_note = format!("⚠ {e}"),
                }
            }
        });
        if !self.audio_note.is_empty() {
            // on its own line, wrapped: error messages can be long
            let col = if self.audio_note.starts_with('⚠') { DANGER } else { TEXT };
            ui.add(egui::Label::new(RichText::new(&self.audio_note).color(col)).wrap());
        }
        ui.add_space(6.0);
        theme::hint(
            ui,
            "Sound is off by default: audio blocks and the sound of video blocks stay silent until an output is chosen \
             here. Choose the output the participant's headphones are connected to (e.g. the MR-compatible headphone \
             amplifier). Sounds are loaded into memory before the run and start exactly at their block's onset.",
        );
        if devices.is_empty() {
            theme::hint(ui, "No sound outputs found on this computer.");
        }
    }

    fn capture_button(ui: &mut Ui, active: bool, key: &str) -> bool {
        let text = if active {
            RichText::new("press now…").color(ACCENT)
        } else if key.is_empty() {
            RichText::new("not set")
        } else {
            RichText::new(format!("key  “{key}”"))
        };
        ui.add(egui::Button::new(text).min_size(Vec2::new(110.0, 24.0)).stroke(egui::Stroke::new(1.0, if active { ACCENT } else { BORDER })))
            .on_hover_text("Click, then press the key / grip button (or send one trigger) to assign it.")
            .clicked()
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        self.poll();
        self.keys(ctx);
        let lamp_level = |t: f64| (1.0 - (now() - t) / 0.6).clamp(0.0, 1.0) as f32;
        if self.scanner.is_some() || lamp_level(self.trig_flash) > 0.0 || self.flashes.iter().any(|f| lamp_level(*f) > 0.0) {
            ctx.request_repaint();
        }
        // a test sound plays ~0.7 s; then the output is closed again
        if self.audio_test.as_ref().map(|t| now() > t.1).unwrap_or(false) {
            self.audio_test = None;
        }
        if self.audio_test.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let out = modal(ctx, "hardware", 680.0, |ui| {
            title(ui, "Hardware – NNL SyncBox, ResponseGrips & sound");
            ui.horizontal(|ui| {
                for (i, t) in ["SyncBox", "ResponseGrips", "Audio"].iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *t).clicked() {
                        self.tab = i;
                    }
                }
            });
            ui.separator();
            ui.allocate_ui(Vec2::new(680.0, 380.0), |ui| {
                ui.set_min_height(380.0);
                if self.tab == 0 {
                    self.sync_tab(ui, lamp_level(self.trig_flash));
                } else if self.tab == 1 {
                    self.grips_tab(ui, &lamp_level);
                } else {
                    self.audio_tab(ui);
                }
            });
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new("Compatible with NordicNeuroLab hardware · not affiliated with or endorsed by NordicNeuroLab AS")
                        .color(TEXT_DIM)
                        .size(11.0),
                )
                .wrap(),
            )
            .on_hover_text(crate::hardware::NNL_DISCLAIMER);
            buttons(ui, "Save hardware setup", true)
        });
        if out != Outcome::Open {
            if let Some(s) = &self.scanner {
                s.cancel();
            }
            self.scanner = None;
            if let Some(mut r) = self.router.take() {
                r.stop();
            }
        }
        out
    }

    fn sync_tab(&mut self, ui: &mut Ui, lamp: f32) {
        row(ui, "Connection", |ui| {
            let mut changed = false;
            changed |= ui.radio_value(&mut self.hw.sync_mode, "keyboard".to_string(), "USB keyboard (HID)").changed();
            changed |= ui.radio_value(&mut self.hw.sync_mode, "serial".to_string(), "Serial (COM port)").changed();
            if changed {
                self.restart_router(false);
            }
        });
        row(ui, "Trigger", |ui| {
            let active = self.capture == Some(Capture::Trigger);
            if Self::capture_button(ui, active, &self.hw.trigger_key) {
                self.capture = if active { None } else { Some(Capture::Trigger) };
            }
        });
        if self.hw.sync_mode == "serial" {
            row(ui, "Port", |ui| {
                let resp = ui.add(egui::TextEdit::singleline(&mut self.port_text).hint_text("e.g. COM3 or /dev/ttyUSB0").desired_width(200.0));
                let mut chosen: Option<String> = None;
                ui.menu_button("▾", |ui| {
                    if self.ports.is_empty() {
                        ui.label(RichText::new("no serial ports").color(TEXT_DIM));
                    }
                    for p in &self.ports {
                        let label = format!("{}{} – {}", if p.likely { "★ " } else { "" }, p.device, p.description);
                        let mut tip = p.description.clone();
                        if !p.manufacturer.is_empty() {
                            tip.push_str(&format!("\nmanufacturer: {}", p.manufacturer));
                        }
                        if let Some((v, pid)) = p.vid_pid {
                            tip.push_str(&format!("\nUSB {v:04X}:{pid:04X}"));
                        }
                        if p.likely {
                            tip.push_str("\n★ looks like an NNL SyncBox");
                        }
                        if ui.button(label).on_hover_text(tip).clicked() {
                            chosen = Some(p.device.clone());
                            ui.close();
                        }
                    }
                });
                if let Some(c) = chosen {
                    self.port_text = c;
                }
                if (resp.lost_focus() || !resp.has_focus()) && self.port_text.trim() != self.hw.serial_port && self.scanner.is_none() {
                    self.hw.serial_port = self.port_text.trim().to_string();
                    self.ser_note.clear();
                    self.restart_router(false);
                }
                if ui.button("Refresh").on_hover_text("Refresh the list of serial ports").clicked() {
                    self.refresh_ports(false);
                }
                if self.scanner.is_none() {
                    if theme::primary_button(ui, "Auto-detect")
                        .on_hover_text(
                            "Finds the SyncBox by NNL's handshake (no scanner needed). If no box answers, it listens for a \
                             device that sends on its own – then run the scanner / SyncBox simulation or press grip buttons.",
                        )
                        .clicked()
                    {
                        self.auto_detect();
                    }
                } else if ui.button("Cancel").clicked() {
                    if let Some(s) = &self.scanner {
                        s.cancel();
                    }
                }
            });
            if !self.ser_note.is_empty() {
                theme::hint(ui, self.ser_note.clone());
            }
            if !self.detect_info.is_empty() {
                theme::hint(ui, self.detect_info.clone());
            }
            let fold = if self.advanced { "▾  Advanced" } else { "▸  Advanced" };
            if ui.add(egui::Button::new(RichText::new(fold).color(TEXT_DIM)).frame(false)).clicked() {
                self.advanced = !self.advanced;
            }
            if self.advanced {
                row(ui, "Baud rate", |ui| {
                    let mut changed = false;
                    egui::ComboBox::from_id_salt("baud").selected_text(self.hw.baud_rate.to_string()).show_ui(ui, |ui| {
                        for b in [9600u32, 19200, 38400, 57600, 115200] {
                            changed |= ui.selectable_value(&mut self.hw.baud_rate, b, b.to_string()).changed();
                        }
                    });
                    if changed {
                        self.restart_router(false);
                    }
                });
                row(ui, "", |ui| {
                    if ui
                        .checkbox(&mut self.hw.syncbox_control, "Control the SyncBox (NNL computer mode)")
                        .on_hover_text(
                            "Before each run the SyncBox is switched to computer mode, configured with the TR and the number of \
                             volumes, and started; afterwards it is stopped and returned to manual mode. Untick if the SyncBox \
                             should keep its own settings and ParadigmShift only listens.",
                        )
                        .changed()
                    {
                        self.restart_router(false);
                    }
                });
            }
        } else if self.scanner.is_none() {
            ui.horizontal(|ui| {
                ui.add_space(LABEL_W + 8.0);
                if ui.button("Auto-detect a serial SyncBox").clicked() {
                    self.auto_detect();
                }
            });
            if !self.detect_info.is_empty() {
                theme::hint(ui, self.detect_info.clone());
            }
        } else {
            theme::hint(ui, self.detect_info.clone());
        }
        ui.add_space(10.0);
        theme::panel_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::lamp(ui, ACCENT, lamp, 24.0);
                let n = self.triggers.len();
                ui.label(RichText::new(format!("{n} pulse{}", if n == 1 { "" } else { "s" })).font(theme::bold(16.0)));
                let tr_text = if n > 1 {
                    let ts = &self.triggers[n.saturating_sub(6)..];
                    let m = (ts[ts.len() - 1] - ts[0]) / (ts.len() - 1) as f64;
                    let ok = (m - self.tr).abs() < 0.05;
                    format!("measured TR {m:.3} s  (paradigm {} s)  {}", fmt_g(self.tr), if ok { "✓" } else { "⚠ differs" })
                } else {
                    format!("measured TR –  (paradigm {} s)", fmt_g(self.tr))
                };
                ui.label(RichText::new(tr_text).color(TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Reset").clicked() {
                        self.triggers.clear();
                    }
                    let resp = ui
                        .add(egui::Button::new("Simulate scanner").selected(self.sim_on))
                        .on_hover_text("Send test triggers at the paradigm TR (by the SyncBox itself when it is controlled over serial).");
                    if resp.clicked() {
                        self.sim_on = !self.sim_on;
                        self.restart_router(self.sim_on);
                    }
                });
            });
        });
        if self.hw.sync_mode == "keyboard" {
            theme::hint(ui, "USB keyboard mode: keep this window in front – the trigger and grip buttons arrive as key presses.");
        } else {
            theme::hint(
                ui,
                "Serial mode: only what the SyncBox sends counts – keys typed on the computer's keyboard are ignored (in runs, \
                 in the function test and in the lamp test here; Esc still aborts a run).",
            );
            // the most common reason why a SyncBox cannot be found in serial mode
            ui.add_space(6.0);
            egui::Frame::new().fill(theme::BG).stroke(egui::Stroke::new(1.0, theme::BORDER)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Driver for serial mode").strong());
                ui.add(egui::Label::new(RichText::new(crate::hardware::FTDI_DRIVER_NOTE).color(TEXT_DIM)).wrap());
                ui.horizontal_wrapped(|ui| {
                    ui.hyperlink_to("Download FTDI VCP driver ↗", crate::hardware::FTDI_VCP_URL);
                    ui.label(RichText::new("·").color(TEXT_DIM));
                    if ui.button("Use USB keyboard mode instead").clicked() {
                        self.hw.sync_mode = "keyboard".into();
                        self.restart_router(false);
                    }
                });
            });
        }
    }

    fn grips_tab(&mut self, ui: &mut Ui, lamp_level: &dyn Fn(f64) -> f32) {
        theme::hint(ui, "Press a grip button – its lamp lights up. To change a key, click it and press the button.");
        ui.add_space(4.0);
        let mut remove = None;
        let mut names_changed = false;
        for i in 0..self.hw.buttons.len() {
            ui.horizontal(|ui| {
                theme::lamp(ui, OK, lamp_level(self.flashes.get(i).copied().unwrap_or(-10.0)), 20.0);
                let b = &mut self.hw.buttons[i];
                names_changed |= ui.add(egui::TextEdit::singleline(&mut b.name).desired_width(220.0)).changed();
                let active = self.capture == Some(Capture::Button(i));
                let key = b.key.clone();
                if Self::capture_button(ui, active, &key) {
                    self.capture = if active { None } else { Some(Capture::Button(i)) };
                }
                if ui.button("✕").on_hover_text("Remove button").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.hw.buttons.remove(i);
            self.flashes.remove(i);
            names_changed = true;
        }
        if ui.button("+ Add button").clicked() {
            let n = self.hw.buttons.len() + 1;
            self.hw.buttons.push(GripButton::new(&format!("Button {n}"), ""));
            self.flashes.push(-10.0);
            names_changed = true;
        }
        let names = self.hw.button_names();
        if names_changed {
            for slot in [&mut self.hw.move_left, &mut self.hw.move_right, &mut self.hw.confirm] {
                if !names.contains(slot) {
                    *slot = names.first().cloned().unwrap_or_default();
                }
            }
            self.restart_router(false);
        }
        ui.add_space(6.0);
        let fold = if self.cursor_fold { "▾  Default buttons for rating scales (cursor ◀ ▶ ✓)" } else { "▸  Default buttons for rating scales (cursor ◀ ▶ ✓)" };
        if ui.add(egui::Button::new(RichText::new(fold).color(TEXT_DIM)).frame(false)).clicked() {
            self.cursor_fold = !self.cursor_fold;
        }
        if self.cursor_fold {
            for (label, which) in [("Move left ◀", 0), ("Move right ▶", 1), ("Confirm ✓", 2)] {
                row(ui, label, |ui| {
                    let slot = match which {
                        0 => &mut self.hw.move_left,
                        1 => &mut self.hw.move_right,
                        _ => &mut self.hw.confirm,
                    };
                    egui::ComboBox::from_id_salt(format!("cursor_{which}")).selected_text(slot.clone()).show_ui(ui, |ui| {
                        for n in &names {
                            ui.selectable_value(slot, n.clone(), n);
                        }
                    });
                });
            }
        }
        ui.add_space(6.0);
        theme::hint(ui, self.last.clone());
    }
}

// =============================================================================
// Quick design wizard
// =============================================================================
/// Colours of the quick design's conditions (dots in the list, frame of the picture box).
const COND_COLORS: [&str; 8] = ["#4C9AFF", "#F5A524", "#2FB37A", "#E05FA5", "#A57BF5", "#2FA8C8", "#E4604E", "#9AAE3A"];
/// Height of the condition list and the picture box.
const QD_BOX_H: f32 = 196.0;
const QD_THUMB: Vec2 = Vec2::new(76.0, 57.0);

fn cond_color(i: usize) -> egui::Color32 {
    hex(COND_COLORS[i % COND_COLORS.len()])
}

/// Pictures dragged out of the picture box: from which condition, which ones.
#[derive(Clone)]
struct PictureDrag {
    from: usize,
    pics: Vec<usize>,
}

/// Changes collected while the dialog is drawn, applied afterwards.
enum QdAct {
    Select(usize),
    AddCondition(&'static str),
    RemoveCondition(usize),
    Click(usize, egui::Modifiers),
    SelectAll,
    RemovePictures(Vec<usize>),
    Move { from: usize, pics: Vec<usize>, to: Option<usize> },
    AddPictures(Vec<String>),
}

#[derive(Clone)]
pub struct QuickDesign {
    tab: usize,
    /// One list for both designs. The block design uses picture and text conditions, the
    /// event-related design the picture conditions.
    conds: Vec<DesignCondition>,
    /// the condition shown in the picture box
    sel: usize,
    /// selected pictures in the box (indices into the shown condition's pictures)
    picked: std::collections::BTreeSet<usize>,
    anchor: Option<usize>,
    /// the pictures of the Media panel
    media: Vec<String>,
    blur: bool,
    bd_block: f64,
    bd_rest: f64,
    bd_reps: u32,
    bd_random: bool,
    bd_lead: f64,
    ev_dur: f64,
    /// the fixation lengths between the pictures and their shares (lengths in seconds)
    jitter: Jitter,
    /// shares in percent (or as numbers of pictures)
    share_pct: bool,
    /// the distribution editor (equalizer) is open
    eq_open: bool,
    /// the list editor is shown, with the list as typed (in seconds or TRs)
    list_open: bool,
    list_text: String,
    list_err: Option<String>,
    ev_rating: bool,
    ev_shuffle: bool,
    pub replace: bool,
    /// the TR set here – building the timeline sets it for the paradigm
    tr: f64,
    /// the paradigm's TR when the dialog was opened
    par_tr: f64,
    /// show and type all times in TRs instead of seconds
    unit_tr: bool,
}

/// Default number of fixation lengths from 2 to 6 s: about one every half TR.
fn default_steps(tr: f64) -> usize {
    ((4.0 / (tr / 2.0)).round() as usize + 1).clamp(2, 9)
}

impl QuickDesign {
    pub fn new(par: &Paradigm, media: &[String], blur: bool) -> Self {
        let tr = if par.tr > 0.0 { par.tr } else { 2.0 };
        let name = if media.is_empty() { "cond1" } else { "pictures" };
        QuickDesign {
            tab: 0,
            conds: vec![DesignCondition { name: name.into(), kind: "image".into(), images: media.to_vec() }],
            sel: 0,
            picked: Default::default(),
            anchor: None,
            media: media.to_vec(),
            blur,
            bd_block: tr * 10.0,
            bd_rest: tr * 10.0,
            bd_reps: 4,
            bd_random: false,
            bd_lead: tr * 5.0,
            ev_dur: 4.0,
            jitter: Jitter::even(2.0, 6.0, default_steps(tr)),
            share_pct: true,
            eq_open: false,
            list_open: false,
            list_text: String::new(),
            list_err: None,
            ev_rating: true,
            ev_shuffle: true,
            replace: par.blocks.is_empty(),
            tr,
            par_tr: par.tr,
            unit_tr: false,
        }
    }

    /// The TR to set for the paradigm when the timeline is built.
    pub fn tr(&self) -> f64 {
        self.tr
    }

    /// Open again with what was set up last time (conditions, pictures, numbers) – for the
    /// current paradigm and Media panel.
    pub fn refresh(&mut self, par: &Paradigm, media: &[String], blur: bool) {
        self.media = media.to_vec();
        self.blur = blur;
        if par.tr > 0.0 {
            self.tr = par.tr;
        }
        self.par_tr = par.tr;
        self.replace = par.blocks.is_empty();
        self.picked.clear();
        self.anchor = None;
    }

    /// For the screenshots: the example pictures sorted into "negative" and "positive",
    /// event-related, with a few pictures selected.
    pub fn demo(&mut self) {
        let all: Vec<String> = self.conds.iter().flat_map(|c| c.images.clone()).collect();
        let (neg, pos): (Vec<String>, Vec<String>) = all.into_iter().partition(|f| crate::model::basename(f).starts_with("neg"));
        self.conds = vec![
            DesignCondition { name: "negative".into(), kind: "image".into(), images: neg },
            DesignCondition { name: "positive".into(), kind: "image".into(), images: pos },
        ];
        self.tab = 1;
        self.sel = 0;
        self.picked = [1, 2].into_iter().collect();
    }

    /// For the screenshots: the distribution editor with more short than long fixations.
    pub fn demo_jitter(&mut self) {
        self.demo();
        self.picked.clear();
        self.jitter = Jitter::even(2.0, 6.0, 5);
        self.jitter.set_counts(&[3, 2, 1, 1, 1]);
        self.eq_open = true;
    }

    /// The conditions the current design uses (empty picture conditions are left out).
    fn used_conditions(&self) -> Vec<DesignCondition> {
        let event = self.tab == 1;
        self.conds
            .iter()
            .filter(|c| if c.kind == "image" { !c.images.is_empty() } else { !event })
            .cloned()
            .collect()
    }

    pub fn blocks(&self, seed: Option<u64>) -> Vec<crate::model::Block> {
        let conds = self.used_conditions();
        if conds.is_empty() {
            return vec![];
        }
        if self.tab == 0 {
            let mut blocks = generate_block_design(&conds, self.bd_block, self.bd_rest, self.bd_reps, self.bd_random, 0.0, self.bd_lead, seed);
            for b in blocks.iter_mut() {
                if b.kind == "text" && !b.condition.is_empty() {
                    b.params.insert("text".into(), serde_json::json!(b.condition.to_uppercase()));
                }
            }
            return blocks;
        }
        let n = conds.iter().map(|c| c.images.len()).sum();
        if (self.list_open && self.list_err.is_some()) || !self.jitter.fits(n) {
            return vec![]; // fix the list / the distribution first
        }
        let itis = self.jitter.values(n, seed);
        generate_event_design(&conds, self.ev_dur, &itis, self.ev_rating, self.ev_shuffle, 0.0, seed)
    }

    // ---- editing the conditions -------------------------------------------------------------

    fn unique_name(&self, base: &str) -> String {
        let mut n = self.conds.len() + 1;
        loop {
            let s = format!("{base}{n}");
            if !self.conds.iter().any(|c| c.name == s) {
                return s;
            }
            n += 1;
        }
    }

    fn select(&mut self, i: usize) {
        if self.sel != i {
            self.sel = i;
            self.picked.clear();
            self.anchor = None;
        }
    }

    fn add_condition(&mut self, kind: &str) -> usize {
        let name = self.unique_name(if kind == "text" { "task" } else { "cond" });
        self.conds.push(DesignCondition { name, kind: kind.into(), images: vec![] });
        let i = self.conds.len() - 1;
        self.select(i);
        i
    }

    fn remove_condition(&mut self, i: usize) {
        if i >= self.conds.len() {
            return;
        }
        self.conds.remove(i);
        if self.sel > i || self.sel >= self.conds.len() {
            self.sel = self.sel.saturating_sub(1);
        }
        self.picked.clear();
        self.anchor = None;
    }

    /// Pictures dropped on the dialog, chosen or taken from the Media panel go into the shown
    /// condition – or into a new one if a text condition is shown.
    pub fn add_pictures(&mut self, files: Vec<String>) {
        let files: Vec<String> = files.into_iter().filter(|f| crate::model::is_image(f)).collect();
        if files.is_empty() {
            return;
        }
        if self.conds.get(self.sel).is_none_or(|c| c.kind != "image") {
            self.add_condition("image");
        }
        let c = &mut self.conds[self.sel];
        for f in files {
            if !c.images.contains(&f) {
                c.images.push(f);
            }
        }
    }

    fn remove_pictures(&mut self, cond: usize, pics: &[usize]) {
        let Some(c) = self.conds.get_mut(cond) else { return };
        let mut idx = pics.to_vec();
        idx.sort_unstable();
        idx.dedup();
        for i in idx.into_iter().rev() {
            if i < c.images.len() {
                c.images.remove(i);
            }
        }
        if cond == self.sel {
            self.picked.clear();
            self.anchor = None;
        }
    }

    /// Move pictures to another picture condition (`None`: a new one).
    fn move_pictures(&mut self, from: usize, pics: &[usize], to: Option<usize>) {
        let Some(src) = self.conds.get(from) else { return };
        let mut idx = pics.to_vec();
        idx.sort_unstable();
        idx.dedup();
        let files: Vec<String> = idx.iter().filter_map(|&i| src.images.get(i).cloned()).collect();
        let to = match to {
            Some(t) if t != from && self.conds.get(t).is_some_and(|c| c.kind == "image") => t,
            Some(_) => return,
            None => {
                let name = self.unique_name("cond");
                self.conds.push(DesignCondition { name, kind: "image".into(), images: vec![] });
                self.conds.len() - 1
            }
        };
        self.remove_pictures(from, &idx);
        let dst = &mut self.conds[to];
        for f in files {
            if !dst.images.contains(&f) {
                dst.images.push(f);
            }
        }
    }

    fn click_picture(&mut self, j: usize, m: egui::Modifiers) {
        if m.shift {
            let a = self.anchor.unwrap_or(j);
            if !m.command {
                self.picked.clear();
            }
            self.picked.extend(a.min(j)..=a.max(j));
        } else if m.command {
            if !self.picked.remove(&j) {
                self.picked.insert(j);
            }
            self.anchor = Some(j);
        } else {
            self.picked = [j].into_iter().collect();
            self.anchor = Some(j);
        }
    }

    fn apply(&mut self, a: QdAct) {
        match a {
            QdAct::Select(i) => self.select(i),
            QdAct::AddCondition(kind) => {
                self.add_condition(kind);
            }
            QdAct::RemoveCondition(i) => self.remove_condition(i),
            QdAct::Click(j, m) => self.click_picture(j, m),
            QdAct::SelectAll => {
                let n = self.conds.get(self.sel).map(|c| c.images.len()).unwrap_or(0);
                self.picked = (0..n).collect();
            }
            QdAct::RemovePictures(p) => self.remove_pictures(self.sel, &p),
            QdAct::Move { from, pics, to } => self.move_pictures(from, &pics, to),
            QdAct::AddPictures(f) => self.add_pictures(f),
        }
    }

    // ---- drawing ----------------------------------------------------------------------------

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        let eq_was_open = self.eq_open;
        let out = modal(ctx, "quick", 780.0, |ui| {
            title(ui, "✦ Quick design");
            theme::hint(ui, "Pick a design, sort your pictures into conditions, fill in a few numbers – the whole timeline is built for you. You can fine-tune every block afterwards.");
            ui.horizontal(|ui| {
                for (i, t) in ["Block design  (ON / OFF)", "Event-related  (single pictures)"].iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *t).clicked() {
                        self.tab = i;
                    }
                }
            });
            ui.separator();
            // on a small screen the middle part scrolls
            let max_h = (ctx.content_rect().height() - 250.0).max(240.0);
            egui::ScrollArea::vertical().id_salt("qd_body").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                self.conditions_ui(ui);
                ui.add_space(6.0);
                self.timing_row(ui);
                ui.allocate_ui(Vec2::new(ui.available_width(), 230.0), |ui| {
                    ui.set_min_height(230.0);
                    if self.tab == 0 {
                        self.block_settings(ui);
                    } else {
                        self.event_settings(ui);
                    }
                });
            });
            let blocks = self.blocks(Some(1));
            let summary = if blocks.is_empty() {
                if self.tab == 1 && self.list_open && self.list_err.is_some() {
                    "Fix the list of fixation lengths to build a timeline.".to_string()
                } else if self.tab == 1 && self.n_events() > 0 && !self.jitter.fits(self.n_events()) {
                    format!("{}. Make them add up to build a timeline.", misfit_text(self.jitter.assigned(self.n_events()), self.n_events()))
                } else if self.tab == 1 {
                    "Add pictures to at least one condition to build a timeline.".to_string()
                } else {
                    "Add pictures or a text condition to build a timeline.".to_string()
                }
            } else {
                let dur = blocks.iter().map(|b| b.end()).fold(0.0, f64::max);
                let vols = if self.tr > 0.0 { (dur / self.tr - 1e-9).ceil() as i64 } else { 0 };
                format!("→ {} blocks  ·  {} min  ·  {vols} volumes at TR {}s", blocks.len(), fmt_time(dur, 1), fmt_g(self.tr))
            };
            let empty: Vec<&str> = self.conds.iter().filter(|c| c.kind == "image" && c.images.is_empty()).map(|c| c.name.as_str()).collect();
            ui.horizontal(|ui| {
                ui.label(RichText::new(summary).color(ACCENT).font(theme::bold(13.0)));
                if !empty.is_empty() && !blocks.is_empty() {
                    ui.add(egui::Label::new(RichText::new(format!("·  left out (no pictures yet): {}", empty.join(", "))).color(TEXT_DIM).size(12.0)).truncate());
                }
            });
            let mut out = Outcome::Open;
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.replace, "Replace the current timeline");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    out = buttons(ui, "Build timeline", !blocks.is_empty());
                });
            });
            out
        });
        // the distribution editor on top; Esc closes only it
        if self.eq_open {
            self.equalizer_ui(ctx);
        }
        if eq_was_open && out == Outcome::Cancel { Outcome::Open } else { out }
    }

    /// Condition list on the left, the shown condition's pictures on the right.
    fn conditions_ui(&mut self, ui: &mut Ui) {
        if self.sel >= self.conds.len() {
            self.sel = self.conds.len().saturating_sub(1);
        }
        let mut acts: Vec<QdAct> = vec![];
        let event = self.tab == 1;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(232.0);
                ui.label(RichText::new("CONDITIONS").font(theme::bold(11.0)).color(TEXT_DIM));
                egui::Frame::new().fill(theme::BG).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(4).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(QD_BOX_H);
                    egui::ScrollArea::vertical().id_salt("qd_conds").auto_shrink([false, false]).show(ui, |ui| {
                        for i in 0..self.conds.len() {
                            self.condition_row(ui, i, event, &mut acts);
                        }
                        if self.conds.is_empty() {
                            ui.label(RichText::new("No conditions yet").color(TEXT_DIM));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui.button("+ Condition").on_hover_text("A new condition with pictures").clicked() {
                        acts.push(QdAct::AddCondition("image"));
                    }
                    if !event
                        && ui
                            .button("+ Text condition")
                            .on_hover_text("A block that shows a task text instead of pictures (its name, e.g. “TAP”) – block design only")
                            .clicked()
                    {
                        acts.push(QdAct::AddCondition("text"));
                    }
                });
            });
            ui.add_space(4.0);
            ui.vertical(|ui| self.picture_box(ui, &mut acts));
        });
        for a in acts {
            self.apply(a);
        }
    }

    fn condition_row(&mut self, ui: &mut Ui, i: usize, event: bool, acts: &mut Vec<QdAct>) {
        let ctx = ui.ctx().clone();
        let selected = self.sel == i;
        let c = &mut self.conds[i];
        let text_cond = c.kind != "image";
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::click());
        // pictures dragged from the box can be dropped on another picture condition
        let drag = egui::DragAndDrop::payload::<PictureDrag>(&ctx);
        let over = ui.input(|inp| inp.pointer.latest_pos()).is_some_and(|p| rect.contains(p));
        let drop_ok = drag.as_ref().is_some_and(|d| d.from != i && !text_cond);
        let p = ui.painter();
        if drop_ok && over {
            p.rect(rect, 4, theme::alpha(ACCENT, 40), egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
        } else if selected {
            p.rect_filled(rect, 4, PANEL_2);
        } else if resp.hovered() {
            p.rect_filled(rect, 4, theme::alpha(PANEL_2, 120));
        }
        let col = if event && text_cond { theme::alpha(cond_color(i), 70) } else { cond_color(i) };
        let dot = egui::Pos2::new(rect.left() + 11.0, rect.center().y);
        if text_cond {
            p.rect_filled(egui::Rect::from_center_size(dot, Vec2::splat(9.0)), 2, col);
        } else {
            p.circle_filled(dot, 5.0, col);
        }
        let count = if text_cond {
            if event { "block design only".to_string() } else { "text".to_string() }
        } else {
            c.images.len().to_string()
        };
        let x_rect = egui::Rect::from_center_size(egui::Pos2::new(rect.right() - 11.0, rect.center().y), Vec2::splat(18.0));
        let g = p.layout_no_wrap(count, theme::font(if text_cond && event { 10.5 } else { 12.0 }), TEXT_DIM);
        let count_w = g.size().x;
        p.galley(egui::Pos2::new(x_rect.left() - 4.0 - count_w, rect.center().y - g.size().y / 2.0), g, TEXT_DIM);
        let name_rect = egui::Rect::from_min_max(
            egui::Pos2::new(rect.left() + 22.0, rect.top() + 3.0),
            egui::Pos2::new(x_rect.left() - 10.0 - count_w, rect.bottom() - 3.0),
        );
        let r = ui.put(name_rect, egui::TextEdit::singleline(&mut c.name).hint_text("name").text_color(if event && text_cond { TEXT_DIM } else { TEXT }));
        if r.gained_focus() || r.clicked() {
            acts.push(QdAct::Select(i));
        }
        let n = c.images.len();
        let tip = if n > 0 { format!("Remove “{}” and its {n} picture(s) from the design", c.name) } else { format!("Remove “{}”", c.name) };
        if ui.put(x_rect, egui::Button::new(RichText::new("×").color(TEXT_DIM)).frame(false)).on_hover_text(tip).clicked() {
            acts.push(QdAct::RemoveCondition(i));
        }
        if resp.clicked() {
            acts.push(QdAct::Select(i));
        }
        if drop_ok && over {
            if ui.input(|inp| inp.pointer.any_released()) {
                if let Some(d) = egui::DragAndDrop::take_payload::<PictureDrag>(&ctx) {
                    acts.push(QdAct::Move { from: d.from, pics: d.pics.clone(), to: Some(i) });
                    acts.push(QdAct::Select(i));
                }
            } else {
                ctx.request_repaint();
            }
        }
    }

    /// The pictures of the shown condition: add, select, remove, drag onto another condition.
    fn picture_box(&mut self, ui: &mut Ui, acts: &mut Vec<QdAct>) {
        let ctx = ui.ctx().clone();
        let ci = self.sel;
        let Some(c) = self.conds.get(ci) else {
            ui.label(RichText::new("PICTURES").font(theme::bold(11.0)).color(TEXT_DIM));
            ui.allocate_space(Vec2::new(ui.available_width(), QD_BOX_H));
            return;
        };
        let text_cond = c.kind != "image";
        let n = c.images.len();
        ui.horizontal(|ui| {
            let head = if text_cond { format!("TEXT CONDITION  “{}”", c.name) } else { format!("PICTURES OF  “{}”  ·  {n}", c.name) };
            ui.label(RichText::new(head).font(theme::bold(11.0)).color(TEXT_DIM));
            if text_cond {
                return;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // the Media panel's pictures that are in no condition yet – handy for sorting
                let free: Vec<String> = self.media.iter().filter(|f| !self.conds.iter().any(|c| c.images.contains(f))).cloned().collect();
                if ui
                    .add_enabled(!free.is_empty(), egui::Button::new("From Media panel"))
                    .on_hover_text(format!("Add the {} picture(s) of the Media panel that are in no condition yet", free.len()))
                    .on_disabled_hover_text(if self.media.is_empty() {
                        "The Media panel has no pictures"
                    } else {
                        "All pictures of the Media panel are already in a condition"
                    })
                    .clicked()
                {
                    acts.push(QdAct::AddPictures(free));
                }
                if ui.button("+ Add pictures…").on_hover_text("Choose pictures for this condition").clicked() {
                    let files = crate::panels::pick_images("Choose pictures");
                    if !files.is_empty() {
                        acts.push(QdAct::AddPictures(files));
                    }
                }
            });
        });
        let desktop_drag = ui.input(|i| !i.raw.hovered_files.is_empty());
        let stroke = if desktop_drag { egui::Stroke::new(2.0, ACCENT) } else { egui::Stroke::new(1.0, theme::alpha(cond_color(ci), if text_cond { 60 } else { 140 })) };
        egui::Frame::new().fill(theme::BG).stroke(stroke).corner_radius(6).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(QD_BOX_H - 4.0);
            let center_hint = |ui: &mut Ui, t: &str| {
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Label::new(RichText::new(t).color(TEXT_DIM)).wrap());
                });
            };
            if text_cond {
                center_hint(
                    ui,
                    &format!(
                        "Each block of this condition shows the text “{}” (its name).\nUsed in the block design only; change the text afterwards in the inspector.",
                        c.name.to_uppercase()
                    ),
                );
                return;
            }
            if n == 0 {
                center_hint(ui, "Drop pictures or a folder here\n– or use + Add pictures… / From Media panel");
                return;
            }
            let pointer = ui.input(|i| i.pointer.latest_pos());
            let mods = ui.input(|i| i.modifiers);
            egui::ScrollArea::vertical().id_salt(("qd_pics", ci)).auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                    for (j, f) in c.images.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(QD_THUMB, egui::Sense::click_and_drag());
                        if !ui.is_rect_visible(r) {
                            continue;
                        }
                        let picked = self.picked.contains(&j);
                        let p = ui.painter();
                        p.rect_filled(r, 4, egui::Color32::from_gray(18));
                        if !crate::stimulus::paint_thumb(p, r.shrink(2.0), f, self.blur) && !std::path::Path::new(f).exists() {
                            p.text(r.center(), egui::Align2::CENTER_CENTER, "missing", theme::font(10.0), DANGER);
                        }
                        let (w, col) = if picked { (2.0, ACCENT) } else if resp.hovered() { (1.0, TEXT_DIM) } else { (1.0, BORDER) };
                        p.rect_stroke(r, 4, egui::Stroke::new(w, col), egui::StrokeKind::Inside);
                        // × to remove this picture
                        let x = egui::Rect::from_center_size(r.right_top() + Vec2::new(-9.0, 9.0), Vec2::splat(15.0));
                        let on_x = pointer.is_some_and(|q| x.contains(q));
                        if resp.hovered() {
                            p.circle_filled(x.center(), 7.5, if on_x { DANGER } else { egui::Color32::from_black_alpha(190) });
                            p.text(x.center(), egui::Align2::CENTER_CENTER, "×", theme::bold(12.0), egui::Color32::WHITE);
                        }
                        let resp = resp.on_hover_text(if on_x { "Remove from this condition".to_string() } else { crate::model::basename(f) });
                        if resp.clicked() {
                            if on_x {
                                acts.push(QdAct::RemovePictures(vec![j]));
                            } else {
                                acts.push(QdAct::Click(j, mods));
                            }
                        }
                        if resp.drag_started() {
                            let pics: Vec<usize> = if picked { self.picked.iter().copied().collect() } else { vec![j] };
                            egui::DragAndDrop::set_payload(&ctx, PictureDrag { from: ci, pics });
                        }
                    }
                });
            });
        });
        // the dragged pictures follow the pointer
        if let (Some(d), Some(pos)) = (egui::DragAndDrop::payload::<PictureDrag>(&ctx), ctx.pointer_latest_pos()) {
            if d.from == ci {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("qd_drag")));
                let r = egui::Rect::from_min_size(pos + Vec2::new(10.0, 6.0), QD_THUMB * 0.7);
                p.rect_filled(r, 4, egui::Color32::from_gray(18));
                if let Some(f) = d.pics.first().and_then(|&j| c.images.get(j)) {
                    crate::stimulus::paint_thumb(&p, r.shrink(2.0), f, self.blur);
                }
                p.rect_stroke(r, 4, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
                if d.pics.len() > 1 {
                    let b = r.right_top();
                    p.circle_filled(b, 10.0, ACCENT);
                    p.text(b, egui::Align2::CENTER_CENTER, d.pics.len().to_string(), theme::bold(11.0), hex("#1B1B1B"));
                }
            }
        }
        // what to do with the selection
        let others: Vec<(usize, String)> =
            self.conds.iter().enumerate().filter(|(i, o)| *i != ci && o.kind == "image").map(|(i, o)| (i, o.name.clone())).collect();
        ui.horizontal(|ui| {
            if self.picked.is_empty() || text_cond {
                if !text_cond && n > 0 {
                    ui.label(
                        RichText::new("Click to select · Ctrl / Shift + click for more · drag onto another condition to move · × or Del removes")
                            .color(TEXT_DIM)
                            .size(11.5),
                    );
                }
                return;
            }
            let pics: Vec<usize> = self.picked.iter().copied().collect();
            ui.label(RichText::new(format!("{} selected", pics.len())).strong());
            egui::ComboBox::from_id_salt("qd_move").selected_text("Move to…").width(120.0).show_ui(ui, |ui| {
                for (i, name) in &others {
                    if ui.selectable_label(false, name).clicked() {
                        acts.push(QdAct::Move { from: ci, pics: pics.clone(), to: Some(*i) });
                    }
                }
                if ui.selectable_label(false, "+ New condition").clicked() {
                    acts.push(QdAct::Move { from: ci, pics: pics.clone(), to: None });
                }
            });
            if ui.button("Remove").on_hover_text("Remove the selected pictures from this condition (Del)").clicked() {
                acts.push(QdAct::RemovePictures(pics.clone()));
            }
            if pics.len() < n && ui.button("Select all").clicked() {
                acts.push(QdAct::SelectAll);
            }
            if ui.button("Deselect").clicked() {
                acts.push(QdAct::Click(usize::MAX, egui::Modifiers::NONE));
            }
        });
        // Del / Backspace removes the selection (not while a name is being typed)
        if !self.picked.is_empty() && !ctx.egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            acts.push(QdAct::RemovePictures(self.picked.iter().copied().collect()));
        }
    }

    // ---- times ------------------------------------------------------------------------------

    /// The unit times are shown in: 1 s, or the TR.
    fn unit(&self) -> f64 {
        if self.unit_tr && self.tr > 0.0 { self.tr } else { 1.0 }
    }

    /// The design's main times (not the jitter), which should usually be whole TRs.
    fn main_times(&mut self) -> Vec<&mut f64> {
        if self.tab == 0 { vec![&mut self.bd_block, &mut self.bd_rest, &mut self.bd_lead] } else { vec![&mut self.ev_dur] }
    }

    fn off_tr_grid(&mut self) -> bool {
        let tr = self.tr;
        tr > 0.0 && self.main_times().into_iter().any(|v| !whole_trs(*v, tr))
    }

    fn round_to_trs(&mut self) {
        let tr = self.tr;
        let tab = self.tab;
        for (i, v) in self.main_times().into_iter().enumerate() {
            // rest and baseline may be 0, block and picture at least one TR
            let min = if tab == 0 && i > 0 { 0.0 } else { 1.0 };
            *v = round4((*v / tr).round().max(min) * tr);
        }
    }

    /// The TR changed while times are shown in TRs: keep the numbers of TRs.
    fn scale_times(&mut self, f: f64) {
        for v in [&mut self.bd_block, &mut self.bd_rest, &mut self.bd_lead, &mut self.ev_dur] {
            *v = round4(*v * f);
        }
        self.jitter.scale(f);
    }

    fn set_unit(&mut self, tr_units: bool) {
        self.unit_tr = tr_units;
        self.sync_list();
    }

    /// The jitter changed elsewhere: show it in the list editor too.
    fn sync_list(&mut self) {
        if self.list_err.is_none() {
            self.list_text = self.jitter.to_list_text(self.n_events().max(self.jitter.steps.len()), self.unit());
        }
    }

    /// TR, seconds ⇄ TRs, round to whole TRs.
    fn timing_row(&mut self, ui: &mut Ui) {
        row(ui, "TR (repetition time)", |ui| {
            let old = self.tr;
            let r = ui
                .add(egui::DragValue::new(&mut self.tr).range(0.1..=20.0).speed(0.01).suffix(" s").max_decimals(3))
                .on_hover_text("The scanner's repetition time. Building the timeline sets it for the whole paradigm.");
            if r.changed() && self.unit_tr && old > 0.0 {
                self.scale_times(self.tr / old);
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Times in").color(TEXT_DIM));
            let mut u = self.unit_tr;
            ui.selectable_value(&mut u, false, "seconds").on_hover_text("Show and type all times in seconds");
            ui.selectable_value(&mut u, true, "TRs").on_hover_text("Show and type all times in TRs – to see at a glance that you work in the TR steps you want");
            if u != self.unit_tr {
                self.set_unit(u);
            }
            if self.off_tr_grid() {
                ui.add_space(8.0);
                let what = if self.tab == 0 { "block length, rest and baseline" } else { "the picture's time on screen" };
                if ui
                    .button("Round to whole TRs")
                    .on_hover_text(format!("Rounds {what} to whole TRs (not the jitter)."))
                    .clicked()
                {
                    self.round_to_trs();
                }
            }
        });
        if self.par_tr > 0.0 && (self.tr - self.par_tr).abs() > 1e-9 {
            row(ui, "", |ui| {
                ui.label(
                    RichText::new(format!("Building the timeline changes the paradigm's TR from {} s to {} s.", fmt_g(self.par_tr), fmt_g(self.tr)))
                        .color(ACCENT)
                        .size(12.0),
                );
            });
        }
    }

    fn block_settings(&mut self, ui: &mut Ui) {
        let (u, tr) = (self.unit_tr, self.tr);
        row(ui, "Block length", |ui| {
            time_value(ui, &mut self.bd_block, u, tr, 0.5..=600.0, true).on_hover_text("The pictures of a condition share its block equally");
        });
        row(ui, "Rest between blocks", |ui| {
            time_value(ui, &mut self.bd_rest, u, tr, 0.0..=600.0, true).on_hover_text("Fixation between blocks. 0 = no rest.");
        });
        row(ui, "Repetitions", |ui| {
            ui.add(egui::DragValue::new(&mut self.bd_reps).range(1..=100).suffix(" cycles"));
        });
        row(ui, "Order", |ui| {
            ui.radio_value(&mut self.bd_random, false, "Alternating  A B A B …");
            ui.radio_value(&mut self.bd_random, true, "Randomised per cycle");
        });
        row(ui, "Baseline at start", |ui| {
            time_value(ui, &mut self.bd_lead, u, tr, 0.0..=600.0, true).on_hover_text("Baseline fixation before the first block");
        });
    }

    /// Number of pictures the event-related design shows.
    fn n_events(&self) -> usize {
        self.conds.iter().filter(|c| c.kind == "image").map(|c| c.images.len()).sum()
    }

    fn event_settings(&mut self, ui: &mut Ui) {
        let (u, tr) = (self.unit_tr, self.tr);
        let n = self.n_events();
        row(ui, "Picture on screen", |ui| {
            time_value(ui, &mut self.ev_dur, u, tr, 0.1..=120.0, true);
        });
        // from … to … in k steps – changing them spreads the lengths evenly again
        let (mut from, mut to, mut k) = (self.jitter.from(), self.jitter.to(), self.jitter.steps.len());
        row(ui, "Fixation between (jitter)", |ui| {
            let a = time_value(ui, &mut from, u, tr, 0.0..=120.0, false).changed();
            ui.label("to");
            let b = time_value(ui, &mut to, u, tr, 0.0..=120.0, false).changed();
            ui.label("in");
            let suffix = if k == 1 { " length" } else { " lengths" };
            let c = ui
                .add(egui::DragValue::new(&mut k).range(1..=20).suffix(suffix))
                .on_hover_text("How many different fixation lengths, evenly spread from … to")
                .changed();
            if a || b || c {
                self.jitter.respace(from, to, k);
                self.list_err = None;
                self.sync_list();
            }
        });
        let values = self.jitter.sorted(n);
        if !(self.list_open && self.list_err.is_some()) {
            row(ui, "", |ui| jitter_preview(ui, &values, self.unit(), if u { "TR" } else { "s" }));
        }
        if n > 0 && !self.jitter.fits(n) {
            row(ui, "", |ui| {
                ui.label(RichText::new(misfit_text(self.jitter.assigned(n), n)).color(ACCENT).size(12.5));
                if ui.button(format!("Fit to {n} pictures")).on_hover_text("Scale the numbers so they add up, keeping the proportions").clicked() {
                    self.jitter.fit(n);
                    self.list_err = None;
                    self.sync_list();
                }
            });
        }
        row(ui, "", |ui| {
            if ui
                .button("✎ Edit distribution…")
                .on_hover_text("Set how often each fixation length comes up – like an equalizer, in percent or as numbers of pictures")
                .clicked()
            {
                self.eq_open = true;
            }
            let list_label = if self.list_open { "Hide list" } else { "Edit as list" };
            if ui.button(list_label).on_hover_text("Type the fixation lengths yourself, e.g. 2, 3, 4.5×2").clicked() {
                self.list_open = !self.list_open;
                self.list_err = None;
                self.sync_list();
            }
            if !self.jitter.is_even() {
                if ui.button("All equally often").on_hover_text("Back to evenly spread lengths, each equally often").clicked() {
                    let (f, t, k) = (self.jitter.from(), self.jitter.to(), self.jitter.steps.len());
                    self.jitter = Jitter::even(f, t, k);
                    self.list_err = None;
                    self.sync_list();
                }
                ui.label(RichText::new("own distribution").color(TEXT_DIM).size(12.0));
            }
        });
        if self.list_open {
            let unit_name = if u { "TRs" } else { "seconds" };
            row(ui, "Lengths", |ui| {
                let r = ui.add(egui::TextEdit::multiline(&mut self.list_text).desired_rows(2).desired_width(400.0).hint_text("e.g. 2, 3, 4.5×2"));
                if r.changed() {
                    match parse_jitter_list(&self.list_text, self.unit()) {
                        Ok(l) => {
                            self.jitter = Jitter::from_list(&l);
                            self.list_err = None;
                        }
                        Err(e) => self.list_err = Some(e),
                    }
                }
                ui.label(RichText::new(unit_name).color(TEXT_DIM));
            });
            row(ui, "", |ui| {
                match &self.list_err {
                    Some(e) => ui.label(RichText::new(e).color(DANGER).size(12.0)),
                    None => ui.label(
                        RichText::new(format!("In {unit_name}; “×3” = three times – together one length for each of the {n} pictures, in random order."))
                            .color(TEXT_DIM)
                            .size(12.0),
                    ),
                };
            });
        }
        row(ui, "Order", |ui| {
            ui.radio_value(&mut self.ev_shuffle, true, "All conditions mixed at random");
            ui.radio_value(&mut self.ev_shuffle, false, "As listed, condition by condition");
        });
        row(ui, "", |ui| {
            ui.checkbox(&mut self.ev_rating, "Participant rates each picture with the grips");
        });
    }

    /// The distribution editor: one slider per fixation length, like the bands of an equalizer.
    fn equalizer_ui(&mut self, ctx: &Context) {
        let n = self.n_events();
        let (u, tr) = (self.unit_tr, self.tr);
        let unit = self.unit();
        let uname = if u { "TR" } else { "s" };
        let lab = move |x: f64| format!("{} {uname}", fmt_g(round_to(x / unit, 3)));
        let mut close = false;
        let resp = egui::Modal::new(Id::unique("qd_equalizer")).frame(egui::Frame::window(&ctx.global_style()).inner_margin(18)).show(ctx, |ui| {
            ui.set_width(720.0);
            title(ui, "Jitter distribution");
            theme::hint(
                ui,
                "Like an equalizer: choose how many different fixation lengths there are, then pull each slider up or down \
                 to set how often that length comes up.",
            );
            ui.add_space(4.0);
            let (mut from, mut to, mut k) = (self.jitter.from(), self.jitter.to(), self.jitter.steps.len());
            row(ui, "Lengths", |ui| {
                let a = time_value(ui, &mut from, u, tr, 0.0..=120.0, false).changed();
                ui.label("to");
                let b = time_value(ui, &mut to, u, tr, 0.0..=120.0, false).changed();
                ui.label("in");
                let c = ui.add(egui::DragValue::new(&mut k).range(1..=20).suffix(" steps")).on_hover_text("How many different fixation lengths").changed();
                if a || b || c {
                    self.jitter.respace(from, to, k);
                }
            });
            row(ui, "Show shares as", |ui| {
                ui.radio_value(&mut self.share_pct, true, "percent");
                ui.radio_value(&mut self.share_pct, false, "number of pictures");
            });
            let note = if n == 0 {
                RichText::new("Add pictures to the conditions first – the sliders move in steps of one picture.").color(ACCENT)
            } else {
                RichText::new(format!(
                    "{n} pictures, so each one is {} %: the sliders move in these steps – what you set is exactly what the timeline gets.",
                    fmt_g(round_to(100.0 / n as f64, 2))
                ))
                .color(TEXT_DIM)
                .size(12.0)
            };
            ui.add(egui::Label::new(note).wrap());
            ui.add_space(6.0);
            // the bands: whole pictures; percent is only another way to show them
            let pct = self.share_pct;
            let mut counts = self.jitter.counts(n);
            let top = n.max(counts.iter().copied().max().unwrap_or(0)).max(1);
            let mut changed = false;
            egui::Frame::new().fill(theme::BG).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_enabled_ui(n > 0, |ui| {
                    egui::ScrollArea::horizontal().id_salt("qd_eq_bands").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.spacing_mut().slider_width = 150.0;
                            for (step, c) in self.jitter.steps.iter().zip(counts.iter_mut()) {
                                ui.vertical(|ui| {
                                    ui.set_width(58.0);
                                    ui.vertical_centered(|ui| {
                                        changed |= ui
                                            .add(egui::Slider::new(c, 0..=top).vertical().show_value(false).trailing_fill(true))
                                            .on_hover_text(format!("How many pictures get {}", lab(step.0)))
                                            .changed();
                                        if pct {
                                            // typed percentages snap to whole pictures
                                            let mut p = *c as f64 * 100.0 / n.max(1) as f64;
                                            let r = ui.add_sized(
                                                [54.0, 20.0],
                                                egui::DragValue::new(&mut p).range(0.0..=100.0).speed(100.0 / n.max(1) as f64 / 4.0).max_decimals(1).suffix(" %"),
                                            );
                                            if r.changed() {
                                                *c = (p * n as f64 / 100.0).round() as usize;
                                                changed = true;
                                            }
                                        } else {
                                            changed |= ui.add_sized([54.0, 20.0], egui::DragValue::new(c).range(0..=9999).speed(0.1).suffix("×")).changed();
                                        }
                                        ui.label(RichText::new(lab(step.0)).strong().size(12.5));
                                    });
                                });
                            }
                        });
                    });
                });
            });
            if changed {
                self.jitter.set_counts(&counts);
            }
            ui.add_space(4.0);
            // total: one length for every picture
            let assigned = self.jitter.assigned(n);
            ui.horizontal(|ui| {
                if n > 0 && assigned == n {
                    ui.label(RichText::new(if pct { format!("Total 100 %  ·  all {n} pictures") } else { format!("Total {n} of {n} pictures") }).color(OK));
                } else if n > 0 {
                    ui.label(RichText::new(misfit_text(assigned, n)).color(ACCENT));
                    if ui.button(format!("Fit to {n} pictures")).on_hover_text("Scale the numbers so they add up, keeping the proportions").clicked() {
                        self.jitter.fit(n);
                    }
                }
            });
            // what the pictures get
            let counts = self.jitter.counts(n);
            let parts: Vec<String> = self.jitter.steps.iter().zip(&counts).filter(|(_, c)| **c > 0).map(|(s, c)| format!("{} ×{c}", lab(s.0))).collect();
            let sorted = self.jitter.sorted(n);
            if n > 0 && !sorted.is_empty() {
                let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
                let res = format!("Fixations:  {}   ·   mean {}  ·  together {}", parts.join("  ·  "), lab(mean), fmt_time(sorted.iter().sum(), 1));
                ui.add(egui::Label::new(RichText::new(res).size(12.5)).wrap());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("All equally often").clicked() {
                    let (f, t, k) = (self.jitter.from(), self.jitter.to(), self.jitter.steps.len());
                    self.jitter = Jitter::even(f, t, k);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Done").font(theme::bold(13.5)).color(hex("#1B1B1B"))).fill(ACCENT)).clicked() {
                        close = true;
                    }
                });
            });
        });
        if close || resp.should_close() {
            self.eq_open = false;
            self.list_err = None;
            self.sync_list();
        }
    }
}

/// "6 of 8 pictures have a fixation length – 2 still missing" / "… 2 too many".
fn misfit_text(assigned: usize, n: usize) -> String {
    if assigned < n {
        format!("{assigned} of {n} pictures have a fixation length – {} still missing", n - assigned)
    } else {
        format!("{assigned} fixation lengths for {n} pictures – {} too many", assigned - n)
    }
}

fn whole_trs(v: f64, tr: f64) -> bool {
    let n = v / tr;
    (n - n.round()).abs() < 1e-4
}

/// A time setting shown in seconds or in TRs (stored in seconds). Next to it the value in the
/// other unit – amber when `mark` and it is not a whole number of TRs.
fn time_value(ui: &mut Ui, v: &mut f64, tr_units: bool, tr: f64, range: std::ops::RangeInclusive<f64>, mark: bool) -> egui::Response {
    let resp = if tr_units && tr > 0.0 {
        let mut x = *v / tr;
        let r = ui.add(egui::DragValue::new(&mut x).range(*range.start() / tr..=*range.end() / tr).speed(0.02).suffix(" TR").max_decimals(3));
        if r.changed() {
            *v = round4(x * tr);
        }
        r
    } else {
        ui.add(egui::DragValue::new(v).range(range).speed(0.05).suffix(" s").max_decimals(3))
    };
    if tr > 0.0 {
        let whole = whole_trs(*v, tr);
        let txt = if tr_units { format!("= {} s", fmt_g(round4(*v))) } else { format!("= {} TR", fmt_g(crate::model::round_to(*v / tr, 3))) };
        let col = if mark && !whole { ACCENT } else { TEXT_DIM };
        let l = ui.label(RichText::new(txt).size(12.0).color(col));
        if mark {
            l.on_hover_text(if whole { "A whole number of TRs" } else { "Not a whole number of TRs" });
        }
    }
    resp
}

/// Small bar chart of the ITIs: how often each length comes up.
fn jitter_preview(ui: &mut Ui, sorted: &[f64], unit: f64, unit_name: &str) {
    if sorted.is_empty() {
        ui.label(RichText::new("Add pictures to see the ITIs they get").color(TEXT_DIM).size(12.0));
        return;
    }
    let lo = sorted[0];
    let hi = sorted[sorted.len() - 1];
    // one bar per length – or 20 bins when the values are not rounded
    let mut bars = compact_values(sorted);
    if bars.len() > 30 {
        let k = 20;
        let w = (hi - lo) / k as f64;
        let mut counts = vec![0u32; k];
        for v in sorted {
            counts[(((v - lo) / w) as usize).min(k - 1)] += 1;
        }
        bars = counts.into_iter().enumerate().map(|(i, c)| (lo + (i as f64 + 0.5) * w, c)).collect();
    }
    let size = Vec2::new(300.0, 56.0);
    let (r, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 4, theme::BG);
    let plot = egui::Rect::from_min_max(r.min + Vec2::new(6.0, 6.0), r.max - Vec2::new(6.0, 16.0));
    let max_c = bars.iter().map(|b| b.1).max().unwrap_or(1).max(1) as f32;
    let span = (hi - lo).max(1e-9);
    let bw = (plot.width() / bars.len().max(1) as f32 * 0.8).clamp(2.0, 18.0);
    for (v, c) in &bars {
        let x = if bars.len() == 1 { plot.center().x } else { plot.left() + bw / 2.0 + ((v - lo) / span) as f32 * (plot.width() - bw) };
        let h = (*c as f32 / max_c) * plot.height();
        p.rect_filled(egui::Rect::from_min_max(egui::Pos2::new(x - bw / 2.0, plot.bottom() - h), egui::Pos2::new(x + bw / 2.0, plot.bottom())), 1, theme::alpha(ACCENT, 200));
    }
    let lab = |x: f64| format!("{} {unit_name}", fmt_g(crate::model::round_to(x / unit, 2)));
    p.text(egui::Pos2::new(plot.left(), r.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, lab(lo), theme::font(10.5), TEXT_DIM);
    p.text(egui::Pos2::new(plot.right(), r.bottom() - 3.0), egui::Align2::RIGHT_BOTTOM, lab(hi), theme::font(10.5), TEXT_DIM);
    let tip = bars.iter().map(|(v, c)| format!("{} × {c}", lab(*v))).collect::<Vec<_>>().join("\n");
    resp.on_hover_text(tip);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let total: f64 = sorted.iter().sum();
    ui.vertical(|ui| {
        ui.label(RichText::new(format!("{} ITIs  ·  mean {}", sorted.len(), lab(mean))).size(12.0));
        ui.label(RichText::new(format!("{} – {}  ·  together {}", lab(lo), lab(hi), fmt_time(total, 1))).size(12.0).color(TEXT_DIM));
        ui.label(RichText::new("hover the bars for the counts").size(11.0).color(TEXT_DIM));
    });
}

// =============================================================================
// Run setup
// =============================================================================
/// Settings for the function test (which screen, simulated pulses).
pub struct TestDialog {
    pub s: crate::selftest::TestSettings,
    monitors: Vec<String>,
    sync: String,
}

impl TestDialog {
    pub fn new(hw: &Hardware, tr: f64, background: &str, monitors: Vec<String>) -> Self {
        let sync = if hw.sync_mode == "keyboard" {
            format!("SyncBox via USB keyboard, trigger key “{}”", hw.trigger_key)
        } else {
            format!("SyncBox via serial port {}", if hw.serial_port.is_empty() { "(not set!)" } else { &hw.serial_port })
        };
        let s = crate::selftest::TestSettings {
            screen: crate::monitors::default_participant_screen(&monitors),
            fullscreen: monitors.len() > 1,
            simulate: false,
            tr: if tr > 0.0 { tr } else { 2.0 },
            background: background.to_string(),
            window_size: None,
        };
        TestDialog { s, monitors, sync }
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "function_test", 540.0, |ui| {
            title(ui, "✓  Function test");
            theme::hint(
                ui,
                "Opens a test screen for the participant, mirrored here: every grip button lights up when it is pressed, a \
                 lamp flashes with every scanner pulse (with the measured TR), corner marks show whether the whole screen is \
                 visible, and a drifting logo shows that the display runs smoothly. Nothing is saved.",
            );
            ui.add_space(6.0);
            theme::hint(ui, self.sync.clone());
            ui.add_space(4.0);
            let s = &mut self.s;
            row(ui, "Show on screen", |ui| {
                let cur = self.monitors.get(s.screen).cloned().unwrap_or_else(|| "1: main screen".into());
                egui::ComboBox::from_id_salt("test_screen").width(260.0).selected_text(cur).show_ui(ui, |ui| {
                    for (i, m) in self.monitors.iter().enumerate() {
                        ui.selectable_value(&mut s.screen, i, m);
                    }
                });
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut s.fullscreen, "Full screen");
            });
            if self.monitors.len() < 2 {
                row(ui, "", |ui| {
                    theme::hint(ui, "Only one screen found: leave “Full screen” off to see the mirror next to the test screen.");
                });
            }
            row(ui, "", |ui| {
                ui.checkbox(&mut s.simulate, format!("Simulate scanner pulses every {} s (no scanner running)", fmt_g(s.tr)));
            });
            let mut out = Outcome::Open;
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::colored_button(ui, "Start test", OK).clicked() {
                    out = Outcome::Accept;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Cancel;
                }
            });
            out
        })
    }
}

/// The run's data files exist already: overwrite them, add today's date to the new names, or go back.
pub struct FileClash {
    pub s: RunSettings,
    pub back: RunDialog,
    /// the suffix "Add today's date" uses
    pub dated: String,
    existing: Vec<String>,
    dir: String,
    who: String,
    example: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ClashChoice {
    Open,
    Cancel,
    Overwrite,
    AddDate,
}

impl FileClash {
    pub fn new(par: &Paradigm, s: RunSettings, back: RunDialog, existing: Vec<std::path::PathBuf>, dated: String) -> Self {
        let names = existing.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).collect();
        let who = format!(
            "participant {}{}, run {} of “{}”",
            if s.sub.is_empty() { "01" } else { &s.sub },
            if s.ses.is_empty() { String::new() } else { format!(", session {}", s.ses) },
            s.run,
            par.name
        );
        let dated_settings = RunSettings { name_suffix: dated.clone(), ..s.clone() };
        let example = format!("{}_events.tsv", crate::runner::output_stem(par, &dated_settings));
        FileClash { dir: s.out_dir.clone(), s, back, dated, existing: names, who, example }
    }

    pub fn ui(&mut self, ctx: &Context) -> ClashChoice {
        let mut choice = ClashChoice::Open;
        let out = modal(ctx, "file_clash", 560.0, |ui| {
            title(ui, "⚠  These data files already exist");
            ui.add_space(4.0);
            ui.add(egui::Label::new(format!("The data of {} has already been saved in", self.who)).wrap());
            ui.label(RichText::new(&self.dir).color(TEXT_DIM).size(12.0));
            ui.add_space(4.0);
            egui::Frame::new().fill(theme::BG).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for n in self.existing.iter().take(8) {
                    ui.label(RichText::new(n).monospace().size(12.0));
                }
                if self.existing.len() > 8 {
                    ui.label(RichText::new(format!("… and {} more", self.existing.len() - 8)).color(TEXT_DIM));
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("What should happen?").strong());
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Add today's date – the new files are named e.g.  {}  and the old ones stay as they are.", self.example))
                        .color(TEXT_DIM)
                        .size(12.5),
                )
                .wrap(),
            );
            ui.add(
                egui::Label::new(RichText::new("Overwrite – the existing files are replaced by the new run's data.").color(TEXT_DIM).size(12.5))
                    .wrap(),
            );
            ui.add(
                egui::Label::new(RichText::new("Cancel – back to the run settings (e.g. to change the run number).").color(TEXT_DIM).size(12.5))
                    .wrap(),
            );
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::colored_button(ui, "Add today's date", OK).on_hover_text(&self.example).clicked() {
                    choice = ClashChoice::AddDate;
                }
                if ui.button(RichText::new("Overwrite").color(DANGER)).on_hover_text("The existing files are lost").clicked() {
                    choice = ClashChoice::Overwrite;
                }
                if ui.button("Cancel").clicked() {
                    choice = ClashChoice::Cancel;
                }
            });
            Outcome::Open
        });
        if out == Outcome::Cancel {
            choice = ClashChoice::Cancel; // Esc
        }
        choice
    }
}

#[derive(Clone)]
pub struct RunDialog {
    pub s: RunSettings,
    monitors: Vec<String>,
    summary: String,
    tr: f64,
}

impl RunDialog {
    pub fn new(par: &Paradigm, out_dir: &str, monitors: Vec<String>) -> Self {
        let hw = &par.hardware;
        let sync = if hw.sync_mode == "keyboard" {
            format!("SyncBox via USB keyboard, trigger key “{}”", hw.trigger_key)
        } else {
            format!("SyncBox via serial port {}", if hw.serial_port.is_empty() { "(not set!)" } else { &hw.serial_port })
        };
        let mut summary = format!("{}  ·  {}  ·  {} volumes\n{sync}", par.name, fmt_time(par.duration(), 1), par.volumes());
        if par.blocks.iter().any(|b| b.sound_file().is_some()) {
            summary.push_str(&match hw.audio_device.as_str() {
                "" => "\nSound: off – audio blocks and videos stay silent (Hardware → Audio)".to_string(),
                crate::media::DEFAULT_OUTPUT => "\nSound: system default output".to_string(),
                _ => format!("\nSound: {}", if hw.audio_device_name.is_empty() { &hw.audio_device } else { &hw.audio_device_name }),
            });
        }
        let s = RunSettings {
            out_dir: out_dir.to_string(),
            // the projector: the first screen that is not the main screen
            screen: crate::monitors::default_participant_screen(&monitors),
            dummy_scans: par.dummy_scans,
            waiting_screen: par.waiting_screen.clone(),
            waiting_text: par.waiting_text.clone(),
            end_text: par.end_text.clone(),
            ..Default::default()
        };
        RunDialog { s, monitors, summary, tr: par.tr }
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "run", 560.0, |ui| {
            title(ui, "▶  Run in the scanner");
            theme::hint(ui, self.summary.clone());
            ui.add_space(6.0);
            let s = &mut self.s;
            row(ui, "Participant", |ui| {
                ui.text_edit_singleline(&mut s.sub);
            });
            row(ui, "Session", |ui| {
                ui.add(egui::TextEdit::singleline(&mut s.ses).hint_text("optional"));
            });
            row(ui, "Run", |ui| {
                ui.add(egui::DragValue::new(&mut s.run).range(1..=99));
            });
            row(ui, "Save logs to", |ui| {
                ui.add(egui::TextEdit::singleline(&mut s.out_dir).desired_width(260.0));
                if ui.button("Browse…").clicked() {
                    if let Some(d) = pick_folder("Save logs to", &s.out_dir) {
                        s.out_dir = d;
                    }
                }
            });
            row(ui, "Show on screen", |ui| {
                let cur = self.monitors.get(s.screen).cloned().unwrap_or_else(|| "1: main screen".into());
                egui::ComboBox::from_id_salt("screen").width(260.0).selected_text(cur).show_ui(ui, |ui| {
                    for (i, m) in self.monitors.iter().enumerate() {
                        ui.selectable_value(&mut s.screen, i, m);
                    }
                });
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut s.fullscreen, "Full screen");
            });
            if self.monitors.len() < 2 {
                row(ui, "", |ui| {
                    theme::hint(ui, "Only one screen found: the participant window covers the control room. Untick “Full screen” for a test run.");
                });
            }
            row(ui, "", |ui| {
                ui.checkbox(&mut s.simulate, "Simulate scanner (no SyncBox connected – for testing)");
            });
            section(ui, "Scanner start");
            row(ui, "Dummy scans", |ui| {
                ui.add(egui::DragValue::new(&mut s.dummy_scans).range(0..=50).suffix(" volumes"))
                    .on_hover_text("Scanner triggers to skip before the paradigm starts (T1 saturation).");
            });
            let n = s.dummy_scans;
            let info = if n > 0 {
                format!("The paradigm starts with trigger {}  ·  {} s of dummy scans", n + 1, fmt_g(n as f64 * self.tr))
            } else {
                "The paradigm starts with trigger 1  ·  on the very first trigger".into()
            };
            row(ui, "", |ui| {
                theme::hint(ui, info);
            });
            row(ui, "Participant sees until start", |ui| {
                let items = [("fixation", "Fixation cross"), ("blank", "Empty screen"), ("text", "Text…")];
                let cur = items.iter().find(|i| i.0 == s.waiting_screen).map(|i| i.1).unwrap_or("Fixation cross");
                egui::ComboBox::from_id_salt("waitmode").selected_text(cur).show_ui(ui, |ui| {
                    for (k, t) in items {
                        ui.selectable_value(&mut s.waiting_screen, k.to_string(), t);
                    }
                });
            });
            if s.waiting_screen == "text" {
                row(ui, "", |ui| {
                    ui.text_edit_singleline(&mut s.waiting_text);
                });
            }
            row(ui, "…and after the run", |ui| {
                ui.add(egui::TextEdit::singleline(&mut s.end_text).hint_text("empty screen"));
            });
            ui.add_space(4.0);
            theme::hint(
                ui,
                "Scanner status, dummy-scan count, progress and answers are shown only in the control room – never on the \
                 participant's screen. Esc twice (or the Abort button) stops the run; data so far is saved.",
            );
            let mut out = Outcome::Open;
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::colored_button(ui, "Start – wait for scanner", OK).clicked() {
                    out = Outcome::Accept;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Cancel;
                }
            });
            out
        })
    }
}

// =============================================================================
// Paradigm templates
// =============================================================================
pub struct TemplateDialog {
    key: String,
    opts: Opts,
    built_for: Option<(String, Opts)>,
    pub result: Result<Paradigm, String>,
    stim_dir: PathBuf,
    names: Vec<String>,
}

impl TemplateDialog {
    pub fn new(hw: &Hardware, stim_dir: PathBuf, start: &str) -> Self {
        let t = templates::template(start).unwrap_or(&TEMPLATES[0]);
        TemplateDialog {
            key: t.key.into(),
            opts: (t.defaults)(),
            built_for: None,
            result: Err(String::new()),
            stim_dir,
            names: hw.button_names(),
        }
    }

    fn rebuild(&mut self) {
        let cur = (self.key.clone(), self.opts.clone());
        if self.built_for.as_ref() == Some(&cur) {
            return;
        }
        self.result = templates::build(&self.key, &self.opts, &self.stim_dir);
        self.built_for = Some(cur);
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        self.rebuild();
        modal(ctx, "templates", 980.0, |ui| {
            title(ui, "Paradigm templates");
            theme::hint(
                ui,
                "Pick a classic task, adjust the numbers, and get a complete, editable timeline. Without your own stimulus \
                 folders, placeholder pictures are generated – replace them with a validated set for real studies.",
            );
            ui.add_space(6.0);
            let h = 540.0;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(300.0, h), Layout::top_down(Align::Min), |ui| {
                    egui::ScrollArea::vertical().id_salt("tpl_list").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_width(286.0);
                        for cat in CATEGORIES {
                            ui.add_space(6.0);
                            theme::panel_title(ui, cat);
                            for t in TEMPLATES.iter().filter(|t| t.category == *cat) {
                                let sel = t.key == self.key;
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(280.0, 56.0), egui::Sense::click());
                                let p = ui.painter();
                                p.rect(
                                    r,
                                    egui::CornerRadius::same(8),
                                    if sel { hex("#2B303B") } else { PANEL_2 },
                                    egui::Stroke::new(1.0, if sel { hex(t.color) } else { BORDER }),
                                    egui::StrokeKind::Inside,
                                );
                                theme::paint_kind_icon(p, t.icon_kind, egui::Rect::from_min_size(r.min + Vec2::new(10.0, 10.0), Vec2::splat(36.0)), hex(t.color));
                                p.text(r.min + Vec2::new(56.0, 19.0), egui::Align2::LEFT_CENTER, t.title, theme::bold(14.0), TEXT);
                                p.text(r.min + Vec2::new(56.0, 38.0), egui::Align2::LEFT_CENTER, t.subtitle, theme::font(11.0), TEXT_DIM);
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !sel {
                                    self.key = t.key.into();
                                    self.opts = (t.defaults)();
                                }
                                ui.add_space(4.0);
                            }
                        }
                    });
                });
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(620.0);
                    ui.set_height(h - 20.0);
                    ui.vertical(|ui| {
                    let t = templates::template(&self.key).unwrap();
                    ui.label(RichText::new(t.title).font(theme::bold(22.0)));
                    ui.add(egui::Label::new(RichText::new(t.description).size(13.0)).wrap());
                    theme::hint(ui, format!("Reference: {}", t.reference));
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().id_salt("tpl_form").auto_shrink([false, false]).show(ui, |ui| {
                        self.form(ui);
                    });
                    });
                });
            });
            ui.add_space(6.0);
            let (summary, color, ok) = match &self.result {
                Ok(par) => {
                    let conds: Vec<String> =
                        par.conditions().into_iter().filter(|c| !["instruction", "cue", "rating"].contains(&c.as_str())).collect();
                    (
                        format!(
                            "→ {} min  ·  {} volumes at TR {}s  ·  {} blocks  ·  conditions: {}",
                            fmt_time(par.duration(), 1),
                            par.volumes(),
                            fmt_g(par.tr),
                            par.blocks.len(),
                            conds.join(", ")
                        ),
                        ACCENT,
                        true,
                    )
                }
                Err(e) => (format!("⚠ {e}"), DANGER, false),
            };
            let mut out = Outcome::Open;
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(summary).color(color).font(theme::bold(13.0))).wrap());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                out = buttons(ui, "Create paradigm", ok);
            });
            out
        })
    }

    fn form(&mut self, ui: &mut Ui) {
        let names = self.names.clone();
        for (name, val) in self.opts.0.iter_mut() {
            let (label, unit) = templates::opt_label(name);
            let label = if label.is_empty() { *name } else { label };
            let tip = templates::opt_tip(name);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(200.0, 24.0), Layout::right_to_left(Align::Center), |ui| {
                    ui.label(label);
                });
                let resp = if name.ends_with("_dir") {
                    let OptVal::S(s) = val else { return };
                    let r = ui.add(egui::TextEdit::singleline(s).hint_text("built-in placeholders").desired_width(280.0));
                    if ui.button("Browse…").clicked() {
                        if let Some(d) = pick_folder("Choose folder", s) {
                            *s = d;
                        }
                    }
                    r
                } else if *name == "button" || name.ends_with("_button") {
                    let OptVal::S(s) = val else { return };
                    let mut items: Vec<(String, String)> = vec![];
                    if *name == "button" {
                        items.push(("any".into(), "Any grip button".into()));
                    }
                    for n in &names {
                        items.push((n.clone(), n.clone()));
                    }
                    if !items.iter().any(|i| i.0 == *s) {
                        items.push((s.clone(), s.clone())); // a default name this hardware setup does not have
                    }
                    let cur = items.iter().find(|i| i.0 == *s).map(|i| i.1.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt(format!("opt_{name}"))
                        .width(220.0)
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            for (v, t) in &items {
                                ui.selectable_value(s, v.clone(), t);
                            }
                        })
                        .response
                } else {
                    match val {
                        OptVal::B(b) => ui.checkbox(b, ""),
                        OptVal::I(i) => ui.add(egui::DragValue::new(i).range(0..=10000).suffix(unit)),
                        OptVal::F(f) => {
                            let speed = if unit == " s" { 0.05 } else { 0.1 };
                            ui.add(egui::DragValue::new(f).range(0.0..=10000.0).speed(speed).suffix(unit).max_decimals(2))
                        }
                        OptVal::S(s) => ui.add(egui::TextEdit::singleline(s).desired_width(280.0)),
                    }
                };
                if let Some(t) = tip {
                    resp.on_hover_text(t);
                }
            });
        }
    }
}

// =============================================================================
// Find and replace text in all blocks
// =============================================================================
pub struct FindReplace {
    find: String,
    repl: String,
    case: bool,
    only_sel: bool,
    fields: Vec<(String, bool)>,
    done: String,
}

impl FindReplace {
    pub fn new() -> Self {
        FindReplace {
            find: String::new(),
            repl: String::new(),
            case: false,
            only_sel: false,
            fields: TEXT_FIELDS.iter().map(|(k, _)| (k.to_string(), true)).collect(),
            done: String::new(),
        }
    }

    pub fn ui(&mut self, ctx: &Context, doc: &mut Document) -> Outcome {
        modal(ctx, "find", 640.0, |ui| {
            title(ui, "Find and replace");
            theme::hint(ui, "Change a word or sentence in all instructions, rating questions and labels at once. One Undo (Ctrl+Z) reverts everything.");
            ui.add_space(4.0);
            let mut changed = false;
            row(ui, "Find", |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("text to find").desired_width(360.0)).changed();
            });
            row(ui, "Replace with", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.repl).hint_text("replace with (empty = delete)").desired_width(360.0));
            });
            row(ui, "", |ui| {
                changed |= ui.checkbox(&mut self.case, "Match upper/lower case").changed();
            });
            if !doc.selection.is_empty() {
                row(ui, "Search in", |ui| {
                    changed |= ui.radio_value(&mut self.only_sel, false, "All blocks").changed();
                    changed |= ui.radio_value(&mut self.only_sel, true, format!("Only the {} selected blocks", doc.selection.len())).changed();
                });
            } else {
                self.only_sel = false;
            }
            row(ui, "Fields", |ui| {
                for (k, on) in self.fields.iter_mut() {
                    let t = text_field_title(k).replace("Instruction / Go-No-Go text", "Texts").replace("Rating ", "");
                    changed |= ui.checkbox(on, t).on_hover_text(text_field_title(k)).changed();
                }
            });
            if changed {
                self.done.clear();
            }
            let fields: Vec<String> = self.fields.iter().filter(|f| f.1).map(|f| f.0.clone()).collect();
            let ids: Option<HashSet<String>> = self.only_sel.then(|| doc.selection.iter().cloned().collect());
            let hits = find_text(&doc.paradigm, &self.find, self.case, &fields, ids.as_ref());
            let msg = if !self.done.is_empty() {
                self.done.clone()
            } else if self.find.is_empty() {
                "Type something to find.".into()
            } else {
                format!("{} field(s) contain “{}”", hits.len(), self.find)
            };
            ui.label(RichText::new(msg).color(ACCENT).font(theme::bold(13.0)));
            egui::Frame::new().fill(theme::BG).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("hits").striped(true).num_columns(3).show(ui, |ui| {
                        for h in ["Time", "Field", "Text"] {
                            ui.label(RichText::new(h).color(TEXT_DIM));
                        }
                        ui.end_row();
                        for (id, f, val) in &hits {
                            let t = doc.paradigm.block(id).map(|b| fmt_time(b.start, 1)).unwrap_or_default();
                            ui.label(t);
                            ui.label(text_field_title(f));
                            ui.add(egui::Label::new(val.replace('\n', " ⏎ ")).truncate());
                            ui.end_row();
                        }
                    });
                });
            });
            let mut out = Outcome::Open;
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(!hits.is_empty(), egui::Button::new(RichText::new("Replace all").font(theme::bold(13.5)).color(hex("#1B1B1B"))).fill(ACCENT)).clicked() {
                    let (needle, repl, case) = (self.find.clone(), self.repl.clone(), self.case);
                    let n = doc.edit(|par| replace_text(par, &needle, &repl, case, &fields, ids.as_ref()));
                    self.done = format!("✓ Replaced in {n} field(s).  (Ctrl+Z in the editor undoes it)");
                }
                if ui.button("Close").clicked() {
                    out = Outcome::Cancel;
                }
            });
            out
        })
    }
}

// =============================================================================
// Preferences
// =============================================================================
pub struct PrefsDialog {
    pub p: Prefs,
    tab: usize,
    pub reset: bool,
}

impl PrefsDialog {
    pub fn new(p: &Prefs) -> Self {
        PrefsDialog { p: p.clone(), tab: 0, reset: false }
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "prefs", 620.0, |ui| {
            title(ui, "Preferences");
            theme::hint(
                ui,
                "Saved for your user account on this computer. Settings for “new paradigms” and “new blocks” do not change \
                 what is already in your timeline – use multi-select in the inspector for that.",
            );
            ui.horizontal(|ui| {
                for (i, t) in ["General", "Paradigms & timeline", "Run & control room"].iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *t).clicked() {
                        self.tab = i;
                    }
                }
            });
            ui.separator();
            let p = &mut self.p;
            ui.allocate_ui(Vec2::new(620.0, 330.0), |ui| {
                ui.set_min_height(330.0);
                match self.tab {
                    0 => {
                        section(ui, "Appearance");
                        row(ui, "Text size", |ui| {
                            let items = [(90u32, "Small (90 %)"), (100, "Normal (100 %)"), (115, "Large (115 %)"), (130, "Extra large (130 %)")];
                            let cur = items.iter().find(|i| i.0 == p.ui_scale).map(|i| i.1).unwrap_or("Normal (100 %)");
                            egui::ComboBox::from_id_salt("ui_scale").selected_text(cur).show_ui(ui, |ui| {
                                for (v, t) in items {
                                    ui.selectable_value(&mut p.ui_scale, v, t);
                                }
                            });
                        });
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.show_hints, "Show usage hints above the timeline");
                        });
                        section(ui, "Unpleasant pictures");
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.blur_previews, "Blur picture previews in the editor (timeline, media bin, inspector, preview)");
                        });
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.blur_control_mirror, "Blur pictures in the control-room mirror");
                        });
                        row(ui, "", |ui| {
                            theme::hint(ui, "The participant's screen always shows the real pictures.");
                        });
                        section(ui, "Files");
                        row(ui, "Autosave", |ui| {
                            ui.add(egui::DragValue::new(&mut p.autosave_minutes).range(0..=60).suffix(" min  (0 = off)"));
                        });
                    }
                    1 => {
                        section(ui, "New paradigms");
                        row(ui, "TR", |ui| {
                            ui.add(egui::DragValue::new(&mut p.new_tr).range(0.1..=20.0).speed(0.01).suffix(" s").max_decimals(3));
                        });
                        row(ui, "Dummy scans", |ui| {
                            ui.add(egui::DragValue::new(&mut p.new_dummy_scans).range(0..=50).suffix(" volumes"));
                        });
                        row(ui, "Background", |ui| {
                            let mut c = hex(&p.new_background);
                            if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                                p.new_background = theme::to_hex(c);
                            }
                            ui.label(RichText::new(p.new_background.clone()).color(TEXT_DIM));
                        });
                        row(ui, "Before start", |ui| {
                            let items = [("fixation", "Fixation cross"), ("blank", "Empty screen"), ("text", "Text")];
                            let cur = items.iter().find(|i| i.0 == p.new_waiting_screen).map(|i| i.1).unwrap_or("Fixation cross");
                            egui::ComboBox::from_id_salt("nws").selected_text(cur).show_ui(ui, |ui| {
                                for (k, t) in items {
                                    ui.selectable_value(&mut p.new_waiting_screen, k.to_string(), t);
                                }
                            });
                        });
                        row(ui, "After the run", |ui| {
                            ui.text_edit_singleline(&mut p.new_end_text);
                        });
                        section(ui, "New blocks");
                        row(ui, "New ratings answered with", |ui| {
                            ui.radio_value(&mut p.rating_mode, "cursor".to_string(), "Cursor ◀ ▶ + confirm");
                            ui.radio_value(&mut p.rating_mode, "direct".to_string(), "One button per value");
                        });
                        row(ui, "Matching trials", |ui| {
                            ui.checkbox(&mut p.match_frame, "Frame the picture the participant chose");
                        });
                        section(ui, "Timeline");
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.snap, "Snap to the TR grid and block edges");
                        });
                        row(ui, "Snap distance", |ui| {
                            ui.add(egui::DragValue::new(&mut p.snap_px).range(2..=40).suffix(" px"));
                        });
                    }
                    _ => {
                        section(ui, "Data");
                        row(ui, "Default data folder", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut p.data_dir).desired_width(260.0));
                            if ui.button("Browse…").clicked() {
                                if let Some(d) = pick_folder("Default data folder", &p.data_dir) {
                                    p.data_dir = d;
                                }
                            }
                        });
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.save_paradigm_with_data, "Save a copy of the paradigm next to each run's logs");
                        });
                        section(ui, "Safety");
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.confirm_abort, "Aborting needs two clicks / two Esc presses");
                        });
                        section(ui, "Control room");
                        row(ui, "Warn after", |ui| {
                            ui.add(egui::DragValue::new(&mut p.watchdog_trials).range(0..=50).suffix(" trials without answer (0 = off)"));
                        });
                        row(ui, "Trigger warning after", |ui| {
                            ui.add(egui::DragValue::new(&mut p.trigger_warning_trs).range(1.0..=20.0).speed(0.1).suffix(" TR").max_decimals(1));
                        });
                        row(ui, "Refresh every", |ui| {
                            ui.add(egui::DragValue::new(&mut p.control_refresh_ms).range(50..=1000).suffix(" ms"));
                        });
                    }
                }
            });
            let mut out = Outcome::Open;
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Restore defaults").clicked() {
                    self.p = Prefs::default();
                    self.reset = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    out = buttons(ui, "OK", true);
                });
            });
            out
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick(media: &[&str]) -> QuickDesign {
        let par = Paradigm { tr: 2.0, ..Default::default() };
        let media: Vec<String> = media.iter().map(|s| s.to_string()).collect();
        QuickDesign::new(&par, &media, false)
    }

    fn names(q: &QuickDesign, c: usize) -> Vec<&str> {
        q.conds[c].images.iter().map(|s| s.as_str()).collect()
    }

    /// Sorting pictures into conditions: select, move, remove, add.
    #[test]
    fn quick_design_sorts_pictures_into_conditions() {
        let mut q = quick(&["n1.png", "n2.png", "p1.png", "p2.png", "p3.png"]);
        assert_eq!(q.conds.len(), 1);
        assert_eq!(names(&q, 0).len(), 5);
        // click p1, shift-click p3 → three selected; move them to a new condition
        q.click_picture(2, egui::Modifiers::NONE);
        q.click_picture(4, egui::Modifiers::SHIFT);
        assert_eq!(q.picked.iter().copied().collect::<Vec<_>>(), [2, 3, 4]);
        q.apply(QdAct::Move { from: 0, pics: q.picked.iter().copied().collect(), to: None });
        assert_eq!(names(&q, 0), ["n1.png", "n2.png"]);
        assert_eq!(names(&q, 1), ["p1.png", "p2.png", "p3.png"]);
        assert!(q.picked.is_empty());
        // ctrl-click toggles; move one back by dropping it on the first condition
        q.select(1);
        q.click_picture(0, egui::Modifiers::COMMAND);
        q.click_picture(2, egui::Modifiers::COMMAND);
        q.click_picture(0, egui::Modifiers::COMMAND);
        assert_eq!(q.picked.iter().copied().collect::<Vec<_>>(), [2]);
        q.apply(QdAct::Move { from: 1, pics: vec![2], to: Some(0) });
        assert_eq!(names(&q, 0), ["n1.png", "n2.png", "p3.png"]);
        // moving onto itself or a text condition does nothing
        let t = q.add_condition("text");
        q.apply(QdAct::Move { from: 0, pics: vec![0], to: Some(t) });
        q.apply(QdAct::Move { from: 0, pics: vec![0], to: Some(0) });
        assert_eq!(names(&q, 0).len(), 3);
        // remove pictures; adding while a text condition is shown makes a new picture condition
        q.select(0);
        q.apply(QdAct::RemovePictures(vec![2, 0]));
        assert_eq!(names(&q, 0), ["n2.png"]);
        q.select(t);
        q.add_pictures(vec!["x.png".into(), "notes.txt".into(), "x.png".into()]);
        assert_eq!(q.conds.len(), 4);
        assert_eq!(q.sel, 3);
        assert_eq!(names(&q, 3), ["x.png"]);
        // names stay unique
        let mut all: Vec<&str> = q.conds.iter().map(|c| c.name.as_str()).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), q.conds.len());
        // removing a condition keeps a valid selection
        q.remove_condition(3);
        assert!(q.sel < q.conds.len());
    }

    /// Event-related uses the picture conditions, the block design also the text ones; empty
    /// picture conditions are left out of both.
    #[test]
    fn quick_design_uses_the_right_conditions() {
        let mut q = quick(&["a.png", "b.png"]);
        q.add_condition("image"); // empty
        q.add_condition("text");
        q.tab = 1;
        let used: Vec<String> = q.used_conditions().into_iter().map(|c| c.name).collect();
        assert_eq!(used, ["pictures"]);
        let ev = q.blocks(Some(1));
        assert_eq!(ev.iter().filter(|b| b.condition == "pictures").count(), 2);
        q.tab = 0;
        assert_eq!(q.used_conditions().len(), 2);
        let bd = q.blocks(Some(1));
        assert!(bd.iter().any(|b| b.kind == "text" && b.params.get("text").and_then(|v| v.as_str()) == Some("TASK3")));
        // nothing usable → no timeline
        let mut e = quick(&[]);
        e.tab = 1;
        assert!(e.blocks(None).is_empty());
    }
}
