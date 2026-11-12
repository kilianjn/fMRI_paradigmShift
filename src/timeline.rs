//! The film-editor style timeline: ruler, track, draggable/resizable blocks.

use std::collections::HashSet;

use egui::epaint::{CornerRadius, PathShape};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, CursorIcon, FontId, Id, LayerId, Order, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::actions::{Action, DragPayload};
use crate::document::Document;
use crate::model::{block_type, fmt_g, fmt_sig3, fmt_time, round4, Block, Paradigm, BLOCK_TYPES};
use crate::presets::{self, PresetCtx};
use crate::stimulus;
use crate::theme::{self, hex, ACCENT, BG, BORDER, DANGER, OK, PANEL, PANEL_2, TEXT, TEXT_DIM};

pub const RULER_H: f32 = 52.0;
pub const HEADER_W: f32 = 156.0;
pub const TRACK_H: f32 = 76.0; // minimum track height – the track grows to fill the panel
const TRACK_PAD: f32 = 8.0;
const EDGE: f32 = 7.0; // px grab zone for resizing
const MIN_DUR: f64 = 0.05;
const SCROLL_H: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Scrub,
    Band,
    Move,
    Left,
    Right,
    Scroll,
}

struct Drag {
    mode: Mode,
    block: String,
    origin: Pos2,
    moved: bool,
    orig: Vec<(String, f64, f64, usize)>,
    base_sel: Vec<String>,
    scroll0: f64,
    /// scrubbing by the playhead's head: pointer x minus playhead x when it was grabbed
    grab_dx: f32,
    /// plain click on a block of a larger selection: if it turns out to be a click (no drag),
    /// only that block stays selected
    collapse: bool,
    /// moved blocks that don't fit where they are: insert them at this block boundary (time, track)
    /// and make room
    insert: Option<(f64, usize)>,
}

pub struct Timeline {
    pub pps: f64, // pixels per second (zoom)
    pub scroll: f64,
    pub playhead: f64,
    pub snap: bool,
    pub snap_px: f64,
    pub show_blur: bool,
    drag: Option<Drag>,
    band: Option<Rect>,
    snap_line: Option<f64>,
    ghost: Vec<Block>,
    ghost_key: Option<(String, i64, usize)>,
    view: Rect,
    pending_fit: bool,
    ctx_target: Option<(Option<String>, f64, usize)>,
    pub stim_dir: Option<std::path::PathBuf>,
    /// the pointer is over the playhead's head (or is dragging it)
    head_hot: bool,
    /// the block clicked last – Shift+click selects everything from here to the clicked block
    anchor: Option<String>,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            pps: 18.0,
            scroll: 0.0,
            playhead: 0.0,
            snap: true,
            snap_px: 10.0,
            show_blur: false,
            drag: None,
            band: None,
            snap_line: None,
            ghost: vec![],
            ghost_key: None,
            view: Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 200.0)),
            pending_fit: false,
            ctx_target: None,
            stim_dir: None,
            head_hot: false,
            anchor: None,
        }
    }
}

/// Text cut to one line with an ellipsis.
pub fn elided(p: &Painter, text: &str, font: FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    p.layout_job(job)
}

/// A tooltip-like box painted on top of everything (used while dragging).
pub fn paint_tip(ctx: &egui::Context, pos: Pos2, text: &str) {
    let p = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("tl_tip")));
    let g = p.layout(text.to_string(), theme::font(12.5), TEXT, 360.0);
    let size = g.size() + Vec2::new(14.0, 10.0);
    let mut r = Rect::from_min_size(pos + Vec2::new(16.0, 16.0), size);
    // keep it inside the window: flip to the left / above the pointer near the edges
    let screen = ctx.content_rect();
    if r.right() > screen.right() - 4.0 {
        r = r.translate(Vec2::new(-(size.x + 32.0), 0.0));
    }
    if r.bottom() > screen.bottom() - 4.0 {
        r = r.translate(Vec2::new(0.0, -(size.y + 32.0)));
    }
    p.rect(r, CornerRadius::same(5), PANEL_2, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    p.galley(r.min + Vec2::new(7.0, 5.0), g, TEXT);
}

impl Timeline {
    pub fn preset_ctx(&self) -> PresetCtx {
        PresetCtx { stim_dir: self.stim_dir.clone(), ..Default::default() }
    }

    // ------------------------------------------------------------------ geometry
    fn content(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.view.left() + HEADER_W, self.view.top() + RULER_H),
            Pos2::new(self.view.right(), self.view.bottom() - SCROLL_H),
        )
    }

    fn track_h(&self, n: usize) -> f32 {
        TRACK_H.max((self.view.height() - RULER_H - SCROLL_H) / n.max(1) as f32)
    }

    pub fn x_of(&self, t: f64) -> f32 {
        self.view.left() + HEADER_W + (t * self.pps - self.scroll) as f32
    }

    pub fn t_of(&self, x: f32) -> f64 {
        ((x - self.view.left() - HEADER_W) as f64 + self.scroll) / self.pps
    }

    fn track_top(&self, i: usize, n: usize) -> f32 {
        self.view.top() + RULER_H + i as f32 * self.track_h(n)
    }

    fn track_at(&self, y: f32, n: usize) -> Option<usize> {
        if y < self.view.top() + RULER_H || y > self.view.bottom() - SCROLL_H {
            return None;
        }
        let i = ((y - self.view.top() - RULER_H) / self.track_h(n)).floor() as usize;
        (i < n).then_some(i)
    }

    fn block_rect(&self, b: &Block, n: usize) -> Rect {
        let top = self.track_top(b.track, n) + TRACK_PAD / 2.0;
        Rect::from_min_size(
            Pos2::new(self.x_of(b.start), top),
            Vec2::new(((b.duration * self.pps) as f32).max(2.0), self.track_h(n) - TRACK_PAD),
        )
    }

    fn block_at(&self, p: &Paradigm, pos: Pos2) -> Option<(String, Mode)> {
        let n = p.tracks.len();
        let mut bl: Vec<&Block> = p.blocks.iter().collect();
        bl.sort_by(|a, b| b.start.total_cmp(&a.start));
        for b in bl {
            let r = self.block_rect(b, n);
            if r.expand2(Vec2::new(2.0, 0.0)).contains(pos) {
                let mut mode = Mode::Move;
                if r.width() > 3.0 * EDGE {
                    if pos.x <= r.left() + EDGE {
                        mode = Mode::Left;
                    } else if pos.x >= r.right() - EDGE {
                        mode = Mode::Right;
                    }
                }
                return Some((b.id.clone(), mode));
            }
        }
        None
    }

    fn content_width(&self, p: &Paradigm) -> f64 {
        (p.duration().max(self.playhead) + 120.0) * self.pps
    }

    fn clamp_scroll(&mut self, p: &Paradigm) {
        let visible = (self.view.width() - HEADER_W) as f64;
        let max = (self.content_width(p) - visible).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    // ------------------------------------------------------------------ zoom
    pub fn set_zoom(&mut self, pps: f64, anchor_x: Option<f32>) {
        let pps = pps.clamp(1.5, 400.0);
        let ax = anchor_x.unwrap_or_else(|| {
            let x = self.x_of(self.playhead);
            if x >= self.view.left() + HEADER_W && x <= self.view.right() {
                x
            } else {
                self.view.left() + HEADER_W + (self.view.width() - HEADER_W) / 2.0
            }
        });
        let t = self.t_of(ax);
        self.pps = pps;
        self.scroll = (t * pps - (ax - self.view.left() - HEADER_W) as f64).max(0.0);
    }

    pub fn zoom_in(&mut self) {
        self.set_zoom(self.pps * 1.4, None);
    }

    pub fn zoom_out(&mut self) {
        self.set_zoom(self.pps / 1.4, None);
    }

    /// Fit the whole run (done on the next frame, when the size is known).
    pub fn zoom_fit(&mut self) {
        self.pending_fit = true;
    }

    fn do_fit(&mut self, p: &Paradigm) {
        let dur = p.duration().max(10.0);
        self.pps = (((self.view.width() - HEADER_W - 110.0) as f64) / dur).clamp(1.5, 400.0);
        self.scroll = 0.0;
    }

    pub fn set_playhead(&mut self, t: f64, follow: bool) {
        self.playhead = t.max(0.0);
        if follow {
            let x = self.x_of(self.playhead);
            let w = self.view.width();
            if x > self.view.left() + w - 60.0 || x < self.view.left() + HEADER_W {
                self.scroll = (self.playhead * self.pps - ((w - HEADER_W) * 0.2) as f64).max(0.0);
            }
        }
    }

    // ------------------------------------------------------------------ snapping
    /// Fine time grid depending on zoom (at least ~6 px per step).
    pub fn grid_step(&self) -> f64 {
        for s in [0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0] {
            if s * self.pps >= 6.0 {
                return s;
            }
        }
        10.0
    }

    fn snap_time(&mut self, p: &Paradigm, t: f64, exclude: &HashSet<String>, free: bool) -> f64 {
        self.snap_line = None;
        if !self.snap || free {
            return (t / 0.01).round() * 0.01;
        }
        let mut cands = vec![0.0, self.playhead];
        if p.tr > 0.0 {
            cands.push((t / p.tr).round() * p.tr);
        }
        for b in &p.blocks {
            if !exclude.contains(&b.id) {
                cands.push(b.start);
                cands.push(b.end());
            }
        }
        let best = cands.into_iter().min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs())).unwrap_or(t);
        if (best - t).abs() * self.pps <= self.snap_px {
            self.snap_line = Some(best);
            return best;
        }
        let g = self.grid_step();
        (t / g).round() * g
    }

    /// Playhead time for pointer x while scrubbing. It follows the mouse smoothly and only snaps
    /// to block edges (not to the TR grid or to its own position); Alt = no snapping at all.
    fn scrub_time(&mut self, p: &Paradigm, x: f32, free: bool) -> f64 {
        self.snap_line = None;
        let t = self.t_of(x.max(self.view.left() + HEADER_W)).max(0.0);
        if self.snap && !free {
            let best = p.blocks.iter().flat_map(|b| [b.start, b.end()]).chain([0.0]).min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()));
            if let Some(best) = best {
                if (best - t).abs() * self.pps <= 6.0 {
                    return best;
                }
            }
        }
        (t / 0.01).round() * 0.01
    }

    /// While blocks are moved: if they overlap other blocks where they are now, the block boundary
    /// on their track where they would be inserted instead (the one nearest to their start).
    /// None if they fit, or if they can't be inserted as a unit.
    fn insert_point(p: &Paradigm, orig: &[(String, f64, f64, usize)]) -> Option<(f64, usize)> {
        let ids: HashSet<&str> = orig.iter().map(|o| o.0.as_str()).collect();
        let moved: Vec<&Block> = p.blocks.iter().filter(|b| ids.contains(b.id.as_str())).collect();
        let track = moved.first()?.track;
        if moved.iter().any(|b| b.track != track) || orig.iter().any(|o| o.3 != orig[0].3) {
            return None;
        }
        if !moved.iter().any(|b| p.has_overlap(b)) {
            return None;
        }
        // a unit only: no other block between the moved ones (at their original place)
        let (g0, g1) = orig.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), o| (a.min(o.1), z.max(o.1 + o.2)));
        if p.blocks.iter().any(|b| b.track == orig[0].3 && !ids.contains(b.id.as_str()) && b.start < g1 - 1e-6 && b.end() > g0 + 1e-6) {
            return None;
        }
        let start = moved.iter().map(|b| b.start).fold(f64::INFINITY, f64::min);
        p.blocks
            .iter()
            .filter(|b| b.track == track && !ids.contains(b.id.as_str()))
            .flat_map(|b| [b.start, b.end()])
            .min_by(|a, b| (a - start).abs().total_cmp(&(b - start).abs()))
            .map(|t| (t, track))
    }

    /// Where an inserted unit will start and what happens to the other blocks (for tips/messages).
    fn insert_summary(orig: &[(String, f64, f64, usize)], t: f64, track: usize) -> (f64, String) {
        let (g0, g1) = orig.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), o| (a.min(o.1), z.max(o.1 + o.2)));
        let len = fmt_g(round4(g1 - g0));
        let same = orig.first().map(|o| o.3 == track).unwrap_or(false);
        if same && t >= g1 - 1e-6 {
            (t - (g1 - g0), format!("the blocks in between move {len}s to the left"))
        } else if same {
            (t, format!("the blocks in between move {len}s to the right"))
        } else {
            (t, format!("the following blocks move {len}s to the right"))
        }
    }

    /// Grab zone of the playhead's head (the whole ruler height around it), if it is in view.
    fn head_rect(&self) -> Option<Rect> {
        let x = self.x_of(self.playhead);
        let v = self.view;
        if x < v.left() + HEADER_W - 1.0 || x > v.right() {
            return None;
        }
        Some(Rect::from_min_max(Pos2::new(x - 11.0, v.top()), Pos2::new(x + 11.0, v.top() + RULER_H)))
    }

    fn scrub_tip(&self, ctx: &egui::Context, p: &Paradigm, pos: Pos2) {
        let mut txt = fmt_time(self.playhead, 2);
        if p.tr > 0.0 {
            txt.push_str(&format!("  ·  volume {}", (self.playhead / p.tr + 1e-9).floor() as i64 + 1));
        }
        if let Some(b) = p.stimulus_at(self.playhead) {
            txt.push_str(&format!("\n{}", b.display_label()));
        }
        paint_tip(ctx, pos, &txt);
    }

    // ------------------------------------------------------------------ ui
    pub fn ui(&mut self, ui: &mut Ui, doc: &mut Document, actions: &mut Vec<Action>) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        self.view = rect;
        if self.pending_fit {
            self.pending_fit = false;
            self.do_fit(&doc.paradigm);
        }
        self.handle_input(ui, &resp, doc, actions);
        self.handle_payload_drop(ui, doc, actions);
        self.clamp_scroll(&doc.paradigm);
        self.paint(ui, doc);
        self.context_menu(&resp, doc, actions);
    }

    fn handle_input(&mut self, ui: &mut Ui, resp: &egui::Response, doc: &mut Document, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let (pressed, down, mods, latest, zoom, scroll) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.modifiers,
                i.pointer.latest_pos(),
                i.zoom_delta(),
                i.smooth_scroll_delta,
            )
        });
        let n = doc.paradigm.tracks.len();
        let hover = resp.hover_pos();

        // wheel: Ctrl+wheel zooms, plain wheel scrolls
        if resp.hovered() && self.drag.is_none() {
            if zoom != 1.0 {
                self.set_zoom(self.pps * zoom as f64, hover.map(|p| p.x));
            } else if scroll != Vec2::ZERO {
                let d = if scroll.x.abs() > 0.0 { scroll.x } else { scroll.y };
                self.scroll -= d as f64;
            }
        }

        // press
        if pressed && resp.hovered() {
            if let Some(pos) = hover {
                self.on_press(pos, mods, doc, actions);
            }
        }
        // drag
        if self.drag.is_some() && down {
            if let Some(pos) = latest {
                self.on_move(pos, mods, doc, &ctx, actions);
            }
        }
        // release
        if self.drag.is_some() && !down {
            self.on_release(doc, actions);
        }

        // double click
        if resp.double_clicked() {
            if let Some(pos) = hover {
                if pos.y > self.view.top() + RULER_H && pos.x > self.view.left() + HEADER_W {
                    match self.block_at(&doc.paradigm, pos) {
                        Some((id, _)) => doc.select(vec![id]),
                        None => {
                            if let Some(track) = self.track_at(pos.y, n) {
                                let t = self.snap_time(&doc.paradigm, self.t_of(pos.x).max(0.0), &HashSet::new(), false);
                                self.snap_line = None;
                                actions.push(Action::AddMenu { t, track, pos });
                            }
                        }
                    }
                }
            }
        }

        // hover feedback
        let scrubbing = self.drag.as_ref().map(|d| d.mode == Mode::Scrub).unwrap_or(false);
        self.head_hot = scrubbing || (self.drag.is_none() && hover.zip(self.head_rect()).map(|(p, r)| r.contains(p)).unwrap_or(false));
        if scrubbing {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else if self.drag.is_none() {
            if let Some(pos) = hover {
                if self.head_hot {
                    ctx.set_cursor_icon(CursorIcon::Grab);
                    if ui.input(|i| i.pointer.time_since_last_movement() > 0.5) {
                        paint_tip(&ctx, pos, "Drag the playhead to scrub through the run");
                    }
                } else if pos.y < self.view.top() + RULER_H && pos.x > self.view.left() + HEADER_W {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                } else if let Some((id, mode)) = self.block_at(&doc.paradigm, pos) {
                    ctx.set_cursor_icon(if mode == Mode::Move { CursorIcon::Grab } else { CursorIcon::ResizeHorizontal });
                    if let Some(b) = doc.paradigm.block(&id) {
                        if ui.input(|i| i.pointer.time_since_last_movement() > 0.5) && !egui::DragAndDrop::has_any_payload(&ctx) {
                            let mut tip = format!("{}\n{} · {:.2}–{:.2}s", b.display_label(), b.title(), b.start, b.end());
                            if !b.condition.is_empty() {
                                tip.push_str(&format!("\ncondition: {}", b.condition));
                            }
                            tip.push_str("\nclick to show it · Shift+click: select all from the last clicked block\ndrag to move · drag edges to trim · double-click to edit");
                            paint_tip(&ctx, pos, &tip);
                        }
                    }
                }
            }
        } else if let Some(d) = &self.drag {
            if matches!(d.mode, Mode::Move) && d.moved {
                ctx.set_cursor_icon(CursorIcon::Grabbing);
            } else if matches!(d.mode, Mode::Left | Mode::Right) {
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
        }
    }

    fn on_press(&mut self, pos: Pos2, mods: egui::Modifiers, doc: &mut Document, actions: &mut Vec<Action>) {
        let v = self.view;
        let mk = |mode: Mode| Drag {
            mode,
            block: String::new(),
            origin: pos,
            moved: false,
            orig: vec![],
            base_sel: vec![],
            scroll0: 0.0,
            grab_dx: 0.0,
            collapse: false,
            insert: None,
        };
        // the playhead's head: grab it where it is (no jump) and scrub
        if self.head_rect().map(|r| r.contains(pos)).unwrap_or(false) {
            let mut d = mk(Mode::Scrub);
            d.grab_dx = pos.x - self.x_of(self.playhead);
            self.drag = Some(d);
            return;
        }
        if pos.y > v.bottom() - SCROLL_H && pos.x > v.left() + HEADER_W {
            // scrollbar: jump the thumb under the pointer, then drag
            let mut d = mk(Mode::Scroll);
            let visible = (v.width() - HEADER_W) as f64;
            let total = self.content_width(&doc.paradigm).max(visible);
            let frac = ((pos.x - v.left() - HEADER_W) as f64 / visible).clamp(0.0, 1.0);
            let thumb = visible * visible / total;
            let thumb_x = self.scroll / total * visible;
            if (pos.x - v.left() - HEADER_W) as f64 > thumb_x + thumb || ((pos.x - v.left() - HEADER_W) as f64) < thumb_x {
                self.scroll = frac * total - visible / 2.0;
            }
            d.scroll0 = self.scroll;
            self.drag = Some(d);
            return;
        }
        if pos.y < v.top() + RULER_H && pos.x > v.left() + HEADER_W {
            // click in the ruler: the playhead jumps there, then follows the mouse
            self.drag = Some(mk(Mode::Scrub));
            let t = self.scrub_time(&doc.paradigm, pos.x, mods.alt);
            self.set_playhead(t, false);
            actions.push(Action::Seek(self.playhead));
            return;
        }
        if pos.x < v.left() + HEADER_W {
            return;
        }
        let multi = mods.shift || mods.command || mods.ctrl;
        match self.block_at(&doc.paradigm, pos) {
            None => {
                if !multi {
                    doc.select(vec![]);
                }
                let mut d = mk(Mode::Band);
                d.base_sel = doc.selection.clone();
                self.drag = Some(d);
            }
            Some((id, mut mode)) => {
                let mut collapse = false;
                let toggle = mods.command || mods.ctrl;
                let range = mods.shift && !toggle;
                let anchor = self.anchor.as_ref().and_then(|a| doc.paradigm.block(a)).map(|a| (a.start, a.track));
                if let (true, Some((a_start, a_track))) = (range, anchor) {
                    // Shift+click: everything from the anchor block to this one (on the tracks
                    // in between, too); the anchor stays, so another Shift+click changes the range
                    let Some(b) = doc.paradigm.block(&id) else { return };
                    let (t0, t1) = (a_start.min(b.start), a_start.max(b.start));
                    let (k0, k1) = (a_track.min(b.track), a_track.max(b.track));
                    let mut hits: Vec<&Block> =
                        doc.paradigm.blocks.iter().filter(|o| o.start >= t0 - 1e-6 && o.start <= t1 + 1e-6 && (k0..=k1).contains(&o.track)).collect();
                    hits.sort_by(|x, y| x.start.total_cmp(&y.start).then(x.track.cmp(&y.track)));
                    let ids: Vec<String> = hits.into_iter().map(|o| o.id.clone()).collect();
                    let n = ids.len();
                    doc.select(ids);
                    actions.push(Action::Status(format!("{n} blocks selected – Del deletes them, Shift+Del also closes the gap")));
                    mode = Mode::Move;
                } else if toggle {
                    let mut sel = doc.selection.clone();
                    if let Some(i) = sel.iter().position(|s| *s == id) {
                        sel.remove(i);
                    } else {
                        sel.push(id.clone());
                    }
                    doc.select(sel);
                    self.anchor = Some(id.clone());
                } else {
                    // keep a larger selection for now – the click may start dragging all of it
                    collapse = doc.is_selected(&id) && doc.selection.len() > 1;
                    if !doc.is_selected(&id) {
                        doc.select(vec![id.clone()]);
                    }
                    self.anchor = Some(id.clone());
                }
                if !doc.is_selected(&id) {
                    return;
                }
                if mode != Mode::Move {
                    doc.select(vec![id.clone()]);
                }
                // show the clicked block in the participant view: the playhead jumps to its start
                // (unless it already stands inside the block)
                if let Some(b) = doc.paradigm.block(&id) {
                    if !(b.start <= self.playhead && self.playhead < b.end()) {
                        self.set_playhead(b.start, false);
                        actions.push(Action::Seek(self.playhead));
                    }
                }
                let orig = doc
                    .selection
                    .iter()
                    .filter_map(|i| doc.paradigm.block(i).map(|b| (i.clone(), b.start, b.duration, b.track)))
                    .collect();
                let mut d = mk(mode);
                d.block = id;
                d.orig = orig;
                d.collapse = collapse;
                self.drag = Some(d);
            }
        }
    }

    fn on_move(&mut self, pos: Pos2, mods: egui::Modifiers, doc: &mut Document, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(mode) = self.drag.as_ref().map(|d| d.mode) else { return };
        let n = doc.paradigm.tracks.len();
        match mode {
            Mode::Scroll => {
                let d = self.drag.as_ref().unwrap();
                let visible = (self.view.width() - HEADER_W) as f64;
                let total = self.content_width(&doc.paradigm).max(visible);
                self.scroll = d.scroll0 + (pos.x - d.origin.x) as f64 * total / visible;
            }
            Mode::Scrub => {
                // auto-scroll when the pointer reaches the edges of the timeline
                if pos.x > self.view.right() - 20.0 && self.t_of(self.view.right()) < doc.paradigm.duration() + 10.0 {
                    self.scroll += 8.0;
                    ctx.request_repaint();
                } else if pos.x < self.view.left() + HEADER_W + 10.0 && self.scroll > 0.0 {
                    self.scroll = (self.scroll - 8.0).max(0.0);
                    ctx.request_repaint();
                }
                let dx = self.drag.as_ref().map(|d| d.grab_dx).unwrap_or(0.0);
                let t = self.scrub_time(&doc.paradigm, pos.x - dx, mods.alt);
                if (t - self.playhead).abs() > 1e-9 {
                    self.set_playhead(t, false);
                    actions.push(Action::Seek(self.playhead));
                }
                self.scrub_tip(ctx, &doc.paradigm, pos);
            }
            Mode::Band => {
                let d = self.drag.as_ref().unwrap();
                let band = Rect::from_two_pos(d.origin, pos);
                let mut ids = d.base_sel.clone();
                for b in &doc.paradigm.blocks {
                    if self.block_rect(b, n).intersects(band) && !ids.contains(&b.id) {
                        ids.push(b.id.clone());
                    }
                }
                self.band = Some(band);
                doc.select(ids);
            }
            Mode::Move | Mode::Left | Mode::Right => {
                let (origin, moved) = {
                    let d = self.drag.as_ref().unwrap();
                    (d.origin, d.moved)
                };
                if !moved && (pos.x - origin.x).abs() < 3.0 && (pos.y - origin.y).abs() < 3.0 {
                    return;
                }
                if !moved {
                    self.drag.as_mut().unwrap().moved = true;
                    doc.begin_edit();
                }
                let d = self.drag.as_ref().unwrap();
                let orig = d.orig.clone();
                let prim_id = d.block.clone();
                let Some(&(_, o_start, o_dur, o_track)) = orig.iter().find(|o| o.0 == prim_id) else { return };
                let dt = (pos.x - origin.x) as f64 / self.pps;
                let ids: HashSet<String> = orig.iter().map(|o| o.0.clone()).collect();
                let free = mods.alt;
                match mode {
                    Mode::Move => {
                        let new_start = (o_start + dt).max(0.0);
                        let s1 = self.snap_time(&doc.paradigm, new_start, &ids, free);
                        let m1 = self.snap_line;
                        let s2 = self.snap_time(&doc.paradigm, new_start + o_dur, &ids, free) - o_dur;
                        let m2 = self.snap_line;
                        let snapped = if m2.is_some() && (m1.is_none() || (s2 - new_start).abs() < (s1 - new_start).abs()) {
                            s2
                        } else {
                            self.snap_line = m1;
                            s1
                        };
                        let min_start = orig.iter().map(|o| o.1).fold(f64::INFINITY, f64::min);
                        let delta = (snapped - o_start).max(-min_start);
                        let dtrack: i64 = match self.track_at(pos.y, n) {
                            Some(t) => t as i64 - o_track as i64,
                            None => 0,
                        };
                        let min_tr = orig.iter().map(|o| o.3).min().unwrap_or(0) as i64;
                        let max_tr = orig.iter().map(|o| o.3).max().unwrap_or(0) as i64;
                        let dtrack = dtrack.clamp(-min_tr, n as i64 - 1 - max_tr);
                        for (i, s, _, tr) in &orig {
                            if let Some(b) = doc.paradigm.block_mut(i) {
                                b.start = round4(s + delta);
                                b.track = (*tr as i64 + dtrack) as usize;
                            }
                        }
                    }
                    Mode::Left => {
                        let end = o_start + o_dur;
                        let ns = self.snap_time(&doc.paradigm, o_start + dt, &ids, free).max(0.0).min(end - MIN_DUR);
                        if let Some(b) = doc.paradigm.block_mut(&prim_id) {
                            b.start = round4(ns);
                            b.duration = round4(end - ns);
                        }
                    }
                    _ => {
                        let ne = self.snap_time(&doc.paradigm, o_start + o_dur + dt, &ids, free).max(o_start + MIN_DUR);
                        if let Some(b) = doc.paradigm.block_mut(&prim_id) {
                            b.duration = round4(ne - o_start);
                        }
                    }
                }
                doc.version += 1;
                // don't fit here? then they are inserted at the nearest block boundary
                let insert = if mode == Mode::Move { Self::insert_point(&doc.paradigm, &orig) } else { None };
                if insert.is_some() {
                    self.snap_line = None;
                }
                if let Some(d) = self.drag.as_mut() {
                    d.insert = insert;
                }
                if let Some(b) = doc.paradigm.block(&prim_id) {
                    let tr = doc.paradigm.tr;
                    let mut txt = format!("{}\nstart {:.2}s · {:.2}s", b.display_label(), b.start, b.duration);
                    if tr > 0.0 {
                        txt.push_str(&format!("\nvolume {} · {:.2} TR", (b.start / tr).floor() as i64 + 1, b.duration / tr));
                    }
                    if let Some((t, track)) = insert {
                        let (at, effect) = Self::insert_summary(&orig, t, track);
                        let n = orig.len();
                        txt.push_str(&format!(
                            "\n↳ release to insert {} here (from {}) –\n   {effect}",
                            if n == 1 { "it".to_string() } else { format!("these {n} blocks") },
                            fmt_time(at, 1),
                        ));
                    } else if doc.paradigm.has_overlap(b) {
                        txt.push_str("\n⚠ overlaps – will snap back");
                    }
                    paint_tip(ctx, pos, &txt);
                }
                // auto-scroll at the edges
                if pos.x > self.view.right() - 20.0 {
                    self.scroll += 8.0;
                } else if pos.x < self.view.left() + HEADER_W + 10.0 {
                    self.scroll -= 8.0;
                }
            }
        }
    }

    fn on_release(&mut self, doc: &mut Document, actions: &mut Vec<Action>) {
        let d = self.drag.take().unwrap();
        self.band = None;
        self.snap_line = None;
        // the playhead jumped to the block when it was grabbed – let it follow the block to its new place
        let follow = |tl: &mut Self, doc: &Document, actions: &mut Vec<Action>| {
            let o_start = d.orig.iter().find(|o| o.0 == d.block).map(|o| o.1);
            if let (Some(o), Some(b)) = (o_start, doc.paradigm.block(&d.block)) {
                if (tl.playhead - o).abs() < 1e-9 && (b.start - o).abs() > 1e-9 {
                    tl.set_playhead(b.start, false);
                    actions.push(Action::Seek(tl.playhead));
                }
            }
        };
        if d.collapse && !d.moved {
            doc.select(vec![d.block.clone()]);
        }
        if let (Mode::Move, true, Some((t, track))) = (d.mode, d.moved, d.insert) {
            let (at, effect) = Self::insert_summary(&d.orig, t, track);
            match doc.paradigm.move_insert(&d.orig, t, track) {
                Ok(len) if len > 0.0 => {
                    doc.commit_edit();
                    follow(self, doc, actions);
                    let n = d.orig.len();
                    actions.push(Action::Status(format!(
                        "Inserted {} at {} – {} (Ctrl+Z undoes it).",
                        if n == 1 { "1 block".to_string() } else { format!("{n} blocks") },
                        fmt_time(at, 1),
                        effect.replace(" move ", " moved ")
                    )));
                }
                Ok(_) => doc.cancel_edit(),
                Err(e) => {
                    doc.cancel_edit();
                    actions.push(Action::Status(e));
                }
            }
            return;
        }
        if matches!(d.mode, Mode::Move | Mode::Left | Mode::Right) && d.moved {
            let overl = d.orig.iter().any(|(i, ..)| doc.paradigm.block(i).map(|b| doc.paradigm.has_overlap(b)).unwrap_or(false));
            if overl {
                doc.cancel_edit();
                actions.push(Action::Status(
                    "Blocks can't overlap – move undone. (Blocks are inserted between others only when they are next to \
                     each other on one track.)"
                        .into(),
                ));
            } else {
                doc.commit_edit();
                if d.mode == Mode::Move {
                    follow(self, doc, actions);
                }
            }
        }
    }

    // ------------------------------------------------------------------ drag & drop from the library
    fn ghost_blocks(&self, payload: &DragPayload, t: f64, track: usize) -> Vec<Block> {
        match payload {
            DragPayload::Kind(k) if k.starts_with("preset:") => match presets::preset(&k[7..]) {
                Some(pr) => pr.build(t, None, &self.preset_ctx()),
                None => vec![],
            },
            DragPayload::Kind(k) => {
                let mut b = Block::create(k, t, None);
                b.track = track;
                vec![b]
            }
            DragPayload::Images(files) => {
                let mut cur = t;
                files
                    .iter()
                    .map(|f| {
                        // pictures become image blocks; videos and sounds video / audio blocks as long as the file
                        let kind = if crate::media::is_video_file(f) {
                            "video"
                        } else if crate::media::is_audio_file(f) {
                            "audio"
                        } else {
                            "image"
                        };
                        let dur = if kind == "image" { None } else { crate::media::media_duration(f).map(|d| round4((d * 100.0).ceil() / 100.0)) };
                        let mut b = Block::create(kind, cur, dur.filter(|d| *d > 0.0)).with(serde_json::json!({ "path": f }));
                        b.track = track;
                        cur += b.duration;
                        b
                    })
                    .collect()
            }
        }
    }

    fn handle_payload_drop(&mut self, ui: &mut Ui, doc: &mut Document, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let payload = egui::DragAndDrop::payload::<DragPayload>(&ctx);
        let pos = ui.input(|i| i.pointer.latest_pos());
        let content = self.content();
        let over = pos.filter(|p| content.expand2(Vec2::new(0.0, 0.0)).contains(*p));
        let (Some(payload), Some(pos)) = (payload, over) else {
            self.ghost.clear();
            self.ghost_key = None;
            return;
        };
        let n = doc.paradigm.tracks.len();
        let track = self.track_at(pos.y, n).unwrap_or(0);
        let t = self.snap_time(&doc.paradigm, self.t_of(pos.x).max(0.0), &HashSet::new(), false).max(0.0);
        let k = (format!("{payload:?}"), (t * 1000.0) as i64, track);
        if self.ghost_key.as_ref() != Some(&k) {
            self.ghost = self.ghost_blocks(&payload, t, track);
            self.ghost_key = Some(k);
        }
        // auto-scroll near the edges
        if pos.x > content.right() - 30.0 {
            self.scroll += 15.0;
        } else if pos.x < content.left() + 20.0 {
            self.scroll -= 15.0;
        }
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(&ctx);
            let blocks = std::mem::take(&mut self.ghost);
            self.ghost_key = None;
            self.snap_line = None;
            match &*payload {
                DragPayload::Kind(k) if k.starts_with("preset:") => {
                    let start = blocks.iter().map(|b| b.start).fold(f64::INFINITY, f64::min);
                    actions.push(Action::ChooseVariant { key: k[7..].to_string(), start: Some(start), pos: Some(pos) });
                }
                _ => {
                    let ids = doc.edit(|par| blocks.into_iter().map(|b| par.ripple_insert(b)).collect::<Vec<_>>());
                    doc.select(ids);
                }
            }
        } else {
            ctx.request_repaint();
        }
    }

    /// Image files dropped from the desktop at `pos` (screen position).
    pub fn drop_images(&mut self, doc: &mut Document, files: Vec<String>, pos: Pos2) {
        let n = doc.paradigm.tracks.len();
        let track = self.track_at(pos.y, n).unwrap_or(0);
        let t = self.snap_time(&doc.paradigm, self.t_of(pos.x).max(0.0), &HashSet::new(), false).max(0.0);
        self.snap_line = None;
        let blocks = self.ghost_blocks(&DragPayload::Images(files), t, track);
        let ids = doc.edit(|par| blocks.into_iter().map(|b| par.ripple_insert(b)).collect::<Vec<_>>());
        doc.select(ids);
    }

    pub fn contains(&self, pos: Pos2) -> bool {
        self.view.contains(pos)
    }

    // ------------------------------------------------------------------ context menu
    fn context_menu(&mut self, resp: &egui::Response, doc: &mut Document, actions: &mut Vec<Action>) {
        if resp.secondary_clicked() {
            self.ctx_target = None;
            if let Some(pos) = resp.interact_pointer_pos() {
                if pos.x > self.view.left() + HEADER_W && pos.y > self.view.top() + RULER_H {
                    let hit = self.block_at(&doc.paradigm, pos).map(|x| x.0);
                    if let Some(id) = &hit {
                        if !doc.is_selected(id) {
                            doc.select(vec![id.clone()]);
                        }
                    }
                    let track = self.track_at(pos.y, doc.paradigm.tracks.len()).unwrap_or(0);
                    let t = self.snap_time(&doc.paradigm, self.t_of(pos.x).max(0.0), &HashSet::new(), false);
                    self.snap_line = None;
                    self.ctx_target = Some((hit, t, track));
                }
            }
        }
        let Some((hit, t, track)) = self.ctx_target.clone() else { return };
        resp.context_menu(|ui| {
            ui.set_min_width(230.0);
            match hit.as_ref().and_then(|id| doc.paradigm.block(id)).cloned() {
                Some(b) => {
                    if ui.button("Edit properties…").clicked() {
                        doc.select(vec![b.id.clone()]);
                        ui.close();
                    }
                    let same: Vec<String> = doc.paradigm.blocks.iter().filter(|o| o.kind == b.kind).map(|o| o.id.clone()).collect();
                    if same.len() > 1 && ui.button(format!("Select all {} {} blocks  (edit together)", same.len(), b.title())).clicked() {
                        actions.push(Action::Select(same.clone()));
                        ui.close();
                    }
                    if !b.condition.is_empty() {
                        let same_c: Vec<String> = doc
                            .paradigm
                            .blocks
                            .iter()
                            .filter(|o| o.kind == b.kind && o.condition == b.condition)
                            .map(|o| o.id.clone())
                            .collect();
                        if same_c.len() > 1
                            && same_c.len() < same.len()
                            && ui.button(format!("Select all {} {} blocks of “{}”", same_c.len(), b.title(), b.condition)).clicked()
                        {
                            actions.push(Action::Select(same_c));
                            ui.close();
                        }
                    }
                    ui.separator();
                    for (label, a) in [
                        ("Duplicate            Ctrl+D", Action::Duplicate),
                        ("Repeat ×N…           Ctrl+R", Action::RepeatAsk),
                        ("Split at playhead    S", Action::Split),
                    ] {
                        if ui.button(label).clicked() {
                            actions.push(a);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Delete               Del").clicked() {
                        actions.push(Action::Delete { ripple: false });
                        ui.close();
                    }
                    if ui.button("Delete and close gap   Shift+Del").clicked() {
                        actions.push(Action::Delete { ripple: true });
                        ui.close();
                    }
                }
                None => {
                    ui.menu_button("Add block here", |ui| add_menu_contents(ui, t, track, actions));
                    ui.separator();
                    if ui.button("Close all gaps on this track").clicked() {
                        actions.push(Action::CloseGaps(track));
                        ui.close();
                    }
                }
            }
        });
    }

    // ------------------------------------------------------------------ painting
    fn paint(&mut self, ui: &mut Ui, doc: &Document) {
        let par = &doc.paradigm;
        let painter = ui.painter_at(self.view);
        let v = self.view;
        let n = par.tracks.len();
        painter.rect_filled(v, CornerRadius::ZERO, BG);
        // tracks
        for i in 0..n {
            let top = self.track_top(i, n);
            let th = self.track_h(n);
            let r = Rect::from_min_size(Pos2::new(v.left() + HEADER_W, top), Vec2::new(v.width() - HEADER_W, th));
            painter.rect_filled(r, CornerRadius::ZERO, if i % 2 == 0 { hex("#1B1E24") } else { hex("#191B21") });
            painter.hline(r.x_range(), top + th - 0.5, Stroke::new(1.0, BORDER));
        }
        let content = self.content();
        let cp = painter.with_clip_rect(content);
        // TR grid
        let tr = par.tr;
        if tr > 0.0 && tr * self.pps >= 6.0 {
            let (t0, t1) = (self.t_of(content.left()), self.t_of(content.right()));
            let mut vv = (t0 / tr).floor().max(0.0) as i64;
            while vv as f64 * tr <= t1 {
                let x = self.x_of(vv as f64 * tr);
                cp.vline(x, content.y_range(), Stroke::new(1.0, Color32::from_white_alpha(12)));
                vv += 1;
            }
        }
        // end marker
        let dur = par.duration();
        if dur > 0.0 {
            let x = self.x_of(dur);
            cp.rect_filled(Rect::from_min_max(Pos2::new(x, content.top()), content.right_bottom()), CornerRadius::ZERO, Color32::from_black_alpha(50));
            cp.add(egui::Shape::dashed_line(&[Pos2::new(x, content.top()), Pos2::new(x, content.bottom())], Stroke::new(1.0, hex("#7A8194")), 4.0, 3.0));
        }
        // blocks (selected ones on top)
        let mut bl: Vec<&Block> = par.blocks.iter().collect();
        bl.sort_by_key(|b| doc.is_selected(&b.id));
        let dragging = self.drag.as_ref().map(|d| matches!(d.mode, Mode::Move | Mode::Left | Mode::Right) && d.moved).unwrap_or(false);
        let insert = self.drag.as_ref().filter(|d| d.moved).and_then(|d| d.insert);
        for b in bl {
            let sel = doc.is_selected(&b.id);
            let bad = dragging && sel && insert.is_none() && par.has_overlap(b);
            self.paint_block(&cp, par, b, sel, bad, false);
        }
        if let Some((t, track)) = insert {
            // insertion marker: a bar at the block boundary with arrows – here it goes, the rest makes room
            let x = self.x_of(t);
            let top = self.track_top(track, n);
            let th = self.track_h(n);
            let col = hex("#7FD3FF");
            cp.rect_filled(Rect::from_min_max(Pos2::new(x - 2.0, top + 1.0), Pos2::new(x + 2.0, top + th - 1.0)), CornerRadius::same(2), col);
            for (y, dir) in [(top + 1.0, 1.0), (top + th - 1.0, -1.0)] {
                cp.add(PathShape::convex_polygon(
                    vec![Pos2::new(x - 7.0, y), Pos2::new(x + 7.0, y), Pos2::new(x, y + 8.0 * dir)],
                    col,
                    Stroke::NONE,
                ));
            }
            let g = cp.layout_no_wrap("insert ▸".to_string(), theme::bold(10.5), Color32::from_rgb(20, 24, 30));
            let tag = Rect::from_min_size(Pos2::new(x + 4.0, top + th / 2.0 - 9.0), Vec2::new(g.size().x + 10.0, 18.0));
            cp.rect_filled(tag, CornerRadius::same(4), col);
            cp.galley(tag.center() - g.size() / 2.0, g, Color32::BLACK);
        }
        for b in &self.ghost {
            self.paint_block(&cp, par, b, false, false, true);
        }
        if let Some(band) = self.band {
            cp.rect(band, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(245, 165, 36, 30), Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        }
        if let Some(sl) = self.snap_line {
            if dragging || !self.ghost.is_empty() {
                let x = self.x_of(sl);
                cp.add(egui::Shape::dashed_line(&[Pos2::new(x, content.top()), Pos2::new(x, content.bottom())], Stroke::new(1.0, hex("#7FD3FF")), 4.0, 3.0));
            }
        }
        if par.blocks.is_empty() && self.ghost.is_empty() {
            let r = Rect::from_min_size(
                Pos2::new(content.left() + 30.0, self.track_top(0, n) + 8.0),
                Vec2::new((content.width() - 60.0).max(300.0), self.track_h(n) - 16.0),
            );
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            for w in pts.windows(2) {
                cp.add(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.5, hex("#3A404D")), 6.0, 4.0));
            }
            stimulus::draw_text(
                &cp,
                r,
                "Drag a block from the library onto this track  ·  drop image files here  ·  or start from  ▦ Templates  (Hariri, Go/No-Go, emotion regulation …)",
                theme::font(13.0),
                TEXT_DIM,
                true,
            );
        }
        self.paint_ruler(&painter, par);
        self.paint_headers(&painter, par);
        self.paint_playhead(&painter);
        self.paint_scrollbar(&painter, par);
    }

    fn paint_block(&self, p: &Painter, par: &Paradigm, b: &Block, sel: bool, bad: bool, ghost: bool) {
        let n = par.tracks.len();
        let r = self.block_rect(b, n);
        let content = self.content();
        if r.right() < content.left() || r.left() > content.right() {
            return;
        }
        let a = if ghost { 0.55 } else { 1.0 };
        let fade = |c: Color32| if ghost { c.gamma_multiply(a) } else { c };
        let base = hex(b.color());
        let rad = CornerRadius::same(6);
        // vertical gradient: lighter at the top, darker at the bottom
        let (top_c, bot_c) = (fade(theme::lighter(base, 112.0)), fade(theme::darker(base, 118.0)));
        p.rect_filled(r, rad, bot_c);
        if r.height() > 14.0 {
            p.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), 7.0)), CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, top_c);
            let (y0, y1) = (r.top() + 6.0, r.bottom() - 6.0);
            let n = ((y1 - y0) / 3.0).ceil().max(1.0) as usize;
            for i in 0..n {
                let a = y0 + (y1 - y0) * i as f32 / n as f32;
                let b = y0 + (y1 - y0) * (i + 1) as f32 / n as f32 + 0.5;
                let c = theme::lerp(top_c, bot_c, (i as f32 + 0.5) / n as f32);
                p.rect_filled(Rect::from_min_max(Pos2::new(r.left(), a), Pos2::new(r.right(), b.min(y1))), CornerRadius::ZERO, c);
            }
        }
        let clip = p.with_clip_rect(r.intersect(p.clip_rect()));
        // picture thumbnails
        let img = b.thumbnail_path();
        let mut has_thumb = false;
        if !img.is_empty() && r.width() > 40.0 {
            if let Some(t) = stimulus::texture(&img, self.show_blur) {
                let th = (r.height() - 22.0).max(16.0);
                let tw = th * t.size.x / t.size.y.max(1.0);
                let mut x = r.left() + 6.0;
                while x < r.right() - 4.0 {
                    let tr = Rect::from_min_size(Pos2::new(x, r.top() + 18.0), Vec2::new(tw, th));
                    clip.image(t.id, tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE.gamma_multiply(0.9 * a));
                    x += tw + 2.0;
                    if b.kind == "rating" {
                        break;
                    }
                }
                clip.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), 18.0)), CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, fade(theme::darker(base, 115.0)));
                has_thumb = true;
            }
        }
        if r.width() > 22.0 {
            theme::draw_kind_glyph(&clip, &b.kind, Rect::from_min_size(Pos2::new(r.left() + 4.0, r.top() + 1.0), Vec2::splat(16.0)), fade(Color32::WHITE));
        }
        if r.width() > 40.0 {
            let g = elided(&clip, &b.display_label(), theme::bold(11.5), fade(Color32::WHITE), r.width() - 26.0);
            clip.galley(Pos2::new(r.left() + 22.0, r.top() + 9.0 - g.size().y / 2.0), g, Color32::WHITE);
        }
        let mut info_right = r.left() + 4.0;
        if r.width() > 50.0 && !has_thumb {
            let mut info = format!("{}s", fmt_g(b.duration));
            if par.tr > 0.0 {
                info.push_str(&format!(" · {} TR", fmt_sig3(b.duration / par.tr)));
            }
            let g = elided(&clip, &info, theme::font(10.5), fade(Color32::from_white_alpha(190)), r.width() - 12.0);
            info_right = r.left() + 10.0 + g.size().x;
            clip.galley(Pos2::new(r.left() + 7.0, r.bottom() - 12.0 - g.size().y / 2.0), g, Color32::WHITE);
        }
        if !b.condition.is_empty() && r.width() > 60.0 && !b.display_label().to_lowercase().contains(&b.condition.to_lowercase()) {
            let g = clip.layout_no_wrap(b.condition.clone(), theme::bold(10.0), fade(Color32::WHITE));
            let tw = g.size().x + 12.0;
            let tag = Rect::from_min_size(Pos2::new(r.right() - tw - 5.0, r.bottom() - 19.0), Vec2::new(tw, 15.0));
            if tag.left() > info_right {
                clip.rect_filled(tag, CornerRadius::same(7), Color32::from_black_alpha(120));
                clip.galley(tag.center() - g.size() / 2.0, g, Color32::WHITE);
            }
        }
        let stroke = if bad {
            Stroke::new(2.5, DANGER)
        } else if sel {
            Stroke::new(2.5, ACCENT)
        } else {
            Stroke::new(1.0, fade(theme::darker(base, 150.0)))
        };
        p.rect_stroke(r.shrink(0.5), rad, stroke, StrokeKind::Inside);
        if sel && r.width() > 3.0 * EDGE {
            for x in [r.left() + 2.0, r.right() - 5.0] {
                p.rect_filled(Rect::from_min_size(Pos2::new(x, r.center().y - 10.0), Vec2::new(3.0, 20.0)), CornerRadius::same(1), ACCENT);
            }
        }
    }

    fn tick_step(&self) -> (f64, f64) {
        for major in [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0] {
            if major * self.pps >= 70.0 {
                let minor = if [1.0, 5.0, 10.0, 60.0, 300.0, 600.0].contains(&major) {
                    major / 5.0
                } else if major == 15.0 {
                    5.0
                } else {
                    major / 4.0
                };
                return (major, minor);
            }
        }
        (600.0, 120.0)
    }

    fn paint_ruler(&self, p: &Painter, par: &Paradigm) {
        let v = self.view;
        let ruler = Rect::from_min_size(v.min, Vec2::new(v.width(), RULER_H));
        p.rect_filled(ruler, CornerRadius::ZERO, PANEL);
        p.text(Pos2::new(v.left() + 12.0, v.top() + 16.0), Align2::LEFT_CENTER, "TIME", theme::bold(10.5), TEXT_DIM);
        p.text(Pos2::new(v.left() + 12.0, v.top() + 39.0), Align2::LEFT_CENTER, format!("VOLUMES · TR {}s", fmt_g(par.tr)), theme::bold(10.5), TEXT_DIM);
        let clip = p.with_clip_rect(Rect::from_min_max(Pos2::new(v.left() + HEADER_W, v.top()), Pos2::new(v.right(), v.top() + RULER_H)));
        let (t0, t1) = (self.t_of(v.left() + HEADER_W), self.t_of(v.right()));
        let (major, minor) = self.tick_step();
        let mut t = (t0 / minor).floor() * minor;
        while t <= t1 {
            let x = self.x_of(t);
            let is_major = (t / major - (t / major).round()).abs() < 1e-6;
            clip.line_segment(
                [Pos2::new(x, v.top() + 26.0 - if is_major { 10.0 } else { 5.0 }), Pos2::new(x, v.top() + 26.0)],
                Stroke::new(1.0, if is_major { hex("#5A6172") } else { hex("#3A404D") }),
            );
            if is_major && t >= -1e-9 {
                clip.text(Pos2::new(x + 4.0, v.top() + 10.0), Align2::LEFT_CENTER, fmt_time(t.max(0.0), 0), theme::font(10.5), TEXT_DIM);
            }
            t += minor;
        }
        // volumes row
        let tr = par.tr;
        let vols = par.volumes() as i64;
        if tr > 0.0 {
            let mut vv = (t0 / tr).floor().max(0.0) as i64;
            let mut every = 1i64;
            while tr * every as f64 * self.pps < 28.0 {
                every *= if every < 4 { 2 } else if every == 4 { 5 } else { 2 };
                if every > 100000 {
                    break;
                }
            }
            let first = vv;
            // cells first, then the labels on top (a label may be wider than its cell)
            while vv as f64 * tr <= t1 {
                let (x0, x1) = (self.x_of(vv as f64 * tr), self.x_of((vv + 1) as f64 * tr));
                let inside = vv < vols;
                let col = if inside {
                    if vv % 2 == 0 { hex("#2A3040") } else { hex("#262B37") }
                } else {
                    hex("#1F232B")
                };
                clip.rect_filled(Rect::from_min_max(Pos2::new(x0, v.top() + 28.0), Pos2::new(x1, v.top() + 48.0)), CornerRadius::ZERO, col);
                vv += 1;
            }
            let mut k = first - first % every;
            while k as f64 * tr <= t1 {
                let inside = k < vols;
                clip.text(
                    Pos2::new(self.x_of(k as f64 * tr) + 3.0, v.top() + 38.0),
                    Align2::LEFT_CENTER,
                    (k + 1).to_string(),
                    theme::font(10.5),
                    if inside { TEXT } else { hex("#4A5060") },
                );
                k += every;
            }
        }
        // end-of-run flag
        let dur = par.duration();
        if dur > 0.0 {
            let x = self.x_of(dur);
            let g = clip.layout_no_wrap(format!("END  {}", fmt_time(dur, 1)), theme::bold(10.0), Color32::WHITE);
            let flag = Rect::from_min_size(Pos2::new(x, v.top() + 3.0), Vec2::new(g.size().x + 12.0, 17.0));
            clip.rect_filled(flag, CornerRadius::same(3), hex("#5A6172"));
            clip.galley(flag.center() - g.size() / 2.0, g, Color32::WHITE);
        }
        p.hline(v.x_range(), v.top() + RULER_H - 0.5, Stroke::new(1.0, BORDER));
    }

    fn paint_headers(&self, p: &Painter, par: &Paradigm) {
        let v = self.view;
        let n = par.tracks.len();
        p.rect_filled(Rect::from_min_max(Pos2::new(v.left(), v.top() + RULER_H), Pos2::new(v.left() + HEADER_W, v.bottom())), CornerRadius::ZERO, PANEL);
        for (i, name) in par.tracks.iter().enumerate() {
            let top = self.track_top(i, n);
            let th = self.track_h(n);
            p.hline(v.left()..=v.left() + HEADER_W, top + th - 0.5, Stroke::new(1.0, BORDER));
            let badge = Rect::from_min_size(Pos2::new(v.left() + 12.0, top + th / 2.0 - 13.0), Vec2::splat(26.0));
            p.rect(badge, CornerRadius::same(6), PANEL_2, Stroke::new(1.0, BORDER), StrokeKind::Inside);
            p.text(badge.center(), Align2::CENTER_CENTER, format!("{}{}", if i == 0 { "V" } else { "R" }, i + 1), theme::bold(11.5), if i == 0 { ACCENT } else { OK });
            p.text(Pos2::new(v.left() + 46.0, top + th / 2.0 - 9.0), Align2::LEFT_CENTER, name, theme::bold(13.0), TEXT);
            p.text(Pos2::new(v.left() + 46.0, top + th / 2.0 + 9.0), Align2::LEFT_CENTER, if i == 0 { "on screen" } else { "extra" }, theme::font(11.0), TEXT_DIM);
        }
        p.vline(v.left() + HEADER_W - 0.5, (v.top() + RULER_H)..=v.bottom(), Stroke::new(1.0, BORDER));
    }

    fn paint_playhead(&self, p: &Painter) {
        let x = self.x_of(self.playhead);
        let v = self.view;
        if x < v.left() + HEADER_W - 1.0 || x > v.right() {
            return;
        }
        let col = if self.head_hot { hex("#FFC65C") } else { ACCENT };
        p.line_segment([Pos2::new(x, v.top() + 18.0), Pos2::new(x, v.bottom() - SCROLL_H)], Stroke::new(if self.head_hot { 2.0 } else { 1.5 }, col));
        let y = v.top();
        let hw = if self.head_hot { 9.0 } else { 8.0 };
        p.add(PathShape::convex_polygon(
            vec![
                Pos2::new(x - hw, y + 1.0),
                Pos2::new(x + hw, y + 1.0),
                Pos2::new(x + hw, y + 13.0),
                Pos2::new(x, y + 21.0),
                Pos2::new(x - hw, y + 13.0),
            ],
            col,
            Stroke::NONE,
        ));
        // grip lines: it can be grabbed
        for dx in [-3.0, 0.0, 3.0] {
            p.vline(x + dx, (y + 4.0)..=(y + 11.0), Stroke::new(1.0, theme::alpha(BG, 150)));
        }
    }

    fn paint_scrollbar(&self, p: &Painter, par: &Paradigm) {
        let v = self.view;
        let bar = Rect::from_min_max(Pos2::new(v.left() + HEADER_W, v.bottom() - SCROLL_H), v.right_bottom());
        p.rect_filled(bar, CornerRadius::ZERO, BG);
        let visible = (v.width() - HEADER_W) as f64;
        let total = self.content_width(par).max(visible);
        let w = (visible / total * bar.width() as f64).max(40.0) as f32;
        let x = bar.left() + (self.scroll / total * bar.width() as f64) as f32;
        let thumb = Rect::from_min_size(Pos2::new(x, bar.top() + 2.0), Vec2::new(w.min(bar.width()), SCROLL_H - 4.0));
        p.rect_filled(thumb, CornerRadius::same(5), hex("#3A404D"));
    }
}

/// Entries of the "add a block here" menu (block kinds + task blocks).
pub fn add_menu_contents(ui: &mut Ui, t: f64, track: usize, actions: &mut Vec<Action>) {
    ui.label(egui::RichText::new(format!("Add at {}", fmt_time(t, 1))).color(TEXT_DIM).small());
    for bt in BLOCK_TYPES {
        if icon_button(ui, bt.kind, hex(bt.color), bt.title).clicked() {
            actions.push(Action::AddKind { kind: bt.kind.into(), start: Some(t), track: Some(track) });
            ui.close();
        }
    }
    ui.menu_button("Task blocks", |ui| {
        for pr in presets::PRESETS {
            if icon_button(ui, pr.icon_kind, hex(pr.color), pr.title).clicked() {
                actions.push(Action::ChooseVariant { key: pr.key.into(), start: Some(t), pos: None });
                ui.close();
            }
        }
    });
}

/// A menu entry with a small kind icon.
pub fn icon_button(ui: &mut Ui, kind: &str, color: Color32, text: &str) -> egui::Response {
    let resp = ui.add(egui::Button::new(format!("      {text}")).frame(false).min_size(Vec2::new(200.0, 22.0)));
    let r = Rect::from_min_size(Pos2::new(resp.rect.left() + 2.0, resp.rect.center().y - 8.0), Vec2::splat(16.0));
    theme::paint_kind_icon(ui.painter(), kind, r, color);
    resp
}

// ------------------------------------------------------------------ editing helpers
pub fn delete_selected(doc: &mut Document, ripple: bool) {
    let ids: HashSet<String> = doc.selection.iter().cloned().collect();
    if ids.is_empty() {
        return;
    }
    doc.edit(|par| {
        if ripple {
            par.ripple_delete(&ids);
        } else {
            par.blocks.retain(|b| !ids.contains(&b.id));
        }
    });
    doc.select(vec![]);
}

pub fn duplicate_selected(doc: &mut Document) {
    repeat_selected(doc, 1, true);
}

pub fn repeat_selected(doc: &mut Document, n: u32, select_new: bool) {
    let mut blocks: Vec<Block> = doc.selected_blocks().into_iter().cloned().collect();
    if blocks.is_empty() || n == 0 {
        return;
    }
    blocks.sort_by(|a, b| a.start.total_cmp(&b.start));
    let start = blocks.iter().map(|b| b.start).fold(f64::INFINITY, f64::min);
    let end = blocks.iter().map(|b| b.end()).fold(f64::NEG_INFINITY, f64::max);
    let span = end - start;
    let tracks: HashSet<usize> = blocks.iter().map(|b| b.track).collect();
    let new_ids = doc.edit(|par| {
        for o in par.blocks.iter_mut() {
            if o.start >= end - 1e-9 && tracks.contains(&o.track) {
                o.start += span * n as f64;
            }
        }
        let mut ids = vec![];
        for k in 1..=n {
            for b in &blocks {
                let mut c = b.clone_new();
                c.start = round4(b.start + span * k as f64);
                ids.push(c.id.clone());
                par.blocks.push(c);
            }
        }
        ids
    });
    if select_new {
        doc.select(new_ids);
    }
}

pub fn split_at_playhead(doc: &mut Document, t: f64) {
    let sel = doc.selected_blocks();
    let pool: Vec<&Block> = if sel.is_empty() { doc.paradigm.active_blocks(t) } else { sel };
    let targets: Vec<String> = pool.into_iter().filter(|b| b.start < t && t < b.end()).map(|b| b.id.clone()).collect();
    if targets.is_empty() {
        return;
    }
    doc.edit(|par| {
        for id in &targets {
            let Some(b) = par.block_mut(id) else { continue };
            let mut c = b.clone_new();
            c.start = t;
            c.duration = round4(b.end() - t);
            b.duration = round4(t - b.start);
            par.blocks.push(c);
        }
    });
}

pub fn add_block(doc: &mut Document, kind: &str, start: Option<f64>, track: Option<usize>, playhead: f64) {
    let Some(bt) = block_type(kind) else { return };
    let ntr = doc.paradigm.tracks.len().max(1);
    let track = track.unwrap_or(bt.default_track).min(ntr - 1);
    let start = start.unwrap_or_else(|| if track == 0 { doc.paradigm.track_end(track) } else { playhead });
    let mut b = Block::create(kind, start, None);
    b.track = track;
    let tr = doc.paradigm.tr;
    if tr > 0.0 && (bt.default_duration % tr).abs() > 1e-6 && bt.default_duration > tr {
        b.duration = (bt.default_duration / tr).round() * tr;
    }
    let id = doc.edit(|par| par.ripple_insert(b));
    doc.select(vec![id]);
}

pub fn add_preset(doc: &mut Document, key: &str, start: Option<f64>, variant: Option<&serde_json::Value>, ctx: &PresetCtx) {
    let Some(pr) = presets::preset(key) else { return };
    let start = start.unwrap_or_else(|| doc.paradigm.track_end(0));
    let blocks = pr.build(start, variant, ctx);
    let ids = doc.edit(|par| par.insert_group(blocks));
    doc.select(ids);
}

pub fn nudge_selected(doc: &mut Document, step: f64) {
    if doc.selection.is_empty() {
        return;
    }
    doc.begin_edit();
    let ids = doc.selection.clone();
    for id in &ids {
        if let Some(b) = doc.paradigm.block_mut(id) {
            b.start = round4(b.start + step).max(0.0);
        }
    }
    let overl = ids.iter().any(|i| doc.paradigm.block(i).map(|b| doc.paradigm.has_overlap(b)).unwrap_or(false));
    if overl {
        doc.cancel_edit();
    } else {
        doc.commit_edit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParamExt;

    /// A timeline 1200 px wide at 10 px/s and a run of `n` 4-second fixation blocks.
    fn setup(n: usize) -> (Timeline, Document) {
        let mut doc = Document::default();
        for i in 0..n {
            doc.paradigm.blocks.push(Block::create("fixation", i as f64 * 4.0, Some(4.0)));
        }
        let tl = Timeline { pps: 10.0, view: Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 200.0)), ..Default::default() };
        (tl, doc)
    }

    fn click(tl: &mut Timeline, doc: &mut Document, i: usize, mods: egui::Modifiers) {
        let b = doc.paradigm.blocks[i].clone();
        let pos = tl.block_rect(&b, doc.paradigm.tracks.len()).center();
        let mut actions = vec![];
        tl.on_press(pos, mods, doc, &mut actions);
        tl.on_release(doc, &mut actions);
    }

    fn selected(doc: &Document) -> Vec<usize> {
        let mut v: Vec<usize> = doc.selection.iter().filter_map(|id| doc.paradigm.blocks.iter().position(|b| &b.id == id)).collect();
        v.sort();
        v
    }

    #[test]
    fn shift_click_selects_everything_in_between() {
        let (mut tl, mut doc) = setup(10);
        click(&mut tl, &mut doc, 2, egui::Modifiers::NONE);
        assert_eq!(selected(&doc), vec![2]);
        click(&mut tl, &mut doc, 6, egui::Modifiers::SHIFT);
        assert_eq!(selected(&doc), vec![2, 3, 4, 5, 6]);
        // the anchor stays: another Shift+click changes the range, also backwards
        click(&mut tl, &mut doc, 0, egui::Modifiers::SHIFT);
        assert_eq!(selected(&doc), vec![0, 1, 2]);
        // Ctrl+click still adds / removes single blocks
        click(&mut tl, &mut doc, 8, egui::Modifiers::CTRL);
        assert_eq!(selected(&doc), vec![0, 1, 2, 8]);
        // a plain click (no drag) on one of them selects only that one …
        click(&mut tl, &mut doc, 1, egui::Modifiers::NONE);
        assert_eq!(selected(&doc), vec![1]);
        click(&mut tl, &mut doc, 0, egui::Modifiers::SHIFT);
        click(&mut tl, &mut doc, 8, egui::Modifiers::CTRL);
        assert_eq!(selected(&doc), vec![0, 1, 8]);
        // … and delete removes the whole range at once
        delete_selected(&mut doc, false);
        assert_eq!(doc.paradigm.blocks.len(), 7);
    }

    #[test]
    fn dropped_videos_become_video_blocks_as_long_as_the_file() {
        let (mut tl, mut doc) = setup(0);
        let video = format!("{}/tests/media/tiny_h264_high.mp4", env!("CARGO_MANIFEST_DIR"));
        let pos = Pos2::new(HEADER_W + 5.0, RULER_H + 20.0);
        tl.drop_images(&mut doc, vec![video.clone()], pos);
        let b = &doc.paradigm.blocks[0];
        assert_eq!(b.kind, "video");
        assert_eq!(b.params.s("path", ""), video);
        assert!((b.duration - 1.0).abs() < 0.02, "{}", b.duration);
    }

    #[test]
    fn clicking_a_block_moves_the_playhead_there() {
        let (mut tl, mut doc) = setup(5);
        click(&mut tl, &mut doc, 3, egui::Modifiers::NONE);
        assert_eq!(tl.playhead, 12.0);
        // already inside the block: the playhead stays
        tl.playhead = 13.5;
        click(&mut tl, &mut doc, 3, egui::Modifiers::NONE);
        assert_eq!(tl.playhead, 13.5);
    }
}
