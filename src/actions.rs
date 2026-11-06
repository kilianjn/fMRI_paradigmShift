//! Commands that panels and menus queue for the main window to carry out after drawing.

use std::path::PathBuf;

use egui::Pos2;
use serde_json::Value;

/// Payload of a drag from the library or the picture bin onto the timeline.
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
    Quit,
    ImportPictures,
    AddKind { kind: String, start: Option<f64>, track: Option<usize> },
    AddPreset { key: String, start: Option<f64>, variant: Option<Value> },
    /// ask which variant of a task block to insert (popup at `pos`)
    ChooseVariant { key: String, start: Option<f64>, pos: Option<Pos2> },
    /// "add a block here" popup (double-click on the empty timeline)
    AddMenu { t: f64, track: usize, pos: Pos2 },
    CloseGaps(usize),
    Status(String),
    ToggleBlur,
    SetShowHints(bool),
    SetConfirmAbort(bool),
    SetSaveCopy(bool),
}
