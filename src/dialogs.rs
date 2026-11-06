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
    find_text, fmt_g, fmt_time, generate_block_design, generate_event_design, replace_text, text_field_title, DesignCondition,
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
     F5\trun"
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
        };
        d.refresh_ports(true);
        d.restart_router(false);
        d
    }

    fn refresh_ports(&mut self, initial: bool) {
        self.ports = describe_ports();
        if self.ports.is_empty() {
            self.ser_note = "No serial ports found. Is the SyncBox connected by USB and switched on? \
                             (In USB keyboard mode it does not show up as a serial port.)"
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
        if let Some(r) = self.router.as_mut() {
            for ev in r.poll() {
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
                    if self.hw.sync_mode == "keyboard" || self.hw.button_for_key(&text).is_some() {
                        if let Some(r) = self.router.as_mut() {
                            r.feed(&text, now());
                        }
                    }
                }
            }
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
        let out = modal(ctx, "hardware", 680.0, |ui| {
            title(ui, "Scanner hardware – NNL SyncBox & ResponseGrips");
            ui.horizontal(|ui| {
                for (i, t) in ["SyncBox", "ResponseGrips"].iter().enumerate() {
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
                } else {
                    self.grips_tab(ui, &lamp_level);
                }
            });
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
pub struct QuickDesign {
    tab: usize,
    conds: Vec<DesignCondition>,
    sel: Option<usize>,
    bd_block: f64,
    bd_rest: f64,
    bd_reps: u32,
    bd_random: bool,
    bd_lead: f64,
    ev_images: Vec<String>,
    ev_cond: String,
    ev_dur: f64,
    ev_min: f64,
    ev_max: f64,
    ev_rating: bool,
    ev_shuffle: bool,
    pub replace: bool,
    tr: f64,
}

impl QuickDesign {
    pub fn new(par: &Paradigm, media: &[String]) -> Self {
        let tr = if par.tr > 0.0 { par.tr } else { 1.0 };
        let mut conds = vec![];
        if !media.is_empty() {
            conds.push(DesignCondition { name: "pictures".into(), kind: "image".into(), images: media.to_vec() });
        }
        QuickDesign {
            tab: 0,
            conds,
            sel: None,
            bd_block: tr * 10.0,
            bd_rest: tr * 10.0,
            bd_reps: 4,
            bd_random: false,
            bd_lead: tr * 5.0,
            ev_images: media.to_vec(),
            ev_cond: "picture".into(),
            ev_dur: 4.0,
            ev_min: 2.0,
            ev_max: 6.0,
            ev_rating: true,
            ev_shuffle: true,
            replace: par.blocks.is_empty(),
            tr: par.tr,
        }
    }

    pub fn blocks(&self, seed: Option<u64>) -> Vec<crate::model::Block> {
        if self.tab == 0 {
            if self.conds.is_empty() {
                return vec![];
            }
            let mut blocks =
                generate_block_design(&self.conds, self.bd_block, self.bd_rest, self.bd_reps, self.bd_random, 0.0, self.bd_lead, seed);
            for b in blocks.iter_mut() {
                if b.kind == "text" && !b.condition.is_empty() {
                    b.params.insert("text".into(), serde_json::json!(b.condition.to_uppercase()));
                }
            }
            return blocks;
        }
        if self.ev_images.is_empty() {
            return vec![];
        }
        let (lo, hi) = if self.ev_min <= self.ev_max { (self.ev_min, self.ev_max) } else { (self.ev_max, self.ev_min) };
        generate_event_design(&self.ev_images, self.ev_dur, lo, hi, &self.ev_cond, self.ev_rating, self.ev_shuffle, 0.0, self.tr, seed)
    }

    pub fn ui(&mut self, ctx: &Context) -> Outcome {
        modal(ctx, "quick", 720.0, |ui| {
            title(ui, "✦ Quick design");
            theme::hint(ui, "Pick a template, fill in a few numbers, and the whole timeline is built for you. You can fine-tune every block afterwards.");
            ui.horizontal(|ui| {
                for (i, t) in ["Block design  (ON / OFF)", "Event-related  (single pictures)"].iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *t).clicked() {
                        self.tab = i;
                    }
                }
            });
            ui.separator();
            ui.allocate_ui(Vec2::new(720.0, 360.0), |ui| {
                ui.set_min_height(360.0);
                if self.tab == 0 {
                    self.block_tab(ui);
                } else {
                    self.event_tab(ui);
                }
            });
            let blocks = self.blocks(Some(1));
            let summary = if blocks.is_empty() {
                "Add at least one condition / picture to build a timeline.".to_string()
            } else {
                let dur = blocks.iter().map(|b| b.end()).fold(0.0, f64::max);
                let vols = if self.tr > 0.0 { (dur / self.tr - 1e-9).ceil() as i64 } else { 0 };
                format!("→ {} blocks  ·  {} min  ·  {vols} volumes at TR {}s", blocks.len(), fmt_time(dur, 1), fmt_g(self.tr))
            };
            ui.label(RichText::new(summary).color(ACCENT).font(theme::bold(13.0)));
            let mut out = Outcome::Open;
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.replace, "Replace the current timeline");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    out = buttons(ui, "Build timeline", !blocks.is_empty());
                });
            });
            out
        })
    }

    fn block_tab(&mut self, ui: &mut Ui) {
        egui::Frame::new().fill(theme::BG).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(130.0);
            egui::Grid::new("conds").num_columns(3).striped(true).min_col_width(140.0).show(ui, |ui| {
                ui.label(RichText::new("Condition").color(TEXT_DIM));
                ui.label(RichText::new("Shows").color(TEXT_DIM));
                ui.label(RichText::new("Pictures").color(TEXT_DIM));
                ui.end_row();
                for (i, c) in self.conds.iter_mut().enumerate() {
                    let r = ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(180.0));
                    if r.gained_focus() {
                        self.sel = Some(i);
                    }
                    let sel = self.sel == Some(i);
                    if ui.selectable_label(sel, if c.images.is_empty() { "Instruction text" } else { "Pictures" }).clicked() {
                        self.sel = Some(i);
                    }
                    ui.label(if c.images.is_empty() { "–".to_string() } else { format!("{} pictures", c.images.len()) });
                    ui.end_row();
                }
            });
        });
        ui.horizontal(|ui| {
            if ui.button("+ Condition with pictures…").clicked() {
                let files = crate::panels::pick_images("Choose pictures");
                if !files.is_empty() {
                    let n = self.conds.len() + 1;
                    self.conds.push(DesignCondition { name: format!("cond{n}"), kind: "image".into(), images: files });
                }
            }
            if ui.button("+ Text / task condition").clicked() {
                let n = self.conds.len() + 1;
                self.conds.push(DesignCondition { name: format!("task{n}"), kind: "text".into(), images: vec![] });
            }
            if ui.add_enabled(self.sel.is_some(), egui::Button::new("Remove")).clicked() {
                if let Some(i) = self.sel.take() {
                    if i < self.conds.len() {
                        self.conds.remove(i);
                    }
                }
            }
        });
        ui.add_space(6.0);
        let step = if self.tr > 0.0 { self.tr } else { 0.5 };
        row(ui, "Block length", |ui| {
            ui.add(egui::DragValue::new(&mut self.bd_block).range(0.5..=600.0).speed(step / 10.0).suffix(" s").max_decimals(2));
        });
        row(ui, "Rest between blocks", |ui| {
            ui.add(egui::DragValue::new(&mut self.bd_rest).range(0.0..=600.0).speed(step / 10.0).suffix(" s").max_decimals(2))
                .on_hover_text("Fixation between blocks. 0 = no rest.");
        });
        row(ui, "Repetitions", |ui| {
            ui.add(egui::DragValue::new(&mut self.bd_reps).range(1..=100).suffix(" cycles"));
        });
        row(ui, "Order", |ui| {
            ui.radio_value(&mut self.bd_random, false, "Alternating  A B A B …");
            ui.radio_value(&mut self.bd_random, true, "Randomised per cycle");
        });
        row(ui, "Baseline at start", |ui| {
            ui.add(egui::DragValue::new(&mut self.bd_lead).range(0.0..=600.0).speed(step / 10.0).suffix(" s").max_decimals(2))
                .on_hover_text("Baseline fixation before the first block");
        });
    }

    fn event_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let n = self.ev_images.len();
            ui.label(if n > 0 {
                format!("{n} pictures selected")
            } else {
                "No pictures yet – choose some, or import them into the Pictures bin first.".into()
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Choose pictures…").clicked() {
                    let files = crate::panels::pick_images("Choose pictures");
                    if !files.is_empty() {
                        self.ev_images = files;
                    }
                }
            });
        });
        ui.add_space(6.0);
        row(ui, "Condition name", |ui| {
            ui.text_edit_singleline(&mut self.ev_cond);
        });
        row(ui, "Picture on screen", |ui| {
            ui.add(egui::DragValue::new(&mut self.ev_dur).range(0.1..=120.0).speed(0.05).suffix(" s").max_decimals(2));
        });
        row(ui, "Fixation between (jitter)", |ui| {
            ui.add(egui::DragValue::new(&mut self.ev_min).range(0.0..=120.0).speed(0.05).suffix(" s").max_decimals(2));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut self.ev_max).range(0.0..=120.0).speed(0.05).suffix(" s").max_decimals(2));
        });
        row(ui, "", |ui| {
            ui.checkbox(&mut self.ev_rating, "Participant rates each picture with the grips");
        });
        row(ui, "", |ui| {
            ui.checkbox(&mut self.ev_shuffle, "Shuffle picture order");
        });
    }
}

// =============================================================================
// Run setup
// =============================================================================
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
        let summary = format!("{}  ·  {}  ·  {} volumes\n{sync}", par.name, fmt_time(par.duration(), 1), par.volumes());
        let s = RunSettings {
            out_dir: out_dir.to_string(),
            screen: if monitors.len() > 1 { 1 } else { 0 }, // the projector is usually the 2nd screen
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
                            ui.checkbox(&mut p.blur_previews, "Blur picture previews in the editor (timeline, picture bin, inspector, preview)");
                        });
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.blur_control_mirror, "Blur pictures in the control-room mirror");
                        });
                        row(ui, "", |ui| {
                            theme::hint(ui, "The participant's screen always shows the real pictures.");
                        });
                        section(ui, "Files");
                        row(ui, "", |ui| {
                            ui.checkbox(&mut p.open_last_file, "Open the last paradigm when the app starts");
                        });
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
