//! ParadigmShift – a film-editor style app for designing and running fMRI paradigms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod controlroom;
mod dialogs;
mod document;
mod hardware;
mod model;
mod panels;
mod placeholders;
mod prefs;
mod presets;
mod rng;
mod runner;
mod selftest;
mod stimulus;
mod templates;
mod theme;
mod timeline;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut file: Option<PathBuf> = None;
    let mut shot: Option<app::ScreenshotPlan> = None;
    let mut size = [1560.0f32, 940.0];
    let mut present = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => {
                let out = PathBuf::from(args.next().unwrap_or_else(|| "screenshot.png".into()));
                shot = Some(app::ScreenshotPlan { out, scene: "editor".into(), frame: 0, done: Arc::new(AtomicBool::new(false)), requested: false });
            }
            "--scene" => {
                if let (Some(s), Some(name)) = (shot.as_mut(), args.next()) {
                    s.scene = name;
                }
            }
            // open the file read-only, straight to the run dialog (e.g. for a desktop shortcut)
            "--present" => present = true,
            "--size" => {
                if let Some(v) = args.next() {
                    let p: Vec<f32> = v.split('x').filter_map(|x| x.parse().ok()).collect();
                    if p.len() == 2 {
                        size = [p[0], p[1]];
                    }
                }
            }
            "--write-icon" => {
                // used by the release build to make the .ico / .icns files
                let out = args.next().unwrap_or_else(|| "icon.png".into());
                let size: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(256);
                let rgba = placeholders::app_icon_rgba(size);
                if let Some(img) = image::RgbaImage::from_raw(size, size, rgba) {
                    let _ = img.save(&out);
                }
                return Ok(());
            }
            "--version" | "-V" => {
                println!("ParadigmShift {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("ParadigmShift {} – timeline editor and presenter for fMRI paradigms\n\nUsage: paradigmshift [file.paradigm.json]\n       paradigmshift --present file.paradigm.json   (open read-only, straight to the run)", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => file = Some(PathBuf::from(other)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ParadigmShift")
            .with_app_id("paradigmshift")
            .with_inner_size(size)
            .with_min_inner_size([1100.0, 700.0])
            .with_icon(Arc::new(theme::app_icon()))
            .with_drag_and_drop(true),
        persist_window: shot.is_none(),
        ..Default::default()
    };
    eframe::run_native("ParadigmShift", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, file, shot, present)))))
}
