//! Main editor window: menus, toolbar, library | participant view | inspector, timeline.
//! While a run is going, the window shows the control room instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Align, Context, Key, KeyboardShortcut, Layout, Modifiers, Pos2, RichText, Ui, Vec2};

use crate::actions::Action;
use crate::controlroom::{CrAction, RunSession};
use crate::dialogs::{
    self, AboutDialog, ClashChoice, Confirm, FileClash, FindReplace, HardwareDialog, Message, Outcome, PrefsDialog, QuickDesign, RelinkAction, RelinkDialog,
    RepeatAsk, RunDialog, SaveChoice, TemplateDialog, TestDialog,
};
use crate::selftest::TestSession;
use crate::document::Document;
use crate::hardware::now;
use crate::model::{self, fmt_g, fmt_time, Paradigm, FILE_SUFFIX, ParamExt};
use crate::panels::{self, Inspector, Library};
use crate::presets;
use crate::prefs::Settings;
use crate::runner::{RunController, RunSettings};
use crate::stimulus;
use crate::theme::{self, hex, ACCENT, DANGER, OK, TEXT_DIM};
use crate::timeline::{self, Timeline};


enum Modal {
    Message(Message),
    About(Box<AboutDialog>),
    RunAnyway(Confirm),
    Unsaved(Action),
    Repeat(RepeatAsk),
    Hardware(Box<HardwareDialog>),
    Quick(Box<QuickDesign>),
    Run(Box<RunDialog>),
    FileClash(Box<FileClash>),
    Test(Box<TestDialog>),
    Templates(Box<TemplateDialog>),
    Find(Box<FindReplace>),
    Prefs(Box<PrefsDialog>),
    /// media files that are not where the paradigm says
    Relink(Box<RelinkDialog>),
    /// "Save as…" would replace the paradigm file of another project folder
    ReplaceProject(Confirm, PathBuf),
}

enum Popup {
    Variants { key: String, start: Option<f64>, pos: Pos2 },
    Add { t: f64, track: usize, pos: Pos2 },
}

/// Scripted states for generating documentation screenshots (`--screenshot out.png --scene name`).

/// Width of the buttons in the tool strip and the padding around them.
const STRIP_BUTTON_W: f32 = 36.0;
const STRIP_PAD: f32 = 3.0;

/// "Open for presenting": a paradigm loaded read-only that goes straight to the run dialog. The
/// editor stays closed, so nothing can be changed by accident; scanner-start settings chosen in
/// the run dialog apply to the run only and are never written back to the file.
pub struct Presenter {
    pub par: crate::model::Paradigm,
    pub path: PathBuf,
    pub problems: Vec<String>,
    /// runs finished in this presentation session
    pub runs: u32,
    /// the settings of the last run – the next run continues from them (same participant, run + 1)
    pub last: Option<RunSettings>,
}

pub struct ScreenshotPlan {
    pub out: PathBuf,
    pub scene: String,
    pub frame: u32,
    pub done: Arc<std::sync::atomic::AtomicBool>,
    pub requested: bool,
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
    pub test: Option<TestSession>,
    pub present: Option<Presenter>,
    /// is the SyncBox there? (checked from start-up on, shown in the status bar)
    syncbox: Option<crate::hardware::SyncBoxMonitor>,
    /// the hardware setup last checked (mode | port | baud), and whether the monitor is paused
    syncbox_checked: String,
    syncbox_paused: bool,
    /// the quick design as it was last closed (for this paradigm): reopening keeps the sorting
    quick_last: Option<Box<QuickDesign>>,
    last_autosave: f64,
    problems: (u64, Vec<String>),
    title: String,
    quit: bool,
    pub shot: Option<ScreenshotPlan>,
    monitors: Vec<String>,
    layout_dirty: bool,
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, shot: Option<ScreenshotPlan>, present: bool) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        stimulus::init(ctx);
        // screenshots for the docs use the default preferences, not the ones saved on this computer
        let settings = if shot.is_some() { Settings::default() } else { Settings::load() };
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
            test: None,
            present: None,
            syncbox: None,
            syncbox_checked: String::new(),
            syncbox_paused: false,
            quick_last: None,
            last_autosave: now(),
            problems: (u64::MAX, vec![]),
            title: String::new(),
            quit: false,
            shot,
            monitors: vec![],
            layout_dirty: false,
        };
        app.tl.stim_dir = Some(presets::default_stim_dir());
        app.lib.closed = app.settings.library_closed.clone();
        app.lib.weights = app.settings.library_weights.clone();
        app.apply_prefs(ctx);
        app.new_paradigm();
        if let Some(hw) = app.settings.hardware.clone() {
            app.doc.paradigm.hardware = hw; // the last hardware setup carries over to new paradigms
        }
        // look for the SyncBox right away (not when making documentation screenshots)
        if app.shot.is_none() {
            let c = ctx.clone();
            app.syncbox = Some(crate::hardware::SyncBoxMonitor::new(Arc::new(move || c.request_repaint())));
        }
        // the app always starts with an empty paradigm – unless a file was passed to it
        // (e.g. by double-clicking a .paradigm.json)
        if let Some(f) = file {
            if present {
                app.actions.push(Action::OpenForPresenting(Some(f)));
            } else {
                app.open_path(&f);
            }
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
        self.quick_last = None;
        self.tl.set_playhead(0.0, false);
        self.tl.scroll = 0.0;
    }

    pub fn open_path(&mut self, path: &Path) {
        match self.doc.load(path) {
            Ok(()) => {
                let parent = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                let base = if model::in_project_folder(path) { parent.parent().map(|d| d.to_path_buf()).unwrap_or(parent) } else { parent };
                self.settings.dir = path_str(&base);
                self.save_settings();
                // media that moved together with the paradigm file are found again by themselves
                let (found, missing) = Self::find_moved_media(&mut self.doc.paradigm, path);
                if !found.is_empty() {
                    self.doc.touch();
                }
                // the Media panel: every picture, video and sound the paradigm uses
                self.refill_media_panel();
                if !missing.is_empty() {
                    self.modal = Some(Modal::Relink(Box::new(RelinkDialog::new(missing, found, false))));
                } else if !found.is_empty() {
                    self.flash(format!("{} media file(s) were found again next to the paradigm – save to keep their new places.", found.len()));
                } else if model::media_outside(&self.doc.paradigm, path) > 0 && !model::in_project_folder(path) {
                    self.flash("Tip: “Save as…” puts this paradigm and all its media into one folder of its own – easy to copy to another computer.");
                }
                self.quick_last = None;
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
                    .set_title("Save paradigm – it gets a project folder of its own")
                    .add_filter("fMRI paradigm", &["json"])
                    .set_file_name(format!("{}{FILE_SUFFIX}", self.doc.paradigm.name));
                if !self.settings.dir.is_empty() {
                    d = d.set_directory(&self.settings.dir);
                }
                let Some(mut p) = d.save_file() else { return false };
                if !path_str(&p).ends_with(".json") {
                    p = PathBuf::from(format!("{}{FILE_SUFFIX}", path_str(&p)));
                }
                // every project in a folder of its own: Studies/Hariri/Hariri.paradigm.json + media/
                let target = model::project_file_for(&p);
                let same = self.doc.path.as_ref().is_some_and(|d| model::absolute(d) == target);
                if target.exists() && !same {
                    let folder = target.parent().map(path_str).unwrap_or_default();
                    let text = format!(
                        "There already is a project “{}” in\n{folder}\n\nReplace its paradigm file with this one? Its media folder is kept \
                         (files of the same name are not overwritten).",
                        model::project_stem(&target)
                    );
                    self.modal = Some(Modal::ReplaceProject(Confirm { title: "Replace project?".into(), text, yes: "Replace".into() }, target));
                    return false;
                }
                target
            }
        };
        self.save_to(&path)
    }

    /// Save to `path`. In a project folder, media from elsewhere are copied into its media folder first.
    fn save_to(&mut self, path: &Path) -> bool {
        let mut note = String::new();
        if model::in_project_folder(path) {
            let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
            match model::collect_media(&mut self.doc.paradigm, &dir) {
                Ok(rep) => {
                    self.lib.rename(&rep.moved);
                    if rep.copied > 0 {
                        note.push_str(&format!(" – {} media file{} copied into media/", rep.copied, if rep.copied == 1 { "" } else { "s" }));
                    }
                    if !rep.missing.is_empty() {
                        note.push_str(&format!(" – {} media file(s) are missing (File → Find missing media…)", rep.missing.len()));
                    }
                }
                Err(e) => {
                    self.message("Could not copy the media into the project folder", e);
                    return false;
                }
            }
        }
        match self.doc.save(path) {
            Ok(()) => {
                // the next "Save as…" starts next to this project, not inside its folder
                let parent = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                let base = if model::in_project_folder(path) { parent.parent().map(|d| d.to_path_buf()).unwrap_or(parent) } else { parent };
                self.settings.dir = path_str(&base);
                self.save_settings();
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let place = if model::in_project_folder(path) { " in its project folder" } else { "" };
                self.flash(format!("Saved {name}{place}{note}"));
                true
            }
            Err(e) => {
                self.message("Could not save", e);
                false
            }
        }
    }

    /// Media files that are not where the paradigm says: look for them next to the paradigm file
    /// (e.g. in its media folder after the whole folder was copied). Returns (found, still missing).
    fn find_moved_media(par: &mut Paradigm, path: &Path) -> (Vec<(String, String)>, Vec<String>) {
        let missing = model::missing_media(par);
        if missing.is_empty() {
            return (vec![], vec![]);
        }
        let dir = model::absolute(path).parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let found = model::find_media(&missing, &[dir]);
        model::relink_media(par, &found);
        let still = model::missing_media(par);
        (found, still)
    }

    /// The Media panel again from the blocks.
    fn refill_media_panel(&mut self) {
        let mut v: Vec<String> = self.doc.paradigm.blocks.iter().flat_map(|b| b.media_files()).collect();
        v.sort();
        v.dedup();
        self.lib.clear();
        self.lib.add_files(&v);
    }

    /// Screenshot scenes: a demo paradigm from the source tree (docs/demo) – or any file with
    /// PARADIGMSHIFT_DEMO_FILE=path. Not part of the app for users: ready-made paradigms come from
    /// "New from template…".
    fn open_demo(&mut self, name: &str) {
        if let Ok(f) = std::env::var("PARADIGMSHIFT_DEMO_FILE") {
            self.open_path(Path::new(&f));
            return;
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs").join("demo");
        self.open_path(&dir.join(format!("{name}{FILE_SUFFIX}")));
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
        matches!(a, Action::New | Action::Open(_) | Action::Templates | Action::Quit)
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
            Action::Save => {
                self.save(false);
            }
            Action::SaveAs => {
                self.save(true);
            }
            Action::FindMedia => {
                let missing = model::missing_media(&self.doc.paradigm);
                if !missing.is_empty() {
                    self.modal = Some(Modal::Relink(Box::new(RelinkDialog::new(missing, vec![], false))));
                }
            }
            Action::Undo => self.doc.undo(),
            Action::Redo => self.doc.redo(),
            Action::Templates => {
                self.modal = Some(Modal::Templates(Box::new(TemplateDialog::new(&self.doc.paradigm.hardware, presets::default_stim_dir(), "hariri"))))
            }
            Action::QuickDesign => {
                // the quick design builds image blocks: only the pictures of the Media panel
                let pictures: Vec<String> = self.lib.media.iter().filter(|f| model::is_image(f)).cloned().collect();
                let blur = self.settings.prefs.blur_previews;
                let d = match self.quick_last.take() {
                    Some(mut d) => {
                        d.refresh(&self.doc.paradigm, &pictures, blur);
                        d
                    }
                    None => Box::new(QuickDesign::new(&self.doc.paradigm, &pictures, blur)),
                };
                self.modal = Some(Modal::Quick(d))
            }
            Action::Hardware => {
                let par = self.present.as_ref().map(|p| &p.par).unwrap_or(&self.doc.paradigm);
                self.modal = Some(Modal::Hardware(Box::new(HardwareDialog::new(ctx, &par.hardware, par.tr))))
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
            Action::OpenForPresenting(path) => self.open_for_presenting(path),
            Action::LeavePresentation => {
                self.present = None;
                self.flash("Back in the editor.".to_string());
            }
            Action::FunctionTest => {
                if self.run.is_none() && self.test.is_none() {
                    let par = self.present.as_ref().map(|p| &p.par).unwrap_or(&self.doc.paradigm);
                    let monitors = if self.monitors.is_empty() { vec!["1: main screen".to_string()] } else { self.monitors.clone() };
                    self.modal = Some(Modal::Test(Box::new(TestDialog::new(&par.hardware, par.tr, &par.background, monitors))));
                }
            }
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
            Action::About => self.modal = Some(Modal::About(Box::new(AboutDialog::new(ctx)))),
            Action::Quit => {
                self.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::ImportMedia => {
                let files = panels::pick_media_files("Import pictures, videos and sounds");
                self.lib.add_files(&files);
            }
            Action::RemoveMedia(files) => {
                let n = self.lib.remove(&files);
                if n > 0 {
                    self.flash(format!("Removed {n} file{} from the Media panel – the files and the timeline are unchanged.", if n == 1 { "" } else { "s" }));
                }
            }
            Action::RemoveUnusedMedia => {
                let used: std::collections::HashSet<String> = self.doc.paradigm.blocks.iter().flat_map(|b| b.media_files()).collect();
                let unused: Vec<String> = self.lib.media.iter().filter(|f| !used.contains(*f)).cloned().collect();
                let n = self.lib.remove(&unused);
                self.flash(if n == 0 {
                    "Every file in the Media panel is used in the timeline.".to_string()
                } else {
                    format!("Removed {n} file{} that no block uses from the Media panel.", if n == 1 { "" } else { "s" })
                });
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
            Action::TogglePanel(key) => {
                self.lib.toggle(&key);
                self.settings.library_closed = self.lib.closed.clone();
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
        if self.run.is_some() || self.test.is_some() {
            return;
        }
        let par = self.present.as_ref().map(|p| &p.par).unwrap_or(&self.doc.paradigm);
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
        let mut d = RunDialog::new(par, &self.settings.prefs.data_dir, monitors);
        // presentation mode: the next run continues from the last one (same participant, next run number)
        if let Some(last) = self.present.as_ref().and_then(|p| p.last.clone()) {
            d.s.sub = last.sub;
            d.s.ses = last.ses;
            d.s.run = (last.run + 1).min(99);
            d.s.out_dir = last.out_dir;
            d.s.screen = last.screen;
            d.s.fullscreen = last.fullscreen;
            // never carried over: a real scan must not run with simulated triggers by mistake
            d.s.dummy_scans = last.dummy_scans;
            d.s.waiting_screen = last.waiting_screen;
            d.s.waiting_text = last.waiting_text;
            d.s.end_text = last.end_text;
        }
        self.modal = Some(Modal::Run(Box::new(d)));
    }

    /// "Open for presenting": load a paradigm read-only and go straight to the run dialog.
    fn open_for_presenting(&mut self, path: Option<PathBuf>) {
        if self.run.is_some() || self.test.is_some() {
            return;
        }
        let path = match path {
            Some(p) => p,
            None => {
                let mut d = rfd::FileDialog::new().set_title("Open a paradigm for presenting (read-only)").add_filter("fMRI paradigm", &["json"]);
                if !self.settings.dir.is_empty() {
                    d = d.set_directory(&self.settings.dir);
                }
                match d.pick_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        match crate::model::Paradigm::load(&path) {
            Ok(mut par) => {
                self.settings.dir = path.parent().map(path_str).unwrap_or_default();
                self.save_settings();
                self.playing = None;
                // moved media are looked for next to the file (only for these runs – the file is not changed)
                let (found, missing) = Self::find_moved_media(&mut par, &path);
                let problems = par.validate();
                self.present = Some(Presenter { par, path, problems, runs: 0, last: None });
                if missing.is_empty() {
                    self.actions.push(Action::Run);
                } else {
                    self.modal = Some(Modal::Relink(Box::new(RelinkDialog::new(missing, found, true))));
                }
            }
            Err(e) => self.message("Could not open", e),
        }
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
        if let Some(pr) = self.present.as_mut() {
            // presentation mode: the settings apply to this run only – the file stays as it is
            let mut par = pr.par.clone();
            par.dummy_scans = s.dummy_scans;
            par.waiting_screen = s.waiting_screen.clone();
            par.waiting_text = s.waiting_text.clone();
            par.end_text = s.end_text.clone();
            pr.last = Some(s.clone());
            pr.runs += 1;
            self.launch_run(ctx, par, s);
            return;
        }
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
        let par = self.doc.paradigm.clone();
        self.launch_run(ctx, par, s);
    }

    fn launch_run(&mut self, ctx: &Context, par: crate::model::Paradigm, s: RunSettings) {
        self.playing = None;
        // preload pictures – no disk access while running
        let imgs: Vec<String> = par.blocks.iter().flat_map(|b| b.image_files()).collect();
        stimulus::preload(ctx, &imgs);
        // sounds (audio blocks, the sound of videos): decoded into memory now, played from memory
        let sound = Self::load_sounds(&par);
        // videos that start right away: decode their first frames while waiting for the scanner
        for b in par.blocks.iter().filter(|b| b.kind == "video" && b.start < 1.5) {
            crate::media::video_prepare(&b.id, &b.params.s("path", ""));
        }
        let c = ctx.clone();
        let mut ctl = RunController::new(par, s, Arc::new(move || c.request_repaint()));
        match sound {
            Ok(Some((engine, clips))) => ctl.set_sound(engine, clips),
            Ok(None) => {}
            Err(e) => ctl.error = e,
        }
        ctl.start();
        self.run = Some(RunSession::new(ctx, ctl, self.settings.controlroom_folded.clone()));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Decode the sounds of a paradigm and open the sound output – if one is chosen and there is
    /// anything to play. Errors keep the run going (silently) but are shown in the control room.
    #[allow(clippy::type_complexity)]
    fn load_sounds(
        par: &crate::model::Paradigm,
    ) -> Result<Option<(crate::media::AudioEngine, std::collections::HashMap<String, (crate::media::AudioClip, f32)>)>, String> {
        let device = par.hardware.audio_device.clone();
        let wanted: Vec<(&crate::model::Block, String)> = par.blocks.iter().filter_map(|b| b.sound_file().map(|f| (b, f))).collect();
        if device.is_empty() || wanted.is_empty() {
            return Ok(None);
        }
        let mut by_file: std::collections::HashMap<String, crate::media::AudioClip> = std::collections::HashMap::new();
        let mut clips = std::collections::HashMap::new();
        let mut problems = vec![];
        for (b, f) in wanted {
            let clip = match by_file.get(&f) {
                Some(c) => Some(c.clone()),
                None => match crate::media::decode_audio(&f) {
                    Ok(c) => {
                        by_file.insert(f.clone(), c.clone());
                        Some(c)
                    }
                    // a video without a sound track is fine
                    Err(e) if b.kind == "video" && e.contains("has no sound") => None,
                    Err(e) => {
                        problems.push(e);
                        None
                    }
                },
            };
            if let Some(c) = clip {
                clips.insert(b.id.clone(), (c, b.params.f("volume", 1.0) as f32));
            }
        }
        let engine = crate::media::AudioEngine::start(&device).map_err(|e| format!("Sound: {e}"))?;
        if !problems.is_empty() {
            problems.dedup();
            return Err(format!("Sound: {}", problems.join(" · ")));
        }
        Ok(Some((engine, clips)))
    }

    // ------------------------------------------------------------------ keyboard
    fn shortcuts(&mut self, ctx: &Context) {
        if self.modal.is_some() || self.popup.is_some() {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let list: [(Modifiers, Key, Action); 28] = [
            (cs, Key::S, Action::SaveAs),
            (cs, Key::O, Action::OpenForPresenting(None)),
            (cs, Key::F, Action::FunctionTest),
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
            (cmd, Key::Num1, Action::TogglePanel("blocks".into())),
            (cmd, Key::Num2, Action::TogglePanel("tasks".into())),
            (cmd, Key::Num3, Action::TogglePanel("pictures".into())),
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
        // Del over the Media panel with files selected there: take them out of the panel
        if self.lib.wants_delete(ctx) && (plain(Key::Delete) || plain(Key::Backspace)) {
            self.actions.push(Action::RemoveMedia(self.lib.selected_files()));
            return;
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
                item!(ui, "Open for presenting…", "Ctrl+Shift+O", Action::OpenForPresenting(None), true);
                item!(ui, "Save", "Ctrl+S", Action::Save, true);
                item!(ui, "Save as…  (own project folder)", "Ctrl+Shift+S", Action::SaveAs, true);
                let missing = !model::missing_media(&self.doc.paradigm).is_empty();
                item!(ui, "Find missing media…", "", Action::FindMedia, missing);
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
                item!(ui, "Function test…", "Ctrl+Shift+F", Action::FunctionTest, true);
                ui.separator();
                item!(ui, "Run…", "F5", Action::Run, true);
            });
            ui.menu_button("View", |ui| {
                item!(ui, if self.playing.is_some() { "Pause" } else { "Play" }, "Space", Action::TogglePlay, true);
                item!(ui, "Go to start", "Home", Action::Seek(0.0), true);
                item!(ui, "Zoom in", "Ctrl+=", Action::ZoomIn, true);
                item!(ui, "Zoom out", "Ctrl+-", Action::ZoomOut, true);
                item!(ui, "Fit whole run", "Ctrl+0", Action::ZoomFit, true);
                ui.separator();
                for (n, (key, title, _)) in panels::PANELS.iter().enumerate() {
                    let mark = if self.lib.is_open(key) { "☑" } else { "☐" };
                    item!(ui, &format!("{mark}  {title} panel"), &format!("Ctrl+{}", n + 1), Action::TogglePanel(key.to_string()), true);
                }
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
                item!(ui, "About ParadigmShift…", "", Action::About, true);
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
            // New, Open, Save and Open for presenting live in the File menu
            tb(ui, "↶ Undo", "Undo (Ctrl+Z)", Action::Undo, self.doc.can_undo());
            tb(ui, "↷ Redo", "Redo (Ctrl+Shift+Z)", Action::Redo, self.doc.can_redo());
            ui.separator();
            tb(ui, "▦ Templates", "Start from a classic task: Hariri, Go/No-Go, emotion regulation … (Ctrl+T)", Action::Templates, true);
            tb(ui, "✦ Quick design", "Build a block or event-related design in a few clicks (Ctrl+Shift+N)", Action::QuickDesign, true);
            tb(ui, "⚙ Hardware", "Set up and test the NNL SyncBox and ResponseGrips (Ctrl+Shift+H)", Action::Hardware, true);
            tb(
                ui,
                "✓ Function test",
                "Test screen for the participant: grip buttons light up, scanner pulses flash, the display is checked (Ctrl+Shift+F)",
                Action::FunctionTest,
                true,
            );
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
                    if conds.is_empty() { String::new() } else { format!("  ·  {} condition{}", conds.len(), if conds.len() == 1 { "" } else { "s" }) }
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
            ui.set_height(ui.available_height().max(0.0));
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

    /// The tool strip on the left: always visible, one button per library panel. Minimised panels
    /// live here; a click brings them back (or minimises an open one), like in an image or video editor.
    fn tool_strip(&mut self, ui: &mut Ui) {
        // the strip is a framed panel like the others: the panel's 8 px margin keeps the same gaps,
        // and its buttons fill the frame exactly (nothing overflows and gets clipped)
        theme::panel_frame().inner_margin(egui::Margin::symmetric(STRIP_PAD as i8, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height().max(0.0));
            ui.vertical(|ui| {
                let n = self.lib.media.len();
                for (i, (key, title, _)) in panels::PANELS.iter().enumerate() {
                    let open = self.lib.is_open(key);
                    let label = title.to_uppercase();
                    let g = ui.painter().layout_no_wrap(label, theme::bold(11.0), TEXT_DIM);
                    let h = 32.0 + g.size().x + 12.0;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(STRIP_BUTTON_W, h), egui::Sense::click());
                    let hovered = resp.hovered();
                    if open || hovered {
                        ui.painter().rect_filled(r, egui::CornerRadius::same(6), if hovered { theme::PANEL_2 } else { theme::BG });
                    }
                    if open {
                        // accent bar: this panel is shown
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(r.left_top() + Vec2::new(0.0, 4.0), Vec2::new(3.0, r.height() - 8.0)),
                            egui::CornerRadius::same(1),
                            ACCENT,
                        );
                    }
                    let icon = egui::Rect::from_center_size(Pos2::new(r.center().x + 1.0, r.top() + 16.0), Vec2::splat(22.0));
                    panels::paint_strip_icon(ui.painter(), key, icon, open || hovered);
                    // vertical caption, read bottom-to-top like in video editors
                    let pos = Pos2::new(r.center().x + 1.0 - g.size().y / 2.0, r.bottom() - 6.0);
                    let color = if open || hovered { theme::TEXT } else { TEXT_DIM };
                    ui.painter().add(egui::epaint::TextShape::new(pos, g, color).with_angle(-std::f32::consts::FRAC_PI_2));
                    let extra = if *key == "pictures" { format!(" ({n} imported)") } else { String::new() };
                    let tip = if open {
                        format!("{title}{extra} – click to minimise (Ctrl+{})", i + 1)
                    } else {
                        format!("{title}{extra} – click to show (Ctrl+{})", i + 1)
                    };
                    if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.actions.push(Action::TogglePanel(key.to_string()));
                    }
                    ui.add_space(8.0);
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
                // the SyncBox in the corner, then the paradigm's state
                self.syncbox_indicator(ui);
                ui.separator();
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

    /// The hardware setup in use: the presented paradigm's, or the editor's.
    fn active_hardware(&self) -> &crate::model::Hardware {
        &self.present.as_ref().map(|p| &p.par).unwrap_or(&self.doc.paradigm).hardware
    }

    /// Keep the SyncBox monitor in step: check again when the hardware setup changes, keep it off
    /// the port while something else uses it, and take over the port the box was found on.
    fn syncbox_upkeep(&mut self) {
        let Some(mon) = &self.syncbox else { return };
        let hw = self.active_hardware().clone();
        let busy = self.run.is_some() || self.test.is_some() || matches!(self.modal, Some(Modal::Hardware(_)));
        if busy != self.syncbox_paused {
            self.syncbox_paused = busy;
            mon.pause(busy);
            if !busy {
                mon.check(&hw); // after a run / test / the hardware dialog: is it still there?
            }
        }
        let sig = format!("{}|{}|{}", hw.sync_mode, hw.serial_port, hw.baud_rate);
        if sig != self.syncbox_checked {
            self.syncbox_checked = sig;
            mon.check(&hw);
        }
        // found on another port (e.g. after plugging it into another USB socket): use that one
        if let Some(crate::hardware::SyncBoxStatus::Connected(port)) = mon.status() {
            if hw.sync_mode == "serial" && hw.serial_port != port {
                let mut new_hw = hw.clone();
                new_hw.serial_port = port.clone();
                if let Some(pr) = self.present.as_mut() {
                    pr.par.hardware.serial_port = port.clone();
                } else {
                    self.doc.paradigm.hardware.serial_port = port.clone();
                }
                self.syncbox_checked = format!("{}|{}|{}", new_hw.sync_mode, new_hw.serial_port, new_hw.baud_rate);
                self.settings.hardware = Some(new_hw);
                self.save_settings();
                self.flash(format!("SyncBox found on {port} – using this port."));
            }
        }
    }

    /// "● SyncBox connected · COM3" in the corner of the status bar; a click checks again.
    fn syncbox_indicator(&mut self, ui: &mut Ui) {
        use crate::hardware::SyncBoxStatus;
        let Some(mon) = &self.syncbox else { return };
        let (dot, text, tip) = match mon.status() {
            None | Some(SyncBoxStatus::Checking) => (TEXT_DIM, "SyncBox: checking…".to_string(), "Asking the SyncBox on its serial port …".to_string()),
            Some(SyncBoxStatus::Keyboard) => (
                TEXT_DIM,
                "SyncBox: USB keyboard mode".to_string(),
                "In USB keyboard mode the SyncBox is just a keyboard and can't be checked. Switch to serial mode in Hardware to see whether it is connected.".to_string(),
            ),
            Some(SyncBoxStatus::Connected(p)) => (OK, format!("SyncBox connected · {p}"), format!("The SyncBox answered on {p}.")),
            Some(SyncBoxStatus::NotConnected(why)) => (DANGER, "SyncBox not connected".to_string(), why),
        };
        let resp = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.add(egui::Label::new(RichText::new(text).color(if dot == TEXT_DIM { TEXT_DIM } else { theme::TEXT })).sense(egui::Sense::click()))
                    | {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::click());
                        ui.painter().circle_filled(r.center(), 4.5, dot);
                        resp
                    }
            })
            .inner;
        let hw = self.active_hardware().clone();
        if resp
            .on_hover_text(format!("{tip}\n\nClick to check again · Hardware settings: Ctrl+Shift+H"))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            if let Some(mon) = &self.syncbox {
                mon.check(&hw);
            }
        }
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
            Modal::About(d) => keep = d.ui(ctx) == Outcome::Open,
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
                    if let Some(pr) = self.present.as_mut() {
                        // presentation mode: for these runs only – the paradigm file is not changed
                        pr.par.hardware = d.hw.clone();
                    } else {
                        self.doc.paradigm.hardware = d.hw.clone();
                        self.doc.touch();
                    }
                    self.settings.hardware = Some(d.hw.clone());
                    self.save_settings();
                    keep = false;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::Quick(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    self.quick_last = Some(d.clone());
                    let blocks = d.blocks(None);
                    let replace = d.replace;
                    let tr = d.tr();
                    if !blocks.is_empty() {
                        let paths: Vec<String> = blocks.iter().filter_map(|b| b.params.get("path").and_then(|v| v.as_str()).map(String::from)).collect();
                        self.doc.edit(|par| {
                            par.tr = tr;
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
                Outcome::Cancel => {
                    self.quick_last = Some(d.clone());
                    keep = false;
                }
                _ => {}
            },
            Modal::Run(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    let mut s = d.s.clone();
                    if s.out_dir.trim().is_empty() {
                        s.out_dir = self.settings.prefs.data_dir.clone();
                    }
                    // never overwrite data by accident: ask first if this participant's run exists
                    let par = self.present.as_ref().map(|p| &p.par).unwrap_or(&self.doc.paradigm);
                    let existing = crate::runner::existing_outputs(par, &s);
                    if !existing.is_empty() {
                        let dated = crate::runner::dated_suffix(par, &s);
                        self.modal = Some(Modal::FileClash(Box::new(FileClash::new(par, s, (**d).clone(), existing, dated))));
                        return;
                    }
                    self.modal = None;
                    self.start_run(ctx, s);
                    return;
                }
                Outcome::Cancel => keep = false,
                _ => {}
            },
            Modal::FileClash(c) => match c.ui(ctx) {
                ClashChoice::AddDate => {
                    let s = crate::runner::RunSettings { name_suffix: c.dated.clone(), ..c.s.clone() };
                    self.modal = None;
                    self.start_run(ctx, s);
                    return;
                }
                ClashChoice::Overwrite => {
                    let s = c.s.clone();
                    self.modal = None;
                    self.start_run(ctx, s);
                    return;
                }
                ClashChoice::Cancel => {
                    self.modal = Some(Modal::Run(Box::new(c.back.clone())));
                    return;
                }
                ClashChoice::Open => {}
            },
            Modal::Test(d) => match d.ui(ctx) {
                Outcome::Accept => {
                    let s = d.s.clone();
                    self.modal = None;
                    self.playing = None;
                    let hw = self.present.as_ref().map(|p| &p.par).unwrap_or(&self.doc.paradigm).hardware.clone();
                    self.test = Some(TestSession::new(ctx, hw, s));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
                        self.quick_last = None;
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
            Modal::Relink(d) => match d.ui(ctx) {
                RelinkAction::Search => {
                    if let Some(dir) = dialogs::pick_folder("Folder with the missing media files", &self.settings.dir) {
                        let found = model::find_media(&d.missing, &[PathBuf::from(dir)]);
                        if d.present {
                            if let Some(pr) = self.present.as_mut() {
                                model::relink_media(&mut pr.par, &found);
                                pr.problems = pr.par.validate();
                            }
                        } else if !found.is_empty() {
                            model::relink_media(&mut self.doc.paradigm, &found);
                            self.lib.rename(&found);
                            self.doc.touch();
                        }
                        d.add_found(found);
                    }
                }
                RelinkAction::SaveAsProject => {
                    keep = false;
                    self.actions.push(Action::SaveAs);
                }
                RelinkAction::Close => {
                    keep = false;
                    if d.present {
                        self.actions.push(Action::Run);
                    }
                }
                RelinkAction::Open => {}
            },
            Modal::ReplaceProject(c, target) => match c.ui(ctx) {
                Outcome::Accept => {
                    keep = false;
                    let target = target.clone();
                    self.save_to(&target);
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
        // pictures dropped on the quick design go into its shown condition
        if let Some(Modal::Quick(d)) = self.modal.as_mut() {
            d.add_pictures(panels::expand_images(&dropped));
            return;
        }
        if let Some(p) = dropped.iter().find(|p| path_str(p).ends_with(".json")) {
            self.actions.push(Action::Open(Some(p.clone())));
            return;
        }
        // pictures, videos and sounds (also whole folders) go into the Media panel; dropped onto the
        // timeline they become image / video / audio blocks
        let files = panels::expand_media(&dropped);
        if files.is_empty() {
            return;
        }
        self.lib.add_files(&files);
        if let Some(pos) = ctx.pointer_latest_pos() {
            if self.tl.contains(pos) {
                self.tl.drop_images(&mut self.doc, files, pos);
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
            None if self.test.is_some() => "Function test — ParadigmShift".to_string(),
            None => match &self.present {
                Some(p) => format!("Presenting {} (read-only) — ParadigmShift", p.par.name),
                None => format!("{} — ParadigmShift{}", self.doc.paradigm.name, if self.doc.dirty { " •" } else { "" }),
            },
        };
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }

    /// Presentation mode: no editor – just the paradigm, the run button and the function test.
    fn present_home(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.modal.is_none() {
            let cs = Modifiers::COMMAND | Modifiers::SHIFT;
            ctx.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::F5) {
                    self.actions.push(Action::Run);
                }
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)) || i.consume_shortcut(&KeyboardShortcut::new(cs, Key::O)) {
                    self.actions.push(Action::OpenForPresenting(None));
                }
                if i.consume_shortcut(&KeyboardShortcut::new(cs, Key::F)) {
                    self.actions.push(Action::FunctionTest);
                }
            });
        }
        // the status bar of presentation mode: the SyncBox in the corner
        egui::Panel::bottom("present_status").frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(12, 4))).show(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.syncbox_indicator(ui));
        });
        let Some(pr) = self.present.as_ref() else { return };
        let a = &mut self.actions;
        egui::Panel::top("present_bar").frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(14, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("ParadigmShift").font(theme::bold(15.0)));
                let g = ui.painter().layout_no_wrap("PRESENTATION MODE  ·  READ-ONLY".into(), theme::bold(11.0), ACCENT);
                let (r, _) = ui.allocate_exact_size(g.size() + Vec2::new(20.0, 10.0), egui::Sense::hover());
                ui.painter().rect_stroke(r, egui::CornerRadius::same(11), egui::Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
                ui.painter().galley(r.center() - g.size() / 2.0, g, ACCENT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("✎ Back to the editor").on_hover_text("Leave presentation mode. The presented file is not opened for editing.").clicked() {
                        a.push(Action::LeavePresentation);
                    }
                    if ui.button("About").on_hover_text("About ParadigmShift: version, credits, repository").clicked() {
                        a.push(Action::About);
                    }
                    if ui.button("Open another…").on_hover_text("Open another paradigm for presenting (Ctrl+O)").clicked() {
                        a.push(Action::OpenForPresenting(None));
                    }
                    if ui.button("✓ Function test").on_hover_text("Test screen: grip buttons, scanner pulses, display (Ctrl+Shift+F)").clicked() {
                        a.push(Action::FunctionTest);
                    }
                    if ui.button("⚙ Hardware").on_hover_text("SyncBox and grips for these runs (the file is not changed)").clicked() {
                        a.push(Action::Hardware);
                    }
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG).inner_margin(16)).show(ui, |ui| {
            let w = ui.available_width().min(680.0);
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.08).min(60.0));
                ui.allocate_ui_with_layout(Vec2::new(w, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                    theme::panel_frame().inner_margin(egui::Margin::same(24)).show(ui, |ui| {
                        ui.set_width(w - 50.0);
                        let p = &pr.par;
                        ui.label(RichText::new(&p.name).font(theme::bold(26.0)));
                        ui.label(RichText::new(pr.path.display().to_string()).color(TEXT_DIM).size(12.0));
                        ui.add_space(12.0);
                        let hw = &p.hardware;
                        let sync = if hw.sync_mode == "keyboard" {
                            format!("USB keyboard, trigger key “{}”", hw.trigger_key)
                        } else {
                            format!("serial port {}", if hw.serial_port.is_empty() { "(not set!)" } else { &hw.serial_port })
                        };
                        let conds = p.conditions();
                        let facts = [
                            ("Duration", fmt_time(p.duration(), 1)),
                            ("Volumes", format!("{}  at TR {} s  (+ {} dummy scans)", p.volumes(), crate::model::fmt_g(p.tr), p.dummy_scans)),
                            ("Blocks", format!("{}  ·  conditions: {}", p.blocks.len(), if conds.is_empty() { "–".into() } else { conds.join(", ") })),
                            ("SyncBox", sync),
                            (
                                "Runs",
                                match &pr.last {
                                    Some(l) => format!("{} in this session  ·  last: {} run {}", pr.runs, l.sub, l.run),
                                    None => "none yet in this session".into(),
                                },
                            ),
                        ];
                        egui::Grid::new("present_facts").striped(false).num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
                            for (k, v) in facts {
                                ui.label(RichText::new(k).color(TEXT_DIM));
                                ui.add(egui::Label::new(v).wrap());
                                ui.end_row();
                            }
                        });
                        if !pr.problems.is_empty() {
                            ui.add_space(10.0);
                            let txt = pr.problems.iter().take(5).map(|p| format!("• {p}")).collect::<Vec<_>>().join("\n");
                            ui.label(RichText::new("⚠ This paradigm has issues:").color(DANGER).strong());
                            ui.add(egui::Label::new(RichText::new(txt).color(DANGER).size(12.5)).wrap());
                        }
                        ui.add_space(18.0);
                        ui.vertical_centered(|ui| {
                            let b = egui::Button::new(RichText::new("▶   Run in scanner…").font(theme::bold(17.0)).color(egui::Color32::WHITE))
                                .fill(OK)
                                .min_size(Vec2::new(260.0, 44.0));
                            if ui.add(b).on_hover_text("Participant, run number and screen – then the participant screen opens and waits for the scanner (F5)").clicked() {
                                a.push(Action::Run);
                            }
                        });
                        ui.add_space(14.0);
                        theme::hint(
                            ui,
                            "The file is open read-only: nothing in it can be changed here. Settings chosen for a run (screen, dummy \
                             scans, hardware) apply to the runs only. After a run you come back to this page – the next run continues \
                             with the same participant and the next run number.",
                        );
                    });
                });
            });
        });
        self.modals(&ctx);
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
        egui::Panel::left("tool_strip")
            .frame(frame)
            .resizable(false)
            // panel margin + frame margin + stroke on both sides around the buttons
            .exact_size(STRIP_BUTTON_W + 2.0 * (8.0 + STRIP_PAD + 1.0))
            .show_separator_line(false)
            .show(ui, |ui| self.tool_strip(ui));
        if self.lib.any_open() {
            egui::Panel::left("library")
                .frame(frame)
                .resizable(true)
                .default_size(268.0)
                .size_range(240.0..=340.0)
                .show_separator_line(false)
                .show(ui, |ui| {
                    let blur = self.settings.prefs.blur_previews;
                    if self.lib.ui(ui, blur, &mut self.actions) {
                        self.settings.library_weights = self.lib.weights.clone();
                        self.layout_dirty = true;
                    }
                });
        }
        // remember a dragged panel layout once the mouse is released (not on every frame of the drag)
        if self.layout_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.layout_dirty = false;
            self.save_settings();
        }
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
    /// Screenshots: ignore where the mouse happens to be, so no hover or tooltip ends up in them.
    fn raw_input_hook(&mut self, _ctx: &Context, raw_input: &mut egui::RawInput) {
        if self.shot.is_some() {
            raw_input.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }));
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        stimulus::poll(&ctx);
        if self.monitors.is_empty() {
            if let Some(w) = frame.winit_window() {
                // the monitors' own names (as in the system's display settings), not "Display 1"
                self.monitors = crate::monitors::describe(w);
            }
        }
        self.update_title(&ctx);
        self.syncbox_upkeep();
        if let Some(t) = self.test.as_mut() {
            t.show_screen(&ctx);
            if t.ui(ui) {
                self.test = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        } else if self.run.is_some() {
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
        } else if self.present.is_some() {
            self.present_home(ui);
            // the editor's paradigm is kept while presenting – don't lose unsaved changes on quit
            if ctx.input(|i| i.viewport().close_requested()) && !self.quit && self.doc.dirty && self.shot.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.modal = Some(Modal::Unsaved(Action::Quit));
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
        crate::media::housekeeping();
        self.shot_overlay(&ctx);
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
    /// Screenshot scenes of the participant's screen: paint exactly what it shows over the whole
    /// main window (same painter, same reference height), so the main window can be captured.
    /// Use with `--size 1280x720` to get the participant window's size.
    fn shot_overlay(&mut self, ctx: &Context) {
        let Some(scene) = self.shot.as_ref().map(|p| p.scene.clone()) else { return };
        if !(scene == "participant" || scene == "test_participant") {
            return;
        }
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("shot_overlay")));
        if let Some(s) = &self.run {
            if let Ok(c) = s.ctl.lock() {
                crate::controlroom::paint_participant(&painter, rect, &c, false);
            }
        }
        if let Some(t) = &self.test {
            t.paint_full(&painter, rect);
        }
    }

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
                "editor" | "bulk" | "find" | "blur" | "rating_direct" => {
                    self.open_demo("emotional_rating");
                    let ratings: Vec<String> = self.doc.paradigm.blocks.iter().filter(|b| b.kind == "rating").map(|b| b.id.clone()).collect();
                    if scene == "bulk" {
                        self.doc.select(ratings);
                    } else if let Some(id) = ratings.get(3) {
                        let t = self.doc.paradigm.block(id).map(|b| b.start + 1.3).unwrap_or(0.0);
                        self.doc.select(vec![id.clone()]);
                        self.tl.set_playhead(t, false);
                    }
                    if scene == "rating_direct" {
                        let sel = self.doc.selection.clone();
                        self.doc.edit(|p| {
                            for id in &sel {
                                if let Some(b) = p.block_mut(id) {
                                    b.params.insert("mode".into(), serde_json::json!("direct"));
                                    b.params.insert("value_buttons".into(), serde_json::json!(["Left thumb", "Left index", "Right index", "Right thumb"]));
                                    b.params.insert("max".into(), serde_json::json!(5));
                                }
                            }
                        });
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
                "quick" | "quick_event" | "quick_jitter" => {
                    self.open_demo("emotional_rating");
                    let pictures: Vec<String> = self.lib.media.iter().filter(|f| model::is_image(f)).cloned().collect();
                    let mut d = QuickDesign::new(&self.doc.paradigm, &pictures, self.settings.prefs.blur_previews);
                    if scene == "quick_event" {
                        d.demo();
                    }
                    if scene == "quick_jitter" {
                        d.demo_jitter();
                    }
                    self.modal = Some(Modal::Quick(Box::new(d)));
                }
                "prefs" => self.actions.push(Action::Preferences),
                "panels" => {
                    self.open_demo("hariri_face_matching");
                    self.lib.closed = vec!["tasks".into()];
                    let pick = self.doc.paradigm.blocks.iter().find(|b| b.kind == "match").map(|b| (b.id.clone(), b.start + 0.1));
                    if let Some((id, t)) = pick {
                        self.doc.select(vec![id]);
                        self.tl.set_playhead(t, false);
                    }
                }
                "run_dialog" => {
                    self.open_demo("hariri_face_matching");
                    self.run_paradigm(true);
                }
                "file_clash" => {
                    // a run whose data exists already: the question before anything is overwritten
                    self.open_demo("hariri_face_matching");
                    let dir = std::env::temp_dir().join("paradigmshift-demo-clash");
                    let _ = std::fs::create_dir_all(&dir);
                    let par = self.doc.paradigm.clone();
                    let d = RunDialog::new(&par, &dir.to_string_lossy(), vec!["1: screen".into()]);
                    let s = d.s.clone();
                    let stem = crate::runner::output_stem(&par, &s);
                    for kind in ["events.tsv", "log.tsv", "buttons.tsv", "paradigm.json"] {
                        let _ = std::fs::write(dir.join(format!("{stem}_{kind}")), "");
                    }
                    let existing = crate::runner::existing_outputs(&par, &s);
                    let dated = crate::runner::dated_suffix(&par, &s);
                    self.modal = Some(Modal::FileClash(Box::new(FileClash::new(&par, s, d, existing, dated))));
                }
                "about" => {
                    self.open_demo("emotional_rating");
                    self.actions.push(Action::About);
                }
                "present" | "present_run" => {
                    self.open_demo("hariri_face_matching");
                    let path = self.doc.path.clone();
                    self.open_for_presenting(path);
                    if scene == "present" {
                        self.actions.retain(|a| !matches!(a, Action::Run));
                    }
                }
                "test" | "test_participant" => {
                    let s = crate::selftest::TestSettings {
                        fullscreen: false,
                        simulate: true,
                        window_size: Some((1280.0, 720.0)),
                        ..Default::default()
                    };
                    self.test = Some(TestSession::new(ctx, self.doc.paradigm.hardware.clone(), s));
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
                    self.open_demo(name);
                    let pick = self.doc.paradigm.blocks.iter().find(|b| b.start > 20.0 && (b.kind == "match" || b.kind == "choice" || b.kind == "gonogo" || b.kind == "image" || b.kind == "feedback")).map(|b| (b.id.clone(), b.start + 0.1));
                    if let Some((id, t)) = pick {
                        self.doc.select(vec![id]);
                        self.tl.set_playhead(t, false);
                    }
                }
                _ => {}
            }
            if run_scene {
                let small = scene.ends_with("_small");
                // test hooks for odd participant windows: PARADIGMSHIFT_DEMO_SIZE=WxH, PARADIGMSHIFT_DEMO_EXAMPLE=name
                let env_size = std::env::var("PARADIGMSHIFT_DEMO_SIZE").ok().and_then(|v| {
                    let p: Vec<f32> = v.split('x').filter_map(|x| x.parse().ok()).collect();
                    (p.len() == 2).then(|| (p[0], p[1]))
                });
                let example = std::env::var("PARADIGMSHIFT_DEMO_EXAMPLE").unwrap_or_else(|_| if small { "stroop".into() } else { "hariri_face_matching".into() });
                self.open_demo(&example);
                let s = RunSettings {
                    window_size: env_size.or(small.then_some((480.0, 360.0))),
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
        let test_scene = scene.starts_with("test");
        if test_scene && frame > 2 && frame % 41 == 0 {
            if let Some(t) = &self.test {
                if let Ok(mut s) = t.state.lock() {
                    // three of the four grips get pressed – the right thumb is still missing
                    let k = ["a", "b", "c", "b"][((frame / 41) % 4) as usize];
                    s.key(k, now());
                }
            }
        }
        if run_scene && frame > 2 && frame % 37 == 0 {
            if let Some(s) = &self.run {
                if let Ok(mut c) = s.ctl.lock() {
                    let k = if (frame / 37) % 3 == 0 { "c" } else { "b" };
                    c.key(k, now());
                }
            }
        }
        let ready = if test_scene {
            frame > 3000
                || self.test.as_ref().and_then(|t| t.state.lock().ok().map(|s| now() - s.t0 >= 6.6 && s.presses.iter().any(|p| p.0.map(|l| now() - l < 0.15).unwrap_or(false)))).unwrap_or(false)
        } else if run_scene {
            // wait until the run has reached a given point (an instruction / a matching trial)
            let target = std::env::var("PARADIGMSHIFT_DEMO_TIME").ok().and_then(|v| v.parse().ok()).unwrap_or(if scene.ends_with("_small") { 2.5 } else { 12.0 });
            frame > 6000
                || self.run.as_ref().and_then(|s| s.ctl.lock().ok().map(|c| c.phase() == crate::runner::Phase::Running && c.elapsed() >= target)).unwrap_or(false)
        } else {
            frame >= 40
        };
        let already = self.shot.as_ref().map(|p| p.requested).unwrap_or(true);
        if ready && !already {
            if let Some(p) = self.shot.as_mut() {
                p.requested = true;
            }
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
            // (the participant screens are immediate viewports, which eframe can't screenshot;
            // for those scenes the main window shows the very same picture – see `shot_overlay`)
            ctx.request_screenshot(cb);
        }
    }
}
