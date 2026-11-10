//! Draws what the participant sees. Shared by the editor preview, the participant screen and the
//! control-room mirror. Pictures are decoded on background threads and kept as GPU textures.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use egui::epaint::{CornerRadius, PathShape, PathStroke};
use egui::text::LayoutJob;
use egui::{Align, Align2, Color32, ColorImage, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Vec2};

use crate::model::{Block, ParamExt, Paradigm};
use crate::runner::PaintState;
use crate::theme::{self, hex};

// =============================================================================
// Image cache
// =============================================================================
#[derive(Clone)]
pub struct Tex {
    pub id: egui::TextureId,
    pub size: Vec2,
}

enum Entry {
    Ready(TextureHandle),
    Failed,
}

pub enum ImgStatus {
    Ready(Tex),
    Loading,
    Missing,
}

struct Cache {
    entries: HashMap<String, Entry>,
    pending: HashSet<String>,
    req_tx: Sender<(String, String, bool)>,
    done_rx: Receiver<(String, Option<ColorImage>)>,
}

static CTX: OnceLock<egui::Context> = OnceLock::new();

const MAX_SIDE: u32 = 4096;

fn decode(path: &str, blur: bool) -> Option<ColorImage> {
    let img = image::open(path).ok()?;
    let mut img = if img.width().max(img.height()) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    if blur {
        // strongly blurred copy: shrink to a few pixels, then scale back up smoothly
        let (w, h) = (img.width(), img.height());
        let base = img.resize(w.min(360 * w / h.max(1)), 360, image::imageops::FilterType::Triangle);
        let (bw, bh) = (base.width(), base.height());
        let tiny = base.resize_exact((bw / 40).max(1), (bh / 40).max(1), image::imageops::FilterType::Triangle);
        img = tiny.resize_exact((bw / 8).max(1), (bh / 8).max(1), image::imageops::FilterType::CatmullRom);
    }
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| {
    let (req_tx, req_rx) = channel::<(String, String, bool)>();
    let (done_tx, done_rx) = channel();
    let req_rx = Arc::new(Mutex::new(req_rx));
    for i in 0..3 {
        let rx = req_rx.clone();
        let tx = done_tx.clone();
        let _ = std::thread::Builder::new().name(format!("image-loader-{i}")).spawn(move || loop {
            let job = { rx.lock().ok().and_then(|r| r.recv().ok()) };
            let Some((key, path, blur)) = job else { return };
            let img = decode(&path, blur);
            if tx.send((key, img)).is_err() {
                return;
            }
            if let Some(c) = CTX.get() {
                c.request_repaint();
            }
        });
    }
    Mutex::new(Cache { entries: HashMap::new(), pending: HashSet::new(), req_tx, done_rx })
});

fn tex_options() -> egui::TextureOptions {
    egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR }
}

pub fn init(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// Turn decoded pictures into textures (call once per frame).
pub fn poll(ctx: &egui::Context) {
    let Ok(mut c) = CACHE.lock() else { return };
    while let Ok((key, img)) = c.done_rx.try_recv() {
        c.pending.remove(&key);
        let e = match img {
            Some(img) => Entry::Ready(ctx.load_texture(key.clone(), img, tex_options())),
            None => Entry::Failed,
        };
        c.entries.insert(key, e);
    }
}

fn key(path: &str, blur: bool) -> String {
    if blur { format!("{path}@blur") } else { path.to_string() }
}

pub fn image(path: &str, blur: bool) -> ImgStatus {
    if path.is_empty() {
        return ImgStatus::Missing;
    }
    let k = key(path, blur);
    let Ok(mut c) = CACHE.lock() else { return ImgStatus::Missing };
    match c.entries.get(&k) {
        Some(Entry::Ready(t)) => ImgStatus::Ready(Tex { id: t.id(), size: t.size_vec2() }),
        Some(Entry::Failed) => ImgStatus::Missing,
        None => {
            if !c.pending.contains(&k) {
                if !std::path::Path::new(path).exists() {
                    c.entries.insert(k, Entry::Failed);
                    return ImgStatus::Missing;
                }
                let _ = c.req_tx.send((k.clone(), path.to_string(), blur));
                c.pending.insert(k);
            }
            ImgStatus::Loading
        }
    }
}

pub fn texture(path: &str, blur: bool) -> Option<Tex> {
    match image(path, blur) {
        ImgStatus::Ready(t) => Some(t),
        _ => None,
    }
}

/// Decode pictures right now (before a run starts: no disk access while running).
pub fn preload(ctx: &egui::Context, paths: &[String]) {
    for p in paths {
        let k = key(p, false);
        let ready = CACHE.lock().map(|c| matches!(c.entries.get(&k), Some(Entry::Ready(_)))).unwrap_or(true);
        if ready {
            continue;
        }
        let e = match decode(p, false) {
            Some(img) => Entry::Ready(ctx.load_texture(k.clone(), img, tex_options())),
            None => Entry::Failed,
        };
        if let Ok(mut c) = CACHE.lock() {
            c.entries.insert(k, e);
        }
    }
}

/// Forget a picture (e.g. after it changed on disk).
pub fn forget_missing() {
    if let Ok(mut c) = CACHE.lock() {
        c.entries.retain(|_, e| matches!(e, Entry::Ready(_)));
    }
}

// =============================================================================
// Timing helpers
// =============================================================================
/// Go/No-Go and choice stimuli are shown for `stim_time` seconds (0 = whole trial), then the ISI symbol.
pub fn stim_visible(b: &Block, t_in_block: f64) -> bool {
    if b.kind != "gonogo" && b.kind != "choice" {
        return true;
    }
    let st = b.params.f("stim_time", if b.kind == "gonogo" { 0.5 } else { 0.0 });
    st <= 0.0 || t_in_block < st
}

/// 0/1 for a flickering image (`flicker_hz` reversals per second), always 0 otherwise.
pub fn flicker_phase(b: &Block, t_in_block: f64) -> u32 {
    let hz = b.params.f("flicker_hz", 0.0);
    if b.kind != "image" || hz <= 0.0 || b.params.nonempty("flicker_path").is_none() {
        return 0;
    }
    ((t_in_block.max(0.0) * hz) as u64 % 2) as u32
}

// =============================================================================
// Painting
// =============================================================================
/// Font sizes are quantised so a continuously resized preview doesn't fill the glyph atlas.
fn qsize(px: f32) -> f32 {
    let px = px.max(4.0);
    if px < 24.0 { px.round() } else if px < 80.0 { (px / 2.0).round() * 2.0 } else { (px / 6.0).round() * 6.0 }
}

/// The screen height (in pixels) that the font sizes of the blocks refer to. Every view – the
/// participant screen, the control-room mirror and the editor preview – scales the text by
/// `its height / REF_HEIGHT`, so all of them show the same picture at different sizes.
pub const REF_HEIGHT: f32 = 1080.0;

/// Lay out `text` so that it fits into `max` (word-wrapped to its width if `wrap`): the font is made
/// smaller when the text would not fit – long words, many lines or a small window never overflow.
fn fitted(p: &Painter, text: &str, font: FontId, color: Color32, max: Vec2, wrap: bool) -> std::sync::Arc<egui::Galley> {
    let layout = |size: f32| {
        let f = FontId::new(size, font.family.clone());
        let mut job = LayoutJob::simple(text.to_string(), f, color, if wrap { max.x.max(10.0) } else { f32::INFINITY });
        job.halign = Align::Center;
        p.layout_job(job)
    };
    let mut size = font.size;
    let mut g = layout(size);
    for _ in 0..8 {
        let sz = g.size();
        let over = (sz.x / max.x.max(1.0)).max(sz.y / max.y.max(1.0));
        if over <= 1.0 || size <= 4.0 {
            break;
        }
        size = (size / over * 0.98).floor().max(4.0);
        g = layout(size);
    }
    g
}

/// Text centred in `rect`, optionally word-wrapped to its width; shrunk if it would not fit.
pub fn draw_text(p: &Painter, rect: Rect, text: &str, font: FontId, color: Color32, wrap: bool) {
    if text.is_empty() {
        return;
    }
    let galley = fitted(p, text, font, color, rect.size() * 0.98, wrap);
    let pos = rect.center() - galley.rect.center().to_vec2();
    p.galley(pos, galley, color);
}

/// One line of text anchored at `pos`, shrunk to fit into `max`.
fn text_in(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32, max: Vec2) {
    if text.is_empty() {
        return;
    }
    let g = fitted(p, text, font, color, max, false);
    let r = anchor.anchor_size(pos, g.size());
    p.galley(r.center() - g.rect.center().to_vec2(), g, color);
}

fn dashed_rect(p: &Painter, r: Rect, color: Color32, width: f32) {
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for w in pts.windows(2) {
        p.add(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(width, color), 6.0, 4.0));
    }
}

/// Draw a picture letterboxed into `area` scaled by `scale`; returns the rect it occupies.
fn draw_image(p: &Painter, screen: Rect, path: &str, scale: f32, area: Rect, blur: bool) -> Rect {
    let bx = Rect::from_center_size(area.center(), area.size() * scale);
    let status = image(path, blur);
    match status {
        ImgStatus::Ready(t) => {
            let s = (bx.width() / t.size.x).min(bx.height() / t.size.y);
            let target = Rect::from_center_size(bx.center(), t.size * s);
            p.image(t.id, target, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            target
        }
        other => {
            dashed_rect(p, bx, Color32::from_gray(0x66), 2.0);
            let msg = match other {
                ImgStatus::Loading => "loading…",
                _ if path.is_empty() => "no image",
                _ => "image missing",
            };
            draw_text(p, bx, msg, theme::font(qsize(screen.height() * 0.035)), Color32::from_gray(0x88), false);
            bx
        }
    }
}

/// Paint `block` into `rect`. `ref_height` is the screen height the pixel sizes in the block refer
/// to, so the small editor preview looks like a scaled-down scanner screen.
pub fn paint_stimulus(
    p: &Painter,
    rect: Rect,
    par: &Paradigm,
    block: Option<&Block>,
    st: &PaintState,
    ref_height: f32,
    blur: bool,
) {
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    p.rect_filled(rect, CornerRadius::ZERO, hex(&par.background));
    let Some(b) = block else { return };
    let k = rect.height() / ref_height;
    let prm = &b.params;
    match b.kind.as_str() {
        "fixation" => {
            let f = theme::sym(qsize(prm.f("size", 72.0) as f32 * k), false);
            draw_text(&p, rect, &prm.s("symbol", "+"), f, hex(&prm.s("color", "#FFFFFF")), false);
        }
        "image" => {
            let path = if flicker_phase(b, st.t) == 1 { prm.s("flicker_path", "") } else { prm.s("path", "") };
            let scale = prm.f("scale", 0.8) as f32;
            let caption = prm.s("caption", "");
            if caption.trim().is_empty() {
                draw_image(&p, rect, &path, scale, rect, blur);
            } else {
                let f = theme::font(qsize(prm.f("caption_size", 40.0) as f32 * k));
                paint_captioned(&p, rect, &path, scale, caption.trim_end(), f, hex(&prm.s("caption_color", "#FFFFFF")), blur);
            }
        }
        "text" => {
            let f = theme::font(qsize(prm.f("size", 40.0) as f32 * k));
            draw_text(&p, rect.shrink2(Vec2::new(rect.width() * 0.1, 0.0)), &prm.s("text", ""), f, hex(&prm.s("color", "#FFFFFF")), true);
        }
        "rating" => paint_rating(&p, rect, b, st, blur),
        "match" => paint_match(&p, rect, b, st.chosen.as_deref(), blur),
        "gonogo" | "choice" => {
            if stim_visible(b, st.t) {
                let is_go = b.kind == "gonogo";
                if let Some(path) = prm.nonempty("path") {
                    draw_image(&p, rect, &path, if is_go { 0.5 } else { 0.6 }, rect, blur);
                    if is_go {
                        return;
                    }
                }
                let text = prm.s("text", "");
                let size = prm.f("size", if is_go { 140.0 } else { 120.0 }) as f32 * k;
                let f = if is_go { theme::bold(qsize(size)) } else { theme::sym(qsize(size), true) };
                draw_text(&p, rect.shrink2(Vec2::new(rect.width() * 0.04, 0.0)), &text, f, hex(&prm.s("color", "#FFFFFF")), false);
            } else if let Some(sym) = prm.nonempty("isi_symbol") {
                draw_text(&p, rect, &sym, theme::sym(qsize(72.0 * k), false), Color32::WHITE, false);
            }
        }
        "feedback" => {
            let ok = st.success;
            let f = theme::bold(qsize(prm.f("size", 72.0) as f32 * k));
            let color = hex(&prm.s(if ok { "hit_color" } else { "miss_color" }, "#FFFFFF"));
            let text = prm.s(if ok { "hit_text" } else { "miss_text" }, "");
            draw_text(&p, rect.shrink2(Vec2::new(rect.width() * 0.08, 0.0)), &text, f, color, true);
        }
        _ => {}
    }
}

/// A picture with a text underneath. Picture and text are centred on the screen as a pair; the
/// picture keeps its size (`scale` of the screen) unless the text needs the room, and the text
/// wraps and shrinks if it is too long (it may take at most a third of the screen height).
fn paint_captioned(p: &Painter, rect: Rect, path: &str, scale: f32, caption: &str, font: FontId, color: Color32, blur: bool) {
    let (w, h) = (rect.width(), rect.height());
    let gap = h * 0.03;
    let g = fitted(p, caption, font, color, Vec2::new(w * 0.9, h * 0.33), true);
    let cap_h = g.size().y;
    let bx = rect.size() * scale;
    let box_h = bx.y.min(h * 0.96 - cap_h - gap).max(h * 0.1);
    let pic = match image(path, blur) {
        ImgStatus::Ready(t) => t.size * (bx.x / t.size.x).min(box_h / t.size.y),
        _ => Vec2::new(bx.x.min(box_h * 4.0 / 3.0), box_h),
    };
    let top = rect.center().y - (pic.y + gap + cap_h) / 2.0;
    let pic_rect = Rect::from_center_size(Pos2::new(rect.center().x, top + pic.y / 2.0), pic);
    let shown = draw_image(p, rect, path, 1.0, pic_rect, blur);
    let cap_rect = Rect::from_min_size(Pos2::new(rect.left(), shown.bottom() + gap), Vec2::new(w, cap_h));
    p.galley(cap_rect.center() - g.rect.center().to_vec2(), g, color);
}

/// Hariri layout: target on top, two choices below. A frame marks the participant's choice.
fn paint_match(p: &Painter, rect: Rect, b: &Block, chosen: Option<&str>, blur: bool) {
    let prm = &b.params;
    let sc = prm.f("scale", 0.9) as f32;
    // three portrait slots; on narrow screens the two lower ones limit the size
    let gap = rect.width() * 0.05;
    let w = (rect.height() * 0.44 * 0.8).min((rect.width() * 0.94 - 2.0 * gap) / 2.0);
    let h = w / 0.8;
    let cx = rect.center().x;
    let top = Rect::from_center_size(Pos2::new(cx, rect.top() + rect.height() * 0.27), Vec2::new(w, h));
    let left = Rect::from_center_size(Pos2::new(cx - w / 2.0 - gap, rect.top() + rect.height() * 0.74), Vec2::new(w, h));
    let right = Rect::from_center_size(Pos2::new(cx + w / 2.0 + gap, rect.top() + rect.height() * 0.74), Vec2::new(w, h));
    draw_image(p, rect, &prm.s("top", ""), sc, top, blur);
    let dl = draw_image(p, rect, &prm.s("left", ""), sc, left, blur);
    let dr = draw_image(p, rect, &prm.s("right", ""), sc, right, blur);
    if let Some(side) = chosen {
        if prm.b("show_choice", true) {
            let lw = (rect.height() * 0.008).max(2.0);
            let pad = lw * 1.6;
            let r = if side == "left" { dl } else { dr };
            p.rect_stroke(r.expand(pad), CornerRadius::ZERO, Stroke::new(lw, hex(&prm.s("choice_color", "#FFD23F"))), StrokeKind::Middle);
        }
    }
}

fn paint_rating(p: &Painter, rect: Rect, b: &Block, st: &PaintState, blur: bool) {
    let prm = &b.params;
    let (w, h) = (rect.width(), rect.height());
    let img_area = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h * 0.04), Vec2::new(w, h * 0.6));
    if let Some(path) = prm.nonempty("path") {
        draw_image(p, rect, &path, prm.f("scale", 0.9) as f32, img_area, blur);
    }
    let (lo, hi) = (prm.i("min", 1), prm.i("max", 4));
    let n = (hi - lo + 1).max(1);
    let q_rect = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h * 0.66), Vec2::new(w, h * 0.08));
    draw_text(p, q_rect, &prm.s("question", ""), theme::font(qsize(h * 0.04)), Color32::WHITE, false);
    let y = rect.top() + h * 0.83;
    let (x0, x1) = (rect.left() + w * 0.22, rect.right() - w * 0.22);
    let lw = (h * 0.004).max(1.0);
    p.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], Stroke::new(lw, Color32::from_gray(0xBB)));
    // the numbers under the options can be switched off (only the circles and the end labels)
    let show_numbers = prm.b("show_numbers", true);
    // circles shrink when many scale points share a narrow screen
    let r = (h * 0.022).min((x1 - x0) / (n.max(2) - 1) as f32 * 0.3);
    for i in 0..n {
        let v = lo + i;
        let x = x0 + (x1 - x0) * if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
        let sel = st.value == Some(v);
        let mut col = if sel { hex("#F5A524") } else { hex("#DDDDDD") };
        if sel && st.confirmed {
            col = hex("#2FB37A");
        }
        let rad = r * if sel { 1.35 } else { 1.0 };
        p.circle(Pos2::new(x, y), rad, if sel { col } else { Color32::BLACK }, Stroke::new(lw, col));
        if show_numbers {
            let f = if sel { theme::bold(qsize(h * 0.03)) } else { theme::font(qsize(h * 0.03)) };
            let slot = Vec2::new(((x1 - x0) / (n.max(2) - 1) as f32).max(r * 2.0), h * 0.05);
            text_in(p, Pos2::new(x, y + r * 1.6), Align2::CENTER_TOP, &v.to_string(), f, Color32::WHITE, slot);
        }
    }
    let lf = theme::font(qsize(h * 0.026));
    let grey = Color32::from_gray(0xAA);
    let (ll, rl) = (prm.s("left_label", ""), prm.s("right_label", ""));
    // the labels go beside the scale, clear of the (enlarged) end circles …
    let gap = r * 1.35 + w * 0.015;
    let side = Vec2::new((x0 - gap - rect.left() - w * 0.02).max(1.0), h * 0.05);
    let natural = |t: &str| if t.is_empty() { 0.0 } else { p.layout_no_wrap(t.to_string(), lf.clone(), grey).size().x };
    let need = natural(&ll).max(natural(&rl));
    if need <= side.x / 0.7 {
        text_in(p, Pos2::new(x0 - gap, y), Align2::RIGHT_CENTER, &ll, lf.clone(), grey, side);
        text_in(p, Pos2::new(x1 + gap, y), Align2::LEFT_CENTER, &rl, lf, grey, side);
    } else {
        // … or, on narrow (portrait) screens where they would have to shrink a lot, under its ends
        let yl = y + r * 1.6 + if show_numbers { h * 0.055 } else { 0.0 };
        let below = Vec2::new(((x1 - x0) / 2.0 + r - w * 0.02).max(1.0), h * 0.045);
        text_in(p, Pos2::new(x0 - r, yl), Align2::LEFT_TOP, &ll, lf.clone(), grey, below);
        text_in(p, Pos2::new(x1 + r, yl), Align2::RIGHT_TOP, &rl, lf, grey, below);
    }
}

/// The neutral screen the participant sees before the run starts (and `text` after it ends).
pub fn paint_waiting(p: &Painter, rect: Rect, par: &Paradigm, text: Option<&str>, ref_height: f32) {
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    p.rect_filled(rect, CornerRadius::ZERO, hex(&par.background));
    let k = rect.height() / ref_height;
    let (mode, msg) = match text {
        Some(t) => ("text", t.to_string()),
        None => (par.waiting_screen.as_str(), par.waiting_text.clone()),
    };
    match mode {
        "fixation" => draw_text(&p, rect, "+", theme::sym(qsize(72.0 * k), false), Color32::WHITE, false),
        "text" if !msg.is_empty() => draw_text(
            &p,
            rect.shrink2(Vec2::new(rect.width() * 0.1, 0.0)),
            &msg,
            theme::font(qsize(40.0 * k)),
            Color32::WHITE,
            true,
        ),
        _ => {}
    }
}

/// The paint state the editor preview uses (rating cursor in the middle, nothing chosen).
pub fn preview_state(b: Option<&Block>, t: f64) -> PaintState {
    let mut st = PaintState::default();
    if let Some(b) = b {
        st.t = t - b.start;
        if b.kind == "rating" && b.params.s("mode", "direct") == "cursor" {
            st.value = Some(b.rating_start());
        }
        st.success = true;
    }
    st
}

/// A "loading" spinner-free placeholder for thumbnails.
pub fn paint_thumb(p: &Painter, r: Rect, path: &str, blur: bool) -> bool {
    if let Some(t) = texture(path, blur) {
        let s = (r.width() / t.size.x).min(r.height() / t.size.y);
        let target = Rect::from_center_size(r.center(), t.size * s);
        p.image(t.id, target, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        true
    } else {
        false
    }
}

/// Hatched area (the part of an aborted run that never happened).
pub fn hatch(p: &Painter, r: Rect, color: Color32) {
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    let mut x = r.left() - r.height();
    while x < r.right() {
        p.add(PathShape::line(
            vec![Pos2::new(x, r.bottom()), Pos2::new(x + r.height(), r.top())],
            PathStroke::new(2.0, color),
        ));
        x += 9.0;
    }
}
