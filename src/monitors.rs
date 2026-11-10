//! Names of the connected screens for the "Show on screen" menus: the name each monitor reports
//! itself (EDID, sent over HDMI / DisplayPort) – the one the system's display settings show –
//! instead of a bare "Display 1".
//!
//! winit already gives these names on macOS. On Windows it only gives the GDI device name
//! (`\\.\DISPLAY1`), so the friendly names are looked up with the Windows display-configuration
//! API. On Linux the output name (e.g. `HDMI-1`) is used.

use winit::monitor::MonitorHandle;
use winit::window::Window;

/// Added to the label of the screen that holds the taskbar / menu bar.
pub const MAIN_TAG: &str = "  ·  main screen";

/// One label per screen, in the order `ViewportBuilder::with_monitor` counts them, e.g.
/// "2: DELL U2415  ·  1920×1200".
pub fn describe(window: &Window) -> Vec<String> {
    let primary = window.primary_monitor();
    let friendly = friendly_names();
    window
        .available_monitors()
        .enumerate()
        .map(|(i, m)| label(i, &m, primary.as_ref(), &friendly))
        .collect()
}

fn label(i: usize, m: &MonitorHandle, primary: Option<&MonitorHandle>, friendly: &[(String, String)]) -> String {
    let raw = m.name().unwrap_or_default();
    let name = friendly
        .iter()
        .find(|(gdi, _)| gdi.eq_ignore_ascii_case(&raw))
        .map(|(_, f)| f.clone())
        .unwrap_or_else(|| if raw.is_empty() { "screen".to_string() } else { raw.clone() });
    let s = m.size();
    let main = if primary == Some(m) { MAIN_TAG } else { "" };
    format!("{}: {name}  ·  {}×{}{main}", i + 1, s.width, s.height)
}

/// The screen the participant window should go to by default: the first one that is not the
/// main screen (usually the projector), or the only one.
pub fn default_participant_screen(labels: &[String]) -> usize {
    if labels.len() < 2 {
        return 0;
    }
    labels.iter().position(|l| !l.ends_with(MAIN_TAG)).unwrap_or(1)
}

/// (GDI device name, friendly monitor name) for every active screen.
#[cfg(windows)]
fn friendly_names() -> Vec<(String, String)> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
        DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED, DISPLAYCONFIG_PATH_INFO,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
    };
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;

    fn wide(s: &[u16]) -> String {
        let end = s.iter().position(|c| *c == 0).unwrap_or(s.len());
        String::from_utf16_lossy(&s[..end]).trim().to_string()
    }

    let mut out = vec![];
    // SAFETY: plain Win32 calls with correctly sized, zero-initialised buffers and headers.
    unsafe {
        let (mut n_paths, mut n_modes) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes) != ERROR_SUCCESS {
            return out;
        }
        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![zeroed(); n_paths as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![zeroed(); n_modes as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, paths.as_mut_ptr(), &mut n_modes, modes.as_mut_ptr(), std::ptr::null_mut())
            != ERROR_SUCCESS
        {
            return out;
        }
        for p in paths.iter().take(n_paths as usize) {
            let mut src: DISPLAYCONFIG_SOURCE_DEVICE_NAME = zeroed();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let mut tgt: DISPLAYCONFIG_TARGET_DEVICE_NAME = zeroed();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut tgt.header) != 0 {
                continue;
            }
            let gdi = wide(&src.viewGdiDeviceName);
            let mut name = wide(&tgt.monitorFriendlyDeviceName);
            if name.is_empty() {
                let tech = tgt.outputTechnology;
                let built_in = tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                    || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                    || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED;
                if built_in {
                    name = "Built-in display".into();
                }
            }
            if !gdi.is_empty() && !name.is_empty() && !out.iter().any(|(g, _): &(String, String)| *g == gdi) {
                out.push((gdi, name));
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn friendly_names() -> Vec<(String, String)> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_screen_avoids_the_main_screen() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(default_participant_screen(&l(&["1: Built-in display  ·  1920×1080  ·  main screen"])), 0);
        assert_eq!(default_participant_screen(&l(&["1: DELL  ·  1920×1080  ·  main screen", "2: EPSON PJ  ·  1280×800"])), 1);
        assert_eq!(default_participant_screen(&l(&["1: EPSON PJ  ·  1280×800", "2: DELL  ·  1920×1080  ·  main screen"])), 0);
        assert_eq!(default_participant_screen(&l(&["1: a  ·  1×1", "2: b  ·  1×1"])), 0);
    }
}
