//! Dark, editing-suite look, fonts and painter-drawn icons (no image assets needed).

use std::sync::Arc;

use egui::epaint::{CornerRadius, PathShape, PathStroke};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Shadow, Stroke, StrokeKind, Vec2};

pub const BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x1D);
pub const PANEL: Color32 = Color32::from_rgb(0x1E, 0x21, 0x28);
pub const PANEL_2: Color32 = Color32::from_rgb(0x25, 0x29, 0x32);
pub const BORDER: Color32 = Color32::from_rgb(0x32, 0x37, 0x43);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEE);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8C, 0x93, 0xA3);
pub const ACCENT: Color32 = Color32::from_rgb(0xF5, 0xA5, 0x24); // playhead / primary actions
pub const DANGER: Color32 = Color32::from_rgb(0xF0, 0x50, 0x6E);
pub const OK: Color32 = Color32::from_rgb(0x2F, 0xB3, 0x7A);

/// `#RRGGBB` → colour (invalid text gives white).
pub fn hex(s: &str) -> Color32 {
    let t = s.trim().trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(t.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255);
    match t.len() {
        6 => Color32::from_rgb(p(0), p(2), p(4)),
        8 => Color32::from_rgba_unmultiplied(p(0), p(2), p(4), p(6)),
        3 => {
            let q = |i: usize| u8::from_str_radix(&t[i..i + 1].repeat(2), 16).unwrap_or(255);
            Color32::from_rgb(q(0), q(1), q(2))
        }
        _ => Color32::WHITE,
    }
}

pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// Qt-style `lighter(factor)` / `darker(factor)` in percent.
pub fn scale(c: Color32, factor: f32) -> Color32 {
    let f = |v: u8| ((v as f32) * factor).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(c.r()), f(c.g()), f(c.b()), c.a())
}

pub fn darker(c: Color32, pct: f32) -> Color32 {
    scale(c, 100.0 / pct)
}

pub fn lighter(c: Color32, pct: f32) -> Color32 {
    scale(c, pct / 100.0)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn lightness(c: Color32) -> f32 {
    let mx = c.r().max(c.g()).max(c.b()) as f32;
    let mn = c.r().min(c.g()).min(c.b()) as f32;
    (mx + mn) / 2.0
}

// --------------------------------------------------------------------------
// Fonts
// --------------------------------------------------------------------------
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, data: &'static [u8]| {
        fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_static(data)));
    };
    add(&mut fonts, "Inter", include_bytes!("../assets/fonts/Inter-Regular.otf"));
    add(&mut fonts, "Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.otf"));
    add(&mut fonts, "DejaVuSans", include_bytes!("../assets/fonts/DejaVuSans.ttf"));
    add(&mut fonts, "DejaVuSans-Bold", include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf"));
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = first.iter().map(|s| s.to_string()).collect();
        v.extend(defaults.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, with(&["Inter", "DejaVuSans"]));
    fonts.families.insert(FontFamily::Name("bold".into()), with(&["Inter-SemiBold", "DejaVuSans-Bold", "DejaVuSans"]));
    fonts.families.insert(FontFamily::Name("sym".into()), with(&["DejaVuSans", "Inter"]));
    fonts.families.insert(FontFamily::Name("symbold".into()), with(&["DejaVuSans-Bold", "DejaVuSans", "Inter"]));
    if let Some(m) = fonts.families.get_mut(&FontFamily::Monospace) {
        m.push("DejaVuSans".into());
    }
    ctx.set_fonts(fonts);
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn sym(size: f32, is_bold: bool) -> FontId {
    FontId::new(size, FontFamily::Name(if is_bold { "symbold" } else { "sym" }.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn rich_bold(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text).font(bold(size))
}

// --------------------------------------------------------------------------
// Visuals
// --------------------------------------------------------------------------
pub fn apply(ctx: &egui::Context, ui_scale: u32) {
    let s = ui_scale.clamp(70, 160) as f32 / 100.0;
    ctx.options_mut(|o| {
        o.zoom_with_keyboard = false;
    });
    ctx.all_styles_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [
            (Small, font(11.0 * s)),
            (Body, font(13.5 * s)),
            (Button, font(13.5 * s)),
            (Heading, bold(21.0 * s)),
            (Monospace, mono(13.0 * s)),
        ]
        .into();
        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_shadow = Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(140) };
        v.popup_shadow = Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(120) };
        v.extreme_bg_color = BG;
        v.text_edit_bg_color = Some(BG);
        v.faint_bg_color = PANEL_2;
        v.code_bg_color = PANEL_2;
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = ACCENT;
        v.error_fg_color = DANGER;
        v.selection.bg_fill = alpha(ACCENT, 110);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.striped = true;
        let r = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = PANEL_2;
        w.inactive.weak_bg_fill = PANEL_2;
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = Color32::from_rgb(0x2C, 0x31, 0x3C);
        w.hovered.weak_bg_fill = Color32::from_rgb(0x2C, 0x31, 0x3C);
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4A, 0x51, 0x62));
        w.hovered.fg_stroke = Stroke::new(1.5, TEXT);
        w.hovered.corner_radius = r;
        w.active.bg_fill = Color32::from_rgb(0x20, 0x24, 0x2C);
        w.active.weak_bg_fill = Color32::from_rgb(0x20, 0x24, 0x2C);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.5, TEXT);
        w.active.corner_radius = r;
        w.open.bg_fill = PANEL_2;
        w.open.weak_bg_fill = PANEL_2;
        w.open.bg_stroke = Stroke::new(1.0, ACCENT);
        w.open.fg_stroke = Stroke::new(1.0, TEXT);
        w.open.corner_radius = r;
        let sp = &mut style.spacing;
        sp.item_spacing = egui::vec2(8.0, 6.0);
        sp.button_padding = egui::vec2(10.0, 5.0);
        sp.interact_size = egui::vec2(36.0, 24.0 * s.max(1.0));
        sp.combo_width = 120.0;
        sp.text_edit_width = 200.0;
        sp.menu_margin = egui::Margin::same(6);
    });
}

/// A panel frame like the original app's `#Panel` (rounded, bordered).
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 10))
}

pub fn panel_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).font(bold(11.0)).color(TEXT_DIM).extra_letter_spacing(1.0));
}

pub fn hint(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(text.into()).color(TEXT_DIM).size(12.0)).wrap())
}

/// A filled call-to-action button (orange "Primary" / green "Run").
pub fn colored_button(ui: &mut egui::Ui, text: &str, fill: Color32) -> egui::Response {
    let fg = if lightness(fill) > 140.0 { Color32::from_rgb(0x1B, 0x1B, 0x1B) } else { Color32::WHITE };
    ui.add(egui::Button::new(egui::RichText::new(text).font(bold(13.5)).color(fg)).fill(fill).stroke(Stroke::NONE))
}

pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    colored_button(ui, text, ACCENT)
}

// --------------------------------------------------------------------------
// Icons – drawn with the painter so the app ships without image files
// --------------------------------------------------------------------------
fn rr(p: &Painter, r: Rect, rad: f32, stroke: Stroke) {
    p.rect_stroke(r, CornerRadius::same(rad as u8), stroke, StrokeKind::Middle);
}

pub fn draw_kind_glyph(p: &Painter, kind: &str, r: Rect, color: Color32) {
    let (w, h) = (r.width(), r.height());
    let c = r.center();
    let st = Stroke::new((w / 11.0).max(1.4), color);
    let pt = |fx: f32, fy: f32| Pos2::new(r.left() + w * fx, r.top() + h * fy);
    match kind {
        "fixation" => {
            p.line_segment([Pos2::new(c.x, r.top() + h * 0.18), Pos2::new(c.x, r.bottom() - h * 0.18)], st);
            p.line_segment([Pos2::new(r.left() + w * 0.18, c.y), Pos2::new(r.right() - w * 0.18, c.y)], st);
        }
        "image" => {
            let fr = Rect::from_min_max(pt(0.12, 0.2), pt(0.88, 0.8));
            rr(p, fr, 2.0, st);
            let fp = |fx: f32, fy: f32| Pos2::new(fr.left() + fr.width() * fx, fr.top() + fr.height() * fy);
            p.add(PathShape::line(
                vec![
                    Pos2::new(fr.left() + 1.0, fr.bottom() - 1.0),
                    fp(0.4, 0.45),
                    fp(0.6, 0.7),
                    fp(0.75, 0.55),
                    Pos2::new(fr.right() - 1.0, fr.bottom() - 1.0),
                ],
                PathStroke::new(st.width, color),
            ));
            p.circle_stroke(fp(0.78, 0.28), w * 0.05, st);
        }
        "rating" => {
            let y = c.y + h * 0.12;
            p.line_segment([Pos2::new(r.left() + w * 0.14, y), Pos2::new(r.right() - w * 0.14, y)], st);
            for i in 0..4 {
                let x = r.left() + w * 0.14 + i as f32 * (w * 0.72) / 3.0;
                p.line_segment([Pos2::new(x, y - h * 0.08), Pos2::new(x, y + h * 0.08)], st);
            }
            let x = r.left() + w * 0.14 + 2.0 * (w * 0.72) / 3.0;
            p.add(PathShape::convex_polygon(
                vec![Pos2::new(x, y - h * 0.14), Pos2::new(x + w * 0.09, y - h * 0.34), Pos2::new(x - w * 0.09, y - h * 0.34)],
                color,
                Stroke::NONE,
            ));
        }
        "text" => {
            for (i, frac) in [0.8, 0.62, 0.72].iter().enumerate() {
                let y = r.top() + h * (0.3 + 0.2 * i as f32);
                p.line_segment([Pos2::new(r.left() + w * 0.16, y), Pos2::new(r.left() + w * (0.16 + 0.68 * frac), y)], st);
            }
        }
        "rest" => {
            let bw = w * 0.12;
            for x in [c.x - bw * 1.6, c.x + bw * 0.6] {
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x, r.top() + h * 0.24), Vec2::new(bw, h * 0.52)),
                    CornerRadius::same(1),
                    color,
                );
            }
        }
        "response" => {
            let body = Rect::from_min_size(Pos2::new(c.x - w * 0.14, r.top() + h * 0.22), Vec2::new(w * 0.28, h * 0.6));
            rr(p, body, w * 0.12, st);
            p.circle_filled(Pos2::new(c.x, r.top() + h * 0.36), w * 0.06, color);
        }
        "match" => {
            let (bw, bh) = (w * 0.26, h * 0.26);
            for (x, y) in [
                (c.x - bw / 2.0, r.top() + h * 0.14),
                (r.left() + w * 0.14, r.top() + h * 0.58),
                (r.right() - w * 0.14 - bw, r.top() + h * 0.58),
            ] {
                rr(p, Rect::from_min_size(Pos2::new(x, y), Vec2::new(bw, bh)), 2.0, st);
            }
        }
        "gonogo" => {
            p.circle_stroke(Pos2::new(r.left() + w * 0.32, c.y), w * 0.15, st);
            p.circle_filled(Pos2::new(r.right() - w * 0.32, c.y), w * 0.15, color);
        }
        "choice" => {
            let bw = w * 0.3;
            for x in [r.left() + w * 0.14, r.right() - w * 0.14 - bw] {
                rr(p, Rect::from_min_size(Pos2::new(x, c.y - bw / 2.0), Vec2::splat(bw)), 2.0, st);
            }
            let x0 = r.right() - w * 0.14;
            p.add(PathShape::line(
                vec![
                    Pos2::new(x0 - bw * 0.75, c.y),
                    Pos2::new(x0 - bw * 0.5, c.y + bw * 0.25),
                    Pos2::new(x0 - bw * 0.15, c.y - bw * 0.25),
                ],
                PathStroke::new(st.width, color),
            ));
        }
        "feedback" => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let rad = if i % 2 == 0 { w * 0.34 } else { w * 0.15 };
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
                })
                .collect();
            p.add(PathShape::closed_line(pts, PathStroke::new(st.width, color)));
        }
        _ => {}
    }
}

/// A coloured rounded square with the kind glyph (library cards, inspector header).
pub fn paint_kind_icon(p: &Painter, kind: &str, r: Rect, color: Color32) {
    p.rect_filled(r, CornerRadius::same((r.width() * 0.22) as u8), color);
    draw_kind_glyph(p, kind, r, Color32::WHITE);
}

pub fn kind_icon(ui: &mut egui::Ui, kind: &str, color: Color32, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_kind_icon(ui.painter(), kind, rect, color);
    resp
}

/// Window icon (also the macOS / Windows app icon at runtime).
pub fn app_icon() -> egui::IconData {
    let size = 128;
    egui::IconData { rgba: crate::placeholders::app_icon_rgba(size), width: size, height: size }
}

/// A small lamp that lights up when an input arrives and fades out.
pub fn lamp(ui: &mut egui::Ui, color: Color32, level: f32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let off = Color32::from_rgb(0x2A, 0x2F, 0x3A);
    let l = level.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * l) as u8;
    let c = Color32::from_rgb(mix(off.r(), color.r()), mix(off.g(), color.g()), mix(off.b(), color.b()));
    ui.painter().circle(rect.center(), size / 2.0 - 1.0, c, Stroke::new(1.0, BORDER));
}
