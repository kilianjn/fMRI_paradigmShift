//! User preferences and small app settings (stored as JSON in the per-user config folder).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::model::set_param_override;

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "ParadigmShift", "ParadigmShift")
}

pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| home_dir().join(".paradigmshift"))
}

/// Per-user data folder (placeholder pictures, unpacked examples).
pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| home_dir().join(".paradigmshift"))
}

pub fn home_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn default_data_dir() -> String {
    home_dir().join("ParadigmShift-data").to_string_lossy().to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    // general
    pub ui_scale: u32, // % of the normal text size
    pub open_last_file: bool,
    pub autosave_minutes: u32, // 0 = off; writes <file>.autosave next to the paradigm
    pub show_hints: bool,
    pub blur_previews: bool,       // blur pictures in the editor
    pub blur_control_mirror: bool, // blur pictures in the control-room mirror
    // new paradigms / new blocks
    pub new_tr: f64,
    pub new_dummy_scans: u32,
    pub new_background: String,
    pub new_waiting_screen: String,
    pub new_end_text: String,
    pub rating_mode: String, // cursor | direct
    pub match_frame: bool,
    // timeline
    pub snap: bool,
    pub snap_px: u32,
    // run & control room
    pub data_dir: String,
    pub confirm_abort: bool,
    pub save_paradigm_with_data: bool,
    pub watchdog_trials: u32,
    pub trigger_warning_trs: f64,
    pub control_refresh_ms: u32,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            ui_scale: 100,
            open_last_file: true,
            autosave_minutes: 5,
            show_hints: true,
            blur_previews: false,
            blur_control_mirror: false,
            new_tr: 2.0,
            new_dummy_scans: 3,
            new_background: "#000000".into(),
            new_waiting_screen: "fixation".into(),
            new_end_text: "Thank you!".into(),
            rating_mode: "cursor".into(),
            match_frame: true,
            snap: true,
            snap_px: 10,
            data_dir: default_data_dir(),
            confirm_abort: true,
            save_paradigm_with_data: true,
            watchdog_trials: 5,
            trigger_warning_trs: 2.5,
            control_refresh_ms: 100,
        }
    }
}

impl Prefs {
    /// Make new blocks follow the preferences (e.g. the rating answer mode).
    pub fn apply_block_defaults(&self) {
        set_param_override("rating", "mode", json!(self.rating_mode));
        set_param_override("match", "show_choice", json!(self.match_frame));
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub prefs: Prefs,
    pub last_file: String,
    pub dir: String,
    pub controlroom_folded: Vec<String>,
    /// library panel on the left: hidden (narrow strip) and its folded sections
    pub library_hidden: bool,
    pub library_blocks_folded: bool,
    pub library_pictures_folded: bool,
    pub hardware: Option<crate::model::Hardware>,
}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, t);
        }
    }
}
