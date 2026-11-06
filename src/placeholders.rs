//! Generated placeholder stimuli so every template runs out of the box.
//!
//! Validated stimulus sets (e.g. NimStim/KDEF faces, IAPS/OASIS pictures) cannot be
//! shipped with the app because of their licences. These drawings stand in for
//! them: schematic faces with angry/fearful expressions, Hariri-style geometric
//! shapes, abstract neutral/negative scenes, a radial checkerboard and MID cues.
//! Replace them with a validated set for real studies – every template accepts
//! your own folders.

use std::collections::BTreeMap;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LinearGradient, Paint, PathBuilder, Pixmap, Point, RadialGradient,
    Rect, SpreadMode, Stroke, Transform,
};

use crate::rng::Rng;

pub const FACE_W: u32 = 300;
pub const FACE_H: u32 = 380;
pub const EMOTIONS: [&str; 2] = ["angry", "fearful"];
pub const SHAPES: [&str; 3] = ["circle", "ellipse_wide", "ellipse_tall"];

// ---------------------------------------------------------------------------- helpers
pub fn hex(c: &str) -> (u8, u8, u8) {
    let s = c.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    if s.len() >= 6 { (p(0), p(2), p(4)) } else { (0, 0, 0) }
}

fn col(c: &str) -> Color {
    let (r, g, b) = hex(c);
    Color::from_rgba8(r, g, b, 255)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

fn grey(v: u8) -> Color {
    rgba(v, v, v, 255)
}

/// Qt's `QColor.darker(factor)`: value divided by factor/100.
fn darker(v: u8, factor: f32) -> u8 {
    (v as f32 * 100.0 / factor).round().clamp(0.0, 255.0) as u8
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn canvas(w: u32, h: u32, bg: &str) -> Pixmap {
    let mut pm = Pixmap::new(w, h).expect("pixmap");
    pm.fill(col(bg));
    pm
}

fn oval(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<tiny_skia::Path> {
    PathBuilder::from_oval(Rect::from_xywh(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry)?)
}

fn fill(pm: &mut Pixmap, path: Option<tiny_skia::Path>, c: Color) {
    if let Some(path) = path {
        pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

fn stroke(pm: &mut Pixmap, path: Option<tiny_skia::Path>, c: Color, width: f32, round: bool) {
    if let Some(path) = path {
        let st = Stroke { width, line_cap: if round { LineCap::Round } else { LineCap::Butt }, ..Default::default() };
        pm.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
    }
}

fn line(pm: &mut Pixmap, a: (f32, f32), b: (f32, f32), c: Color, width: f32, round: bool) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.0, a.1);
    pb.line_to(b.0, b.1);
    stroke(pm, pb.finish(), c, width, round);
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn save(pm: &Pixmap, out: &Path) -> Result<(), String> {
    pm.save_png(out).map_err(|e| format!("{}: {e}", out.display()))
}

// ---------------------------------------------------------------------------- faces
/// Greyscale schematic face. `identity` changes face shape, skin tone and hair.
pub fn draw_face(out: &Path, identity: u32, emotion: &str) -> Result<(), String> {
    let mut rng = Rng::seeded(identity as u64 * 7919 + 1);
    let (w, h) = (FACE_W as f32, FACE_H as f32);
    let mut pm = canvas(FACE_W, FACE_H, "#3A3A3A");
    let (cx, cy) = (w / 2.0, h * 0.53);
    let fw = w * rng.uniform(0.56, 0.68) as f32;
    let fh = h * rng.uniform(0.62, 0.70) as f32;
    let skin = rng.randint(150, 205) as u8;
    let hair = rng.randint(25, 120) as u8;
    let style = identity % 3;

    if style == 2 {
        // long hair behind the head
        fill(&mut pm, rounded_rect(cx - fw * 0.62, cy - fh * 0.55, fw * 1.24, fh * 1.05, 60.0), grey(hair));
    }
    // neck + head
    fill(&mut pm, Rect::from_xywh(cx - fw * 0.17, cy + fh * 0.35, fw * 0.34, fh * 0.3).map(PathBuilder::from_rect),
         grey(darker(skin, 115.0)));
    fill(&mut pm, oval(cx, cy, fw / 2.0, fh / 2.0), grey(skin));
    // hair on top
    let mut top = PathBuilder::new();
    top.move_to(cx - fw * 0.52, cy - fh * 0.05);
    if style == 1 {
        top.cubic_to(cx - fw * 0.6, cy - fh * 0.75, cx + fw * 0.7, cy - fh * 0.7, cx + fw * 0.52, cy - fh * 0.08);
        top.cubic_to(cx + fw * 0.3, cy - fh * 0.32, cx - fw * 0.1, cy - fh * 0.28, cx - fw * 0.52, cy - fh * 0.05);
    } else {
        top.cubic_to(cx - fw * 0.55, cy - fh * 0.72, cx + fw * 0.55, cy - fh * 0.72, cx + fw * 0.52, cy - fh * 0.05);
        top.cubic_to(cx + fw * 0.35, cy - fh * 0.36, cx - fw * 0.35, cy - fh * 0.36, cx - fw * 0.52, cy - fh * 0.05);
    }
    top.close();
    fill(&mut pm, top.finish(), grey(hair));

    let ink = col("#1E1E1E");
    let eye_y = cy - fh * 0.05;
    let eye_dx = fw * 0.2;
    let fear = emotion == "fearful";
    // eyes: wide open for fear, narrowed for anger
    let eye_h = fh * if fear { 0.07 } else { 0.035 };
    for sx in [-1.0f32, 1.0] {
        let ex = cx + sx * eye_dx;
        fill(&mut pm, oval(ex, eye_y, fw * 0.085, eye_h), col("#F2F2F2"));
        stroke(&mut pm, oval(ex, eye_y, fw * 0.085, eye_h), ink, 2.0, false);
        let pr = fw * 0.035;
        fill(&mut pm, oval(ex, eye_y, pr, eye_h.min(pr)), ink);
    }
    // brows
    let brow = if hair > 60 { grey(darker(hair, 130.0)) } else { ink };
    for sx in [-1.0f32, 1.0] {
        let mut inner = (cx + sx * fw * 0.07, eye_y - fh * if fear { 0.12 } else { 0.05 });
        let outer = (cx + sx * fw * 0.32, eye_y - fh * if fear { 0.09 } else { 0.11 });
        if fear {
            inner.1 = eye_y - fh * 0.16; // raised, inner ends pulled up
        }
        line(&mut pm, inner, outer, brow, 6.0, true);
    }
    // nose
    let nose = grey(darker(skin, 150.0));
    let nl = fh * rng.uniform(0.1, 0.15) as f32;
    line(&mut pm, (cx, eye_y + fh * 0.04), (cx - fw * 0.04, eye_y + fh * 0.04 + nl), nose, 3.0, true);
    line(&mut pm, (cx - fw * 0.04, eye_y + fh * 0.04 + nl), (cx + fw * 0.03, eye_y + fh * 0.05 + nl), nose, 3.0, true);
    // mouth
    let my = cy + fh * 0.25;
    if fear {
        // open, stretched
        fill(&mut pm, oval(cx, my, fw * 0.15, fh * 0.06), col("#2A1A1A"));
        stroke(&mut pm, oval(cx, my, fw * 0.15, fh * 0.06), ink, 4.0, true);
    } else {
        // tight, down-turned
        let mut m = PathBuilder::new();
        m.move_to(cx - fw * 0.17, my + fh * 0.025);
        m.cubic_to(cx - fw * 0.05, my - fh * 0.02, cx + fw * 0.05, my - fh * 0.02, cx + fw * 0.17, my + fh * 0.025);
        stroke(&mut pm, m.finish(), ink, 4.0, true);
    }
    save(&pm, out)
}

/// Hariri sensorimotor control: simple geometric forms, same frame as the faces.
pub fn draw_shape(out: &Path, kind: &str) -> Result<(), String> {
    let mut pm = canvas(FACE_W, FACE_H, "#3A3A3A");
    let (cx, cy) = (FACE_W as f32 / 2.0, FACE_H as f32 / 2.0);
    let (rx, ry) = match kind {
        "ellipse_wide" => (120.0, 65.0),
        "ellipse_tall" => (65.0, 125.0),
        _ => (95.0, 95.0),
    };
    fill(&mut pm, oval(cx, cy, rx, ry), col("#D9D9D9"));
    stroke(&mut pm, oval(cx, cy, rx, ry), col("#F5F5F5"), 3.0, false);
    save(&pm, out)
}

// ---------------------------------------------------------------------------- scenes
/// Abstract landscape: calm and grey-blue (neutral) or dark, red and jagged (negative).
pub fn draw_scene(out: &Path, valence: &str, seed: u64) -> Result<(), String> {
    let mut rng = Rng::seeded(seed);
    let (w, h) = (800u32, 600u32);
    let (wf, hf) = (w as f32, h as f32);
    let mut pm = canvas(w, h, "#000000");
    let negative = valence == "negative";
    let (sky, layers, jag) = if negative {
        let sky = [("#1B0F12", "#5A1E1E"), ("#101418", "#4A2A1A"), ("#1A1010", "#6B2B1B")][(seed % 3) as usize];
        (sky, ["#3B1414", "#260C0C", "#120606"], 1.0f32)
    } else {
        let sky = [("#7C93A6", "#C9D3DA"), ("#8C9A8F", "#D5DAD0"), ("#9AA3AD", "#DEE2E6")][(seed % 3) as usize];
        (sky, ["#7D8A93", "#66727A", "#525C63"], 0.0)
    };
    let full = Rect::from_xywh(0.0, 0.0, wf, hf).unwrap();
    if let Some(sh) = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, hf),
        vec![GradientStop::new(0.0, col(sky.0)), GradientStop::new(1.0, col(sky.1))],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        let mut p = paint(Color::BLACK);
        p.shader = sh;
        pm.fill_rect(full, &p, Transform::identity(), None);
    }
    if negative && rng.random() < 0.6 {
        // fire glow
        let c = Point::from_xy(wf * rng.uniform(0.2, 0.8) as f32, hf * 0.7);
        if let Some(sh) = RadialGradient::new(
            c,
            0.0,
            c,
            260.0,
            vec![GradientStop::new(0.0, rgba(220, 70, 20, 170)), GradientStop::new(1.0, rgba(220, 70, 20, 0))],
            SpreadMode::Pad,
            Transform::identity(),
        ) {
            let mut p = paint(Color::BLACK);
            p.shader = sh;
            pm.fill_rect(full, &p, Transform::identity(), None);
        }
    }
    for (i, c) in layers.iter().enumerate() {
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, hf);
        let base = hf * (0.5 + 0.13 * i as f32);
        let amp = rng.uniform(25.0, 70.0) as f32;
        let ph = rng.uniform(0.0, 6.0) as f32;
        let mut x = 0u32;
        while x <= w + 9 {
            let xf = x as f32;
            let mut y = base - amp * (ph + 2.0 * xf / wf * PI).sin();
            if jag > 0.0 {
                y -= rng.uniform(0.0, 45.0) as f32 * jag;
            }
            pb.line_to(xf, y);
            x += 10;
        }
        pb.line_to(wf, hf);
        pb.close();
        fill(&mut pm, pb.finish(), col(c));
    }
    if negative {
        // broken diagonal strokes
        let n = rng.randint(3, 7);
        for _ in 0..n {
            let x = rng.uniform(0.0, wf as f64) as f32;
            let dx = rng.uniform(-120.0, 120.0) as f32;
            line(&mut pm, (x, hf * 0.3), (x + dx, hf), rgba(0, 0, 0, 160), 6.0, false);
        }
    }
    save(&pm, out)
}

// ---------------------------------------------------------------------------- localizers / MID
/// Radial (polar) checkerboard with a red fixation dot; `phase` 0/1 are contrast-reversed.
pub fn draw_checkerboard(out: &Path, phase: u32) -> Result<(), String> {
    let (rings, wedges) = (8u32, 24u32);
    let (w, h) = (1920u32, 1080u32);
    let mut pm = canvas(w, h, "#808080");
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let rmax = h as f32 * 0.5;
    for ring in (1..=rings).rev() {
        // rings get wider towards the periphery (roughly cortical magnification)
        let r = rmax * (ring as f32 / rings as f32).powf(1.4);
        for k in 0..wedges {
            let black = (ring + k + phase) % 2 == 0;
            let a0 = k as f32 * 2.0 * PI / wedges as f32;
            let a1 = (k + 1) as f32 * 2.0 * PI / wedges as f32 + 0.002;
            let mut pb = PathBuilder::new();
            pb.move_to(cx, cy);
            let steps = 12;
            for s in 0..=steps {
                let a = a0 + (a1 - a0) * s as f32 / steps as f32;
                pb.line_to(cx + r * a.cos(), cy - r * a.sin());
            }
            pb.close();
            fill(&mut pm, pb.finish(), if black { col("#000000") } else { col("#FFFFFF") });
        }
    }
    fill(&mut pm, oval(cx, cy, rmax * 0.045, rmax * 0.045), col("#808080"));
    fill(&mut pm, oval(cx, cy, rmax * 0.018, rmax * 0.018), col("#E53935"));
    save(&pm, out)
}

/// Knutson-style incentive cue: circle = win, square = lose, triangle = nothing; lines = amount.
pub fn draw_mid_cue(out: &Path, shape: &str, lines: u32) -> Result<(), String> {
    let mut pm = canvas(500, 500, "#000000");
    let fillc = if shape != "triangle" { col("#E0E0E0") } else { col("#B0B0B0") };
    let path = match shape {
        "circle" => oval(250.0, 250.0, 150.0, 150.0),
        "square" => Rect::from_xywh(100.0, 100.0, 300.0, 300.0).map(PathBuilder::from_rect),
        _ => {
            let mut pb = PathBuilder::new();
            pb.move_to(250.0, 95.0);
            pb.line_to(405.0, 395.0);
            pb.line_to(95.0, 395.0);
            pb.close();
            pb.finish()
        }
    };
    fill(&mut pm, path.clone(), fillc);
    stroke(&mut pm, path, col("#FFFFFF"), 6.0, false);
    for i in 0..lines {
        let y = 250.0 - (lines as f32 - 1.0) * 18.0 + i as f32 * 36.0;
        line(&mut pm, (170.0, y), (330.0, y), col("#000000"), 10.0, false);
    }
    save(&pm, out)
}

// ---------------------------------------------------------------------------- sets
fn ensure(path: PathBuf, draw: impl FnOnce(&Path) -> Result<(), String>) -> String {
    if !path.exists() {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = draw(&path) {
            eprintln!("placeholder: {e}");
        }
    }
    path.to_string_lossy().to_string()
}

/// `{"angry": [...], "fearful": [...]}` – index i is the same identity in both lists.
pub fn hariri_faces(folder: &Path, identities: u32) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for e in EMOTIONS {
        let v = (0..identities)
            .map(|i| ensure(folder.join(format!("face{:02}_{e}.png", i + 1)), |p| draw_face(p, i, e)))
            .collect();
        out.insert(e.to_string(), v);
    }
    out
}

pub fn hariri_shapes(folder: &Path) -> BTreeMap<String, String> {
    SHAPES
        .iter()
        .map(|k| (k.to_string(), ensure(folder.join(format!("shape_{k}.png")), |p| draw_shape(p, k))))
        .collect()
}

pub fn scenes(folder: &Path, valence: &str, n: usize) -> Vec<String> {
    let off = if valence == "neutral" { 0 } else { 1000 };
    (0..n)
        .map(|i| {
            ensure(folder.join(format!("{valence}_{:02}.png", i + 1)), |p| {
                draw_scene(p, valence, off + i as u64)
            })
        })
        .collect()
}

pub fn checkerboards(folder: &Path) -> (String, String) {
    let a = ensure(folder.join("checkerboard_0.png"), |p| draw_checkerboard(p, 0));
    let b = ensure(folder.join("checkerboard_1.png"), |p| draw_checkerboard(p, 1));
    (a, b)
}

pub fn mid_cues(folder: &Path) -> BTreeMap<String, String> {
    [("gain", "circle", 1), ("loss", "square", 1), ("neutral", "triangle", 0)]
        .iter()
        .map(|(k, shape, lines)| {
            (k.to_string(), ensure(folder.join(format!("cue_{k}.png")), |p| draw_mid_cue(p, shape, *lines)))
        })
        .collect()
}

/// Draw into an RGBA buffer instead of a file (used for the app icon).
pub fn app_icon_rgba(size: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(size, size).expect("pixmap");
    let s = size as f32 / 128.0;
    fill(&mut pm, rounded_rect(4.0 * s, 4.0 * s, 120.0 * s, 120.0 * s, 26.0 * s), col("#252932"));
    let cols = ["#4C8DF6", "#E0913A", "#D45C9E", "#2FB37A"];
    for (i, (x, w)) in [(14.0, 36.0), (52.0, 28.0), (82.0, 32.0)].iter().enumerate() {
        fill(&mut pm, rounded_rect(x * s, 38.0 * s, w * s, 22.0 * s, 5.0 * s), col(cols[i]));
    }
    fill(&mut pm, rounded_rect(40.0 * s, 70.0 * s, 44.0 * s, 22.0 * s, 5.0 * s), col(cols[3]));
    line(&mut pm, (66.0 * s, 22.0 * s), (66.0 * s, 106.0 * s), col("#F5A524"), 5.0 * s, true);
    // tiny-skia stores premultiplied RGBA; convert to straight alpha
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for px in pm.pixels() {
        let c = px.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_all_placeholders() {
        let dir = std::env::temp_dir().join(format!("ps_ph_{}", crate::model::new_id()));
        let faces = hariri_faces(&dir.join("faces"), 2);
        assert_eq!(faces["angry"].len(), 2);
        assert!(Path::new(&faces["fearful"][1]).exists());
        let shapes = hariri_shapes(&dir.join("shapes"));
        assert_eq!(shapes.len(), 3);
        let sc = scenes(&dir.join("neg"), "negative", 2);
        assert!(Path::new(&sc[1]).exists());
        let (a, b) = checkerboards(&dir.join("cb"));
        assert!(Path::new(&a).exists() && Path::new(&b).exists());
        let cues = mid_cues(&dir.join("mid"));
        let img = image::open(&cues["gain"]).unwrap();
        assert_eq!(img.width(), 500);
        assert_eq!(app_icon_rgba(64).len(), 64 * 64 * 4);
        std::fs::remove_dir_all(dir).ok();
    }
}
