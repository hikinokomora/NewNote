//! The page canvas: camera, backgrounds, ink input from pen / touch / mouse,
//! eraser, lasso selection with move/scale/rotate, ruler and protractor,
//! and placement of content containers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, CursorIcon, Mesh, Pos2, Rect, Sense, Shape, Stroke as EStroke, Vec2, pos2, vec2};

use crate::ink::{self, MeshCache, ShapeKind, Xform};
use crate::items;
use crate::model::*;
use crate::pen::{self, PenPhase};
use crate::richtext;
use crate::settings::Settings;
use crate::storage::Notebook;
use crate::textures::Textures;
use crate::winapi::Speller;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EraserMode {
    Stroke,
    Point,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Select,
    Text,
    Pen,
    Highlighter,
    Eraser(EraserMode),
    Lasso,
    Shape(ShapeKind),
    Pan,
}

impl Tool {
    pub fn is_ink(self) -> bool {
        matches!(self, Tool::Pen | Tool::Highlighter | Tool::Eraser(_) | Tool::Lasso | Tool::Shape(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Pen(u32),
    Touch(u64),
    Mouse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LiveKind {
    Ink { highlighter: bool },
    Erase(EraserMode),
    Lasso,
    Shape(ShapeKind),
}

struct Live {
    src: Src,
    kind: LiveKind,
    points: Vec<InkPoint>,
    smooth: Option<InkPoint>,
    snapshot_taken: bool,
}

#[derive(Clone, Copy, Debug)]
enum DragOp {
    Move,
    Scale { anchor: Pos2, start: Pos2 },
    Rotate { center: Pos2, start_angle: f32 },
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub strokes: Vec<usize>,
    pub items: Vec<Id>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.items.is_empty()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ruler {
    pub center: Pos2,
    pub angle: f32,
    pub protractor: bool,
}

#[derive(Clone, Default)]
pub struct Clip {
    pub strokes: Vec<Stroke>,
    pub items: Vec<Item>,
}

/// Find & replace state for the current page.
#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub case: bool,
    pub whole: bool,
    pub current: usize,
}

/// Rendering the page to an image tile by tile using GPU screenshots.
pub struct Capture {
    pub region: Rect,
    pub zoom: f32,
    tiles: Vec<Pos2>,
    idx: usize,
    pub image: Option<image::RgbaImage>,
    shot: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    waiting: u8,
    saved_view: (Vec2, f32),
    pub done: bool,
    pub header: String,
}

pub struct Editor {
    pub page_id: Id,
    pub offset: Vec2,
    pub zoom: f32,
    pub tool: Tool,
    pub pen_color: [u8; 4],
    pub pen_width: f32,
    pub hl_color: [u8; 4],
    pub hl_width: f32,
    pub eraser_radius: f32,
    pub shape_fill: Option<[u8; 4]>,
    live: Option<Live>,
    pub selection: Selection,
    drag: Option<(DragOp, Pos2)>,
    preview: Xform,
    pub editing: Option<Id>,
    pub table_cell: Option<(usize, usize)>,
    pub ruler: Option<Ruler>,
    ruler_drag: Option<Pos2>,
    hover: Option<(Pos2, bool)>,
    last_pen_time: f64,
    pub cache: MeshCache,
    pub undo: Vec<Page>,
    pub redo: Vec<Page>,
    pub find: Find,
    pub format_painter: Option<richtext::Inline>,
    pub crop: Option<Id>,
    pub capture: Option<Capture>,
    pub readonly: bool,
    pub focus_item: Option<Id>,
    pub scroll_to: Option<Pos2>,
    item_drag: Option<(Id, bool)>,
    pub barrel_lasso: bool,
}

impl Editor {
    pub fn new(page_id: Id) -> Self {
        Self {
            page_id,
            offset: vec2(-40.0, -30.0),
            zoom: 1.0,
            tool: Tool::Select,
            pen_color: [20, 20, 20, 255],
            pen_width: 2.0,
            hl_color: [255, 230, 0, 110],
            hl_width: 14.0,
            eraser_radius: 8.0,
            shape_fill: None,
            live: None,
            selection: Selection::default(),
            drag: None,
            preview: Xform::default(),
            editing: None,
            table_cell: None,
            ruler: None,
            ruler_drag: None,
            hover: None,
            last_pen_time: -10.0,
            cache: MeshCache::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            find: Find::default(),
            format_painter: None,
            crop: None,
            capture: None,
            readonly: false,
            focus_item: None,
            scroll_to: None,
            item_drag: None,
            barrel_lasso: false,
        }
    }

    pub fn snapshot(&mut self, page: &Page) {
        self.undo.push(page.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, page: &mut Page) -> bool {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(page, p));
            self.selection = Selection::default();
            self.editing = None;
            return true;
        }
        false
    }

    pub fn redo(&mut self, page: &mut Page) -> bool {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(page, p));
            self.selection = Selection::default();
            self.editing = None;
            return true;
        }
        false
    }

    fn to_screen(&self, origin: Pos2, p: Pos2) -> Pos2 {
        origin + (p.to_vec2() - self.offset) * self.zoom
    }

    fn to_canvas(&self, origin: Pos2, s: Pos2) -> Pos2 {
        ((s - origin) / self.zoom + self.offset).to_pos2()
    }

    pub fn zoom_at(&mut self, origin: Pos2, screen: Pos2, factor: f32) {
        let before = self.to_canvas(origin, screen);
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
        let after = self.to_canvas(origin, screen);
        self.offset += before - after;
    }

    pub fn start_capture(&mut self, region: Rect, zoom: f32, header: String) {
        self.capture = Some(Capture {
            region,
            zoom,
            tiles: Vec::new(),
            idx: 0,
            image: None,
            shot: Arc::new(Mutex::new(None)),
            waiting: 0,
            saved_view: (self.offset, self.zoom),
            done: false,
            header,
        });
        self.selection = Selection::default();
        self.editing = None;
    }

    pub fn selection_bounds(&self, page: &Page) -> Rect {
        let mut r = Rect::NOTHING;
        for &i in &self.selection.strokes {
            if let Some(s) = page.strokes.get(i) {
                r = r.union(s.bbox());
            }
        }
        for id in &self.selection.items {
            if let Some(it) = page.item(*id) {
                r = r.union(it.rect());
            }
        }
        r
    }

    pub fn copy_selection(&self, page: &Page) -> Clip {
        Clip {
            strokes: self.selection.strokes.iter().filter_map(|&i| page.strokes.get(i).cloned()).collect(),
            items: self.selection.items.iter().filter_map(|id| page.item(*id).cloned()).collect(),
        }
    }

    pub fn delete_selection(&mut self, page: &mut Page) {
        if self.selection.is_empty() {
            return;
        }
        self.snapshot(page);
        let mut idx = self.selection.strokes.clone();
        idx.sort_unstable();
        for i in idx.into_iter().rev() {
            if i < page.strokes.len() {
                page.strokes.remove(i);
            }
        }
        let ids = self.selection.items.clone();
        page.items.retain(|it| !ids.contains(&it.id));
        self.selection = Selection::default();
    }

    pub fn paste(&mut self, page: &mut Page, clip: &Clip, at: Option<Pos2>) {
        if clip.strokes.is_empty() && clip.items.is_empty() {
            return;
        }
        self.snapshot(page);
        let mut bb = Rect::NOTHING;
        for s in &clip.strokes {
            bb = bb.union(s.bbox());
        }
        for it in &clip.items {
            bb = bb.union(it.rect());
        }
        let delta = match at {
            Some(p) => p - bb.min,
            None => vec2(20.0, 20.0),
        };
        let x = Xform { offset: delta, ..Default::default() };
        self.selection = Selection::default();
        for s in &clip.strokes {
            page.strokes.push(x.apply_stroke(s));
            self.selection.strokes.push(page.strokes.len() - 1);
        }
        for it in &clip.items {
            let mut it = it.clone();
            it.id = new_id();
            it.pos = [it.pos[0] + delta.x, it.pos[1] + delta.y];
            self.selection.items.push(it.id);
            page.items.push(it);
        }
    }

    /// Visible canvas area for a given screen rect.
    pub fn visible(&self, rect: Rect) -> Rect {
        Rect::from_min_size(self.offset.to_pos2(), rect.size() / self.zoom)
    }
}

pub struct Env<'a> {
    pub settings: &'a Settings,
    pub nb: &'a Notebook,
    pub textures: &'a mut Textures,
    pub speller: &'a Speller,
    pub dark: bool,
    pub actions: &'a mut Vec<Action>,
    pub clip: &'a mut Clip,
    pub highlights: &'a Highlights,
    pub no_copy: bool,
}

/// Requests from the canvas to the application.
#[derive(Clone, Debug)]
pub enum Action {
    OpenLink(String),
    Status(String),
    OcrImage(Id),
    OcrInk,
    PdfPrintout(Id),
    CopyText(String),
    ItemLink(Id),
    SetTaskDue(Id),
    Dirty,
}

fn color32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn page_is_dark(page: &Page, dark: bool) -> bool {
    let c = page_bg(page, dark);
    (c.r() as u32 + c.g() as u32 + c.b() as u32) < 300
}

pub fn page_bg(page: &Page, dark: bool) -> Color32 {
    match page.bg_color {
        Some(c) => color32(c),
        None if dark => Color32::from_gray(30),
        None => Color32::from_gray(253),
    }
}

/// Returns true when the page content changed.
pub fn show(ui: &mut egui::Ui, page: &mut Page, ed: &mut Editor, env: &mut Env) -> bool {
    let mut changed = false;
    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
    let origin = rect.min;
    let ctx = ui.ctx().clone();
    ed.cache.begin_frame();
    let ppp = ctx.pixels_per_point();
    let time = ctx.input(|i| i.time);

    // ---- export capture drives the camera ----------------------------------
    let capturing = capture_step(ed, rect, &ctx);

    if let Some(p) = ed.scroll_to.take() {
        ed.offset = p.to_vec2() - vec2(40.0, 40.0) / ed.zoom;
    }

    // ---- camera input ------------------------------------------------------
    let (scroll, zoom_d, multi, modifiers, any_touch) =
        ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.multi_touch(), i.modifiers, i.any_touches()));
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    if hovered && !capturing {
        if let Some(mt) = multi {
            if mt.num_touches >= 2 {
                ed.zoom_at(origin, mt.center_pos, mt.zoom_delta);
                ed.offset -= mt.translation_delta / ed.zoom;
            }
        } else if zoom_d != 1.0 {
            if let Some(p) = ctx.pointer_hover_pos() {
                ed.zoom_at(origin, p, zoom_d);
            }
        } else if scroll != Vec2::ZERO {
            let s = if modifiers.shift && scroll.x == 0.0 { vec2(scroll.y, 0.0) } else { scroll };
            ed.offset -= s / ed.zoom;
        }
    }
    let space_down = ctx.input(|i| i.key_down(egui::Key::Space)) && ed.editing.is_none();
    let middle = ctx.input(|i| i.pointer.middle_down());
    let pan_mode = ed.tool == Tool::Pan || space_down || middle;

    // ---- background -----------------------------------------------------------
    let painter = ui.painter_at(rect);
    let bg = page_bg(page, env.dark);
    painter.rect_filled(rect, 0.0, if page.size == PageSize::Infinite { bg } else if env.dark { Color32::from_gray(18) } else { Color32::from_gray(228) });
    let visible = ed.visible(rect);
    let content = page.content_bounds();
    if let Some(sheet) = page.size.size() {
        let sheets = ((content.max.y.max(0.0) + 40.0) / sheet.y).ceil().max(1.0) as usize;
        for k in 0..sheets {
            let r = Rect::from_min_size(pos2(0.0, k as f32 * (sheet.y + 24.0)), sheet);
            let sr = Rect::from_min_max(ed.to_screen(origin, r.min), ed.to_screen(origin, r.max));
            painter.rect_filled(sr.translate(vec2(3.0, 3.0)), 2.0, Color32::from_black_alpha(40));
            painter.rect_filled(sr, 0.0, bg);
            draw_pattern(&painter, page, ed, origin, r.intersect(visible), env.dark);
        }
    } else {
        draw_pattern(&painter, page, ed, origin, visible, env.dark);
    }
    if !capturing && page.sensitivity != Sensitivity::None {
        let c = match page.sensitivity {
            Sensitivity::Confidential => Color32::from_rgb(200, 40, 40),
            Sensitivity::Internal => Color32::from_rgb(220, 140, 0),
            _ => Color32::from_rgb(40, 140, 60),
        };
        painter.text(rect.right_top() + vec2(-12.0, 8.0), egui::Align2::RIGHT_TOP, page.sensitivity.label(), egui::FontId::proportional(13.0), c);
    }
    if let Some(cap) = &ed.capture {
        if !cap.header.is_empty() {
            let p = ed.to_screen(origin, cap.region.min + vec2(0.0, -28.0));
            painter.text(p, egui::Align2::LEFT_TOP, &cap.header, egui::FontId::proportional(14.0 * ed.zoom), Color32::from_gray(110));
        }
    }

    // ---- ink below content (highlighters) --------------------------------------
    let curve = env.settings.pressure_curve;
    draw_strokes(&painter, page, ed, origin, visible, curve, true, page_is_dark(page, env.dark));

    // ---- items ---------------------------------------------------------------
    let items_interactive = !ed.tool.is_ink() && !pan_mode && !capturing;
    let mut ops: Vec<items::Op> = Vec::new();
    let item_count = page.items.len();
    for idx in 0..item_count {
        let r = page.items[idx].rect();
        let moving = ed.selection.items.contains(&page.items[idx].id) && !ed.preview.is_identity();
        let r_canvas = if moving {
            Rect::from_min_size(ed.preview.apply(r.min), r.size() * ed.preview.scale)
        } else {
            r
        };
        if !r_canvas.expand(50.0).intersects(visible) && ed.editing != Some(page.items[idx].id) {
            continue;
        }
        let sr = Rect::from_min_size(ed.to_screen(origin, r_canvas.min), r_canvas.size() * ed.zoom);
        let selected = ed.selection.items.contains(&page.items[idx].id);
        let res = items::item_ui(ui, &mut page.items[idx], sr, rect, ed, env, items_interactive, selected, &mut ops);
        if res {
            changed = true;
        }
    }
    if !ops.is_empty() {
        changed |= items::apply_ops(ops, page, ed, env);
    }

    // ---- ink above content ---------------------------------------------------
    draw_strokes(&painter, page, ed, origin, visible, curve, false, page_is_dark(page, env.dark));

    // ---- input: pen samples ----------------------------------------------------
    let samples = pen::drain();
    let mut pen_active = pen::pen_in_range() || time - ed.last_pen_time < 0.4;
    for s in samples {
        ed.last_pen_time = time;
        pen_active = true;
        let sp = pos2((s.x / ppp as f64) as f32, (s.y / ppp as f64) as f32);
        let cp = ed.to_canvas(origin, sp);
        match s.phase {
            PenPhase::Hover => ed.hover = Some((sp, s.eraser)),
            PenPhase::Leave => ed.hover = None,
            PenPhase::Down => {
                ed.hover = Some((sp, s.eraser));
                if !rect.contains(sp) || capturing || ed.readonly || !env.settings.pen_draws {
                    continue;
                }
                let kind = if s.eraser {
                    Some(LiveKind::Erase(match ed.tool {
                        Tool::Eraser(m) => m,
                        _ => EraserMode::Stroke,
                    }))
                } else if s.barrel {
                    ed.barrel_lasso = true;
                    Some(LiveKind::Lasso)
                } else {
                    tool_live_kind(ed.tool)
                };
                if let Some(kind) = kind {
                    // Don't start ink on top of popups / other windows.
                    if ui.ctx().layer_id_at(sp).is_some_and(|l| l.order != egui::Order::Background) {
                        continue;
                    }
                    begin_live(ed, page, Src::Pen(s.id), kind, cp, s.pressure);
                }
            }
            PenPhase::Move => {
                ed.hover = Some((sp, s.eraser));
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Pen(s.id)) {
                    changed |= extend_live(ed, page, cp, s.pressure, env.settings);
                }
            }
            PenPhase::Up => {
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Pen(s.id)) {
                    changed |= extend_live(ed, page, cp, s.pressure, env.settings);
                    changed |= finish_live(ed, page, env.settings);
                }
                ed.barrel_lasso = false;
            }
        }
    }

    // ---- input: touch ----------------------------------------------------------
    let touches: Vec<(u64, egui::TouchPhase, Pos2, Option<f32>)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Touch { id, phase, pos, force, .. } => Some((id.0, *phase, *pos, *force)),
                _ => None,
            })
            .collect()
    });
    for (id, phase, pos, force) in touches {
        if pen::is_pen_id(id) {
            continue;
        }
        let draw_touch = env.settings.touch_draws && ed.tool.is_ink() && !capturing && !ed.readonly;
        let cp = ed.to_canvas(origin, pos);
        match phase {
            egui::TouchPhase::Start => {
                if draw_touch && rect.contains(pos) && multi.is_none() {
                    if let Some(kind) = tool_live_kind(ed.tool) {
                        begin_live(ed, page, Src::Touch(id), kind, cp, force.unwrap_or(0.6));
                    }
                }
            }
            egui::TouchPhase::Move => {
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Touch(id)) {
                    if multi.is_some() {
                        // Second finger: it was a gesture, not a stroke.
                        ed.live = None;
                    } else {
                        changed |= extend_live(ed, page, cp, force.unwrap_or(0.6), env.settings);
                    }
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Touch(id)) {
                    changed |= finish_live(ed, page, env.settings);
                }
            }
        }
    }
    // One-finger pan when touch does not draw.
    if any_touch && !pen_active && multi.is_none() && resp.dragged() && ed.live.is_none() && ed.drag.is_none() && ed.item_drag.is_none() {
        if !(env.settings.touch_draws && ed.tool.is_ink()) && (ed.tool.is_ink() || ed.tool == Tool::Pan) {
            ed.offset -= resp.drag_delta() / ed.zoom;
        }
    }

    // ---- input: mouse ------------------------------------------------------------
    let is_mouse = !any_touch && !pen_active;
    if is_mouse && !capturing {
        if pan_mode && resp.dragged() {
            ed.offset -= resp.drag_delta() / ed.zoom;
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else if ed.tool.is_ink() && env.settings.mouse_draws && !ed.readonly {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let (Some(p), Some(kind)) = (resp.interact_pointer_pos(), tool_live_kind(ed.tool)) {
                    if ruler_hit(ed, ed.to_canvas(origin, p)) {
                        ed.ruler_drag = Some(ed.to_canvas(origin, p));
                    } else if !selection_handle_hit(ed, page, origin, p) {
                        begin_live(ed, page, Src::Mouse, kind, ed.to_canvas(origin, p), 0.6);
                    }
                }
            }
            if ed.live.as_ref().is_some_and(|l| l.src == Src::Mouse) {
                if let Some(p) = ctx.pointer_interact_pos() {
                    changed |= extend_live(ed, page, ed.to_canvas(origin, p), 0.6, env.settings);
                }
                if resp.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
                    changed |= finish_live(ed, page, env.settings);
                }
            }
        }
    }
    // Pen in Select mode with lasso via barrel already handled; ruler dragging with mouse/touch:
    if let Some(start) = ed.ruler_drag {
        if let (Some(r), Some(p)) = (ed.ruler.as_mut(), ctx.pointer_interact_pos()) {
            let cp = ((p - origin) / ed.zoom + ed.offset).to_pos2();
            r.center += cp - start;
            ed.ruler_drag = Some(cp);
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            ed.ruler_drag = None;
        }
    }
    // Ruler rotation: wheel over ruler or two-finger rotate.
    if let Some(r) = ed.ruler.as_mut() {
        let hover_canvas = ctx.pointer_hover_pos().map(|p| ((p - origin) / ed.zoom + ed.offset).to_pos2());
        if let Some(hp) = hover_canvas {
            let local = rot(hp - r.center, -r.angle);
            let on = if r.protractor { local.length() < 160.0 && local.y <= 0.0 } else { local.x.abs() < 360.0 && local.y.abs() < 32.0 };
            if on {
                let (rd, sc) = ctx.input(|i| (i.rotation_delta(), i.smooth_scroll_delta.y));
                if rd != 0.0 {
                    r.angle += rd;
                } else if sc != 0.0 && modifiers.alt {
                    r.angle += sc.signum() * 1f32.to_radians();
                }
            }
        }
    }

    // ---- selection transform ----------------------------------------------------
    if !ed.selection.is_empty() && !capturing {
        changed |= selection_ui(ui, page, ed, origin, rect, env, is_mouse || !pen_active || ed.tool == Tool::Select || ed.tool == Tool::Lasso);
    }

    // ---- empty-canvas clicks (select / text tools) -------------------------
    if !ed.tool.is_ink() && !pan_mode && !capturing && resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let cp = ed.to_canvas(origin, p);
            let was_editing = ed.editing.take();
            ed.selection = Selection::default();
            ed.table_cell = None;
            ed.crop = None;
            if !ed.readonly && (ed.tool == Tool::Text || (was_editing.is_none() && ed.tool == Tool::Select)) {
                ed.snapshot(page);
                let it = Item::new(cp, 420.0, ItemKind::Text(TextBlock::new("", env.settings.default_size, &env.settings.default_font)), &env.settings.author);
                ed.editing = Some(it.id);
                ed.focus_item = Some(it.id);
                page.items.push(it);
                changed = true;
            }
        }
    }
    // Drop empty text containers once they lose editing.
    let editing = ed.editing;
    let before = page.items.len();
    page.items.retain(|it| match &it.kind {
        ItemKind::Text(t) => Some(it.id) == editing || !t.text.trim().is_empty() || !it.tags.is_empty(),
        _ => true,
    });
    if page.items.len() != before {
        changed = true;
    }

    // Right-click on empty canvas: paste menu.
    resp.context_menu(|ui| {
        let at = ui.ctx().pointer_interact_pos().map(|p| ((p - origin) / ed.zoom + ed.offset).to_pos2());
        if ui.button("Вставить").clicked() {
            if !env.clip.items.is_empty() || !env.clip.strokes.is_empty() {
                let clip = env.clip.clone();
                ed.paste(page, &clip, at);
                changed = true;
            }
            ui.close();
        }
        if ui.button("Выделить всё").clicked() {
            ed.selection.strokes = (0..page.strokes.len()).collect();
            ed.selection.items = page.items.iter().map(|i| i.id).collect();
            ui.close();
        }
        if !page.strokes.is_empty() && ui.button("Распознать рукописный текст").clicked() {
            env.actions.push(Action::OcrInk);
            ui.close();
        }
    });

    // ---- overlays ----------------------------------------------------------------
    if !capturing {
        draw_live(&painter, ed, origin, curve, page_is_dark(page, env.dark));
        draw_ruler(&painter, ed, origin);
        if let Some((p, eraser)) = ed.hover {
            if rect.contains(p) && ed.live.is_none() {
                let (r, c) = if eraser || matches!(ed.tool, Tool::Eraser(_)) {
                    (ed.eraser_radius * ed.zoom, Color32::from_gray(120))
                } else if ed.tool == Tool::Highlighter {
                    (ed.hl_width * 0.5 * ed.zoom, color32(ed.hl_color))
                } else {
                    ((ed.pen_width * 0.5 * ed.zoom).max(1.5), color32(ed.pen_color))
                };
                painter.circle_stroke(p, r.max(2.0), EStroke::new(1.0, c));
            }
        }
        if matches!(ed.tool, Tool::Eraser(_)) && is_mouse {
            if let Some(p) = ctx.pointer_hover_pos() {
                if rect.contains(p) {
                    painter.circle_stroke(p, ed.eraser_radius * ed.zoom, EStroke::new(1.0, Color32::from_gray(120)));
                }
            }
        }
        if ed.tool.is_ink() && hovered && !pan_mode && ed.tool != Tool::Pan {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        } else if pan_mode && hovered {
            ctx.set_cursor_icon(CursorIcon::Grab);
        }
    }

    // Keep drawing smooth while the pen is moving.
    if ed.live.is_some() {
        ctx.request_repaint();
    }
    if capturing {
        ctx.request_repaint();
    }
    changed
}

fn tool_live_kind(t: Tool) -> Option<LiveKind> {
    match t {
        Tool::Pen => Some(LiveKind::Ink { highlighter: false }),
        Tool::Highlighter => Some(LiveKind::Ink { highlighter: true }),
        Tool::Eraser(m) => Some(LiveKind::Erase(m)),
        Tool::Lasso => Some(LiveKind::Lasso),
        Tool::Shape(k) => Some(LiveKind::Shape(k)),
        _ => None,
    }
}

fn begin_live(ed: &mut Editor, page: &mut Page, src: Src, kind: LiveKind, cp: Pos2, pressure: f32) {
    if ed.live.is_some() {
        return;
    }
    if kind != LiveKind::Lasso {
        ed.selection = Selection::default();
    }
    let p = InkPoint(cp.x, cp.y, pressure);
    ed.live = Some(Live { src, kind, points: vec![p], smooth: Some(p), snapshot_taken: false });
    if matches!(kind, LiveKind::Erase(_)) {
        ed.snapshot(page);
        if let Some(l) = ed.live.as_mut() {
            l.snapshot_taken = true;
        }
        erase_at(ed, page, cp);
    }
}

fn snap_to_ruler(ed: &Editor, p: Pos2) -> Pos2 {
    let Some(r) = ed.ruler else { return p };
    let local = rot(p - r.center, -r.angle);
    if r.protractor {
        return p;
    }
    // Ruler edge is the top side (local y = -30).
    let tol = 22.0 / ed.zoom;
    if local.x.abs() < 360.0 && (local.y + 30.0).abs() < tol {
        let snapped = vec2(local.x, -30.0);
        return r.center + rot(snapped, r.angle);
    }
    p
}

fn extend_live(ed: &mut Editor, page: &mut Page, cp: Pos2, pressure: f32, s: &Settings) -> bool {
    let zoom = ed.zoom;
    let snapped = snap_to_ruler(ed, cp);
    let Some(live) = ed.live.as_mut() else { return false };
    match live.kind {
        LiveKind::Erase(_) => {
            live.points.push(InkPoint(cp.x, cp.y, pressure));
            return erase_at(ed, page, cp);
        }
        LiveKind::Lasso => {
            live.points.push(InkPoint(cp.x, cp.y, 1.0));
        }
        LiveKind::Shape(_) => {
            if live.points.len() > 1 {
                live.points.pop();
            }
            live.points.push(InkPoint(cp.x, cp.y, 1.0));
        }
        LiveKind::Ink { .. } => {
            // Exponential smoothing of position and pressure; ruler snapping overrides.
            let a = (1.0 - s.smoothing).clamp(0.15, 1.0);
            let prev = live.smooth.unwrap_or(InkPoint(snapped.x, snapped.y, pressure));
            let on_ruler = snapped != cp;
            let np = if on_ruler {
                InkPoint(snapped.x, snapped.y, prev.2 + (pressure - prev.2) * 0.5)
            } else {
                InkPoint(prev.0 + (cp.x - prev.0) * a, prev.1 + (cp.y - prev.1) * a, prev.2 + (pressure - prev.2) * 0.5)
            };
            live.smooth = Some(np);
            let last = *live.points.last().unwrap();
            if (np.pos() - last.pos()).length() * zoom >= 0.4 {
                live.points.push(np);
            } else if let Some(l) = live.points.last_mut() {
                l.2 = l.2.max(np.2);
            }
        }
    }
    false
}

fn erase_at(ed: &mut Editor, page: &mut Page, cp: Pos2) -> bool {
    let Some(live) = ed.live.as_ref() else { return false };
    let LiveKind::Erase(mode) = live.kind else { return false };
    let r = ed.eraser_radius;
    let mut changed = false;
    let mut i = 0;
    while i < page.strokes.len() {
        match mode {
            EraserMode::Stroke => {
                if ink::stroke_hits_circle(&page.strokes[i], cp, r) {
                    page.strokes.remove(i);
                    changed = true;
                    continue;
                }
            }
            EraserMode::Point => {
                if let Some(pieces) = ink::erase_points(&page.strokes[i], cp, r) {
                    let n = pieces.len();
                    page.strokes.splice(i..=i, pieces);
                    changed = true;
                    i += n;
                    continue;
                }
            }
        }
        i += 1;
    }
    changed
}

fn finish_live(ed: &mut Editor, page: &mut Page, s: &Settings) -> bool {
    let Some(live) = ed.live.take() else { return false };
    match live.kind {
        LiveKind::Erase(_) => {
            // Drop the snapshot if nothing was erased.
            if live.snapshot_taken && ed.undo.last().is_some_and(|p| p.strokes.len() == page.strokes.len() && same_strokes(p, page)) {
                ed.undo.pop();
            }
            false
        }
        LiveKind::Lasso => {
            let poly: Vec<Pos2> = live.points.iter().map(|p| p.pos()).collect();
            if poly.len() < 3 {
                ed.selection = Selection::default();
                return false;
            }
            ed.selection.strokes = page.strokes.iter().enumerate().filter(|(_, st)| ink::stroke_in_lasso(st, &poly)).map(|(i, _)| i).collect();
            ed.selection.items = page.items.iter().filter(|it| ink::point_in_polygon(it.rect().center(), &poly)).map(|it| it.id).collect();
            false
        }
        LiveKind::Shape(kind) => {
            let a = live.points[0].pos();
            let b = live.points.last().unwrap().pos();
            if (b - a).length() < 3.0 {
                return false;
            }
            ed.snapshot(page);
            let (pts, closed) = ink::shape_points(kind, a, b);
            page.strokes.push(ink::make_shape_stroke(pts, closed, ed.pen_color, ed.pen_width.max(1.0), ed.shape_fill));
            true
        }
        LiveKind::Ink { highlighter } => {
            let mut pts = live.points;
            if pts.is_empty() {
                return false;
            }
            ed.snapshot(page);
            let eps = (0.25 / ed.zoom).max(0.05);
            let simplified = ink::simplify(&pts, eps);
            let (color, width) = if highlighter { (ed.hl_color, ed.hl_width) } else { (ed.pen_color, ed.pen_width) };
            if s.auto_shapes && !highlighter {
                if let Some(rec) = ink::recognize(&pts) {
                    let (p, closed) = match rec {
                        ink::Recognized::Line(a, b) => (vec![a, b], false),
                        ink::Recognized::Polygon(v) => (v, true),
                        ink::Recognized::Ellipse(r) => (ink::ellipse_points(r, 72), true),
                    };
                    page.strokes.push(ink::make_shape_stroke(p, closed, color, width, None));
                    return true;
                }
            }
            pts = simplified;
            page.strokes.push(Stroke {
                points: Arc::new(pts),
                color,
                width,
                kind: if highlighter { InkKind::Highlighter } else { InkKind::Pen },
                fill: None,
                closed: false,
            });
            true
        }
    }
}

fn same_strokes(a: &Page, b: &Page) -> bool {
    a.strokes.iter().zip(b.strokes.iter()).all(|(x, y)| Arc::ptr_eq(&x.points, &y.points))
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

fn ruler_hit(ed: &Editor, cp: Pos2) -> bool {
    let Some(r) = ed.ruler else { return false };
    let local = rot(cp - r.center, -r.angle);
    if r.protractor {
        local.length() < 150.0 && local.y <= 0.0 && local.length() > 20.0
    } else {
        local.x.abs() < 350.0 && local.y > -26.0 && local.y < 30.0
    }
}

fn draw_pattern(painter: &egui::Painter, page: &Page, ed: &Editor, origin: Pos2, area: Rect, dark: bool) {
    if !area.is_positive() {
        return;
    }
    let line = if dark { Color32::from_rgba_unmultiplied(120, 150, 200, 50) } else { Color32::from_rgba_unmultiplied(80, 130, 210, 55) };
    let strong = if dark { Color32::from_rgba_unmultiplied(120, 150, 200, 90) } else { Color32::from_rgba_unmultiplied(80, 130, 210, 100) };
    let z = ed.zoom;
    let hline = |y: f32, c: Color32| {
        let a = ed.to_screen(origin, pos2(area.min.x, y));
        let b = ed.to_screen(origin, pos2(area.max.x, y));
        painter.line_segment([a, b], EStroke::new(1.0, c));
    };
    let vline = |x: f32, c: Color32| {
        let a = ed.to_screen(origin, pos2(x, area.min.y));
        let b = ed.to_screen(origin, pos2(x, area.max.y));
        painter.line_segment([a, b], EStroke::new(1.0, c));
    };
    match page.background {
        Background::Plain => {}
        Background::Lines => {
            let step = 32.0;
            if step * z < 5.0 {
                return;
            }
            let mut y = (area.min.y / step).floor() * step;
            while y <= area.max.y {
                hline(y, line);
                y += step;
            }
        }
        Background::Grid | Background::Millimeter => {
            let mm = 96.0 / 25.4;
            let (step, major) = if page.background == Background::Grid { (24.0, 0) } else { (mm, 10) };
            let mut step_used = step;
            let mut major = major;
            if step * z < 5.0 {
                if major > 0 {
                    step_used = step * major as f32;
                    major = 0;
                } else {
                    return;
                }
            }
            let i0 = (area.min.y / step_used).floor() as i64;
            let i1 = (area.max.y / step_used).ceil() as i64;
            for i in i0..=i1 {
                hline(i as f32 * step_used, if major > 0 && i % major == 0 { strong } else { line });
            }
            let j0 = (area.min.x / step_used).floor() as i64;
            let j1 = (area.max.x / step_used).ceil() as i64;
            for j in j0..=j1 {
                vline(j as f32 * step_used, if major > 0 && j % major == 0 { strong } else { line });
            }
        }
        Background::Dots => {
            let step = 24.0;
            if step * z < 6.0 {
                return;
            }
            let r = (1.2 * z).clamp(0.8, 2.5);
            let mut y = (area.min.y / step).floor() * step;
            while y <= area.max.y {
                let mut x = (area.min.x / step).floor() * step;
                while x <= area.max.x {
                    painter.circle_filled(ed.to_screen(origin, pos2(x, y)), r, strong);
                    x += step;
                }
                y += step;
            }
        }
    }
}

fn draw_strokes(painter: &egui::Painter, page: &Page, ed: &mut Editor, origin: Pos2, visible: Rect, curve: f32, highlighters: bool, dark: bool) {
    let offset = origin.to_vec2() - ed.offset * ed.zoom;
    let mut mesh = Mesh::default();
    let selected: std::collections::HashSet<usize> = ed.selection.strokes.iter().copied().collect();
    let preview = ed.preview;
    for (i, s) in page.strokes.iter().enumerate() {
        if (s.kind == InkKind::Highlighter) != highlighters {
            continue;
        }
        let moving = selected.contains(&i) && !preview.is_identity();
        let (m, bb) = if moving {
            let mut t = preview.apply_stroke(s);
            t.color = ink::display_color(t.color, dark);
            (Arc::new(ink::tessellate(&t, curve)), t.bbox())
        } else {
            ed.cache.get(s, curve, dark)
        };
        if !bb.intersects(visible) {
            continue;
        }
        ink::append_transformed(&mut mesh, &m, ed.zoom, offset);
        // Flush big meshes to keep index ranges small.
        if mesh.vertices.len() > 60_000 {
            painter.add(Shape::mesh(std::mem::take(&mut mesh)));
        }
        if selected.contains(&i) {
            let r = bb;
            let sr = Rect::from_min_max(ed.to_screen(origin, r.min), ed.to_screen(origin, r.max));
            if !moving {
                painter.rect_stroke(sr, 2.0, EStroke::new(1.0, Color32::from_rgba_unmultiplied(60, 130, 255, 120)), egui::StrokeKind::Outside);
            }
        }
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

fn draw_live(painter: &egui::Painter, ed: &Editor, origin: Pos2, curve: f32, dark: bool) {
    let Some(live) = &ed.live else { return };
    let offset = origin.to_vec2() - ed.offset * ed.zoom;
    match live.kind {
        LiveKind::Ink { highlighter } => {
            let (color, width) = if highlighter { (ed.hl_color, ed.hl_width) } else { (ed.pen_color, ed.pen_width) };
            let s = Stroke {
                points: Arc::new(live.points.clone()),
                color: ink::display_color(color, dark),
                width,
                kind: if highlighter { InkKind::Highlighter } else { InkKind::Pen },
                fill: None,
                closed: false,
            };
            let m = ink::tessellate(&s, curve);
            let mut out = Mesh::default();
            ink::append_transformed(&mut out, &m, ed.zoom, offset);
            painter.add(Shape::mesh(out));
        }
        LiveKind::Lasso => {
            let pts: Vec<Pos2> = live.points.iter().map(|p| ed.to_screen(origin, p.pos())).collect();
            painter.add(Shape::dashed_line(&pts, EStroke::new(1.5, Color32::from_rgb(60, 130, 255)), 6.0, 4.0));
        }
        LiveKind::Shape(k) => {
            if live.points.len() > 1 {
                let (pts, closed) = ink::shape_points(k, live.points[0].pos(), live.points.last().unwrap().pos());
                let s = ink::make_shape_stroke(pts, closed, ink::display_color(ed.pen_color, dark), ed.pen_width.max(1.0), ed.shape_fill);
                let m = ink::tessellate(&s, curve);
                let mut out = Mesh::default();
                ink::append_transformed(&mut out, &m, ed.zoom, offset);
                painter.add(Shape::mesh(out));
            }
        }
        LiveKind::Erase(_) => {}
    }
}

fn draw_ruler(painter: &egui::Painter, ed: &Editor, origin: Pos2) {
    let Some(r) = ed.ruler else { return };
    let tc = |v: Vec2| ed.to_screen(origin, r.center + rot(v, r.angle));
    let fill = Color32::from_rgba_unmultiplied(200, 200, 210, 120);
    let edge = Color32::from_rgba_unmultiplied(40, 40, 60, 200);
    if r.protractor {
        let n = 60;
        let mut pts = vec![tc(vec2(-150.0, 0.0))];
        for i in 0..=n {
            let a = std::f32::consts::PI + i as f32 / n as f32 * std::f32::consts::PI;
            pts.push(tc(vec2(a.cos() * 150.0, a.sin() * 150.0)));
        }
        painter.add(Shape::convex_polygon(pts, fill, EStroke::new(1.0, edge)));
        for deg in (0..=180).step_by(5) {
            let a = std::f32::consts::PI + (deg as f32).to_radians();
            let len = if deg % 30 == 0 { 22.0 } else if deg % 10 == 0 { 14.0 } else { 7.0 };
            let d = vec2(a.cos(), a.sin());
            painter.line_segment([tc(d * 150.0), tc(d * (150.0 - len))], EStroke::new(1.0, edge));
            if deg % 30 == 0 {
                painter.text(tc(d * 118.0), egui::Align2::CENTER_CENTER, format!("{deg}°"), egui::FontId::proportional(10.0 * ed.zoom.max(0.6)), edge);
            }
        }
        painter.circle_filled(tc(Vec2::ZERO), 3.0, edge);
    } else {
        let corners = [tc(vec2(-350.0, -30.0)), tc(vec2(350.0, -30.0)), tc(vec2(350.0, 30.0)), tc(vec2(-350.0, 30.0))];
        painter.add(Shape::convex_polygon(corners.to_vec(), fill, EStroke::new(1.0, edge)));
        let mm = 96.0 / 25.4;
        let n = (700.0 / mm) as i32;
        for i in 0..=n {
            let x = -350.0 + i as f32 * mm;
            let len = if i % 10 == 0 { 16.0 } else if i % 5 == 0 { 10.0 } else { 5.0 };
            painter.line_segment([tc(vec2(x, -30.0)), tc(vec2(x, -30.0 + len))], EStroke::new(1.0, edge));
            if i % 10 == 0 && ed.zoom > 0.5 {
                painter.text(tc(vec2(x, -6.0)), egui::Align2::CENTER_CENTER, format!("{}", i / 10), egui::FontId::proportional(10.0 * ed.zoom), edge);
            }
        }
        let deg = (-r.angle.to_degrees()).rem_euclid(360.0);
        painter.text(tc(vec2(0.0, 16.0)), egui::Align2::CENTER_CENTER, format!("{deg:.0}°"), egui::FontId::proportional(12.0 * ed.zoom.max(0.7)), edge);
    }
}

fn handles(b: Rect) -> ([Pos2; 4], Pos2) {
    ([b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom()], b.center_top() - vec2(0.0, 26.0))
}

fn selection_handle_hit(ed: &Editor, page: &Page, origin: Pos2, p: Pos2) -> bool {
    if ed.selection.is_empty() {
        return false;
    }
    let b = ed.selection_bounds(page);
    let sb = Rect::from_min_max(ed.to_screen(origin, b.min), ed.to_screen(origin, b.max));
    let (c, r) = handles(sb);
    sb.expand(4.0).contains(p) || c.iter().any(|h| (*h - p).length() < 10.0) || (r - p).length() < 10.0
}

fn selection_ui(ui: &mut egui::Ui, page: &mut Page, ed: &mut Editor, origin: Pos2, clip: Rect, env: &mut Env, _pointer_ok: bool) -> bool {
    let mut changed = false;
    let b = ed.selection_bounds(page);
    if !b.is_positive() {
        return false;
    }
    let ctx = ui.ctx().clone();
    let pb = Rect::from_min_size(ed.preview.apply(b.min), b.size() * ed.preview.scale.abs());
    let show_b = if ed.preview.angle == 0.0 { pb } else { b };
    let sb = Rect::from_min_max(ed.to_screen(origin, show_b.min), ed.to_screen(origin, show_b.max));
    let painter = ui.painter_at(clip);
    painter.rect_stroke(sb, 0.0, EStroke::new(1.0, Color32::from_rgb(60, 130, 255)), egui::StrokeKind::Outside);
    let (corners, rh) = handles(sb);
    let id = ui.id().with(("sel", ed.page_id));
    // Body move.
    let body = ui.interact(sb, id.with("body"), Sense::drag());
    if body.drag_started() {
        if let Some(p) = body.interact_pointer_pos() {
            ed.drag = Some((DragOp::Move, ed.to_canvas(origin, p)));
            ed.preview = Xform::default();
        }
    }
    if body.hovered() {
        ctx.set_cursor_icon(CursorIcon::Move);
    }
    for (k, c) in corners.iter().enumerate() {
        let hr = Rect::from_center_size(*c, vec2(12.0, 12.0));
        painter.rect_filled(hr, 2.0, Color32::WHITE);
        painter.rect_stroke(hr, 2.0, EStroke::new(1.0, Color32::from_rgb(60, 130, 255)), egui::StrokeKind::Inside);
        let h = ui.interact(hr.expand(4.0), id.with(("corner", k)), Sense::drag());
        if h.hovered() {
            ctx.set_cursor_icon(if k % 2 == 0 { CursorIcon::ResizeNwSe } else { CursorIcon::ResizeNeSw });
        }
        if h.drag_started() {
            let anchor_s = corners[(k + 2) % 4];
            ed.drag = Some((DragOp::Scale { anchor: ed.to_canvas(origin, anchor_s), start: ed.to_canvas(origin, *c) }, ed.to_canvas(origin, *c)));
            ed.preview = Xform::default();
        }
    }
    painter.line_segment([sb.center_top(), rh], EStroke::new(1.0, Color32::from_rgb(60, 130, 255)));
    painter.circle(rh, 6.0, Color32::WHITE, EStroke::new(1.0, Color32::from_rgb(60, 130, 255)));
    let rr = ui.interact(Rect::from_center_size(rh, vec2(18.0, 18.0)), id.with("rot"), Sense::drag());
    if rr.hovered() {
        ctx.set_cursor_icon(CursorIcon::Alias);
    }
    if rr.drag_started() {
        if let Some(p) = rr.interact_pointer_pos() {
            let center = b.center();
            let cp = ed.to_canvas(origin, p);
            ed.drag = Some((DragOp::Rotate { center, start_angle: (cp - center).angle() }, cp));
            ed.preview = Xform::default();
        }
    }
    // Update preview while dragging.
    if let Some((op, start)) = ed.drag {
        if let Some(p) = ctx.pointer_interact_pos() {
            let cp = ed.to_canvas(origin, p);
            ed.preview = match op {
                DragOp::Move => Xform { offset: cp - start, ..Default::default() },
                DragOp::Scale { anchor, start } => {
                    let d0 = start - anchor;
                    let d1 = cp - anchor;
                    let k = if d0.length() > 1e-3 { (d1.length() / d0.length()).clamp(0.05, 20.0) } else { 1.0 };
                    Xform { pivot: anchor, scale: Vec2::splat(k), ..Default::default() }
                }
                DragOp::Rotate { center, start_angle } => {
                    let mut a = (cp - center).angle() - start_angle;
                    if ctx.input(|i| i.modifiers.shift) {
                        let step = 15f32.to_radians();
                        a = (a / step).round() * step;
                    }
                    Xform { pivot: center, angle: a, ..Default::default() }
                }
            };
            if ed.drag.is_some() && matches!(op, DragOp::Rotate { .. }) {
                // Draw a rotated outline preview.
                let pts: Vec<Pos2> = [b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom(), b.left_top()]
                    .iter()
                    .map(|q| ed.to_screen(origin, ed.preview.apply(*q)))
                    .collect();
                painter.add(Shape::dashed_line(&pts, EStroke::new(1.0, Color32::from_rgb(60, 130, 255)), 5.0, 3.0));
            }
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            // Commit.
            let x = ed.preview;
            if !x.is_identity() {
                ed.snapshot(page);
                for &i in &ed.selection.strokes {
                    if let Some(s) = page.strokes.get_mut(i) {
                        *s = x.apply_stroke(s);
                    }
                }
                let ids = ed.selection.items.clone();
                for it in page.items.iter_mut().filter(|it| ids.contains(&it.id)) {
                    let np = x.apply(it.pos2());
                    let k = x.scale.x;
                    it.pos = [np.x, np.y];
                    if k != 1.0 {
                        it.width *= k;
                        if let ItemKind::Text(t) = &mut it.kind {
                            t.size = (t.size * k).clamp(4.0, 200.0);
                        }
                    }
                }
                changed = true;
            }
            ed.drag = None;
            ed.preview = Xform::default();
        }
    }
    // Floating actions.
    if ed.drag.is_none() {
        let bar = Rect::from_min_size(sb.left_bottom() + vec2(0.0, 8.0), vec2(330.0, 26.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        egui::Frame::popup(ui.style()).inner_margin(2.0).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Копировать").clicked() && !env.no_copy {
                    *env.clip = ed.copy_selection(page);
                }
                if ui.small_button("Вырезать").clicked() && !env.no_copy {
                    *env.clip = ed.copy_selection(page);
                    ed.delete_selection(page);
                    changed = true;
                }
                if ui.small_button("Дублировать").clicked() {
                    let c = ed.copy_selection(page);
                    ed.paste(page, &c, None);
                    changed = true;
                }
                if ui.small_button("Удалить").clicked() {
                    ed.delete_selection(page);
                    changed = true;
                }
                if !ed.selection.strokes.is_empty() && ui.small_button("В текст").clicked() {
                    env.actions.push(Action::OcrInk);
                }
            });
        });
    }
    changed
}

/// Advances an export capture. Returns true while capturing.
fn capture_step(ed: &mut Editor, rect: Rect, ctx: &egui::Context) -> bool {
    let Some(cap) = ed.capture.as_mut() else { return false };
    if cap.done {
        return true;
    }
    let ppp = ctx.pixels_per_point();
    let tile = rect.size() / cap.zoom;
    if cap.tiles.is_empty() {
        let mut y = cap.region.min.y;
        while y < cap.region.max.y {
            let mut x = cap.region.min.x;
            while x < cap.region.max.x {
                cap.tiles.push(pos2(x, y));
                x += tile.x;
            }
            y += tile.y;
        }
        let w = (cap.region.width() * cap.zoom * ppp).ceil() as u32;
        let h = (cap.region.height() * cap.zoom * ppp).ceil() as u32;
        cap.image = Some(image::RgbaImage::new(w.clamp(1, 16000), h.clamp(1, 32000)));
    }
    // Collect a finished screenshot.
    let shot = cap.shot.lock().unwrap().take();
    if let Some(img) = shot {
        let t = cap.tiles[cap.idx];
        let dst = cap.image.as_mut().unwrap();
        let px0 = ((t.x - cap.region.min.x) * cap.zoom * ppp).round() as i64;
        let py0 = ((t.y - cap.region.min.y) * cap.zoom * ppp).round() as i64;
        let sx0 = (rect.min.x * ppp).round() as i64;
        let sy0 = (rect.min.y * ppp).round() as i64;
        let tw = (rect.width() * ppp) as i64;
        let th = (rect.height() * ppp) as i64;
        for y in 0..th {
            let dy = py0 + y;
            let sy = sy0 + y;
            if dy < 0 || dy >= dst.height() as i64 || sy < 0 || sy >= img.size[1] as i64 {
                continue;
            }
            for x in 0..tw {
                let dx = px0 + x;
                let sx = sx0 + x;
                if dx < 0 || dx >= dst.width() as i64 || sx < 0 || sx >= img.size[0] as i64 {
                    continue;
                }
                let c = img.pixels[(sy as usize) * img.size[0] + sx as usize];
                dst.put_pixel(dx as u32, dy as u32, image::Rgba([c.r(), c.g(), c.b(), 255]));
            }
        }
        cap.idx += 1;
        cap.waiting = 0;
        if cap.idx >= cap.tiles.len() {
            cap.done = true;
            let (o, z) = cap.saved_view;
            ed.offset = o;
            ed.zoom = z;
            return true;
        }
    }
    if cap.idx < cap.tiles.len() {
        let t = cap.tiles[cap.idx];
        ed.offset = t.to_vec2();
        ed.zoom = cap.zoom;
        let cap = ed.capture.as_mut().unwrap();
        cap.waiting += 1;
        // Give the frame one pass to lay out at the new camera, then shoot.
        if cap.waiting == 3 {
            let slot = cap.shot.clone();
            let c2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                *slot.lock().unwrap() = Some(img);
                c2.request_repaint();
            });
        }
        if cap.waiting > 60 {
            cap.waiting = 0;
        }
    }
    true
}

/// Region of the page that should be exported.
pub fn export_region(page: &Page) -> Rect {
    let b = page.content_bounds();
    match page.size.size() {
        Some(s) => {
            let sheets = ((b.max.y.max(0.0) + 40.0) / s.y).ceil().max(1.0);
            Rect::from_min_size(Pos2::ZERO, vec2(s.x, sheets * (s.y + 24.0) - 24.0))
        }
        None if b.is_positive() => b.expand(24.0),
        None => Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
    }
}

/// Used by item widgets to know which items have search highlights.
pub type Highlights = HashMap<Id, Vec<(usize, usize)>>;
