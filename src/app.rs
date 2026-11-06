//! Main editor window: menus, toolbar, library | participant view | inspector, timeline.
//! While a run is going, the window shows the control room instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Align, Context, Key, KeyboardShortcut, Layout, Modifiers, Pos2, RichText, Ui, Vec2};

use crate::actions::Action;
use crate::controlroom::{CrAction, RunSession};
use crate::dialogs::{self, Confirm, FindReplace, HardwareDialog, Message, Outcome, PrefsDialog, QuickDesign, RepeatAsk, RunDialog, SaveChoice, TemplateDialog};
use crate::document::Document;
use crate::hardware::now;
use crate::model::{self, fmt_g, fmt_time, Paradigm, FILE_SUFFIX};
use crate::panels::{self, Inspector, Library};
use crate::presets;
use crate::prefs::{self, Settings};
use crate::runner::{RunController, RunSettings};
use crate::stimulus;
use crate::theme::{self, hex, ACCENT, DANGER, OK, TEXT_DIM};
use crate::timeline::{self, Timeline};

mod examples {
    include!(concat!(env!("OUT_DIR"), "/examples.rs"));
}

enum Modal {
    Message(Message),
    RunAnyway(Confirm),
    Unsaved(Action),
    Repeat(RepeatAsk),
    Hardware(Box<HardwareDialog>),
    Quick(Box<QuickDesign>),
    Run(Box<RunDialog>),
    Templates(Box<TemplateDialog>),
    Find(Box<FindReplace>),
    Prefs(Box<PrefsDialog>),
}

enum Popup {
    Variants { key: String, start: Option<f64>, pos: Pos2 },
    Add { t: f64, track: usize, pos: Pos2 },
}

/// Scripted states for generating documentation screenshots (`--screenshot out.png --scene name`).
pub struct ScreenshotPlan {
    pub out: PathBuf,
    pub scene: String,
    pub frame: u32,
    pub done: Arc<std::sync::atomic::AtomicBool>,
}

pub struct App {
    pub doc: Document,
    pub settings: Settings,
    pub tl: Timeline,
    pub lib: Library,
    insp: Inspector,
    playing: Option<(f64, f64)>,
    actions: Vec<Action>,
    modal: Option<Modal>,
    popup: Option<Popup>,
    status: Option<(String, f64)>,
    pub run: Option<RunSession>,
    last_autosave: f64,
    problems: (u64, Vec<String>),
    title: String,
    quit: bool,
    pub shot: Option<ScreenshotPlan>,
    monitors: Vec<String>,
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, shot: Option<ScreenshotPlan>) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        stimulus::init(ctx);
        let settings = Settings::load();
        theme::apply(ctx, settings.prefs.ui_scale);
        let mut app = App {
            doc: Document::default(),
            settings,
            tl: Timeline::default(),
            lib: Library::default(),
            insp: Inspector::default(),
            playing: None,
            actions: vec![],
            modal: None,
            popup: None,
            status: None,
            run: None,
            last_autosave: now(),
            problems: (u64::MAX, vec![]),
            title: String::new(),
            quit: false,
            shot,
            monitors: vec![],
        };
        app.tl.stim_dir = Some(presets::default_stim_dir());
        app.apply_prefs(ctx);
        app.new_paradigm();
        if let Some(hw) = app.settings.hardware.clone() {
            app.doc.paradigm.hardware = hw; // the last hardware setup carries over to new paradigms
        }
        let last = PathBuf::from(&app.settings.last_file);
        if let Some(f) = file {
            app.open_path(&f);
        } else if app.shot.is_none() && app.settings.prefs.open_last_file && !app.settings.last_file.is_empty() && last.exists() {
            app.open_path(&last);
        }
        app
    }

    fn apply_prefs(&mut self, ctx: &Context) {
        let p = &self.settings.prefs;
        theme::apply(ctx, p.ui_scale);
        self.tl.snap = p.snap;
        self.tl.snap_px = p.snap_px as f64;
        self.tl.show_blur = p.blur_previews;
        p.apply_block_defaults();
    }

    fn save_settings(&self) {
        if self.shot.is_none() {
            self.settings.save();
        }
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), now() + 4.0));
    }

    fn message(&mut self, title: &str, text: impl Into<String>) {
        self.modal = Some(Modal::Message(Message { title: title.into(), text: text.into() }));
    }

    // ------------------------------------------------------------------ files
    fn new_paradigm(&mut self) {
        let p = &self.settings.prefs;
        let hw = self.doc.paradigm.hardware.clone();
        let par = Paradigm {
            hardware: hw,
            tr: p.new_tr,
            dummy_scans: p.new_dummy_scans,
            background: p.new_background.clone(),
            waiting_screen: p.new_waiting_screen.clone(),
            end_text: p.new_end_text.clone(),
            ..Default::default()
        };
        self.doc.reset(par);
        self.tl.set_playhead(0.0, false);
        self.tl.scroll = 0.0;
    }

    pub fn open_path(&mut self, path: &Path) {
        match self.doc.load(path) {
            Ok(()) => {
                self.settings.dir = path.parent().map(path_str).unwrap_or_default();
                self.settings.last_file = path_str(path);
                self.save_settings();
                let imgs: Vec<String> = {
                    let mut v: Vec<String> = self.doc.paradigm.blocks.iter().flat_map(|b| b.image_files()).collect();
                    v.sort();
                    v.dedup();
                    v
                };
                self.lib.clear();
                self.lib.add_files(&imgs);
                self.tl.set_playhead(0.0, false);
                self.tl.zoom_fit();
                self.playing = None;
            }
            Err(e) => self.message("Could not open", e),
        }
    }

    fn save(&mut self, save_as: bool) -> bool {
        let path = match (&self.doc.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let mut d = rfd::FileDialog::new()
                    .set_title("Save paradigm")
                    .add_filter("fMRI paradigm", &["json"])
                    .set_file_name(format!("{}{FILE_SUFFIX}", self.doc.paradigm.name));
                if !self.settings.dir.is_empty() {
                    d = d.set_directory(&self.settings.dir);
                }
                let Some(mut p) = d.save_file() else { return false };
                if !path_str(&p).ends_with(".json") {
                    p = PathBuf::from(format!("{}{FILE_SUFFIX}", path_str(&p)));
                }
                p
            }
        };
        match self.doc.save(&path) {
            Ok(()) => {
                self.settings.dir = path.parent().map(path_str).unwrap_or_default();
                self.settings.last_file = path_str(&path);
                self.save_settings();
                self.flash(format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.message("Could not save", e);
                false
            }
        }
    }

    fn open_example(&mut self, name: &str) {
        let dir = prefs::data_dir().join("examples");
        for (rel, data) in examples::EXAMPLE_FILES {
            let p = dir.join(rel);
            let same = std::fs::metadata(&p).map(|m| m.len() == data.len() as u64).unwrap_or(false);
            if !same {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(&p, data);
            }
        }
        self.open_path(&dir.join(format!("{name}{FILE_SUFFIX}")));
    }

    fn example_names() -> Vec<String> {
        let mut v: Vec<String> = examples::EXAMPLE_FILES
            .iter()
            .filter(|(r, _)| r.ends_with(FILE_SUFFIX) && !r.contains('/'))
            .map(|(r, _)| r.trim_end_matches(FILE_SUFFIX).to_string())
            .collect();
        v.sort();
        v
    }

    fn autosave(&mut self) {
        let mins = self.settings.prefs.autosave_minutes;
        if mins == 0 || now() - self.last_autosave < mins as f64 * 60.0 {
            return;
        }
        self.last_autosave = now();
        if let (Some(p), true) = (self.doc.path.clone(), self.doc.dirty) {
            if self.doc.paradigm.save(&PathBuf::from(format!("{}.autosave", path_str(&p)))).is_ok() {
                self.flash("Autosaved a backup copy");
            }
        }
    }

    // ------------------------------------------------------------------ actions
    fn needs_confirm(a: &Action) -> bool {
        matches!(a, Action::New | Action::Open(_) | Action::OpenExample(_) | Action::Templates | Action::Quit)
    }

    fn perform(&mut self, ctx: &Context, a: Action, confirmed: bool) {
        if !confirmed && self.doc.dirty && Self::needs_confirm(&a) {
            self.modal = Some(Modal::Unsaved(a));
            return;
        }
        let ph = self.tl.playhead;
        match a {
            Action::New => self.new_paradigm(),
            Action::Open(p) => {
                let p = p.or_else(|| {
                    let mut d = rfd::FileDialog::new().set_title("Open paradigm").add_filter("fMRI paradigm", &["json"]);
                    if !self.settings.dir.is_empty() {
                        d = d.set_directory(&self.settings.dir);
                    }
                    d.pick_file()
                });
                if let Some(p) = p {
                    self.open_path(&p);
                }
            }
            Action::OpenExample(n) => self.open_example(&n),
            Action::Save => {
                self.save(false);
            }
            Action::SaveAs => {
                self.save(true);
            }
            Action::Undo => self.doc.undo(),
            Action::Redo => self.doc.redo(),
            Action::Templates => {
                self.modal = Some(Modal::Templates(Box::new(TemplateDialog::new(&self.doc.paradigm.hardware, presets::default_stim_dir(), "hariri"))))
            }
            Action::QuickDesign => self.modal = Some(Modal::Quick(Box::new(QuickDesign::new(&self.doc.paradigm, &self.lib.media)))),
            Action::Hardware => {
                self.modal = Some(Modal::Hardware(Box::new(HardwareDialog::new(ctx, &self.doc.paradigm.hardware, self.doc.paradigm.tr))))
            }
            Action::Export => {
                if self.doc.paradigm.blocks.is_empty() {
                    return;
                }
                if let Some(d) = dialogs::pick_folder("Export onsets to folder", &self.settings.dir) {
                    match model::export_onsets(&self.doc.paradigm, Path::new(&d)) {
                        Ok(files) => {
                            let names: Vec<String> =
                                files.iter().map(|f| f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).collect();
                            self.message("Exported", format!("Written:\n{}", names.join("\n")));
                        }
                        Err(e) => self.message("Export failed", e),
                    }
                }
            }
            Action::TogglePlay => self.toggle_play(),
            Action::Seek(t) => {
                if let Some(pl) = self.playing.as_mut() {
                    *pl = (now(), t);
                }
            }
            Action::ZoomIn => self.tl.zoom_in(),
            Action::ZoomOut => self.tl.zoom_out(),
            Action::ZoomFit => self.tl.zoom_fit(),
            Action::ToggleSnap => {
                self.tl.snap = !self.tl.snap;
                self.settings.prefs.snap = self.tl.snap;
                self.save_settings();
            }
            Action::Validate => self.validate(),
            Action::Run => self.run_paradigm(false),
            Action::Duplicate => timeline::duplicate_selected(&mut self.doc),
            Action::RepeatAsk => {
                if !self.doc.selection.is_empty() {
                    self.modal = Some(Modal::Repeat(RepeatAsk { n: 3 }));
                }
            }
            Action::Repeat(n) => timeline::repeat_selected(&mut self.doc, n, false),
            Action::Split => timeline::split_at_playhead(&mut self.doc, ph),
            Action::Delete { ripple } => timeline::delete_selected(&mut self.doc, ripple),
            Action::SelectAll => {
                let all = self.doc.paradigm.blocks.iter().map(|b| b.id.clone()).collect();
                self.doc.select(all);
            }
            Action::Select(ids) => self.doc.select(ids),
            Action::FindReplace => self.modal = Some(Modal::Find(Box::new(FindReplace::new()))),
            Action::Preferences => self.modal = Some(Modal::Prefs(Box::new(PrefsDialog::new(&self.settings.prefs)))),
            Action::Shortcuts => self.message("Keyboard shortcuts", dialogs::shortcuts_text()),
            Action::Quit => {
                self.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::ImportPictures => {
                let files = panels::pick_images("Import pictures");
                self.lib.add_files(&files);
            }
            Action::AddKind { kind, start, track } => timeline::add_block(&mut self.doc, &kind, start, track, ph),
            Action::AddPreset { key, start, variant } => {
                let pctx = self.tl.preset_ctx();
                timeline::add_preset(&mut self.doc, &key, start, variant.as_ref(), &pctx);
            }
            Action::ChooseVariant { key, start, pos } => {
                let n = presets::preset(&key).map(|p| (p.variants)().len()).unwrap_or(0);
                if n <= 1 {
                    self.actions.push(Action::AddPreset { key, start, variant: None });
                } else {
                    let pos = pos.or_else(|| ctx.pointer_latest_pos()).unwrap_or(Pos2::new(300.0, 300.0));
                    self.popup = Some(Popup::Variants { key, start, pos });
                }
            }
            Action::AddMenu { t, track, pos } => self.popup = Some(Popup::Add { t, track, pos }),
            Action::CloseGaps(track) => self.doc.edit(|p| p.close_gaps(track)),
            Action::Status(s) => self.flash(s),
            Action::ToggleBlur => {
                self.settings.prefs.blur_previews = !self.settings.prefs.blur_previews;
                self.apply_prefs(ctx);
                self.save_settings();
            }
            Action::SetShowHints(v) => {
                self.settings.prefs.show_hints = v;
                self.save_settings();
            }
            Action::SetConfirmAbort(v) => {
                self.settings.prefs.confirm_abort = v;
                self.save_settings();
            }
            Action::SetSaveCopy(v) => {
                self.settings.prefs.save_paradigm_with_data = v;
                self.save_settings();
            }
        }
    }

    fn toggle_play(&mut self) {
        if self.playing.is_some() {
            self.playing = None;
        } else {
            if self.tl.playhead >= self.doc.paradigm.duration() {
                self.tl.set_playhead(0.0, false);
            }
            self.playing = Some((now(), self.tl.playhead));
        }
    }

    fn validate(&mut self) {
        let par = &self.doc.paradigm;
        let problems = par.validate();
        let off_grid = par.blocks.iter().filter(|b| par.tr > 0.0 && (b.start / par.tr - (b.start / par.tr).round()).abs() > 1e-6).count();
        let mut msg = if problems.is_empty() {
            "✓ No problems found.".to_string()
        } else {
            problems.iter().map(|p| format!("• {p}")).collect::<Vec<_>>().join("\n")
        };
        if off_grid > 0 {
            msg.push_str(&format!(
                "\n\nℹ {off_grid} block(s) start between volumes. That is fine for event-related designs (jitter), but unusual \
                 for block designs."
            ));
        }
        self.message("Check paradigm", msg);
    }

    fn run_paradigm(&mut self, checked: bool) {
        let par = &self.doc.paradigm;
        if par.blocks.is_empty() {
            self.message("Nothing to run", "Add some blocks to the timeline first.");
            return;
        }
        let problems = par.validate();
        if !problems.is_empty() && !checked {
            let text = problems.iter().take(10).cloned().collect::<Vec<_>>().join("\n") + "\n\nRun anyway?";
            self.modal = Some(Modal::RunAnyway(Confirm { title: "Paradigm has issues".into(), text, yes: "Run anyway".into() }));
            return;
        }
        let monitors = if self.monitors.is_empty() { vec!["1: main screen".to_string()] } else { self.monitors.clone() };
        self.modal = Some(Modal::Run(Box::new(RunDialog::new(par, &self.settings.prefs.data_dir, monitors))));
    }

    fn start_run(&mut self, ctx: &Context, mut s: RunSettings) {
        let p = self.settings.prefs.clone();
        if s.out_dir.trim().is_empty() {
            s.out_dir = p.data_dir.clone();
        }
        self.settings.prefs.data_dir = s.out_dir.clone();
        s.confirm_abort = p.confirm_abort;
        s.save_paradigm_with_data = p.save_paradigm_with_data;
        s.watchdog_trials = p.watchdog_trials;
        s.trigger_warning_trs = p.trigger_warning_trs;
        s.control_refresh_ms = p.control_refresh_ms;
        s.blur_control_mirror = p.blur_control_mirror;
        self.save_settings();
        // remember the scanner-start settings in the paradigm
        let par = &self.doc.paradigm;
        if par.dummy_scans != s.dummy_scans || par.waiting_screen != s.waiting_screen || par.waiting_text != s.waiting_text || par.end_text != s.end_text {
            let s2 = s.clone();
            self.doc.edit(|p| {
                p.dummy_scans = s2.dummy_scans;
                p.waiting_screen = s2.waiting_screen.clone();
                p.waiting_text = s2.waiting_text.clone();
                p.end_text = s2.end_text.clone();
            });
        }
        self.playing = None;
        let par = self.doc.paradigm.clone();
        // preload pictures – no disk access while running
        let imgs: Vec<String> = par.blocks.iter().flat_map(|b| b.image_files()).collect();
        stimulus::preload(ctx, &imgs);
        let c = ctx.clone();
        let mut ctl = RunController::new(par, s, Arc::new(move || c.request_repaint()));
        ctl.start();
        self.run = Some(RunSession::new(ctl, self.settings.controlroom_folded.clone()));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ------------------------------------------------------------------ keyboard
    fn shortcuts(&mut self, ctx: &Context) {
        if self.modal.is_some() || self.popup.is_some() {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let list: [(Modifiers, Key, Action); 23] = [
            (cs, Key::S, Action::SaveAs),
            (cs, Key::Z, Action::Redo),
            (cs, Key::N, Action::QuickDesign),
            (cs, Key::H, Action::Hardware),
            (cs, Key::B, Action::ToggleBlur),
            (cmd, Key::N, Action::New),
            (cmd, Key::O, Action::Open(None)),
            (cmd, Key::S, Action::Save),
            (cmd, Key::Z, Action::Undo),
            (cmd, Key::Y, Action::Redo),
            (cmd, Key::T, Action::Templates),
            (cmd, Key::E, Action::Export),
            (cmd, Key::Equals, Action::ZoomIn),
            (cmd, Key::Plus, Action::ZoomIn),
            (cmd, Key::Minus, Action::ZoomOut),
            (cmd, Key::Num0, Action::ZoomFit),
            (cmd, Key::K, Action::Validate),
            (cmd, Key::D, Action::Duplicate),
            (cmd, Key::R, Action::RepeatAsk),
            (cmd, Key::H, Action::FindReplace),
            (cmd, Key::Comma, Action::Preferences),
            (cmd, Key::Q, Action::Quit),
            (cmd, Key::A, Action::SelectAll),
        ];
        let typing = ctx.text_edit_focused();
        for (m, k, a) in list {
            if typing && matches!(a, Action::SelectAll | Action::Undo | Action::Redo) {
                continue; // text fields handle these themselves
            }
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k))) {
                self.actions.push(a);
            }
        }
        let plain = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        if plain(Key::F5) {
            self.actions.push(Action::Run);
        }
        if plain(Key::F1) {
            self.actions.push(Action::Shortcuts);
        }
        if typing {
            return;
        }
        if plain(Key::Space) {
            self.actions.push(Action::TogglePlay);
        }
        if plain(Key::Home) {
            self.tl.set_playhead(0.0, true);
            self.actions.push(Action::Seek(0.0));
        }
        if plain(Key::N) {
            self.actions.push(Action::ToggleSnap);
        }
        if plain(Key::S) {
            self.actions.push(Action::Split);
        }
        let shift_del = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Delete) || i.consume_key(Modifiers::SHIFT, Key::Backspace));
        if shift_del {
            self.actions.push(Action::Delete { ripple: true });
        } else if plain(Key::Delete) || plain(Key::Backspace) {
            self.actions.push(Action::Delete { ripple: false });
        }
        let tr = self.doc.paradigm.tr;
        for (k, sign) in [(Key::ArrowLeft, -1.0), (Key::ArrowRight, 1.0)] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, k)) {
                timeline::nudge_selected(&mut self.doc, sign * if tr > 0.0 { tr } else { 1.0 });
            } else if plain(k) {
                timeline::nudge_selected(&mut self.doc, sign * self.tl.grid_step());
            }
        }
    }

    // ------------------------------------------------------------------ layout
    fn menu_bar(&mut self, ui: &mut Ui) {
        let a = &mut self.actions;
        let p = &self.settings.prefs;
        fn item_fn(ui: &mut Ui, a: &mut Vec<Action>, text: &str, sc: &str, act: Action, enabled: bool) {
            let b = egui::Button::new(text).shortcut_text(sc);
            if ui.add_enabled(enabled, b).clicked() {
                a.push(act);
                ui.close();
            }
        }
        macro_rules! item {
            ($ui:expr, $t:expr, $sc:expr, $act:expr, $en:expr) => {
                item_fn($ui, a, $t, $sc, $act, $en)
            };
        }
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                item!(ui, "New", "Ctrl+N", Action::New, true);
                item!(ui, "New from template…", "Ctrl+T", Action::Templates, true);
                item!(ui, "Open…", "Ctrl+O", Action::Open(None), true);
                item!(ui, "Save", "Ctrl+S", Action::Save, true);
                item!(ui, "Save as…", "Ctrl+Shift+S", Action::SaveAs, true);
                ui.menu_button("Open example", |ui| {
                    for n in App::example_names() {
                        item!(ui, &n, "", Action::OpenExample(n.clone()), true);
                    }
                });
                ui.separator();
                item!(ui, "Export onsets…", "Ctrl+E", Action::Export, true);
                ui.separator();
                item!(ui, "Quit", "Ctrl+Q", Action::Quit, true);
            });
            ui.menu_button("Edit", |ui| {
                item!(ui, "Undo", "Ctrl+Z", Action::Undo, self.doc.can_undo());
                item!(ui, "Redo", "Ctrl+Shift+Z", Action::Redo, self.doc.can_redo());
                ui.separator();
                let has = !self.doc.selection.is_empty();
                item!(ui, "Duplicate", "Ctrl+D", Action::Duplicate, has);
                item!(ui, "Repeat selection ×N…", "Ctrl+R", Action::RepeatAsk, has);
                item!(ui, "Split at playhead", "S", Action::Split, true);
                item!(ui, "Delete", "Del", Action::Delete { ripple: false }, has);
                item!(ui, "Delete and close gap", "Shift+Del", Action::Delete { ripple: true }, has);
                item!(ui, "Select all", "Ctrl+A", Action::SelectAll, true);
                ui.menu_button("Select…", |ui| select_menu(ui, &self.doc, a));
                ui.separator();
                item!(ui, "Find and replace text…", "Ctrl+H", Action::FindReplace, true);
            });
            ui.menu_button("Paradigm", |ui| {
                item!(ui, "Templates…", "Ctrl+T", Action::Templates, true);
                item!(ui, "Quick design…", "Ctrl+Shift+N", Action::QuickDesign, true);
                item!(ui, "Check paradigm", "Ctrl+K", Action::Validate, true);
                item!(ui, "Hardware…", "Ctrl+Shift+H", Action::Hardware, true);
                ui.separator();
                item!(ui, "Run…", "F5", Action::Run, true);
            });
            ui.menu_button("View", |ui| {
                item!(ui, if self.playing.is_some() { "Pause" } else { "Play" }, "Space", Action::TogglePlay, true);
                item!(ui, "Go to start", "Home", Action::Seek(0.0), true);
                item!(ui, "Zoom in", "Ctrl+=", Action::ZoomIn, true);
                item!(ui, "Zoom out", "Ctrl+-", Action::ZoomOut, true);
                item!(ui, "Fit whole run", "Ctrl+0", Action::ZoomFit, true);
            });
            ui.menu_button("Options", |ui| {
                let check = |ui: &mut Ui, on: bool, text: &str, sc: &str| -> bool {
                    ui.add(egui::Button::new(format!("{}  {text}", if on { "☑" } else { "☐" })).shortcut_text(sc)).clicked()
                };
                if check(ui, self.tl.snap, "Snap to TR grid and block edges", "N") {
                    a.push(Action::ToggleSnap);
                    ui.close();
                }
                if check(ui, p.show_hints, "Show usage hints", "") {
                    a.push(Action::SetShowHints(!p.show_hints));
                    ui.close();
                }
                if check(ui, p.blur_previews, "Blur picture previews (unpleasant images)", "Ctrl+Shift+B") {
                    a.push(Action::ToggleBlur);
                    ui.close();
                }
                ui.separator();
                if check(ui, p.confirm_abort, "Abort a run only after confirming", "") {
                    a.push(Action::SetConfirmAbort(!p.confirm_abort));
                    ui.close();
                }
                if check(ui, p.save_paradigm_with_data, "Save the paradigm with each run's data", "") {
                    a.push(Action::SetSaveCopy(!p.save_paradigm_with_data));
                    ui.close();
                }
                ui.separator();
                item!(ui, "Preferences…", "Ctrl+,", Action::Preferences, true);
            });
            ui.menu_button("Help", |ui| {
                item!(ui, "Keyboard shortcuts", "F1", Action::Shortcuts, true);
                if ui.button("About ParadigmShift").clicked() {
                    a.push(Action::Status(format!("ParadigmShift {} – timeline editor and presenter for fMRI paradigms", env!("CARGO_PKG_VERSION"))));
                    ui.close();
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let a = &mut self.actions;
            let mut tb = |ui: &mut Ui, text: &str, tip: &str, act: Action, enabled: bool| {
                if ui.add_enabled(enabled, egui::Button::new(text).frame_when_inactive(false)).on_hover_text(tip).clicked() {
                    a.push(act);
                }
            };
            tb(ui, "New", "New paradigm (Ctrl+N)", Action::New, true);
            tb(ui, "Open", "Open a paradigm (Ctrl+O)", Action::Open(None), true);
            tb(ui, "Save", "Save (Ctrl+S)", Action::Save, true);
            ui.separator();
            tb(ui, "↶ Undo", "Undo (Ctrl+Z)", Action::Undo, self.doc.can_undo());
            tb(ui, "↷ Redo", "Redo (Ctrl+Shift+Z)", Action::Redo, self.doc.can_redo());
            ui.separator();
            tb(ui, "▦ Templates", "Start from a classic task: Hariri, Go/No-Go, emotion regulation … (Ctrl+T)", Action::Templates, true);
            tb(ui, "✦ Quick design", "Build a block or event-related design in a few clicks (Ctrl+Shift+N)", Action::QuickDesign, true);
            tb(ui, "⚙ Hardware", "Set up and test the NNL SyncBox and ResponseGrips (Ctrl+Shift+H)", Action::Hardware, true);
            tb(ui, "⇩ Export onsets", "Export planned onsets (BIDS events.tsv + FSL 3-column files) (Ctrl+E)", Action::Export, true);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::colored_button(ui, "▶  Run in scanner", OK)
                    .on_hover_text("Present the paradigm, wait for the SyncBox trigger and log grip responses (F5)")
                    .clicked()
                {
                    self.actions.push(Action::Run);
                }
                ui.add_space(10.0);
                let file = self.doc.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "unsaved".into());
                ui.label(RichText::new(format!("{}  ·  {file}{}", self.doc.paradigm.name, if self.doc.dirty { " •" } else { "" })).color(TEXT_DIM));
            });
        });
    }

    fn timeline_panel(&mut self, ui: &mut Ui) {
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            theme::panel_title(ui, "Timeline");
            let par = &self.doc.paradigm;
            let conds = par.conditions();
            ui.horizontal(|ui| {
                let info = format!(
                    "{}  ·  {} volumes @ TR {}s  ·  {} blocks{}",
                    fmt_time(par.duration(), 1),
                    par.volumes(),
                    fmt_g(par.tr),
                    par.blocks.len(),
                    if conds.is_empty() { String::new() } else { format!("  ·  {} conditions", conds.len()) }
                );
                ui.label(RichText::new(info).color(TEXT_DIM).size(12.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Fit").on_hover_text("Fit whole run (Ctrl+0)").clicked() {
                        self.actions.push(Action::ZoomFit);
                    }
                    if ui.button("+").on_hover_text("Zoom in (Ctrl+=)").clicked() {
                        self.actions.push(Action::ZoomIn);
                    }
                    if ui.button("−").on_hover_text("Zoom out (Ctrl+-)").clicked() {
                        self.actions.push(Action::ZoomOut);
                    }
                    let snap = egui::Button::new(RichText::new("⊓ Snap").color(if self.tl.snap { ACCENT } else { TEXT_DIM })).selected(self.tl.snap);
                    if ui.add(snap).on_hover_text("Snap to TR grid and block edges (hold Alt while dragging to place freely) – N").clicked() {
                        self.actions.push(Action::ToggleSnap);
                    }
                    if self.settings.prefs.show_hints {
                        ui.label(RichText::new("drag to move · drag edges to trim · Ctrl+wheel zoom · right-click for more").color(TEXT_DIM).size(12.0));
                    }
                });
            });
            ui.add_space(2.0);
            self.tl.ui(ui, &mut self.doc, &mut self.actions);
        });
    }

    fn monitor_panel(&mut self, ui: &mut Ui) {
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height() - 22.0);
            panels::panel_header(ui, "Participant view", "what the participant sees at the playhead");
            let h = ui.available_height() - 40.0;
            ui.allocate_ui(Vec2::new(ui.available_width(), h.max(100.0)), |ui| {
                panels::monitor_ui(ui, &self.doc, self.tl.playhead, self.settings.prefs.blur_previews);
            });
            ui.horizontal(|ui| {
                if ui.button("|◀").on_hover_text("Go to start (Home)").clicked() {
                    self.tl.set_playhead(0.0, true);
                    self.actions.push(Action::Seek(0.0));
                }
                let play = if self.playing.is_some() { "❚❚" } else { "▶" };
                if ui.button(RichText::new(play).size(16.0)).on_hover_text("Play / pause the preview (Space)").clicked() {
                    self.actions.push(Action::TogglePlay);
                }
                ui.label(RichText::new(fmt_time(self.tl.playhead, 2)).font(theme::mono(20.0)).color(ACCENT));
                let par = &self.doc.paradigm;
                if par.tr > 0.0 {
                    ui.label(
                        RichText::new(format!(
                            "volume {} / {}   ·   end {}",
                            (self.tl.playhead / par.tr).floor() as i64 + 1,
                            par.volumes(),
                            fmt_time(par.duration(), 1)
                        ))
                        .color(TEXT_DIM),
                    );
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        if self.problems.0 != self.doc.version {
            self.problems = (self.doc.version, self.doc.paradigm.validate());
        }
        ui.horizontal(|ui| {
            if let Some((m, until)) = &self.status {
                if now() < *until {
                    ui.label(RichText::new(m).color(TEXT_DIM));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = self.problems.1.len();
                if n > 0 {
                    if ui.add(egui::Label::new(RichText::new(format!("⚠ {n} issue(s) – Ctrl+K to check")).color(DANGER)).sense(egui::Sense::click())).clicked() {
                        self.actions.push(Action::Validate);
                    }
                } else {
                    ui.label(RichText::new("✓ ready to run").color(OK));
                }
            });
        });
    }

    fn popups(&mut self, ctx: &Context) {
        let Some(pop) = &self.popup else { return };
        let pos = match pop {
            Popup::Variants { pos, .. } | Popup::Add { pos, .. } => *pos,
        };
        let mut close = false;
        let mut chosen: Option<Action> = None;
        let resp = egui::Area::new(egui::Id::unique("popup_menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::menu(&ctx.global_style()).show(ui, |ui| {
                ui.set_min_width(220.0);
                match pop {
                    Popup::Variants { key, start, .. } => {
                        if let Some(pr) = presets::preset(key) {
                            ui.label(RichText::new(pr.title).font(theme::bold(12.0)).color(TEXT_DIM));
                            for (label, v) in (pr.variants)() {
                                if ui.add(egui::Button::new(label).frame(false).min_size(Vec2::new(210.0, 22.0))).clicked() {
                                    chosen = Some(Action::AddPreset { key: key.clone(), start: *start, variant: Some(v) });
                                }
                            }
                        }
                    }
                    Popup::Add { t, track, .. } => {
                        let mut acts = vec![];
                        timeline::add_menu_contents(ui, *t, *track, &mut acts);
                        chosen = acts.pop();
                    }
                }
            });
        });
        if chosen.is_some() {
            close = true;
        }
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx.pointer_latest_pos().map(|p| !resp.response.rect.contains(p)).unwrap_or(false)
            && !egui::Popup::is_any_open(ctx);
        if clicked_outside || ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        if close {
            self.popup = None;
        }
        if let Some(a) = chosen {
            self.actions.push(a);
        }
    }

    fn modals(&mut self, ctx: &Context) {
        let Some(mut m) = self.modal.take() else { return };
        let mut keep = true;
        match &mut m {
            Modal::Message(d) => keep = d.ui(ctx) == Outcome::Open,
            Modal::RunAnyway(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    keep = false;
                    self.run_paradigm(true);
                    return;
                }
                Outcome::Cancel => keep = false,
                Outcome::Open => {}
            },
            Modal::Unsaved(a) => match dialogs::unsaved_ui(ctx) {
                SaveChoice::Save => {
                    keep = false;
                    if self.save(false) {
                        self.actions.push(Action::Status("Saved".into()));
                        let a = a.clone();
                        self.modal = None;
                        self.perform(ctx, a, true);
                        return;
                    }
                }
                SaveChoice::Discard => {
                    let a = a.clone();
                    self.modal = None;
                    if matches!(a, Action::Quit) {
                        self.doc.dirty = false;
                    }
                    self.perform(ctx, a, true);
                    return;
                }
                SaveChoice::Cancel => keep = false,
                SaveChoice::Open => {}
            },
            Modal::Repeat(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    self.actions.push(Action::Repeat(d.n));
                    keep = false;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::Hardware(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    self.doc.paradigm.hardware = d.hw.clone();
                    self.settings.hardware = Some(d.hw.clone());
                    self.save_settings();
                    self.doc.touch();
                    keep = false;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::Quick(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    let blocks = d.blocks(None);
                    let replace = d.replace;
                    if !blocks.is_empty() {
                        let paths: Vec<String> = blocks.iter().filter_map(|b| b.params.get("path").and_then(|v| v.as_str()).map(String::from)).collect();
                        self.doc.edit(|par| {
                            if replace {
                                par.blocks.clear();
                            }
                            let offset = if replace { 0.0 } else { par.duration() };
                            for mut b in blocks {
                                b.start = model::round4(b.start + offset);
                                par.blocks.push(b);
                            }
                        });
                        self.lib.add_files(&paths);
                        self.doc.select(vec![]);
                        self.tl.zoom_fit();
                    }
                    keep = false;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::Run(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    let s = d.s.clone();
                    self.modal = None;
                    self.start_run(ctx, s);
                    return;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::Templates(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    if let Ok(mut par) = d.result.clone() {
                        let hw = self.doc.paradigm.hardware.clone();
                        for b in par.blocks.iter_mut() {
                            // left/right answers follow the cursor buttons from the hardware setup
                            if b.kind == "match" {
                                b.params.insert("left_button".into(), serde_json::json!(hw.move_left));
                                b.params.insert("right_button".into(), serde_json::json!(hw.move_right));
                            }
                        }
                        par.hardware = hw;
                        let files: Vec<String> = {
                            let mut v: Vec<String> = par.blocks.iter().flat_map(|b| b.image_files()).collect();
                            v.sort();
                            v.dedup();
                            v
                        };
                        self.doc.reset(par);
                        self.doc.dirty = true;
                        self.lib.clear();
                        self.lib.add_files(&files);
                        self.tl.set_playhead(0.0, false);
                        self.tl.zoom_fit();
                    }
                    keep = false;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::Find(d) => keep = d.ui(ctx, &mut self.doc) == Outcome::Open,
            Modal::Prefs(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    self.settings.prefs = d.p.clone();
                    self.apply_prefs(ctx);
                    self.save_settings();
                    keep = false;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
        }
        if keep && self.modal.is_none() {
            self.modal = Some(m);
        }
    }

    fn handle_file_drops(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if dropped.is_empty() {
            return;
        }
        if let Some(p) = dropped.iter().find(|p| path_str(p).ends_with(".json")) {
            self.actions.push(Action::Open(Some(p.clone())));
            return;
        }
        let imgs = panels::expand_images(&dropped);
        if imgs.is_empty() {
            return;
        }
        self.lib.add_files(&imgs);
        if let Some(pos) = ctx.pointer_latest_pos() {
            if self.tl.contains(pos) {
                self.tl.drop_images(&mut self.doc, imgs, pos);
            }
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let t = match &self.run {
            Some(s) => {
                let c = s.ctl.lock().map(|c| (c.p().name.clone(), c.phase() == crate::runner::Phase::Done, c.aborted)).ok();
                match c {
                    Some((n, true, true)) => format!("Control room – {n} – ABORTED"),
                    Some((n, true, false)) => format!("Control room – {n} – finished"),
                    Some((n, _, _)) => format!("Control room – {n}"),
                    None => "Control room".into(),
                }
            }
            None => format!("{} — ParadigmShift{}", self.doc.paradigm.name, if self.doc.dirty { " •" } else { "" }),
        };
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }

    fn editor(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.handle_file_drops(&ctx);
        if let Some((t0, from)) = self.playing {
            let t = from + now() - t0;
            let dur = self.doc.paradigm.duration();
            if t >= dur {
                self.tl.set_playhead(dur, true);
                self.playing = None;
            } else {
                self.tl.set_playhead(t, true);
                ctx.request_repaint();
            }
        }
        let frame = egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(8, 4));
        egui::Panel::top("menu").frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(8, 2))).show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").frame(frame).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(12, 2))).show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("timeline")
            .frame(frame)
            .resizable(true)
            .default_size(250.0)
            .size_range(200.0..=600.0)
            .show_separator_line(false)
            .show(ui, |ui| self.timeline_panel(ui));
        egui::Panel::left("library")
            .frame(frame)
            .resizable(true)
            .default_size(268.0)
            .size_range(240.0..=320.0)
            .show_separator_line(false)
            .show(ui, |ui| {
                let blur = self.settings.prefs.blur_previews;
                self.lib.ui(ui, blur, &mut self.actions)
            });
        egui::Panel::right("inspector")
            .frame(frame)
            .resizable(true)
            .default_size(380.0)
            .size_range(340.0..=460.0)
            .show_separator_line(false)
            .show(ui, |ui| {
                let blur = self.settings.prefs.blur_previews;
                self.insp.ui(ui, &mut self.doc, blur, &mut self.actions)
            });
        egui::CentralPanel::default().frame(frame).show(ui, |ui| self.monitor_panel(ui));
        self.popups(&ctx);
        self.modals(&ctx);
    }
}

fn select_menu(ui: &mut Ui, doc: &Document, a: &mut Vec<Action>) {
    let par = &doc.paradigm;
    let mut kinds: Vec<(String, Vec<String>)> = vec![];
    for b in &par.blocks {
        match kinds.iter_mut().find(|k| k.0 == b.kind) {
            Some(k) => k.1.push(b.id.clone()),
            None => kinds.push((b.kind.clone(), vec![b.id.clone()])),
        }
    }
    if kinds.is_empty() {
        ui.add_enabled(false, egui::Button::new("(timeline is empty)"));
        return;
    }
    ui.label(RichText::new("All blocks of a type").color(TEXT_DIM).small());
    for (kind, ids) in &kinds {
        let bt = model::block_type(kind).unwrap();
        if timeline::icon_button(ui, kind, hex(bt.color), &format!("{}  ({})", bt.title, ids.len())).clicked() {
            a.push(Action::Select(ids.clone()));
            ui.close();
        }
    }
    let conds = par.conditions();
    if !conds.is_empty() {
        ui.separator();
        ui.label(RichText::new("All blocks of a condition").color(TEXT_DIM).small());
        for c in conds {
            let ids: Vec<String> = par.blocks.iter().filter(|b| b.condition == c).map(|b| b.id.clone()).collect();
            if ui.button(format!("{c}  ({})", ids.len())).clicked() {
                a.push(Action::Select(ids));
                ui.close();
            }
        }
    }
    let sel = doc.selected_blocks();
    if sel.len() == 1 {
        let b = sel[0];
        let same: Vec<String> = par.blocks.iter().filter(|o| o.kind == b.kind && o.condition == b.condition).map(|o| o.id.clone()).collect();
        ui.separator();
        if ui.button(format!("Same type and condition as the selected block  ({})", same.len())).clicked() {
            a.push(Action::Select(same));
            ui.close();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        stimulus::poll(&ctx);
        if self.monitors.is_empty() {
            if let Some(w) = frame.winit_window() {
                self.monitors = w
                    .available_monitors()
                    .enumerate()
                    .map(|(i, m)| {
                        let s = m.size();
                        format!("{}: {}  {}×{}", i + 1, m.name().unwrap_or_else(|| "screen".into()), s.width, s.height)
                    })
                    .collect();
            }
        }
        self.update_title(&ctx);
        if self.run.is_some() {
            let focused = self.shot.is_some()
                || ctx.input(|i| i.viewport().focused.unwrap_or(true))
                || ctx.input_for(egui::ViewportId::from_hash_of(crate::controlroom::PARTICIPANT_ID), |i| i.viewport().focused.unwrap_or(false));
            let session = self.run.as_mut().unwrap();
            session.show_participant(&ctx);
            let ctl = session.ctl.clone();
            if session.cr.ui(ui, &ctl, focused) == CrAction::Close {
                self.settings.controlroom_folded = session.cr.folded.iter().cloned().collect();
                self.settings.controlroom_folded.sort();
                self.run = None;
                self.save_settings();
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        } else {
            self.editor(ui);
            // close request: ask about unsaved changes first
            if ctx.input(|i| i.viewport().close_requested()) && !self.quit && self.doc.dirty && self.shot.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.modal = Some(Modal::Unsaved(Action::Quit));
            }
        }
        let actions = std::mem::take(&mut self.actions);
        for a in actions {
            self.perform(&ctx, a, false);
        }
        if self.run.is_none() {
            self.autosave();
        }
        if let Some((_, until)) = self.status {
            if now() < until {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        self.demo_step(&ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_settings();
    }
}

// =============================================================================
// Scripted screenshots for the documentation (`--screenshot out.png --scene <name>`)
// =============================================================================
impl App {
    fn demo_step(&mut self, ctx: &Context) {
        let (frame, scene, done_flag, out_path) = {
            let Some(plan) = self.shot.as_mut() else { return };
            plan.frame += 1;
            (plan.frame, plan.scene.clone(), plan.done.clone(), plan.out.clone())
        };
        ctx.request_repaint();
        if done_flag.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(s) = &self.run {
                if let Ok(mut c) = s.ctl.lock() {
                    c.finish(true);
                }
            }
            self.doc.dirty = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let run_scene = scene.starts_with("control") || scene.starts_with("participant");
        if frame == 2 {
            match scene.as_str() {
                "editor" | "bulk" | "find" | "blur" => {
                    self.open_example("emotional_rating");
                    let ratings: Vec<String> = self.doc.paradigm.blocks.iter().filter(|b| b.kind == "rating").map(|b| b.id.clone()).collect();
                    if scene == "bulk" {
                        self.doc.select(ratings);
                    } else if let Some(id) = ratings.get(3) {
                        let t = self.doc.paradigm.block(id).map(|b| b.start + 1.3).unwrap_or(0.0);
                        self.doc.select(vec![id.clone()]);
                        self.tl.set_playhead(t, false);
                    }
                    if scene == "find" {
                        self.modal = Some(Modal::Find(Box::new(FindReplace::new())));
                    }
                    if scene == "blur" {
                        self.settings.prefs.blur_previews = true;
                        self.apply_prefs(ctx);
                    }
                }
                "templates" => self.modal = Some(Modal::Templates(Box::new(TemplateDialog::new(&self.doc.paradigm.hardware, presets::default_stim_dir(), "hariri")))),
                "templates_stroop" => self.modal = Some(Modal::Templates(Box::new(TemplateDialog::new(&self.doc.paradigm.hardware, presets::default_stim_dir(), "stroop")))),
                "hardware" => self.actions.push(Action::Hardware),
                "quick" => {
                    self.open_example("emotional_rating");
                    self.actions.push(Action::QuickDesign);
                }
                "prefs" => self.actions.push(Action::Preferences),
                "run_dialog" => {
                    self.open_example("hariri_face_matching");
                    self.run_paradigm(true);
                }
                "hariri" | "gonogo" | "mid" | "stroop" | "emoreg" | "nback" | "checkerboard" => {
                    let name = match scene.as_str() {
                        "hariri" => "hariri_face_matching",
                        "gonogo" => "go_nogo",
                        "mid" => "monetary_incentive_delay",
                        "emoreg" => "emotion_regulation",
                        "nback" => "n_back",
                        "checkerboard" => "visual_checkerboard",
                        other => other,
                    };
                    self.open_example(name);
                    let pick = self.doc.paradigm.blocks.iter().find(|b| b.start > 20.0 && (b.kind == "match" || b.kind == "choice" || b.kind == "gonogo" || b.kind == "image" || b.kind == "feedback")).map(|b| (b.id.clone(), b.start + 0.1));
                    if let Some((id, t)) = pick {
                        self.doc.select(vec![id]);
                        self.tl.set_playhead(t, false);
                    }
                }
                _ => {}
            }
            if run_scene {
                self.open_example("hariri_face_matching");
                let s = RunSettings {
                    out_dir: std::env::temp_dir().join("paradigmshift-demo").to_string_lossy().to_string(),
                    fullscreen: false,
                    simulate: true,
                    dummy_scans: 3,
                    waiting_screen: "fixation".into(),
                    end_text: "Thank you!".into(),
                    ..Default::default()
                };
                self.start_run(ctx, s);
            }
        }
        // grip presses during the simulated run
        if run_scene && frame > 2 && frame % 37 == 0 {
            if let Some(s) = &self.run {
                if let Ok(mut c) = s.ctl.lock() {
                    let k = if (frame / 37) % 3 == 0 { "c" } else { "b" };
                    c.key(k, now());
                }
            }
        }
        let wait_frames = if run_scene { 900 } else { 40 };
        if frame == wait_frames {
            let out = out_path.clone();
            let done = done_flag.clone();
            let cb = move |img: Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let mut buf = image::RgbaImage::new(w as u32, h as u32);
                for (i, px) in img.pixels.iter().enumerate() {
                    buf.put_pixel((i % w) as u32, (i / w) as u32, image::Rgba(px.to_srgba_unmultiplied()));
                }
                let _ = buf.save(&out);
                done.store(true, std::sync::atomic::Ordering::Relaxed);
            };
            if scene.starts_with("participant") {
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::from_hash_of(crate::controlroom::PARTICIPANT_ID),
                    egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(cb)),
                );
            } else {
                ctx.request_screenshot(cb);
            }
        }
    }
}
