//! Commands that panels and menus queue for the main window to carry out after drawing.

use std::path::PathBuf;

use egui::Pos2;
use serde_json::Value;

/// Payload of a drag from the library or the media bin onto the timeline.
#[derive(Clone, Debug)]
pub enum DragPayload {
    /// a block kind, or "preset:<key>" for a task block
    Kind(String),
    Images(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum Action {
    New,
    Open(Option<PathBuf>),
    OpenExample(String),
    Save,
    SaveAs,
    Undo,
    Redo,
    Templates,
    QuickDesign,
    Hardware,
    Export,
    TogglePlay,
    Seek(f64),
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ToggleSnap,
    Validate,
    Run,
    /// open a paradigm read-only and go straight to the run – no editor (None = ask for the file)
    OpenForPresenting(Option<PathBuf>),
    /// back from presentation mode to the editor
    LeavePresentation,
    /// the test screen for buttons, scanner pulses and display
    FunctionTest,
    Duplicate,
    RepeatAsk,
    Repeat(u32),
    Split,
    Delete { ripple: bool },
    SelectAll,
    Select(Vec<String>),
    FindReplace,
    Preferences,
    Shortcuts,
    /// Help → About
    About,
    Quit,
    /// pictures, videos and sounds into the Media panel
    ImportMedia,
    AddKind { kind: String, start: Option<f64>, track: Option<usize> },
    AddPreset { key: String, start: Option<f64>, variant: Option<Value> },
    /// ask which variant of a task block to insert (popup at `pos`)
    ChooseVariant { key: String, start: Option<f64>, pos: Option<Pos2> },
    /// "add a block here" popup (double-click on the empty timeline)
    AddMenu { t: f64, track: usize, pos: Pos2 },
    CloseGaps(usize),
    Status(String),
    ToggleBlur,
    /// minimise a library panel ("blocks" | "tasks" | "pictures") into the tool strip, or bring it back
    TogglePanel(String),
    SetShowHints(bool),
    SetConfirmAbort(bool),
    SetSaveCopy(bool),
}
