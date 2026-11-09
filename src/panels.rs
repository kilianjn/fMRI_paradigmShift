//! Side panels: block library + picture bin, participant-view monitor, inspector.

use egui::epaint::CornerRadius;
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{json, Value};

use crate::actions::{Action, DragPayload};
use crate::document::Document;
use crate::model::{fmt_g, fmt_time, is_image, round4, Block, ParamExt, BLOCK_TYPES, IMAGE_EXT};
use crate::presets::{self, FieldSpec};
use crate::stimulus;
use crate::theme::{self, hex, ACCENT, BG, BORDER, DANGER, PANEL_2, TEXT, TEXT_DIM};

pub fn pick_images(title: &str) -> Vec<String> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Images", IMAGE_EXT)
        .pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

pub fn pick_image(title: &str, start: &str) -> Option<String> {
    let mut d = rfd::FileDialog::new().set_title(title).add_filter("Images", IMAGE_EXT);
    if let Some(parent) = std::path::Path::new(start).parent() {
        if parent.is_dir() {
            d = d.set_directory(parent);
        }
    }
    d.pick_file().map(|p| p.to_string_lossy().to_string())
}

/// Files of a dropped folder (or the file itself), images only.
pub fn expand_images(paths: &[std::path::PathBuf]) -> Vec<String> {
    let mut out = vec![];
    for p in paths {
        if p.is_dir() {
            out.extend(crate::model::list_images(&p.to_string_lossy()));
        } else if is_image(&p.to_string_lossy()) {
            out.push(p.to_string_lossy().to_string());
        }
    }
    out
}

/// A panel title row with an optional hint on the right.
pub fn panel_header(ui: &mut Ui, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        theme::panel_title(ui, title);
        if !hint.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(hint).color(TEXT_DIM).size(12.0));
            });
        }
    });
}

// =============================================================================
// Library
// =============================================================================
#[derive(Default)]
pub struct Library {
    pub tab: usize,
    pub media: Vec<String>,
    pub media_sel: Vec<usize>,
    anchor: Option<usize>,
    pub blocks_folded: bool,
    pub pictures_folded: bool,
}

/// Title row of a foldable library section: ▾/▸ TITLE, a hint, and (optionally) the « hide button.
fn fold_header(ui: &mut Ui, title: &str, hint: &str, folded: &mut bool, hide: bool, actions: &mut Vec<Action>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let arrow = if *folded { "▸  " } else { "▾  " };
        let r = ui
            .add(egui::Button::new(RichText::new(format!("{arrow}{}", title.to_uppercase())).font(theme::bold(11.0)).color(TEXT_DIM)).frame(false))
            .on_hover_text(if *folded { "Click to unfold" } else { "Click to fold" });
        if r.clicked() {
            *folded = !*folded;
            changed = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if hide
                && ui
                    .add(egui::Button::new(RichText::new("«").font(theme::bold(14.0)).color(TEXT_DIM)).frame(false))
                    .on_hover_text("Hide the library – it shrinks to a narrow strip (Ctrl+L)")
                    .clicked()
            {
                actions.push(Action::ToggleLibrary);
            }
            if !hint.is_empty() && !*folded {
                ui.label(RichText::new(hint).color(TEXT_DIM).size(12.0));
            }
        });
    });
    changed
}

/// Small icons for the collapsed library strip.
pub fn paint_strip_icon(p: &egui::Painter, which: &str, r: Rect, active: bool) {
    let col = if active { TEXT } else { TEXT_DIM };
    if which == "blocks" {
        let cols = [hex("#4C8DF6"), hex("#E0913A"), hex("#D45C9E")];
        for (i, c) in cols.iter().enumerate() {
            let y = r.top() + 5.0 + i as f32 * 7.0;
            let x0 = r.left() + 4.0 + (i % 2) as f32 * 5.0;
            p.rect_filled(Rect::from_min_size(Pos2::new(x0, y), Vec2::new(r.width() - 13.0, 5.0)), CornerRadius::same(2), *c);
        }
    } else {
        theme::draw_kind_glyph(p, "image", r, col);
    }
}

impl Library {
    pub fn add_files(&mut self, files: &[String]) {
        for f in files {
            if is_image(f) && !self.media.contains(f) {
                self.media.push(f.clone());
            }
        }
    }

    pub fn clear(&mut self) {
        self.media.clear();
        self.media_sel.clear();
        self.anchor = None;
    }

    /// Returns true when a section was folded or unfolded (so the app can remember it).
    pub fn ui(&mut self, ui: &mut Ui, blur: bool, actions: &mut Vec<Action>) -> bool {
        let total_h = ui.available_height();
        let mut changed = false;
        let blocks_h = if self.pictures_folded { total_h - 64.0 } else { (total_h * 0.68).max(200.0) };
        let (mut bf, mut pf) = (self.blocks_folded, self.pictures_folded);
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !bf {
                ui.set_height((blocks_h - 24.0).max(0.0));
            }
            changed |= fold_header(ui, "Blocks", "drag → timeline", &mut bf, true, actions);
            if bf {
                return;
            }
            ui.horizontal(|ui| {
                for (i, t) in ["Basic", "Task blocks"].iter().enumerate() {
                    let sel = self.tab == i;
                    let r = ui.add(egui::Button::new(RichText::new(*t).color(if sel { TEXT } else { TEXT_DIM })).frame(false));
                    if sel {
                        ui.painter().hline(r.rect.x_range(), r.rect.bottom() + 1.0, Stroke::new(2.0, ACCENT));
                    }
                    if r.clicked() {
                        self.tab = i;
                    }
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("lib_scroll").auto_shrink([false, false]).show(ui, |ui| {
                if self.tab == 0 {
                    for bt in BLOCK_TYPES {
                        let sub = format!("{}s · stimulus", fmt_g(bt.default_duration));
                        let tip = format!("{}\n{}\n\nDrag onto the timeline · double-click to add at the end", bt.title, bt.description);
                        card(ui, bt.kind, bt.kind, hex(bt.color), bt.title, &sub, &tip, actions);
                    }
                } else {
                    let mut cat = "";
                    for pr in presets::PRESETS {
                        if pr.category != cat {
                            cat = pr.category;
                            ui.add_space(4.0);
                            theme::panel_title(ui, cat);
                        }
                        let mut tip = format!("{}\n{}", pr.title, pr.description);
                        let vars = (pr.variants)();
                        if !vars.is_empty() {
                            tip.push_str(&format!(
                                "\nVariants: {} (chosen when you drop it)",
                                vars.iter().map(|v| v.0).collect::<Vec<_>>().join(", ")
                            ));
                        }
                        tip.push_str("\n\nDrag onto the timeline · double-click to add at the end");
                        card(ui, &format!("preset:{}", pr.key), pr.icon_kind, hex(pr.color), pr.title, pr.subtitle, &tip, actions);
                    }
                }
            });
        });
        ui.add_space(8.0);
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !pf {
                ui.set_height((ui.available_height() - 22.0).max(0.0));
            }
            let hint = if self.media.is_empty() { String::new() } else { format!("{}", self.media.len()) };
            changed |= fold_header(ui, "Pictures", &hint, &mut pf, false, actions);
            if pf {
                return;
            }
            let bin_h = (ui.available_height() - 34.0).max(60.0);
            egui::Frame::new().fill(BG).stroke(Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(6)).inner_margin(4).show(ui, |ui| {
                ui.set_height(bin_h);
                ui.set_width(ui.available_width());
                if self.media.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Drop pictures or folders here").color(TEXT_DIM));
                    });
                } else {
                    egui::ScrollArea::vertical().id_salt("media_scroll").auto_shrink([false, false]).show(ui, |ui| {
                        self.media_grid(ui, blur);
                    });
                }
            });
            if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("+  Import pictures…")).clicked() {
                actions.push(Action::ImportPictures);
            }
        });
        self.blocks_folded = bf;
        self.pictures_folded = pf;
        changed
    }

    fn media_grid(&mut self, ui: &mut Ui, blur: bool) {
        let cell = Vec2::new(76.0, 70.0);
        let cols = ((ui.available_width() / cell.x).floor() as usize).max(1);
        let mods = ui.input(|i| i.modifiers);
        let mut clicked: Option<usize> = None;
        let mut drag_start: Option<usize> = None;
        for row in self.media.chunks(cols).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (j, path) in row.1.iter().enumerate() {
                    let idx = row.0 * cols + j;
                    let (r, resp) = ui.allocate_exact_size(cell, Sense::click_and_drag());
                    let sel = self.media_sel.contains(&idx);
                    if sel {
                        ui.painter().rect_filled(r.shrink(1.0), CornerRadius::same(4), theme::alpha(ACCENT, 60));
                    }
                    let thumb = Rect::from_min_size(Pos2::new(r.left() + 6.0, r.top() + 3.0), Vec2::new(64.0, 48.0));
                    if !stimulus::paint_thumb(ui.painter(), thumb, path, blur) {
                        ui.painter().rect_stroke(thumb, CornerRadius::same(3), Stroke::new(1.0, BORDER), StrokeKind::Inside);
                    }
                    let name: String = crate::model::stem(path).chars().take(11).collect();
                    ui.painter().text(Pos2::new(r.center().x, r.bottom() - 9.0), Align2::CENTER_CENTER, name, theme::font(11.0), TEXT);
                    let resp = resp.on_hover_text(format!(
                        "{path}\n\nDrag onto the timeline, or onto a picture field in the inspector"
                    ));
                    if resp.clicked() {
                        clicked = Some(idx);
                    }
                    if resp.drag_started() {
                        drag_start = Some(idx);
                    }
                    if resp.dragged() {
                        let files: Vec<String> = if self.media_sel.contains(&idx) {
                            self.media_sel.iter().filter_map(|i| self.media.get(*i).cloned()).collect()
                        } else {
                            vec![path.clone()]
                        };
                        let n = files.len();
                        egui::DragAndDrop::set_payload(ui.ctx(), DragPayload::Images(files));
                        if let Some(p) = ui.ctx().pointer_interact_pos() {
                            crate::timeline::paint_tip(ui.ctx(), p, &format!("{n} picture{}", if n == 1 { "" } else { "s" }));
                        }
                    }
                }
            });
        }
        if let Some(i) = drag_start {
            if !self.media_sel.contains(&i) {
                self.media_sel = vec![i];
                self.anchor = Some(i);
            }
        }
        if let Some(i) = clicked {
            if mods.shift {
                let a = self.anchor.unwrap_or(i);
                let (lo, hi) = (a.min(i), a.max(i));
                self.media_sel = (lo..=hi).collect();
            } else if mods.command || mods.ctrl {
                if let Some(p) = self.media_sel.iter().position(|x| *x == i) {
                    self.media_sel.remove(p);
                } else {
                    self.media_sel.push(i);
                }
                self.anchor = Some(i);
            } else {
                self.media_sel = vec![i];
                self.anchor = Some(i);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn card(ui: &mut Ui, kind: &str, icon_kind: &str, color: Color32, title: &str, sub: &str, tip: &str, actions: &mut Vec<Action>) {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click_and_drag());
    let hovered = resp.hovered();
    let p = ui.painter();
    p.rect(
        r,
        CornerRadius::same(8),
        if hovered { hex("#2B303B") } else { PANEL_2 },
        Stroke::new(1.0, if hovered { color } else { BORDER }),
        StrokeKind::Inside,
    );
    theme::paint_kind_icon(p, icon_kind, Rect::from_min_size(Pos2::new(r.left() + 8.0, r.center().y - 13.0), Vec2::splat(26.0)), color);
    p.text(Pos2::new(r.left() + 44.0, r.top() + 13.0), Align2::LEFT_CENTER, title, theme::bold(13.0), TEXT);
    p.text(Pos2::new(r.left() + 44.0, r.top() + 28.0), Align2::LEFT_CENTER, sub, theme::font(11.0), TEXT_DIM);
    let resp = resp.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(tip);
    if resp.double_clicked() {
        if let Some(key) = kind.strip_prefix("preset:") {
            actions.push(Action::ChooseVariant { key: key.to_string(), start: None, pos: None });
        } else {
            actions.push(Action::AddKind { kind: kind.to_string(), start: None, track: None });
        }
    }
    if resp.dragged() {
        egui::DragAndDrop::set_payload(ui.ctx(), DragPayload::Kind(kind.to_string()));
        if let Some(pt) = ui.ctx().pointer_interact_pos() {
            let lp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag_icon")));
            theme::paint_kind_icon(&lp, icon_kind, Rect::from_center_size(pt, Vec2::splat(36.0)), color);
        }
    }
    ui.add_space(5.0);
}

// =============================================================================
// Participant view (program monitor)
// =============================================================================
pub fn monitor_ui(ui: &mut Ui, doc: &Document, t: f64, blur: bool) {
    let avail = ui.available_rect_before_wrap();
    let (resp_rect, _) = ui.allocate_exact_size(avail.size(), Sense::hover());
    let r = resp_rect.shrink(4.0);
    let w = r.width().min(r.height() * 16.0 / 9.0);
    let screen = Rect::from_center_size(r.center(), Vec2::new(w, w * 9.0 / 16.0));
    let par = &doc.paradigm;
    let b = par.stimulus_at(t);
    let st = stimulus::preview_state(b, t);
    stimulus::paint_stimulus(ui.painter(), screen, par, b, &st, 1080.0, blur);
    ui.painter().rect_stroke(screen, CornerRadius::ZERO, Stroke::new(1.0, BORDER), StrokeKind::Outside);
    let (text, col) = match b {
        Some(b) => {
            let title = if b.kind == "gonogo" {
                b.display_label()
            } else {
                presets::trial_title(&b.params.s("preset", "")).unwrap_or(b.title()).to_string()
            };
            let text = if b.condition.is_empty() { title } else { format!("{title} · {}", b.condition) };
            (text, hex(b.color()))
        }
        None => ("background".to_string(), hex("#5B6577")),
    };
    let p = ui.painter();
    let g = p.layout_no_wrap(text, theme::bold(11.0), Color32::WHITE);
    let tag = Rect::from_min_size(screen.min + Vec2::new(8.0, 8.0), Vec2::new(g.size().x + 14.0, 18.0));
    p.rect_filled(tag, CornerRadius::same(9), theme::alpha(col, 210));
    p.galley(tag.center() - g.size() / 2.0, g, Color32::WHITE);
}

// =============================================================================
// Inspector
// =============================================================================
const LABEL_W: f32 = 112.0;

#[derive(Default)]
pub struct Inspector {}

fn label_cell(ui: &mut Ui, text: RichText) {
    ui.allocate_ui_with_layout(Vec2::new(LABEL_W, 24.0), Layout::right_to_left(Align::Center), |ui| {
        ui.label(text);
    });
}

fn row(ui: &mut Ui, label: RichText, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        label_cell(ui, label);
        add(ui);
    });
}

fn section(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    if !text.is_empty() {
        ui.label(RichText::new(text).font(theme::bold(11.0)).color(TEXT_DIM));
    } else {
        ui.separator();
    }
}

fn mixed(blocks: &[Block], key: &str) -> bool {
    blocks.iter().any(|b| b.params.get(key) != blocks[0].params.get(key))
}

/// Form label that turns orange (≠) when the selected blocks differ in this setting.
fn lab(text: &str, blocks: &[Block], keys: &[&str]) -> RichText {
    if blocks.len() > 1 && keys.iter().any(|k| mixed(blocks, k)) {
        RichText::new(format!("{text} ≠")).color(ACCENT)
    } else {
        RichText::new(text)
    }
}

fn ids(blocks: &[Block]) -> Vec<String> {
    blocks.iter().map(|b| b.id.clone()).collect()
}

fn set_params(doc: &mut Document, ids: &[String], key: &str, value: Value) {
    let mk = format!("{key}@{}", ids.join(","));
    doc.edit_merge(Some(&mk), |par| {
        for id in ids {
            if let Some(b) = par.block_mut(id) {
                b.params.insert(key.to_string(), value.clone());
            }
        }
    });
}

fn set_task(doc: &mut Document, ids: &[String], key: &str, value: Value) {
    let mk = format!("task:{key}@{}", ids.join(","));
    doc.edit_merge(Some(&mk), |par| {
        for id in ids {
            if let Some(b) = par.block_mut(id) {
                b.params.insert(key.to_string(), value.clone());
                presets::derive(b);
            }
        }
    });
}

fn line_edit(ui: &mut Ui, salt: &str, value: &mut String, hint: &str) -> bool {
    ui.add(egui::TextEdit::singleline(value).id_salt(salt).hint_text(hint).desired_width(f32::INFINITY)).changed()
}

fn text_param(ui: &mut Ui, doc: &mut Document, blocks: &[Block], key: &str, title: &str, hint: &str) {
    let mx = mixed(blocks, key);
    let mut s = if mx { String::new() } else { blocks[0].params.s(key, "") };
    let hint = if mx { "different texts – type to set all" } else { hint };
    row(ui, lab(title, blocks, &[key]), |ui| {
        if line_edit(ui, &format!("tp_{key}"), &mut s, hint) {
            set_params(doc, &ids(blocks), key, json!(s));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn num_param(
    ui: &mut Ui,
    doc: &mut Document,
    blocks: &[Block],
    key: &str,
    title: &str,
    default: f64,
    range: (f64, f64),
    speed: f64,
    decimals: usize,
    suffix: &str,
    int: bool,
) {
    let mut v = blocks[0].params.f(key, default);
    row(ui, lab(title, blocks, &[key]), |ui| {
        let dv = egui::DragValue::new(&mut v).range(range.0..=range.1).speed(speed).suffix(suffix).max_decimals(decimals);
        let dv = if int { dv.fixed_decimals(0) } else { dv };
        if ui.add(dv).changed() {
            set_params(doc, &ids(blocks), key, if int { json!(v.round() as i64) } else { json!(v) });
        }
    });
}

fn combo_param(ui: &mut Ui, doc: &mut Document, blocks: &[Block], key: &str, title: &str, items: &[(String, Value)], default: Value) {
    let mx = blocks.len() > 1 && mixed(blocks, key);
    let cur = blocks[0].params.get(key).cloned().unwrap_or(default);
    let text = if mx {
        "different – choose to set all".to_string()
    } else {
        items.iter().find(|i| i.1 == cur).map(|i| i.0.clone()).unwrap_or_else(|| crate::model::v_str(Some(&cur)).unwrap_or_default())
    };
    let mut chosen = None;
    row(ui, lab(title, blocks, &[key]), |ui| {
        egui::ComboBox::from_id_salt(format!("cb_{key}")).width(ui.available_width() - 4.0).selected_text(text).show_ui(ui, |ui| {
            for (t, val) in items {
                if ui.selectable_label(!mx && *val == cur, t.as_str()).clicked() {
                    chosen = Some(val.clone());
                }
            }
        });
    });
    if let Some(v) = chosen {
        set_params(doc, &ids(blocks), key, v);
    }
}

fn color_edit(ui: &mut Ui, color: &str) -> Option<String> {
    let mut c = hex(color);
    let mut out = None;
    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
        out = Some(theme::to_hex(c));
    }
    ui.label(RichText::new(color.to_uppercase()).color(TEXT_DIM).monospace());
    out
}

fn color_param(ui: &mut Ui, doc: &mut Document, blocks: &[Block], key: &str, title: &str, default: &str) {
    let cur = blocks[0].params.s(key, default);
    row(ui, lab(title, blocks, &[key]), |ui| {
        if let Some(c) = color_edit(ui, &cur) {
            set_params(doc, &ids(blocks), key, json!(c));
        }
    });
}

/// A picture field: preview, file name and "Choose…". Pictures can also be dropped onto it – from the
/// picture bin on the left or straight from the desktop.
fn image_picker(ui: &mut Ui, path: &str, height: f32, blur: bool) -> Option<String> {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, height), Sense::hover());
    let ctx = ui.ctx().clone();
    let bin_payload = egui::DragAndDrop::payload::<DragPayload>(&ctx).and_then(|p| match &*p {
        DragPayload::Images(files) => files.first().cloned(),
        DragPayload::Kind(_) => None,
    });
    let desktop_drag = ui.input(|i| !i.raw.hovered_files.is_empty());
    let over = ui
        .input(|i| i.pointer.latest_pos())
        .map(|pos| r.contains(pos) && ui.clip_rect().contains(pos))
        .unwrap_or(false);
    let dragging = bin_payload.is_some() || desktop_drag;
    let mut out = None;

    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(6), if dragging && over { theme::alpha(ACCENT, 40) } else { BG });
    let (bw, bc) = if dragging { (if over { 2.5 } else { 1.5 }, ACCENT) } else { (1.0, BORDER) };
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for wv in pts.windows(2) {
        p.add(egui::Shape::dashed_line(&[wv[0], wv[1]], Stroke::new(bw, bc), 5.0, 3.0));
    }
    if dragging && over {
        stimulus::draw_text(p, r, "Drop to use this picture", theme::bold(13.0), ACCENT, true);
    } else if path.is_empty() {
        let msg = if dragging { "Drop a picture here" } else { "No picture – drag one here from Pictures, or click Choose…" };
        stimulus::draw_text(p, r.shrink(6.0), msg, theme::font(12.0), if dragging { ACCENT } else { TEXT_DIM }, true);
    } else if !stimulus::paint_thumb(p, r.shrink(3.0), path, blur) {
        let msg = if std::path::Path::new(path).exists() { "loading…" } else { "File not found" };
        stimulus::draw_text(p, r, msg, theme::font(12.0), TEXT_DIM, false);
    }
    if over {
        // dropped from the picture bin
        if let Some(f) = &bin_payload {
            if ui.input(|i| i.pointer.any_released()) {
                egui::DragAndDrop::clear_payload(&ctx);
                out = Some(f.clone());
            } else {
                ctx.request_repaint();
            }
        }
        // dropped from the desktop / file manager
        let dropped: Vec<std::path::PathBuf> =
            ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if let Some(f) = expand_images(&dropped).into_iter().next() {
            out = Some(f);
        }
    }
    ui.horizontal(|ui| {
        let name = crate::model::basename(path);
        ui.add(egui::Label::new(RichText::new(name).color(TEXT_DIM).size(12.0)).truncate()).on_hover_text(path);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Choose…").clicked() {
                out = pick_image("Choose picture", path);
            }
        });
    });
    out
}

impl Inspector {
    pub fn ui(&mut self, ui: &mut Ui, doc: &mut Document, blur: bool, actions: &mut Vec<Action>) {
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height((ui.available_height() - 22.0).max(0.0));
            panel_header(ui, "Inspector", "");
            egui::ScrollArea::vertical().id_salt("inspector_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(ui.available_width() - 8.0);
                let blocks: Vec<Block> = doc.selected_blocks().into_iter().cloned().collect();
                if blocks.is_empty() {
                    self.paradigm_ui(ui, doc);
                } else if blocks.len() == 1 {
                    self.block_ui(ui, doc, &blocks[0], blur, actions);
                } else {
                    self.multi_ui(ui, doc, &blocks, blur, actions);
                }
            });
        });
    }

    fn paradigm_ui(&mut self, ui: &mut Ui, doc: &mut Document) {
        ui.label(RichText::new("Paradigm").font(theme::bold(22.0)));
        theme::hint(ui, "Nothing selected – these settings apply to the whole run.\nClick a block in the timeline to edit it.");
        ui.add_space(6.0);
        let mut name = doc.paradigm.name.clone();
        row(ui, RichText::new("Name"), |ui| {
            if line_edit(ui, "par_name", &mut name, "") {
                doc.edit_merge(Some("par_name"), |p| p.name = name.clone());
            }
        });
        section(ui, "SCANNER");
        let mut tr = doc.paradigm.tr;
        row(ui, RichText::new("TR"), |ui| {
            if ui
                .add(egui::DragValue::new(&mut tr).range(0.1..=20.0).speed(0.01).suffix(" s").max_decimals(3))
                .on_hover_text("Repetition time of your EPI sequence. The timeline grid and volume counter follow it.")
                .changed()
            {
                doc.edit_merge(Some("par_tr"), |p| p.tr = tr);
            }
        });
        let mut dummy = doc.paradigm.dummy_scans;
        row(ui, RichText::new("Dummy scans"), |ui| {
            if ui
                .add(egui::DragValue::new(&mut dummy).range(0..=50).suffix(" volumes"))
                .on_hover_text("Triggers to ignore before the paradigm starts (scanner warm-up).")
                .changed()
            {
                doc.edit_merge(Some("par_dummy"), |p| p.dummy_scans = dummy);
            }
        });
        let ws = doc.paradigm.waiting_screen.clone();
        row(ui, RichText::new("Before start"), |ui| {
            let items = [("fixation", "Fixation cross"), ("blank", "Empty screen"), ("text", "Text")];
            let cur = items.iter().find(|i| i.0 == ws).map(|i| i.1).unwrap_or("Fixation cross");
            egui::ComboBox::from_id_salt("par_wait").selected_text(cur).show_ui(ui, |ui| {
                for (k, t) in items {
                    if ui.selectable_label(ws == k, t).clicked() {
                        doc.edit(|p| p.waiting_screen = k.into());
                    }
                }
            })
            .response
            .on_hover_text("What the participant sees until the paradigm starts (incl. dummy scans).\nTrigger counts are only shown in the control room.");
        });
        if doc.paradigm.waiting_screen == "text" {
            let mut wt = doc.paradigm.waiting_text.clone();
            row(ui, RichText::new(""), |ui| {
                if line_edit(ui, "par_wt", &mut wt, "") {
                    doc.edit_merge(Some("par_wt"), |p| p.waiting_text = wt.clone());
                }
            });
        }
        let mut et = doc.paradigm.end_text.clone();
        row(ui, RichText::new("After the run"), |ui| {
            if line_edit(ui, "par_et", &mut et, "empty screen") {
                doc.edit_merge(Some("par_et"), |p| p.end_text = et.clone());
            }
        });
        section(ui, "SCREEN");
        let bg = doc.paradigm.background.clone();
        row(ui, RichText::new("Background"), |ui| {
            if let Some(c) = color_edit(ui, &bg) {
                doc.edit_merge(Some("par_bg"), |p| p.background = c.clone());
            }
        });
        section(ui, "SUMMARY");
        let par = &doc.paradigm;
        let conds = par.conditions();
        for (k, v) in [
            ("Duration", fmt_time(par.duration(), 1)),
            ("Volumes", format!("{} (+{} dummy)", par.volumes(), par.dummy_scans)),
            ("Blocks", par.blocks.len().to_string()),
            ("Conditions", if conds.is_empty() { "–".into() } else { conds.join(", ") }),
        ] {
            row(ui, RichText::new(k).color(TEXT_DIM), |ui| {
                ui.add(egui::Label::new(v).wrap());
            });
        }
    }

    fn header(ui: &mut Ui, kind: &str, color: Color32, title: &str) {
        ui.horizontal(|ui| {
            theme::kind_icon(ui, kind, color, 34.0);
            ui.label(RichText::new(title).font(theme::bold(22.0)));
        });
    }

    fn block_ui(&mut self, ui: &mut Ui, doc: &mut Document, b: &Block, blur: bool, actions: &mut Vec<Action>) {
        let bid = b.id.clone();
        let title = presets::trial_title(&b.params.s("preset", "")).unwrap_or(b.title());
        Self::header(ui, &b.kind, hex(b.color()), title);
        theme::hint(ui, b.btype().description);
        ui.add_space(4.0);
        let mut label = b.label.clone();
        row(ui, RichText::new("Label"), |ui| {
            if line_edit(ui, "blk_label", &mut label, &b.display_label()) {
                let l = label.clone();
                doc.edit_merge(Some(&format!("label@{bid}")), |p| {
                    if let Some(x) = p.block_mut(&bid) {
                        x.label = l;
                    }
                });
            }
        });
        condition_row(ui, doc, std::slice::from_ref(b));

        section(ui, "TIMING");
        let tr = doc.paradigm.tr;
        let step = if tr > 0.0 { tr } else { 0.5 };
        let mut start = b.start;
        row(ui, RichText::new("Start"), |ui| {
            if ui.add(egui::DragValue::new(&mut start).range(0.0..=100000.0).speed(step / 20.0).suffix(" s").max_decimals(3)).changed() {
                doc.edit_merge(Some(&format!("start@{bid}")), |p| {
                    if let Some(x) = p.block_mut(&bid) {
                        x.start = round4(start);
                    }
                });
            }
        });
        let mut dur = b.duration;
        row(ui, RichText::new("Duration"), |ui| {
            if ui.add(egui::DragValue::new(&mut dur).range(0.05..=100000.0).speed(step / 20.0).suffix(" s").max_decimals(3)).changed() {
                doc.edit_merge(Some(&format!("dur@{bid}")), |p| {
                    if let Some(x) = p.block_mut(&bid) {
                        x.duration = round4(dur);
                    }
                });
            }
            if tr > 0.0 && ui.button("⟷ TR").on_hover_text("Round start and duration to whole TRs").clicked() {
                doc.edit(|p| {
                    if let Some(x) = p.block_mut(&bid) {
                        x.start = (x.start / tr).round() * tr;
                        x.duration = ((x.duration / tr).round() * tr).max(tr);
                    }
                });
            }
        });
        if tr > 0.0 {
            let on_grid = (b.start / tr - (b.start / tr).round()).abs() < 1e-6;
            let info = format!(
                "Volume {} → {}  ·  {:.2} TR{}",
                (b.start / tr).floor() as i64 + 1,
                (b.start.max(b.end() - 1e-6) / tr).floor() as i64 + 1,
                b.duration / tr,
                if on_grid { "" } else { "  ·  not on a TR boundary" }
            );
            row(ui, RichText::new(""), |ui| {
                theme::hint(ui, info);
            });
        }
        params_ui(ui, doc, std::slice::from_ref(b), blur);
        let same: Vec<String> = doc.paradigm.blocks.iter().filter(|o| o.kind == b.kind).map(|o| o.id.clone()).collect();
        if same.len() > 1 {
            section(ui, "");
            if ui
                .add_sized([ui.available_width(), 26.0], egui::Button::new(format!("Edit all {} {} blocks together", same.len(), b.title())))
                .on_hover_text("Selects every block of this type – changes in the inspector then apply to all of them.")
                .clicked()
            {
                actions.push(Action::Select(same));
            }
        }
        section(ui, "");
        ui.horizontal(|ui| {
            if ui.button("Duplicate").clicked() {
                actions.push(Action::Duplicate);
            }
            if ui.button(RichText::new("Delete").color(DANGER)).clicked() {
                actions.push(Action::Delete { ripple: false });
            }
        });
    }

    fn multi_ui(&mut self, ui: &mut Ui, doc: &mut Document, blocks: &[Block], blur: bool, actions: &mut Vec<Action>) {
        let mut kinds: Vec<(String, Vec<String>)> = vec![];
        for b in blocks {
            match kinds.iter_mut().find(|k| k.0 == b.kind) {
                Some(k) => k.1.push(b.id.clone()),
                None => kinds.push((b.kind.clone(), vec![b.id.clone()])),
            }
        }
        if kinds.len() == 1 {
            Self::header(ui, &blocks[0].kind, hex(blocks[0].color()), &format!("{} {} blocks", blocks.len(), blocks[0].title()));
        } else {
            ui.label(RichText::new(format!("{} blocks", blocks.len())).font(theme::bold(22.0)));
        }
        let start = blocks.iter().map(|b| b.start).fold(f64::INFINITY, f64::min);
        let span = blocks.iter().map(|b| b.end()).fold(f64::NEG_INFINITY, f64::max) - start;
        theme::hint(ui, format!("Span {span:.2}s · starting at {start:.2}s\nChanges below apply to all selected blocks."));
        if kinds.len() > 1 {
            theme::hint(
                ui,
                "Different block types are selected. To change type-specific settings (e.g. how a rating is answered) for \
                 all of them at once, keep only one type:",
            );
            kinds.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            ui.horizontal_wrapped(|ui| {
                for (kind, kids) in &kinds {
                    let title = crate::model::block_type(kind).map(|t| t.title).unwrap_or("");
                    if ui.button(format!("{} × {title}", kids.len())).on_hover_text(format!("Keep only the {title} blocks selected")).clicked() {
                        actions.push(Action::Select(kids.clone()));
                    }
                }
            });
        }
        condition_row(ui, doc, blocks);
        let durs: Vec<f64> = blocks.iter().map(|b| round4(b.duration)).collect();
        let same_dur = durs.iter().all(|d| *d == durs[0]);
        let mut dur = durs[0];
        let tr = doc.paradigm.tr;
        let ids_all = ids(blocks);
        row(ui, RichText::new(if same_dur { "Duration" } else { "Duration ≠" }).color(if same_dur { TEXT } else { ACCENT }), |ui| {
            if ui
                .add(egui::DragValue::new(&mut dur).range(0.05..=100000.0).speed(if tr > 0.0 { tr / 20.0 } else { 0.025 }).suffix(" s").max_decimals(3))
                .changed()
            {
                let v = round4(dur);
                doc.edit_merge(Some(&format!("mdur@{}", ids_all.join(","))), |par| {
                    // keep the sequence tight: shift following blocks on each track
                    let mut order = ids_all.clone();
                    order.sort_by(|a, b| {
                        let sa = par.block(a).map(|x| x.start).unwrap_or(0.0);
                        let sb = par.block(b).map(|x| x.start).unwrap_or(0.0);
                        sa.total_cmp(&sb)
                    });
                    for id in order {
                        let Some(b) = par.block(&id).cloned() else { continue };
                        let delta = v - b.duration;
                        for o in par.blocks.iter_mut() {
                            if o.track == b.track && o.start >= b.end() - 1e-9 && o.id != b.id {
                                o.start = round4(o.start + delta);
                            }
                        }
                        if let Some(x) = par.block_mut(&id) {
                            x.duration = v;
                        }
                    }
                });
            }
        });
        row(ui, RichText::new(""), |ui| {
            theme::hint(ui, "Changing the duration moves the following blocks so no gaps or overlaps appear.");
        });
        if kinds.len() == 1 {
            params_ui(ui, doc, blocks, blur);
        }
        section(ui, "");
        ui.horizontal(|ui| {
            if ui.button("Duplicate").clicked() {
                actions.push(Action::Duplicate);
            }
            if ui.button("Repeat ×N…").clicked() {
                actions.push(Action::RepeatAsk);
            }
            if ui.button(RichText::new("Delete").color(DANGER)).clicked() {
                actions.push(Action::Delete { ripple: false });
            }
        });
    }
}

fn condition_row(ui: &mut Ui, doc: &mut Document, blocks: &[Block]) {
    let mx = blocks.iter().any(|b| b.condition != blocks[0].condition);
    let mut c = if mx { String::new() } else { blocks[0].condition.clone() };
    let conds = doc.paradigm.conditions();
    let id_list = ids(blocks);
    let mut set: Option<String> = None;
    row(ui, if mx { RichText::new("Condition ≠").color(ACCENT) } else { RichText::new("Condition") }, |ui| {
        let w = ui.available_width() - 34.0;
        let hint = if mx { "different – type to set all" } else { "e.g. positive, negative…" };
        let r = ui.add(egui::TextEdit::singleline(&mut c).id_salt("cond_edit").hint_text(hint).desired_width(w));
        if r.changed() {
            set = Some(c.trim().to_string());
        }
        r.on_hover_text("Condition name – becomes 'trial_type' in the exported events file.");
        ui.menu_button("▾", |ui| {
            if conds.is_empty() {
                ui.label(RichText::new("no conditions yet").color(TEXT_DIM));
            }
            for k in &conds {
                if ui.button(k).clicked() {
                    set = Some(k.clone());
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("(none)").clicked() {
                set = Some(String::new());
                ui.close();
            }
        });
    });
    if let Some(v) = set {
        doc.edit_merge(Some(&format!("cond@{}", id_list.join(","))), |p| {
            for id in &id_list {
                if let Some(b) = p.block_mut(id) {
                    b.condition = v.clone();
                }
            }
        });
    }
}

fn task_fields_ui(ui: &mut Ui, doc: &mut Document, blocks: &[Block]) {
    let first = blocks[0].params.s("preset", "");
    if first.is_empty() || blocks.iter().any(|b| b.params.s("preset", "") != first) {
        return;
    }
    let fields = presets::trial_fields(&first);
    if fields.is_empty() {
        return;
    }
    let title = presets::trial_title(&first).unwrap_or("").to_uppercase();
    section(ui, &if blocks.len() == 1 { title } else { format!("{title}  ·  {} TRIALS", blocks.len()) });
    let id_list = ids(blocks);
    for (param, label, spec) in fields {
        let cur = blocks[0].params.get(param).cloned().unwrap_or(Value::Null);
        let mx = blocks.len() > 1 && mixed(blocks, param);
        row(ui, lab(label, blocks, &[param]), |ui| match &spec {
            FieldSpec::Text => {
                let mut s = if mx { String::new() } else { crate::model::v_str(Some(&cur)).unwrap_or_default() };
                if line_edit(ui, &format!("task_{param}"), &mut s, if mx { "different – type to set all" } else { "" }) && !s.is_empty() {
                    set_task(doc, &id_list, param, json!(s));
                }
            }
            FieldSpec::Int => {
                let mut v = cur.as_f64().unwrap_or(0.0) as i64;
                if ui.add(egui::DragValue::new(&mut v).range(0..=9)).changed() {
                    set_task(doc, &id_list, param, json!(v));
                }
            }
            FieldSpec::Choices(items) => {
                let text = if mx {
                    "different – choose to set all".to_string()
                } else {
                    items.iter().find(|i| i.1 == cur).map(|i| i.0.to_string()).unwrap_or_default()
                };
                let mut chosen = None;
                egui::ComboBox::from_id_salt(format!("task_cb_{param}")).width(ui.available_width() - 4.0).selected_text(text).show_ui(ui, |ui| {
                    for (t, v) in items {
                        if ui.selectable_label(!mx && *v == cur, *t).clicked() {
                            chosen = Some(v.clone());
                        }
                    }
                });
                if let Some(v) = chosen {
                    set_task(doc, &id_list, param, v);
                }
            }
        });
    }
    row(ui, RichText::new(""), |ui| {
        theme::hint(ui, "Text, colour, correct button and condition follow these settings automatically.");
    });
}

/// Block-type settings – for one block or several blocks of the same type.
fn params_ui(ui: &mut Ui, doc: &mut Document, blocks: &[Block], blur: bool) {
    let b = &blocks[0];
    let single = blocks.len() == 1;
    let id_list = ids(blocks);
    let names: Vec<(String, Value)> = doc.paradigm.hardware.button_names().into_iter().map(|n| (n.clone(), json!(n))).collect();
    task_fields_ui(ui, doc, blocks);
    match b.kind.as_str() {
        "image" | "rating" => {
            section(ui, "PICTURE");
            if single {
                if let Some(p) = image_picker(ui, &b.params.s("path", ""), 110.0, blur) {
                    set_params(doc, &id_list, "path", json!(p));
                }
            }
            let mut sc = b.params.f("scale", 0.8) * 100.0;
            row(ui, lab("Size", blocks, &["scale"]), |ui| {
                if ui.add(egui::DragValue::new(&mut sc).range(5.0..=100.0).speed(0.5).suffix(" %").max_decimals(0)).on_hover_text("How much of the screen the picture may fill").changed() {
                    set_params(doc, &id_list, "scale", json!(round4(sc / 100.0)));
                }
            });
            if b.kind == "image" {
                num_param(ui, doc, blocks, "flicker_hz", "Flicker", 0.0, (0.0, 60.0), 0.1, 1, " Hz", false);
                if single && b.params.f("flicker_hz", 0.0) > 0.0 {
                    theme::hint(ui, "Alternates with");
                    if let Some(p) = image_picker(ui, &b.params.s("flicker_path", ""), 72.0, blur) {
                        set_params(doc, &id_list, "flicker_path", json!(p));
                    }
                }
            }
        }
        _ => {}
    }
    match b.kind.as_str() {
        "fixation" => {
            section(ui, "APPEARANCE");
            let mx = mixed(blocks, "symbol");
            let mut s = if mx { String::new() } else { b.params.s("symbol", "+") };
            row(ui, lab("Symbol", blocks, &["symbol"]), |ui| {
                let w = ui.available_width() - 34.0;
                if ui.add(egui::TextEdit::singleline(&mut s).id_salt("fix_sym").desired_width(w)).changed() && !s.is_empty() {
                    set_params(doc, &id_list, "symbol", json!(s));
                }
                ui.menu_button("▾", |ui| {
                    for sym in ["+", "•", "×", "○"] {
                        if ui.button(RichText::new(sym).font(theme::sym(16.0, false))).clicked() {
                            set_params(doc, &id_list, "symbol", json!(sym));
                            ui.close();
                        }
                    }
                });
            });
            num_param(ui, doc, blocks, "size", "Size", 72.0, (8.0, 600.0), 1.0, 0, " px", true);
            color_param(ui, doc, blocks, "color", "Colour", "#FFFFFF");
        }
        "text" => {
            section(ui, "TEXT");
            let mx = mixed(blocks, "text");
            let mut s = if mx { String::new() } else { b.params.s("text", "") };
            ui.label(lab("Text", blocks, &["text"]));
            let hint = if mx {
                "Different texts – type here to give all selected blocks the same text.\n(To change only a word everywhere: Edit → Find and replace)"
            } else {
                ""
            };
            if ui
                .add(egui::TextEdit::multiline(&mut s).id_salt("txt_multi").hint_text(hint).desired_rows(4).desired_width(f32::INFINITY))
                .changed()
            {
                set_params(doc, &id_list, "text", json!(s));
            }
            num_param(ui, doc, blocks, "size", "Size", 40.0, (8.0, 300.0), 1.0, 0, " px", true);
            color_param(ui, doc, blocks, "color", "Colour", "#FFFFFF");
        }
        "rating" => {
            section(ui, "RATING SCALE");
            text_param(ui, doc, blocks, "question", "Question", "");
            let (mut lo, mut hi) = (b.params.i("min", 1), b.params.i("max", 4));
            row(ui, lab("Values", blocks, &["min", "max"]), |ui| {
                if ui.add(egui::DragValue::new(&mut lo).range(-100..=100)).changed() {
                    set_params(doc, &id_list, "min", json!(lo));
                }
                ui.label("to");
                if ui.add(egui::DragValue::new(&mut hi).range(-100..=100)).changed() {
                    set_params(doc, &id_list, "max", json!(hi));
                }
            });
            text_param(ui, doc, blocks, "left_label", "Left label", "");
            text_param(ui, doc, blocks, "right_label", "Right label", "");
            combo_param(
                ui,
                doc,
                blocks,
                "mode",
                "Answer with",
                &[("Move cursor ◀ ▶ and confirm".into(), json!("cursor")), ("One grip button per value".into(), json!("direct"))],
                json!("direct"),
            );
            rating_buttons_ui(ui, doc, blocks);
        }
        "match" => {
            section(ui, if single { "PICTURES  –  TOP IS THE TARGET" } else { "MATCHING TRIALS" });
            if single {
                for (key, title) in [("top", "Top (target)"), ("left", "Bottom left"), ("right", "Bottom right")] {
                    theme::hint(ui, title);
                    if let Some(p) = image_picker(ui, &b.params.s(key, ""), 72.0, blur) {
                        set_params(doc, &id_list, key, json!(p));
                    }
                }
                combo_param(ui, doc, blocks, "correct", "Correct match", &[("Bottom left".into(), json!("left")), ("Bottom right".into(), json!("right"))], json!("left"));
            }
            combo_param(
                ui,
                doc,
                blocks,
                "show_choice",
                "Feedback",
                &[("Frame the chosen picture".into(), json!(true)), ("No frame".into(), json!(false))],
                json!(true),
            );
            color_param(ui, doc, blocks, "choice_color", "Frame colour", "#FFD23F");
            combo_param(ui, doc, blocks, "left_button", "Button = left", &names, json!("Left index"));
            combo_param(ui, doc, blocks, "right_button", "Button = right", &names, json!("Right index"));
        }
        "gonogo" => {
            section(ui, "GO / NO-GO");
            combo_param(
                ui,
                doc,
                blocks,
                "trial",
                "Trial",
                &[("Go  – participant presses".into(), json!("go")), ("No-Go  – participant holds back".into(), json!("nogo"))],
                json!("go"),
            );
            text_param(ui, doc, blocks, "text", "Stimulus", "letter or word");
            num_param(ui, doc, blocks, "stim_time", "Visible for", 0.5, (0.05, 60.0), 0.01, 2, " s", false);
            let mut go_names = vec![("Any grip button".to_string(), json!("any"))];
            go_names.extend(names.iter().cloned());
            combo_param(ui, doc, blocks, "button", "Go button", &go_names, json!("any"));
            if single {
                theme::hint(ui, "…or show a picture instead of the text");
                if let Some(p) = image_picker(ui, &b.params.s("path", ""), 72.0, blur) {
                    set_params(doc, &id_list, "path", json!(p));
                }
            }
        }
        "choice" => {
            section(ui, "STIMULUS");
            text_param(ui, doc, blocks, "text", "Text", "word, letter or symbol");
            color_param(ui, doc, blocks, "color", "Colour", "#FFFFFF");
            num_param(ui, doc, blocks, "size", "Size", 120.0, (8.0, 600.0), 1.0, 0, " px", true);
            let mut st = b.params.f("stim_time", 0.0);
            row(ui, lab("Visible for", blocks, &["stim_time"]), |ui| {
                let r = ui.add(
                    egui::DragValue::new(&mut st)
                        .range(0.0..=60.0)
                        .speed(0.01)
                        .max_decimals(2)
                        .custom_formatter(|v, _| if v <= 0.0 { "whole trial".into() } else { format!("{v:.2} s") }),
                );
                if r.on_hover_text("How long the stimulus is visible; afterwards the fixation symbol. 0 = whole trial.").changed() {
                    set_params(doc, &id_list, "stim_time", json!(st));
                }
            });
            section(ui, "ANSWER");
            let mut corr = vec![("Any button (hit)".to_string(), json!("any"))];
            corr.extend(names.iter().cloned());
            corr.push(("No press (withhold)".into(), json!("")));
            combo_param(ui, doc, blocks, "correct", "Correct", &corr, json!("Right index"));
            let mut acc = vec![("Any grip button".to_string(), json!("any"))];
            acc.extend(names.iter().cloned());
            combo_param(ui, doc, blocks, "buttons", "Accept", &acc, json!("any"));
            if single {
                theme::hint(ui, "…or a picture");
                if let Some(p) = image_picker(ui, &b.params.s("path", ""), 72.0, blur) {
                    set_params(doc, &id_list, "path", json!(p));
                }
            }
        }
        "feedback" => {
            section(ui, "FEEDBACK  –  ABOUT THE PREVIOUS TRIAL");
            text_param(ui, doc, blocks, "hit_text", "If done right", "");
            color_param(ui, doc, blocks, "hit_color", "Colour", "#2FB37A");
            text_param(ui, doc, blocks, "miss_text", "If missed / wrong", "");
            color_param(ui, doc, blocks, "miss_color", "Colour", "#F0506E");
            if single {
                let info = match doc.paradigm.previous_scored(b) {
                    Some(prev) => format!("Reports on: {} “{}” at {}", prev.title(), prev.display_label(), fmt_time(prev.start, 1)),
                    None => "⚠ No scored trial before this block.".into(),
                };
                row(ui, RichText::new(""), |ui| {
                    theme::hint(ui, info);
                });
            }
        }
        _ => {}
    }
}

/// A combo box for choosing a grip button; `""` stands for the hardware setup's default.
fn button_choice(ui: &mut Ui, salt: &str, current: &str, default_name: &str, mixed: bool, names: &[String]) -> Option<String> {
    let text = if mixed {
        "different – choose to set all".to_string()
    } else if current.is_empty() {
        format!("{default_name}  (default)")
    } else {
        current.to_string()
    };
    let mut out = None;
    egui::ComboBox::from_id_salt(salt).width(ui.available_width() - 4.0).selected_text(text).show_ui(ui, |ui| {
        if ui.selectable_label(!mixed && current.is_empty(), format!("Default from Hardware  ({default_name})")).clicked() {
            out = Some(String::new());
        }
        ui.separator();
        for n in names {
            if ui.selectable_label(!mixed && current == n, n.as_str()).clicked() {
                out = Some(n.clone());
            }
        }
    });
    out
}

/// Which grip buttons answer a rating scale – chosen freely per block (or for all selected blocks).
fn rating_buttons_ui(ui: &mut Ui, doc: &mut Document, blocks: &[Block]) {
    let b = &blocks[0];
    let hw = doc.paradigm.hardware.clone();
    let names = hw.button_names();
    let id_list = ids(blocks);
    if blocks.len() > 1 && mixed(blocks, "mode") {
        row(ui, RichText::new(""), |ui| {
            theme::hint(ui, "Some selected ratings use buttons, some the cursor – choose one way to set the buttons.");
        });
        return;
    }
    section(ui, "BUTTONS");
    if b.params.s("mode", "direct") == "cursor" {
        let defaults = [hw.move_left.clone(), hw.move_right.clone(), hw.confirm.clone()];
        for (i, (key, title)) in [("cursor_left", "Move left ◀"), ("cursor_right", "Move right ▶"), ("cursor_confirm", "Confirm ✓")]
            .iter()
            .enumerate()
        {
            let cur = b.params.s(key, "");
            let mx = blocks.len() > 1 && blocks.iter().any(|o| o.params.s(key, "") != cur);
            let mut chosen = None;
            row(ui, lab(title, blocks, &[key]), |ui| {
                chosen = button_choice(ui, &format!("rb_{key}"), &cur, &defaults[i], mx, &names);
            });
            if let Some(v) = chosen {
                set_params(doc, &id_list, key, json!(v));
            }
        }
        let eff = b.rating_cursor_buttons(&hw);
        if eff[0] == eff[1] || eff[0] == eff[2] || eff[1] == eff[2] {
            row(ui, RichText::new(""), |ui| {
                ui.add(egui::Label::new(RichText::new("⚠ the same button is used twice").color(DANGER).size(12.0)).wrap());
            });
        }
        row(ui, RichText::new(""), |ui| {
            theme::hint(ui, "The cursor starts in the middle; the participant moves it and confirms. Defaults: Hardware → ResponseGrips.");
        });
        return;
    }
    // direct: one button per value
    let (lo, hi) = (b.params.i("min", 1), b.params.i("max", 4));
    if blocks.iter().any(|o| o.params.i("min", 1) != lo || o.params.i("max", 4) != hi) {
        row(ui, RichText::new(""), |ui| {
            theme::hint(ui, "The selected ratings have different scales – select ratings with the same values to set the button for each value.");
        });
        return;
    }
    let n = (hi - lo + 1).max(0) as usize;
    let custom = |o: &Block, i: usize| -> String {
        match o.params.get("value_buttons") {
            Some(serde_json::Value::Array(a)) => a.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            _ => String::new(),
        }
    };
    for i in 0..n {
        let cur = custom(b, i);
        let mx = blocks.len() > 1 && blocks.iter().any(|o| custom(o, i) != cur);
        let default_name = names.get(i).cloned().unwrap_or_else(|| "none".into());
        let mut chosen = None;
        let title = format!("Value {}", lo + i as i64);
        let label = if mx { RichText::new(format!("{title} ≠")).color(ACCENT) } else { RichText::new(title) };
        row(ui, label, |ui| {
            chosen = button_choice(ui, &format!("rb_val_{i}"), &cur, &default_name, mx, &names);
        });
        if let Some(v) = chosen {
            let mk = format!("value_buttons@{}", id_list.join(","));
            doc.edit_merge(Some(&mk), |par| {
                for id in &id_list {
                    if let Some(x) = par.block_mut(id) {
                        let mut arr: Vec<serde_json::Value> = match x.params.get("value_buttons") {
                            Some(serde_json::Value::Array(a)) => a.clone(),
                            _ => vec![],
                        };
                        if arr.len() < n {
                            arr.resize(n, json!(""));
                        }
                        arr[i] = json!(v.clone());
                        x.params.insert("value_buttons".into(), serde_json::Value::Array(arr));
                    }
                }
            });
        }
    }
    let eff = b.rating_value_buttons(&hw);
    let mut seen: Vec<&String> = vec![];
    let dup = eff.iter().filter(|x| !x.is_empty()).any(|x| {
        let d = seen.contains(&x);
        seen.push(x);
        d
    });
    if dup {
        row(ui, RichText::new(""), |ui| {
            ui.add(egui::Label::new(RichText::new("⚠ one button is used for several values – only the first one counts").color(DANGER).size(12.0)).wrap());
        });
    }
    if eff.iter().any(|x| x.is_empty()) {
        row(ui, RichText::new(""), |ui| {
            ui.add(egui::Label::new(RichText::new("⚠ more values than grip buttons – choose a button for each value or use cursor mode").color(DANGER).size(12.0)).wrap());
        });
    }
}
