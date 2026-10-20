//! The page canvas: camera (zoom, pan, rotation, mirror), backgrounds and
//! layers, ink input from pen / touch / mouse with smoothing, all drawing
//! tools, selection with transforms, rulers, guides, minimap and replay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, CursorIcon, Mesh, Pos2, Rect, Sense, Shape, Stroke as EStroke, Vec2, pos2, vec2};

use crate::brush::{self, Affine};
use crate::ink::{self, Xform};
use crate::items;
use crate::model::*;
use crate::pen::{self, PenPhase};
use crate::settings::{PenPreset, Settings};
use crate::shapes::{self, ShapeKind};
use crate::smooth::{OneEuro, SimPressure, Stabilizer};
use crate::storage::Notebook;
use crate::textures::Textures;
use crate::winapi::Speller;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EraserMode {
    Stroke,
    Point,
    Highlighter,
    Color,
}

impl EraserMode {
    pub const ALL: [EraserMode; 4] = [EraserMode::Stroke, EraserMode::Point, EraserMode::Highlighter, EraserMode::Color];
    pub fn label(self) -> &'static str {
        match self {
            EraserMode::Stroke => "По штрихам",
            EraserMode::Point => "По точкам",
            EraserMode::Highlighter => "Только маркер",
            EraserMode::Color => "По цвету",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Select,
    Text,
    Draw,
    Eraser,
    Lasso,
    RectSelect,
    Shape(ShapeKind),
    Pan,
    Laser,
    Compass,
    Measure,
    Fill,
    Marker,
    Redact,
    Eyedropper,
}

impl Tool {
    pub fn is_ink(self) -> bool {
        !matches!(self, Tool::Select | Tool::Text | Tool::Pan)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerKind {
    Ruler,
    Protractor,
    SetSquare,
}

#[derive(Clone, Copy, Debug)]
pub struct Ruler {
    pub center: Pos2,
    pub angle: f32,
    pub kind: RulerKind,
}

#[derive(Clone, Debug)]
pub struct ShapeStyle {
    pub color: [u8; 4],
    pub fill: Option<[u8; 4]>,
    pub width: f32,
    pub dash: Dash,
    pub arrow_start: bool,
    pub arrow_end: bool,
    pub shadow: bool,
    pub glow: bool,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self { color: [30, 30, 30, 255], fill: None, width: 2.0, dash: Dash::Solid, arrow_start: false, arrow_end: false, shadow: false, glow: false }
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
    Ink,
    Erase(EraserMode),
    Lasso,
    Rect,
    Shape(ShapeKind),
    Laser,
    Compass,
    Measure,
    Redact,
}

struct Live {
    src: Src,
    kind: LiveKind,
    points: Vec<InkPoint>,
    tilt: Vec<u8>,
    euro: OneEuro,
    stab: Stabilizer,
    sim: Option<SimPressure>,
    pressure: f32,
    last_raw: Option<(Pos2, f32, u8)>,
    last_move: (Pos2, f64),
    path_len: f32,
    quick: Option<Vec<shapes::Part>>,
    quick_line: bool,
    snapshot_taken: bool,
    erase_color: Option<[u8; 4]>,
    add: bool,
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

#[derive(Clone, Default)]
pub struct Clip {
    pub strokes: Vec<Stroke>,
    pub items: Vec<Item>,
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub case: bool,
    pub whole: bool,
    pub current: usize,
}

pub struct Capture {
    pub region: Rect,
    pub zoom: f32,
    tiles: Vec<Pos2>,
    idx: usize,
    pub image: Option<image::RgbaImage>,
    shot: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    waiting: u8,
    saved_view: (Vec2, f32, f32, bool),
    pub done: bool,
    pub header: String,
}

pub struct Replay {
    pub start: f64,
    pub speed: f32,
    pub paused: Option<f64>,
}

struct CompassState {
    center: Pos2,
    radius: f32,
    start: Option<f32>,
    last: f32,
    sweep: f32,
}

pub struct Editor {
    pub page_id: Id,
    pub offset: Vec2,
    pub zoom: f32,
    pub rot: f32,
    pub mirror: bool,
    pub tool: Tool,
    pub pen: PenPreset,
    pub pen_slot: Option<usize>,
    pub eraser_mode: EraserMode,
    pub eraser_radius: f32,
    pub shape: ShapeStyle,
    pub layer: u8,
    pub annotate: bool,
    pub free_rotate: bool,
    pub edit_underlay: bool,
    live: Option<Live>,
    pub selection: Selection,
    drag: Option<(DragOp, Pos2)>,
    preview: Xform,
    rubber: Option<(Pos2, Pos2)>,
    pub editing: Option<Id>,
    pub table_cell: Option<(usize, usize)>,
    pub ruler: Option<Ruler>,
    ruler_drag: Option<Pos2>,
    hover: Option<(Pos2, bool)>,
    last_pen_time: f64,
    barrel_down: bool,
    last_barrel: f64,
    pub cache: brush::Cache,
    pub undo: Vec<Page>,
    pub redo: Vec<Page>,
    pub find: Find,
    pub format_painter: Option<crate::richtext::Inline>,
    pub crop: Option<Id>,
    pub capture: Option<Capture>,
    pub readonly: bool,
    pub focus_item: Option<Id>,
    pub scroll_to: Option<Pos2>,
    pub replay: Option<Replay>,
    laser: Vec<Vec<(Pos2, f64)>>,
    polyline: Vec<Pos2>,
    compass: Option<CompassState>,
    measure: Option<(Pos2, Pos2)>,
    touches: HashMap<u64, (Pos2, f64, bool)>,
    touch_peak: usize,
    touch_start: f64,
    pub last_rect: Rect,
    pub status: Option<String>,
}

impl Editor {
    pub fn new(page_id: Id) -> Self {
        Self {
            page_id,
            offset: vec2(-40.0, -30.0),
            zoom: 1.0,
            rot: 0.0,
            mirror: false,
            tool: Tool::Select,
            pen: PenPreset::new("Чёрное перо", InkKind::Pen, [20, 20, 20, 255], 0.7),
            pen_slot: None,
            eraser_mode: EraserMode::Stroke,
            eraser_radius: 8.0,
            shape: ShapeStyle::default(),
            layer: 1,
            annotate: false,
            free_rotate: false,
            edit_underlay: false,
            live: None,
            selection: Selection::default(),
            drag: None,
            preview: Xform::default(),
            rubber: None,
            editing: None,
            table_cell: None,
            ruler: None,
            ruler_drag: None,
            hover: None,
            last_pen_time: -10.0,
            barrel_down: false,
            last_barrel: -10.0,
            cache: brush::Cache::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            find: Find::default(),
            format_painter: None,
            crop: None,
            capture: None,
            readonly: false,
            focus_item: None,
            scroll_to: None,
            replay: None,
            laser: Vec::new(),
            polyline: Vec::new(),
            compass: None,
            measure: None,
            touches: HashMap::new(),
            touch_peak: 0,
            touch_start: 0.0,
            last_rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            status: None,
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

    pub fn live_cancel(&mut self) {
        self.editing = None;
        self.crop = None;
        self.format_painter = None;
        self.polyline.clear();
        self.measure = None;
        self.live = None;
    }

    // ---- view transform ---------------------------------------------------

    fn rot_m(&self) -> [f32; 4] {
        let (s, c) = self.rot.sin_cos();
        let mx = if self.mirror { -1.0 } else { 1.0 };
        [c * mx, -s, s * mx, c]
    }

    /// Canvas → screen for the canvas rect on screen.
    pub fn view(&self, rect: Rect) -> Affine {
        let r = self.rot_m();
        let z = self.zoom;
        let a = [r[0] * z, r[1] * z, r[2] * z, r[3] * z];
        let half = rect.size() * 0.5;
        // screen = origin + half + R·M·((p − offset)·z − half)
        let rm_half = vec2(r[0] * half.x + r[1] * half.y, r[2] * half.x + r[3] * half.y);
        let a_off = vec2(a[0] * self.offset.x + a[1] * self.offset.y, a[2] * self.offset.x + a[3] * self.offset.y);
        Affine { m: a, t: rect.min.to_vec2() + half - a_off - rm_half }
    }

    pub fn rotated(&self) -> bool {
        self.rot.abs() > 1e-4 || self.mirror
    }

    pub fn to_screen(&self, rect: Rect, p: Pos2) -> Pos2 {
        self.view(rect).apply(p)
    }

    pub fn to_canvas(&self, rect: Rect, s: Pos2) -> Pos2 {
        self.view(rect).inverse().apply(s)
    }

    fn vec_to_canvas(&self, rect: Rect, d: Vec2) -> Vec2 {
        let inv = self.view(rect).inverse();
        vec2(inv.m[0] * d.x + inv.m[1] * d.y, inv.m[2] * d.x + inv.m[3] * d.y)
    }

    pub fn zoom_at(&mut self, rect: Rect, screen: Pos2, factor: f32) {
        let before = self.to_canvas(rect, screen);
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
        let after = self.to_canvas(rect, screen);
        self.offset += before - after;
    }

    pub fn rotate_by(&mut self, rect: Rect, screen: Pos2, angle: f32) {
        let before = self.to_canvas(rect, screen);
        self.rot = (self.rot + angle).rem_euclid(std::f32::consts::TAU);
        if self.rot > std::f32::consts::TAU - 1e-3 {
            self.rot = 0.0;
        }
        let after = self.to_canvas(rect, screen);
        self.offset += before - after;
    }

    pub fn reset_rotation(&mut self) {
        let rect = self.last_rect;
        let c = self.to_canvas(rect, rect.center());
        self.rot = 0.0;
        self.mirror = false;
        let after = self.to_canvas(rect, rect.center());
        self.offset += c - after;
    }

    pub fn toggle_mirror(&mut self) {
        let rect = self.last_rect;
        let c = self.to_canvas(rect, rect.center());
        self.mirror = !self.mirror;
        let after = self.to_canvas(rect, rect.center());
        self.offset += c - after;
    }

    pub fn visible(&self, rect: Rect) -> Rect {
        let inv = self.view(rect).inverse();
        Rect::from_points(&[inv.apply(rect.left_top()), inv.apply(rect.right_top()), inv.apply(rect.right_bottom()), inv.apply(rect.left_bottom())])
    }

    pub fn view_center(&self) -> Pos2 {
        self.to_canvas(self.last_rect, self.last_rect.center())
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
            saved_view: (self.offset, self.zoom, self.rot, self.mirror),
            done: false,
            header,
        });
        self.rot = 0.0;
        self.mirror = false;
        self.selection = Selection::default();
        self.editing = None;
    }

    // ---- selection ------------------------------------------------------------

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
        idx.dedup();
        for i in idx.into_iter().rev() {
            if i < page.strokes.len() && !page.strokes[i].locked {
                page.strokes.remove(i);
            }
        }
        let ids = self.selection.items.clone();
        page.items.retain(|it| !ids.contains(&it.id) || it.locked);
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
        let mut group_map: HashMap<u32, u32> = HashMap::new();
        for s in &clip.strokes {
            let mut n = x.apply_stroke(s);
            n.id = rand_u64();
            n.locked = false;
            n.attach = [None, None];
            if n.group != 0 {
                n.group = *group_map.entry(n.group).or_insert_with(|| rand_u64() as u32 | 1);
            }
            page.strokes.push(n);
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

    pub fn expand_groups(&mut self, page: &Page) {
        let groups: Vec<u32> = self.selection.strokes.iter().filter_map(|&i| page.strokes.get(i)).map(|s| s.group).filter(|g| *g != 0).collect();
        if groups.is_empty() {
            return;
        }
        for (i, s) in page.strokes.iter().enumerate() {
            if s.group != 0 && groups.contains(&s.group) && !self.selection.strokes.contains(&i) {
                self.selection.strokes.push(i);
            }
        }
    }

    pub fn select_all(&mut self, page: &Page) {
        self.selection.strokes = page.strokes.iter().enumerate().filter(|(_, s)| !s.locked && page.layers[(s.layer as usize).min(2)].visible).map(|(i, _)| i).collect();
        self.selection.items = page.items.iter().filter(|i| !i.locked && !i.underlay).map(|i| i.id).collect();
    }

    pub fn set_pen(&mut self, p: &PenPreset, slot: Option<usize>) {
        self.pen = p.clone();
        self.pen.kind = p.brush();
        self.pen_slot = slot;
        self.tool = Tool::Draw;
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
    InkToMath,
    PdfPrintout(Id),
    CopyText(String),
    ItemLink(Id),
    SetTaskDue(Id),
    PickColor([u8; 4]),
    ExportSelection(&'static str),
    Dirty,
}

fn color32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn page_bg(page: &Page, dark: bool) -> Color32 {
    match page.bg_color {
        Some(c) => color32(c),
        None if dark => Color32::from_gray(30),
        None => Color32::from_gray(253),
    }
}

pub fn page_is_dark(page: &Page, dark: bool) -> bool {
    let c = page_bg(page, dark);
    (c.r() as u32 + c.g() as u32 + c.b() as u32) < 300
}

fn layer_ok(page: &Page, s: &Stroke) -> bool {
    let l = page.layers[(s.layer as usize).min(2)];
    l.visible && !l.locked && !s.locked
}

/// Returns true when the page content changed.
pub fn show(ui: &mut egui::Ui, page: &mut Page, ed: &mut Editor, env: &mut Env) -> bool {
    let mut changed = false;
    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
    ed.last_rect = rect;
    let ctx = ui.ctx().clone();
    ed.cache.begin_frame();
    let ppp = ctx.pixels_per_point();
    let time = ctx.input(|i| i.time);
    let dark_page = page_is_dark(page, env.dark);
    let curve = env.settings.pressure_curve.max(0.1) * ed.pen.curve.max(0.1);

    let capturing = capture_step(ed, rect, &ctx);
    if let Some(p) = ed.scroll_to.take() {
        let c = ed.to_canvas(rect, rect.left_top() + vec2(40.0, 40.0));
        ed.offset += p - c;
    }

    // ---- camera input --------------------------------------------------------
    let (scroll, zoom_d, multi, modifiers, any_touch) =
        ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.multi_touch(), i.modifiers, i.any_touches()));
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    let pen_recent = pen::pen_in_range() || time - ed.last_pen_time < 1.0;
    let palm_block = env.settings.palm_rejection && pen_recent;
    if hovered && !capturing {
        if let Some(mt) = multi {
            if mt.num_touches >= 2 && !palm_block && ed.live.is_none() {
                ed.zoom_at(rect, mt.center_pos, mt.zoom_delta);
                let d = ed.vec_to_canvas(rect, mt.translation_delta);
                ed.offset -= d;
                if ed.free_rotate && mt.rotation_delta.abs() > 1e-4 {
                    ed.rotate_by(rect, mt.center_pos, mt.rotation_delta);
                }
            }
        } else if zoom_d != 1.0 {
            if let Some(p) = ctx.pointer_hover_pos() {
                ed.zoom_at(rect, p, zoom_d);
            }
        } else if scroll != Vec2::ZERO && !over_ruler(ed, rect, &ctx, modifiers) {
            let s = if modifiers.shift && scroll.x == 0.0 { vec2(scroll.y, 0.0) } else { scroll };
            let d = ed.vec_to_canvas(rect, s);
            ed.offset -= d;
        }
    }
    let space_down = ctx.input(|i| i.key_down(egui::Key::Space)) && ed.editing.is_none();
    let middle = ctx.input(|i| i.pointer.middle_down());
    let pan_mode = ed.tool == Tool::Pan || space_down || middle;

    // ---- background --------------------------------------------------------------
    let view = ed.view(rect);
    let painter = ui.painter_at(rect);
    let bg = page_bg(page, env.dark);
    painter.rect_filled(rect, 0.0, if page.size == PageSize::Infinite { bg } else if env.dark { Color32::from_gray(18) } else { Color32::from_gray(228) });
    let visible = ed.visible(rect);
    let content = page.content_bounds();
    if let Some(sheet) = page.size.size() {
        let sheets = ((content.max.y.max(0.0) + 40.0) / sheet.y).ceil().max(1.0) as usize;
        for k in 0..sheets {
            let r = Rect::from_min_size(pos2(0.0, k as f32 * (sheet.y + 24.0)), sheet);
            let corners: Vec<Pos2> = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].iter().map(|p| view.apply(*p)).collect();
            let shadow: Vec<Pos2> = corners.iter().map(|p| *p + vec2(3.0, 3.0)).collect();
            painter.add(Shape::convex_polygon(shadow, Color32::from_black_alpha(40), EStroke::NONE));
            painter.add(Shape::convex_polygon(corners.clone(), bg, EStroke::NONE));
            draw_pattern(&painter, page, &view, r.intersect(visible), dark_page, ed.zoom);
            let mut c2 = corners.clone();
            c2.push(corners[0]);
            painter.add(Shape::dashed_line(&c2, EStroke::new(1.0, Color32::from_gray(150)), 6.0, 4.0));
        }
    } else {
        draw_pattern(&painter, page, &view, visible, dark_page, ed.zoom);
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
            let p = view.apply(cap.region.min + vec2(0.0, 4.0));
            painter.text(p, egui::Align2::LEFT_TOP, &cap.header, egui::FontId::proportional(14.0 * ed.zoom), Color32::from_gray(110));
        }
    }

    // ---- underlays, background layer -----------------------------------------
    let rotated = ed.rotated();
    for it in page.items.iter().filter(|i| i.underlay && !(ed.edit_underlay && !rotated)) {
        static_item(&painter, it, &view, ed.rot, env, dark_page);
    }
    let replay_lim = replay_limit(ed, page, time);
    draw_strokes(&painter, page, ed, &view, visible, curve, &[0], dark_page, replay_lim);

    // ---- items ------------------------------------------------------------------
    let items_interactive = !ed.tool.is_ink() && !pan_mode && !capturing && !rotated && ed.replay.is_none();
    let before_pos: HashMap<Id, [f32; 2]> = page.items.iter().map(|i| (i.id, i.pos)).collect();
    if rotated {
        for it in page.items.iter().filter(|i| !i.underlay) {
            static_item(&painter, it, &view, ed.rot, env, dark_page);
        }
    } else {
        let mut ops: Vec<items::Op> = Vec::new();
        for idx in 0..page.items.len() {
            let it = &page.items[idx];
            if it.underlay && !ed.edit_underlay {
                continue;
            }
            let r = it.rect();
            let moving = ed.selection.items.contains(&it.id) && !ed.preview.is_identity();
            let r_canvas = if moving { Rect::from_min_size(ed.preview.apply(r.min), r.size() * ed.preview.scale.abs()) } else { r };
            if !r_canvas.expand(50.0).intersects(visible) && ed.editing != Some(it.id) {
                continue;
            }
            let sr = Rect::from_min_size(view.apply(r_canvas.min), r_canvas.size() * ed.zoom);
            let selected = ed.selection.items.contains(&it.id);
            let interactive = items_interactive && !it.locked;
            if items::item_ui(ui, &mut page.items[idx], sr, rect, ed, env, interactive, selected, &mut ops) {
                changed = true;
            }
        }
        if !ops.is_empty() {
            changed |= items::apply_ops(ops, page, ed, env);
        }
    }
    // Annotations follow their container.
    let mut moved: Vec<(Id, Vec2)> = Vec::new();
    for it in &page.items {
        if let Some(old) = before_pos.get(&it.id) {
            let d = vec2(it.pos[0] - old[0], it.pos[1] - old[1]);
            if d != Vec2::ZERO {
                moved.push((it.id, d));
            }
        }
    }
    for (id, d) in moved {
        let x = Xform { offset: d, ..Default::default() };
        for s in page.strokes.iter_mut().filter(|s| s.anchor == Some(id)) {
            *s = x.apply_stroke(s);
        }
        changed = true;
    }

    // ---- ink above content ---------------------------------------------------
    draw_strokes(&painter, page, ed, &view, visible, curve, &[1, 2], dark_page, replay_lim);
    draw_labels(&painter, page, ed, &view, visible, dark_page);

    // ---- pen input -------------------------------------------------------------------
    let samples = pen::drain();
    let mut pen_active = pen::pen_in_range() || time - ed.last_pen_time < 0.4;
    let shift = modifiers.shift;
    for s in samples {
        ed.last_pen_time = time;
        pen_active = true;
        let sp = pos2((s.x / ppp as f64) as f32, (s.y / ppp as f64) as f32);
        let cp = ed.to_canvas(rect, sp);
        let tilt = (s.tilt() * 255.0) as u8;
        // Barrel button: double press while hovering toggles pen and eraser.
        if s.phase == PenPhase::Hover {
            if s.barrel && !ed.barrel_down {
                if time - ed.last_barrel < 0.45 {
                    ed.tool = if ed.tool == Tool::Eraser { Tool::Draw } else { Tool::Eraser };
                    ed.last_barrel = -10.0;
                } else {
                    ed.last_barrel = time;
                }
            }
            ed.barrel_down = s.barrel;
        }
        match s.phase {
            PenPhase::Hover => ed.hover = Some((sp, s.eraser)),
            PenPhase::Leave => ed.hover = None,
            PenPhase::Down => {
                ed.hover = Some((sp, s.eraser));
                if !rect.contains(sp) || capturing || ed.readonly || !env.settings.pen_draws {
                    continue;
                }
                if ui.ctx().layer_id_at(sp).is_some_and(|l| l.order != egui::Order::Background) {
                    continue;
                }
                let tool = if s.eraser {
                    Tool::Eraser
                } else if s.barrel {
                    Tool::Lasso
                } else {
                    ed.tool
                };
                if tool.is_ink() {
                    changed |= pointer_down(ed, page, env, Src::Pen(s.id), tool, cp, s.pressure, tilt, s.time, shift);
                }
            }
            PenPhase::Move => {
                ed.hover = Some((sp, s.eraser));
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Pen(s.id)) {
                    changed |= extend_live(ed, page, env, cp, s.pressure, tilt, s.time, shift);
                }
            }
            PenPhase::Up => {
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Pen(s.id)) {
                    changed |= finish_live(ed, page, env);
                }
            }
        }
    }

    // ---- touch input ---------------------------------------------------------
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
        track_gesture(ed, page, env, id, phase, pos, time, &mut changed);
        if palm_block {
            continue;
        }
        let draw_touch = env.settings.touch_draws && ed.tool.is_ink() && !capturing && !ed.readonly;
        let cp = ed.to_canvas(rect, pos);
        match phase {
            egui::TouchPhase::Start => {
                if draw_touch && rect.contains(pos) && multi.is_none() {
                    changed |= pointer_down(ed, page, env, Src::Touch(id), ed.tool, cp, force.unwrap_or(0.0), 0, time, shift);
                }
            }
            egui::TouchPhase::Move => {
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Touch(id)) {
                    if multi.is_some() {
                        ed.live = None;
                    } else {
                        changed |= extend_live(ed, page, env, cp, force.unwrap_or(0.0), 0, time, shift);
                    }
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if ed.live.as_ref().is_some_and(|l| l.src == Src::Touch(id)) {
                    changed |= finish_live(ed, page, env);
                }
            }
        }
    }
    if any_touch && !pen_active && !palm_block && multi.is_none() && resp.dragged() && ed.live.is_none() && ed.drag.is_none() {
        if !(env.settings.touch_draws && ed.tool.is_ink()) && (ed.tool.is_ink() || ed.tool == Tool::Pan) {
            let d = ed.vec_to_canvas(rect, resp.drag_delta());
            ed.offset -= d;
        }
    }

    // ---- mouse input -------------------------------------------------------------
    let is_mouse = !any_touch && !pen_active;
    if is_mouse && !capturing {
        if pan_mode && resp.dragged() {
            let d = ed.vec_to_canvas(rect, resp.drag_delta());
            ed.offset -= d;
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else if ed.tool.is_ink() && !ed.readonly {
            let mouse_draws = env.settings.mouse_draws || !matches!(ed.tool, Tool::Draw);
            let click_tool = matches!(ed.tool, Tool::Fill | Tool::Marker | Tool::Eyedropper | Tool::Shape(ShapeKind::Polyline));
            if resp.drag_started_by(egui::PointerButton::Primary) || (resp.clicked() && click_tool) {
                if let Some(p) = resp.interact_pointer_pos() {
                    let cp = ed.to_canvas(rect, p);
                    if ruler_hit(ed, cp) && !click_tool {
                        ed.ruler_drag = Some(cp);
                    } else if !selection_handle_hit(ed, page, &view, p) && mouse_draws {
                        changed |= pointer_down(ed, page, env, Src::Mouse, ed.tool, cp, 0.0, 0, time, shift);
                    }
                }
            }
            if ed.live.as_ref().is_some_and(|l| l.src == Src::Mouse) {
                if let Some(p) = ctx.pointer_interact_pos() {
                    let cp = ed.to_canvas(rect, p);
                    changed |= extend_live(ed, page, env, cp, 0.0, 0, time, shift);
                }
                if resp.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
                    changed |= finish_live(ed, page, env);
                }
            }
        }
    }
    // Polyline finishes on double click or Enter.
    if !ed.polyline.is_empty() {
        let finish = resp.double_clicked() || ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if cancel {
            ed.polyline.clear();
        } else if finish && ed.polyline.len() > 2 {
            ed.snapshot(page);
            let mut pts = std::mem::take(&mut ed.polyline);
            pts.dedup_by(|a, b| (*a - *b).length() < 0.5);
            let st = ed.shape.clone();
            page.strokes.push(shape_stroke(pts, false, &st, ed.layer, false, false));
            changed = true;
        }
    }
    // Ruler dragging and rotation.
    if let Some(start) = ed.ruler_drag {
        if let Some(p) = ctx.pointer_interact_pos() {
            let cp = ed.to_canvas(rect, p);
            if let Some(r) = ed.ruler.as_mut() {
                r.center += cp - start;
            }
            ed.ruler_drag = Some(cp);
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            ed.ruler_drag = None;
        }
    }
    if over_ruler(ed, rect, &ctx, modifiers) {
        let (rd, sc) = ctx.input(|i| (i.rotation_delta(), i.smooth_scroll_delta.y));
        if let Some(r) = ed.ruler.as_mut() {
            if rd != 0.0 {
                r.angle += rd;
            } else if sc != 0.0 {
                r.angle += sc.signum() * 1f32.to_radians();
                if modifiers.shift {
                    let step = 15f32.to_radians();
                    r.angle = (r.angle / step).round() * step;
                }
            }
        }
    }

    // ---- select tool: click / rubber band / drag strokes ---------------------
    if ed.tool == Tool::Select && !pan_mode && !capturing && !rotated {
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = resp.interact_pointer_pos() {
                let cp = ed.to_canvas(rect, p);
                let hit = ink::stroke_at(&page.strokes, cp, 6.0 / ed.zoom).filter(|&i| layer_ok(page, &page.strokes[i]));
                match hit {
                    Some(i) => {
                        if !ed.selection.strokes.contains(&i) {
                            if !shift {
                                ed.selection = Selection::default();
                            }
                            ed.selection.strokes.push(i);
                            ed.expand_groups(page);
                        }
                        ed.drag = Some((DragOp::Move, cp));
                        ed.preview = Xform::default();
                    }
                    None => {
                        if !shift {
                            ed.selection = Selection::default();
                        }
                        ed.editing = None;
                        ed.rubber = Some((cp, cp));
                    }
                }
            }
        }
        if let Some((a, _)) = ed.rubber {
            if let Some(p) = ctx.pointer_interact_pos() {
                ed.rubber = Some((a, ed.to_canvas(rect, p)));
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                let (a, b) = ed.rubber.take().unwrap();
                rect_select(ed, page, Rect::from_two_pos(a, b));
            }
        }
    }

    // ---- selection transform ---------------------------------------------------
    if !ed.selection.is_empty() && !capturing && ed.live.is_none() {
        changed |= selection_ui(ui, page, ed, rect, &view, env);
    }

    // ---- clicks in select / text tools -----------------------------------------
    if !ed.tool.is_ink() && !pan_mode && !capturing && !rotated && resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let cp = ed.to_canvas(rect, p);
            let hit = ink::stroke_at(&page.strokes, cp, 6.0 / ed.zoom).filter(|&i| layer_ok(page, &page.strokes[i]));
            if let (Some(i), Tool::Select) = (hit, ed.tool) {
                if !shift {
                    ed.selection = Selection::default();
                }
                ed.selection.strokes.push(i);
                ed.expand_groups(page);
                ed.editing = None;
            } else {
                let was_editing = ed.editing.take();
                let had_selection = !ed.selection.is_empty();
                ed.selection = Selection::default();
                ed.table_cell = None;
                ed.crop = None;
                if !ed.readonly && (ed.tool == Tool::Text || (was_editing.is_none() && !had_selection && ed.tool == Tool::Select)) {
                    ed.snapshot(page);
                    let it = Item::new(cp, 420.0, ItemKind::Text(TextBlock::new("", env.settings.default_size, &env.settings.default_font)), &env.settings.author);
                    ed.editing = Some(it.id);
                    ed.focus_item = Some(it.id);
                    page.items.push(it);
                    changed = true;
                }
            }
        }
    }
    let editing = ed.editing;
    let before = page.items.len();
    page.items.retain(|it| match &it.kind {
        ItemKind::Text(t) => Some(it.id) == editing || !t.text.trim().is_empty() || !it.tags.is_empty() || t.bg.is_some(),
        _ => true,
    });
    if page.items.len() != before {
        changed = true;
    }

    changed |= guides_ui(ui, page, ed, rect, &view, &ctx);

    resp.context_menu(|ui| canvas_menu(ui, page, ed, env, rect, &mut changed));

    // ---- overlays ----------------------------------------------------------------
    if !capturing {
        draw_live(&painter, ed, &view, curve, dark_page);
        draw_ruler(&painter, ed, &view);
        draw_laser(&painter, ed, &view, time);
        draw_tool_overlays(&painter, ed, &view, &ctx);
        if let Some((a, b)) = ed.rubber {
            let pts: Vec<Pos2> = [a, pos2(b.x, a.y), b, pos2(a.x, b.y), a].iter().map(|p| view.apply(*p)).collect();
            painter.add(Shape::dashed_line(&pts, EStroke::new(1.2, Color32::from_rgb(60, 130, 255)), 5.0, 3.0));
        }
        cursor_preview(&painter, ed, &ctx, rect, is_mouse, hovered, pan_mode);
        if env.settings.minimap && !ed.rotated() {
            minimap(ui, page, ed, rect);
        }
        if ed.replay.is_some() {
            replay_bar(ui, ed, page, rect, env, time);
        }
        if rotated {
            painter.text(
                rect.left_bottom() + vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                format!("Холст повёрнут на {:.0}°{} — блоки только для просмотра. Ctrl+Alt+R — повернуть ещё", ed.rot.to_degrees(), if ed.mirror { ", отражён" } else { "" }),
                egui::FontId::proportional(12.0),
                Color32::from_gray(140),
            );
        }
    }
    if ed.live.is_some() || capturing || !ed.laser.is_empty() || ed.replay.is_some() || ed.compass.is_some() {
        ctx.request_repaint();
    }
    changed
}

// ----------------------------------------------------------------------------
// Pointer handling

#[allow(clippy::too_many_arguments)]
fn pointer_down(ed: &mut Editor, page: &mut Page, env: &mut Env, src: Src, tool: Tool, cp: Pos2, pressure: f32, tilt: u8, t: f64, shift: bool) -> bool {
    if ed.live.is_some() {
        return false;
    }
    match tool {
        Tool::Fill => {
            let hit = page.strokes.iter().rposition(|s| s.closed && layer_ok(page, s) && ink::point_in_polygon(cp, &s.points.iter().map(|p| p.pos()).collect::<Vec<_>>()));
            if let Some(i) = hit {
                ed.snapshot(page);
                page.strokes[i].fill = Some(ed.pen.color);
                return true;
            }
            false
        }
        Tool::Eyedropper => {
            if let Some(i) = ink::stroke_at(&page.strokes, cp, 6.0 / ed.zoom) {
                let c = page.strokes[i].color;
                ed.pen.color = c;
                ed.shape.color = c;
                env.actions.push(Action::PickColor(c));
            }
            ed.tool = Tool::Draw;
            false
        }
        Tool::Marker => {
            let n = page.strokes.iter().filter(|s| s.style.marker).filter_map(|s| s.style.label.parse::<u32>().ok()).max().unwrap_or(0) + 1;
            ed.snapshot(page);
            let pts = ink::ellipse_points(Rect::from_center_size(cp, Vec2::splat(22.0)), 40);
            let mut s = ink::make_shape_stroke(pts, true, ed.pen.color, 1.5, Some(ed.pen.color));
            s.style = Arc::new(StrokeStyle { label: n.to_string(), marker: true, ..Default::default() });
            s.layer = 2;
            s.anchor = if ed.annotate { item_at(page, cp) } else { None };
            page.strokes.push(s);
            true
        }
        Tool::Shape(ShapeKind::Polyline) => {
            let p = snap_point(cp, page, ed, env.settings);
            if ed.polyline.is_empty() {
                ed.polyline.push(p);
            }
            ed.polyline.push(p);
            false
        }
        _ => {
            let kind = match tool {
                Tool::Draw => LiveKind::Ink,
                Tool::Eraser => LiveKind::Erase(ed.eraser_mode),
                Tool::Lasso => LiveKind::Lasso,
                Tool::RectSelect => LiveKind::Rect,
                Tool::Shape(k) => LiveKind::Shape(k),
                Tool::Laser => LiveKind::Laser,
                Tool::Compass => LiveKind::Compass,
                Tool::Measure => LiveKind::Measure,
                Tool::Redact => LiveKind::Redact,
                _ => return false,
            };
            if matches!(kind, LiveKind::Ink) {
                ed.selection = Selection::default();
            }
            let cp = if matches!(kind, LiveKind::Shape(_) | LiveKind::Redact | LiveKind::Measure | LiveKind::Compass) { snap_point(cp, page, ed, env.settings) } else { cp };
            let simulated = matches!(src, Src::Mouse) || (matches!(src, Src::Touch(_)) && pressure <= 0.0);
            let mut sim = simulated.then(SimPressure::new);
            let p0 = match &mut sim {
                Some(s) => s.next(cp, t, ed.zoom),
                None => pressure,
            };
            let mut live = Live {
                src,
                kind,
                points: vec![InkPoint(cp.x, cp.y, p0)],
                tilt: vec![tilt],
                euro: OneEuro::new(ed.pen.smoothing),
                stab: Stabilizer::new(ed.pen.stabilizer),
                sim,
                pressure: p0,
                last_raw: None,
                last_move: (cp, t),
                path_len: 0.0,
                quick: None,
                quick_line: false,
                snapshot_taken: false,
                erase_color: None,
                add: shift,
            };
            live.euro.filter(cp, t, ed.zoom);
            live.stab.filter(cp, ed.zoom);
            if kind == LiveKind::Laser {
                ed.laser.push(vec![(cp, t)]);
            }
            if kind == LiveKind::Compass {
                ed.compass = Some(CompassState { center: cp, radius: 0.0, start: None, last: 0.0, sweep: 0.0 });
            }
            if kind == LiveKind::Measure {
                ed.measure = Some((cp, cp));
            }
            ed.live = Some(live);
            if let LiveKind::Erase(_) = kind {
                ed.snapshot(page);
                if let Some(l) = ed.live.as_mut() {
                    l.snapshot_taken = true;
                }
                return erase_at(ed, page, cp);
            }
            false
        }
    }
}

fn snap_point(p: Pos2, page: &Page, ed: &Editor, s: &Settings) -> Pos2 {
    let mut q = p;
    if s.snap_grid {
        let step = page.pattern().snap_step(page.pattern_step);
        q = pos2((p.x / step).round() * step, (p.y / step).round() * step);
    }
    let tol = 8.0 / ed.zoom;
    for g in &page.guides {
        if g.vertical && (q.x - g.at).abs() < tol {
            q.x = g.at;
        }
        if !g.vertical && (q.y - g.at).abs() < tol {
            q.y = g.at;
        }
    }
    q
}

fn item_at(page: &Page, p: Pos2) -> Option<Id> {
    page.items.iter().rev().find(|i| i.rect().contains(p)).map(|i| i.id)
}

fn snap_to_ruler(ed: &Editor, p: Pos2) -> Option<Pos2> {
    let r = ed.ruler?;
    let local = rot(p - r.center, -r.angle);
    let tol = 22.0 / ed.zoom;
    match r.kind {
        RulerKind::Ruler => {
            if local.x.abs() < 360.0 && (local.y + 30.0).abs() < tol {
                return Some(r.center + rot(vec2(local.x, -30.0), r.angle));
            }
        }
        RulerKind::SetSquare => {
            let leg = 260.0;
            let edges = [(vec2(0.0, 0.0), vec2(leg, 0.0)), (vec2(0.0, 0.0), vec2(0.0, -leg)), (vec2(leg, 0.0), vec2(0.0, -leg))];
            let mut best: Option<(f32, Vec2)> = None;
            for (a, b) in edges {
                let ab = b - a;
                let t = ((local - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0);
                let q = a + ab * t;
                let d = (local - q).length();
                if d < tol && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, q));
                }
            }
            if let Some((_, q)) = best {
                return Some(r.center + rot(q, r.angle));
            }
        }
        RulerKind::Protractor => {
            let d = local.length();
            if local.y <= 0.0 && (d - 150.0).abs() < tol && d > 1.0 {
                return Some(r.center + rot(local / d * 150.0, r.angle));
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn extend_live(ed: &mut Editor, page: &mut Page, env: &mut Env, cp: Pos2, pressure: f32, tilt: u8, t: f64, shift: bool) -> bool {
    let zoom = ed.zoom;
    let ruler_pt = snap_to_ruler(ed, cp);
    let snapped = snap_point(cp, page, ed, env.settings);
    let quick_enabled = env.settings.quick_shape;
    let use_pressure = ed.pen.pressure;
    let use_tilt = ed.pen.tilt;
    let Some(live) = ed.live.as_mut() else { return false };
    match live.kind {
        LiveKind::Erase(_) => {
            live.points.push(InkPoint(cp.x, cp.y, pressure));
            return erase_at(ed, page, cp);
        }
        LiveKind::Lasso | LiveKind::Laser => {
            live.points.push(InkPoint(cp.x, cp.y, 1.0));
            if live.kind == LiveKind::Laser {
                if let Some(trail) = ed.laser.last_mut() {
                    trail.push((cp, t));
                }
            }
        }
        LiveKind::Shape(_) | LiveKind::Rect | LiveKind::Redact => {
            if live.points.len() > 1 {
                live.points.pop();
            }
            live.points.push(InkPoint(snapped.x, snapped.y, 1.0));
        }
        LiveKind::Measure => {
            if let Some(m) = ed.measure.as_mut() {
                m.1 = snapped;
            }
        }
        LiveKind::Compass => {
            live.points.push(InkPoint(cp.x, cp.y, 1.0));
            let n = live.points.len();
            if let Some(c) = ed.compass.as_mut() {
                let d = cp - c.center;
                let ang = d.y.atan2(d.x);
                match c.start {
                    None => {
                        c.radius = d.length();
                        // Holding Shift locks the radius and starts drawing the arc.
                        if d.length() * zoom > 12.0 && (shift || n > 40) {
                            c.start = Some(ang);
                            c.last = ang;
                        }
                    }
                    Some(_) => {
                        let mut da = ang - c.last;
                        while da > std::f32::consts::PI {
                            da -= std::f32::consts::TAU;
                        }
                        while da < -std::f32::consts::PI {
                            da += std::f32::consts::TAU;
                        }
                        c.sweep = (c.sweep + da).clamp(-std::f32::consts::TAU, std::f32::consts::TAU);
                        c.last = ang;
                    }
                }
            }
        }
        LiveKind::Ink => {
            let prev_last = live.points.last().map(|p| p.pos()).unwrap_or(cp);
            let raw_p = if use_pressure { pressure } else { 1.0 };
            let raw_tilt = if use_tilt { tilt } else { 0 };
            live.last_raw = Some((cp, raw_p, raw_tilt));
            if live.quick.is_some() {
                if live.quick_line {
                    let a = live.points[0].pos();
                    let b = if shift { shapes::constrain(ShapeKind::Line, a, cp) } else { cp };
                    live.quick = Some(vec![shapes::Part { pts: vec![a, b], closed: false, arrow_start: false, arrow_end: false }]);
                }
                return false;
            }
            let point = if let Some(rp) = ruler_pt {
                rp
            } else {
                let Some(st) = live.stab.filter(cp, zoom) else { return false };
                live.euro.filter(st, t, zoom)
            };
            let p = match &mut live.sim {
                Some(sim) => sim.next(point, t, zoom),
                None => {
                    live.pressure += (raw_p - live.pressure) * 0.55;
                    live.pressure
                }
            };
            if shift {
                let a = live.points[0];
                let b = shapes::constrain(ShapeKind::Line, a.pos(), point);
                live.points.truncate(1);
                live.points.push(InkPoint(b.x, b.y, p));
                live.tilt.truncate(1);
                live.tilt.push(raw_tilt);
                return false;
            }
            if (point - prev_last).length() * zoom >= 0.35 {
                live.path_len += (point - prev_last).length();
                live.points.push(InkPoint(point.x, point.y, p));
                live.tilt.push(raw_tilt);
            } else if let Some(l) = live.points.last_mut() {
                l.2 = l.2.max(p);
            }
            // Holding still at the end turns the stroke into a clean shape.
            if (cp - live.last_move.0).length() * zoom > 3.0 {
                live.last_move = (cp, t);
            } else if quick_enabled && t - live.last_move.1 > 0.55 && live.path_len * zoom > 30.0 {
                let parts = match ink::recognize(&live.points) {
                    Some(ink::Recognized::Line(a, b)) => {
                        live.quick_line = true;
                        vec![shapes::Part { pts: vec![a, b], closed: false, arrow_start: false, arrow_end: false }]
                    }
                    Some(ink::Recognized::Polygon(v)) => vec![shapes::Part { pts: v, closed: true, arrow_start: false, arrow_end: false }],
                    Some(ink::Recognized::Ellipse(r)) => vec![shapes::Part { pts: ink::ellipse_points(r, 72), closed: true, arrow_start: false, arrow_end: false }],
                    None => {
                        live.quick_line = true;
                        vec![shapes::Part { pts: vec![live.points[0].pos(), cp], closed: false, arrow_start: false, arrow_end: false }]
                    }
                };
                live.quick = Some(parts);
            }
        }
    }
    false
}

fn erase_at(ed: &mut Editor, page: &mut Page, cp: Pos2) -> bool {
    let r = ed.eraser_radius / ed.zoom.max(0.05);
    let Some(live) = ed.live.as_mut() else { return false };
    let LiveKind::Erase(mode) = live.kind else { return false };
    let layers = page.layers;
    let usable = |s: &Stroke| {
        let l = layers[(s.layer as usize).min(2)];
        l.visible && !l.locked && !s.locked
    };
    let mut changed = false;
    let mut i = 0;
    while i < page.strokes.len() {
        let s = &page.strokes[i];
        if !usable(s) {
            i += 1;
            continue;
        }
        match mode {
            EraserMode::Point => {
                if let Some(pieces) = ink::erase_points(s, cp, r) {
                    let n = pieces.len();
                    page.strokes.splice(i..=i, pieces);
                    changed = true;
                    i += n;
                    continue;
                }
            }
            EraserMode::Stroke | EraserMode::Highlighter | EraserMode::Color => {
                let kind_ok = mode != EraserMode::Highlighter || s.kind == InkKind::Highlighter;
                if kind_ok && ink::stroke_hits_circle(s, cp, r) {
                    if mode == EraserMode::Color {
                        let c = *live.erase_color.get_or_insert(s.color);
                        if s.color != c {
                            i += 1;
                            continue;
                        }
                    }
                    page.strokes.remove(i);
                    changed = true;
                    continue;
                }
            }
        }
        i += 1;
    }
    if changed {
        ed.selection = Selection::default();
    }
    changed
}

fn shape_stroke(pts: Vec<Pos2>, closed: bool, st: &ShapeStyle, layer: u8, arrow_start: bool, arrow_end: bool) -> Stroke {
    let mut s = ink::make_shape_stroke(pts, closed, st.color, st.width.max(0.5), st.fill);
    s.style = Arc::new(StrokeStyle {
        dash: st.dash,
        arrow_start: arrow_start || (st.arrow_start && !closed),
        arrow_end: arrow_end || (st.arrow_end && !closed),
        shadow: st.shadow,
        glow: st.glow,
        ..Default::default()
    });
    s.layer = layer;
    s
}

fn finish_live(ed: &mut Editor, page: &mut Page, env: &mut Env) -> bool {
    let Some(mut live) = ed.live.take() else { return false };
    let zoom = ed.zoom;
    match live.kind {
        LiveKind::Erase(_) => {
            if live.snapshot_taken && ed.undo.last().is_some_and(|p| same_strokes(p, page)) {
                ed.undo.pop();
            }
            false
        }
        LiveKind::Laser | LiveKind::Measure => false,
        LiveKind::Lasso => {
            let poly: Vec<Pos2> = live.points.iter().map(|p| p.pos()).collect();
            if !live.add {
                ed.selection = Selection::default();
            }
            if poly.len() < 3 {
                return false;
            }
            let layers = page.layers;
            for (i, st) in page.strokes.iter().enumerate() {
                let l = layers[(st.layer as usize).min(2)];
                if l.visible && !l.locked && !st.locked && ink::stroke_in_lasso(st, &poly) && !ed.selection.strokes.contains(&i) {
                    ed.selection.strokes.push(i);
                }
            }
            for it in &page.items {
                if !it.locked && !it.underlay && ink::point_in_polygon(it.rect().center(), &poly) && !ed.selection.items.contains(&it.id) {
                    ed.selection.items.push(it.id);
                }
            }
            ed.expand_groups(page);
            false
        }
        LiveKind::Rect => {
            let a = live.points[0].pos();
            let b = live.points.last().unwrap().pos();
            if !live.add {
                ed.selection = Selection::default();
            }
            rect_select(ed, page, Rect::from_two_pos(a, b));
            false
        }
        LiveKind::Redact => {
            let a = live.points[0].pos();
            let b = live.points.last().unwrap().pos();
            if (b - a).length() < 3.0 {
                return false;
            }
            ed.snapshot(page);
            let r = Rect::from_two_pos(a, b);
            let mut s = ink::make_shape_stroke(vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true, [40, 40, 40, 255], 1.0, None);
            s.style = Arc::new(StrokeStyle { redact: true, ..Default::default() });
            s.layer = 2;
            page.strokes.push(s);
            true
        }
        LiveKind::Compass => {
            let Some(c) = ed.compass.take() else { return false };
            if c.radius * zoom < 4.0 {
                return false;
            }
            ed.snapshot(page);
            let full = c.start.is_none() || c.sweep.abs() > 6.1;
            let (pts, closed) = if full {
                (ink::ellipse_points(Rect::from_center_size(c.center, Vec2::splat(c.radius * 2.0)), 96), true)
            } else {
                let a0 = c.start.unwrap();
                let n = ((c.sweep.abs() / std::f32::consts::TAU) * 96.0).ceil().max(4.0) as usize;
                let pts = (0..=n)
                    .map(|i| {
                        let a = a0 + c.sweep * i as f32 / n as f32;
                        c.center + vec2(a.cos(), a.sin()) * c.radius
                    })
                    .collect();
                (pts, false)
            };
            let st = ShapeStyle { color: ed.pen.color, width: ed.pen.width, ..ShapeStyle::default() };
            page.strokes.push(shape_stroke(pts, closed, &st, ed.layer, false, false));
            true
        }
        LiveKind::Shape(kind) => {
            let a = live.points[0].pos();
            let mut b = live.points.last().unwrap().pos();
            if (b - a).length() * zoom < 3.0 {
                return false;
            }
            if live.add {
                b = shapes::constrain(kind, a, b);
            }
            ed.snapshot(page);
            let parts = shapes::build(kind, a, b);
            let group = if parts.len() > 1 { rand_u64() as u32 | 1 } else { 0 };
            let st = ed.shape.clone();
            let first = page.strokes.len();
            for p in parts {
                let mut s = shape_stroke(p.pts, p.closed, &st, ed.layer, p.arrow_start, p.arrow_end);
                s.group = group;
                page.strokes.push(s);
            }
            if kind.is_connector() {
                let n = page.strokes.len();
                let ids = [closed_shape_at(page, a, first), closed_shape_at(page, b, first)];
                if ids[0].is_some() || ids[1].is_some() {
                    page.strokes[n - 1].attach = ids;
                    update_connectors(page);
                }
            }
            true
        }
        LiveKind::Ink => {
            let pen = ed.pen.clone();
            let kind = pen.brush();
            let (color, width) = (pen.color, pen.width);
            if let Some(parts) = live.quick.take() {
                ed.snapshot(page);
                let st = ShapeStyle { color, width, ..ShapeStyle::default() };
                for p in parts {
                    page.strokes.push(shape_stroke(p.pts, p.closed, &st, ed.layer, false, false));
                }
                return true;
            }
            // Catch up with the pointer (stabilizer and filter lag).
            if let Some((p, pr, tl)) = live.last_raw {
                if live.points.last().map(|q| (q.pos() - p).length()).unwrap_or(0.0) * zoom > 0.5 {
                    let pr = if live.sim.is_some() { live.points.last().map(|q| q.2).unwrap_or(pr) } else { pr };
                    live.points.push(InkPoint(p.x, p.y, pr));
                    live.tilt.push(tl);
                }
            }
            let mut pts = live.points;
            let tilt = live.tilt;
            if pts.is_empty() {
                return false;
            }
            if live.sim.is_some() {
                crate::smooth::taper_end(&mut pts);
            }
            // Scratch-out: a zigzag over ink deletes it.
            if env.settings.scratch_out && kind != InkKind::Highlighter {
                let poly: Vec<Pos2> = pts.iter().map(|p| p.pos()).collect();
                if crate::smooth::is_scratch(&poly) {
                    let rr = (width * 1.5).max(4.0 / zoom);
                    let layers = page.layers;
                    let hit: Vec<usize> = page
                        .strokes
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            let l = layers[(s.layer as usize).min(2)];
                            l.visible && !l.locked && !s.locked && poly.iter().step_by(2).any(|p| ink::stroke_hits_circle(s, *p, rr))
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if !hit.is_empty() {
                        ed.snapshot(page);
                        for i in hit.into_iter().rev() {
                            page.strokes.remove(i);
                        }
                        ed.status = Some("Зачёркнутое удалено (Ctrl+Z — вернуть)".into());
                        return true;
                    }
                }
            }
            ed.snapshot(page);
            if env.settings.auto_shapes && kind != InkKind::Highlighter {
                if let Some(rec) = ink::recognize(&pts) {
                    let (p, closed) = match rec {
                        ink::Recognized::Line(a, b) => (vec![a, b], false),
                        ink::Recognized::Polygon(v) => (v, true),
                        ink::Recognized::Ellipse(r) => (ink::ellipse_points(r, 72), true),
                    };
                    let st = ShapeStyle { color, width, ..ShapeStyle::default() };
                    page.strokes.push(shape_stroke(p, closed, &st, ed.layer, false, false));
                    return true;
                }
            }
            let eps = (0.18 / zoom).max(0.04);
            let mask = ink::simplify_mask(&pts, eps);
            let keep_tilt = tilt.len() == pts.len() && tilt.iter().any(|t| *t > 8);
            let simplified: Vec<InkPoint> = pts.iter().zip(&mask).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
            let tilt: Vec<u8> = if keep_tilt { tilt.iter().zip(&mask).filter(|(_, k)| **k).map(|(t, _)| *t).collect() } else { Vec::new() };
            let start = pts[0].pos();
            let mut s = Stroke::new(simplified, color, width, kind);
            s.tilt = Arc::new(tilt);
            s.style = Arc::new(StrokeStyle { effect: pen.effect, no_pressure: !pen.pressure, ..Default::default() });
            s.layer = if kind == InkKind::Highlighter && pen.under_text { 0 } else { ed.layer };
            if ed.annotate {
                s.anchor = item_at(page, start);
            }
            page.strokes.push(s);
            true
        }
    }
}

fn closed_shape_at(page: &Page, p: Pos2, before: usize) -> Option<u64> {
    page.strokes[..before].iter().rev().find(|s| s.closed && s.bbox().contains(p)).map(|s| s.id)
}

/// Re-routes connector ends to the borders of the shapes they are glued to.
pub fn update_connectors(page: &mut Page) {
    let boxes: HashMap<u64, Rect> = page
        .strokes
        .iter()
        .filter(|s| s.closed)
        .map(|s| (s.id, Rect::from_points(&s.points.iter().map(|p| p.pos()).collect::<Vec<_>>())))
        .collect();
    for s in page.strokes.iter_mut() {
        if (s.attach[0].is_none() && s.attach[1].is_none()) || s.points.len() < 2 {
            continue;
        }
        let mut pts: Vec<InkPoint> = s.points.to_vec();
        let n = pts.len();
        let ra = s.attach[0].and_then(|id| boxes.get(&id)).copied();
        let rb = s.attach[1].and_then(|id| boxes.get(&id)).copied();
        let a_target = ra.map(|r| r.center()).unwrap_or(pts[0].pos());
        let b_target = rb.map(|r| r.center()).unwrap_or(pts[n - 1].pos());
        if let Some(r) = ra {
            let p = border_point(r, b_target);
            pts[0] = InkPoint(p.x, p.y, 1.0);
        }
        if let Some(r) = rb {
            let p = border_point(r, a_target);
            pts[n - 1] = InkPoint(p.x, p.y, 1.0);
        }
        if n > 2 {
            let (a, b) = (pts[0].pos(), pts[n - 1].pos());
            for (i, q) in pts.iter_mut().enumerate().take(n - 1).skip(1) {
                let t = i as f32 / (n - 1) as f32;
                let p = a + (b - a) * t;
                *q = InkPoint(p.x, p.y, 1.0);
            }
        }
        s.points = Arc::new(pts);
    }
}

fn border_point(r: Rect, toward: Pos2) -> Pos2 {
    let c = r.center();
    let d = toward - c;
    if d.length_sq() < 1e-6 {
        return c;
    }
    let tx = if d.x.abs() > 1e-6 { (r.width() * 0.5) / d.x.abs() } else { f32::INFINITY };
    let ty = if d.y.abs() > 1e-6 { (r.height() * 0.5) / d.y.abs() } else { f32::INFINITY };
    c + d * tx.min(ty).min(1.0)
}

fn rect_select(ed: &mut Editor, page: &Page, r: Rect) {
    if r.width() < 1.0 && r.height() < 1.0 {
        return;
    }
    for (i, s) in page.strokes.iter().enumerate() {
        if layer_ok(page, s) && ink::stroke_in_rect(s, r.expand(s.width)) && !ed.selection.strokes.contains(&i) {
            ed.selection.strokes.push(i);
        }
    }
    for it in &page.items {
        if !it.locked && !it.underlay && r.contains(it.rect().center()) && !ed.selection.items.contains(&it.id) {
            ed.selection.items.push(it.id);
        }
    }
    ed.expand_groups(page);
}

fn same_strokes(a: &Page, b: &Page) -> bool {
    a.strokes.len() == b.strokes.len() && a.strokes.iter().zip(b.strokes.iter()).all(|(x, y)| Arc::ptr_eq(&x.points, &y.points))
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

fn ruler_hit(ed: &Editor, cp: Pos2) -> bool {
    let Some(r) = ed.ruler else { return false };
    let local = rot(cp - r.center, -r.angle);
    match r.kind {
        RulerKind::Protractor => local.length() < 150.0 && local.y <= 0.0 && local.length() > 20.0,
        RulerKind::Ruler => local.x.abs() < 350.0 && local.y > -26.0 && local.y < 30.0,
        RulerKind::SetSquare => local.x > 8.0 && local.y < -8.0 && local.x - local.y < 250.0,
    }
}

fn over_ruler(ed: &Editor, rect: Rect, ctx: &egui::Context, m: egui::Modifiers) -> bool {
    let Some(p) = ctx.pointer_hover_pos() else { return false };
    if !rect.contains(p) || ed.ruler.is_none() {
        return false;
    }
    (m.alt || ctx.input(|i| i.multi_touch().is_some())) && ruler_hit(ed, ed.to_canvas(rect, p))
}

// ----------------------------------------------------------------------------
// Rendering

fn draw_pattern(painter: &egui::Painter, page: &Page, view: &Affine, area: Rect, dark: bool, zoom: f32) {
    if !area.is_positive() {
        return;
    }
    let pattern = page.pattern();
    if pattern == Pattern::Plain {
        return;
    }
    let base = page.line_color.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
    let line = base.unwrap_or(if dark { Color32::from_rgba_unmultiplied(120, 150, 200, 50) } else { Color32::from_rgba_unmultiplied(80, 130, 210, 55) });
    let strong = base.map(|c| c.gamma_multiply(1.6)).unwrap_or(if dark { Color32::from_rgba_unmultiplied(120, 150, 200, 95) } else { Color32::from_rgba_unmultiplied(80, 130, 210, 105) });
    let step = if page.pattern_step > 0.0 { page.pattern_step } else { pattern.default_step() };
    let seg = |a: Pos2, b: Pos2, c: Color32| painter.line_segment([view.apply(a), view.apply(b)], EStroke::new(1.0, c));
    let h = |y: f32, c: Color32| seg(pos2(area.min.x, y), pos2(area.max.x, y), c);
    let v = |x: f32, c: Color32| seg(pos2(x, area.min.y), pos2(x, area.max.y), c);
    let range = |a: f32, b: f32, s: f32| ((a / s).floor() as i64, (b / s).ceil() as i64);
    match pattern {
        Pattern::LinesNarrow | Pattern::LinesMedium | Pattern::LinesWide => {
            if step * zoom < 5.0 {
                return;
            }
            let (a, b) = range(area.min.y, area.max.y, step);
            for i in a..=b {
                h(i as f32 * step, line);
            }
            if area.min.x < 60.0 && area.max.x > 60.0 {
                v(60.0, Color32::from_rgba_unmultiplied(220, 80, 80, 90));
            }
        }
        Pattern::Handwriting => {
            if step * zoom < 8.0 {
                return;
            }
            let (a, b) = range(area.min.y, area.max.y, step);
            for i in a..=b {
                let y = i as f32 * step;
                h(y, strong);
                let ym = y - step * 0.45;
                let mut x = (area.min.x / 12.0).floor() * 12.0;
                while x < area.max.x {
                    seg(pos2(x, ym), pos2(x + 6.0, ym), line);
                    x += 12.0;
                }
            }
        }
        Pattern::GridSmall | Pattern::GridMedium | Pattern::GridLarge | Pattern::Millimeter => {
            let (mut s, mut major) = if pattern == Pattern::Millimeter { (step, 10) } else { (step, 0) };
            if s * zoom < 5.0 {
                if major > 0 {
                    s *= major as f32;
                    major = 0;
                } else {
                    return;
                }
            }
            let (a, b) = range(area.min.y, area.max.y, s);
            for i in a..=b {
                h(i as f32 * s, if major > 0 && i % major == 0 { strong } else { line });
            }
            let (a, b) = range(area.min.x, area.max.x, s);
            for j in a..=b {
                v(j as f32 * s, if major > 0 && j % major == 0 { strong } else { line });
            }
        }
        Pattern::DotsSmall | Pattern::DotsLarge => {
            if step * zoom < 6.0 {
                return;
            }
            let r = if pattern == Pattern::DotsLarge { (1.8 * zoom).clamp(1.0, 3.5) } else { (1.2 * zoom).clamp(0.8, 2.5) };
            let (a, b) = range(area.min.y, area.max.y, step);
            let (c, d) = range(area.min.x, area.max.x, step);
            if (b - a) * (d - c) > 40000 {
                return;
            }
            for i in a..=b {
                for j in c..=d {
                    painter.circle_filled(view.apply(pos2(j as f32 * step, i as f32 * step)), r, strong);
                }
            }
        }
        Pattern::Isometric => {
            if step * zoom < 7.0 {
                return;
            }
            let dx = step * 3f32.sqrt() * 0.5;
            let (a, b) = range(area.min.x, area.max.x, dx);
            for j in a..=b {
                v(j as f32 * dx, line);
            }
            let k = 30f32.to_radians().tan();
            let span = area.width() * k;
            let (a, b) = range(area.min.y - span, area.max.y + span, step);
            for i in a..=b {
                let y0 = i as f32 * step;
                seg(pos2(area.min.x, y0), pos2(area.max.x, y0 + area.width() * k), line);
                seg(pos2(area.min.x, y0), pos2(area.max.x, y0 - area.width() * k), line);
            }
        }
        Pattern::Staff => {
            if step * zoom < 3.0 {
                return;
            }
            let gap = step * 7.0;
            let (a, b) = range(area.min.y, area.max.y, gap);
            for i in a..=b {
                for l in 0..5 {
                    h(i as f32 * gap + l as f32 * step, strong);
                }
            }
        }
        Pattern::Plain => {}
    }
}

/// How much of the drawing a replay shows: (complete strokes, points of the next).
fn replay_limit(ed: &Editor, page: &Page, time: f64) -> Option<(usize, usize)> {
    let r = ed.replay.as_ref()?;
    let now = r.paused.unwrap_or(time);
    let mut budget = ((now - r.start) * 140.0 * r.speed as f64).max(0.0) as usize;
    for (i, s) in page.strokes.iter().enumerate() {
        let n = s.points.len().max(1);
        if budget >= n {
            budget -= n;
        } else {
            return Some((i, budget));
        }
    }
    Some((page.strokes.len(), 0))
}

#[allow(clippy::too_many_arguments)]
fn draw_strokes(painter: &egui::Painter, page: &Page, ed: &mut Editor, view: &Affine, visible: Rect, curve: f32, layers: &[u8], dark: bool, limit: Option<(usize, usize)>) {
    let mut mesh = Mesh::default();
    let selected: std::collections::HashSet<usize> = ed.selection.strokes.iter().copied().collect();
    let preview = ed.preview;
    let zoom = ed.zoom;
    let (opts, _) = brush::Opts::for_zoom(zoom, curve, dark);
    for &layer in layers {
        let ls = page.layers[layer as usize];
        if !ls.visible {
            continue;
        }
        for (i, s) in page.strokes.iter().enumerate() {
            if s.layer.min(2) != layer {
                continue;
            }
            if let Some((full, part)) = limit {
                if i > full || (i == full && part < 2) {
                    continue;
                }
                if i == full {
                    let mut p = s.clone();
                    p.points = Arc::new(s.points[..part.min(s.points.len())].to_vec());
                    p.tilt = Arc::new(Vec::new());
                    brush::append(&mut mesh, &brush::tessellate(&p, &opts), view, ls.opacity);
                    continue;
                }
            }
            let moving = selected.contains(&i) && !preview.is_identity();
            let bb = if moving { preview.apply_stroke(s).bbox() } else { s.bbox() };
            if !bb.intersects(visible) {
                continue;
            }
            let m = if moving { Arc::new(brush::tessellate(&preview.apply_stroke(s), &opts)) } else { ed.cache.get(s, zoom, curve, dark) };
            brush::append(&mut mesh, &m, view, ls.opacity);
            if mesh.vertices.len() > 60_000 {
                painter.add(Shape::mesh(std::mem::take(&mut mesh)));
            }
            if selected.contains(&i) && !moving {
                let pts: Vec<Pos2> = [bb.left_top(), bb.right_top(), bb.right_bottom(), bb.left_bottom(), bb.left_top()].iter().map(|p| view.apply(*p)).collect();
                painter.add(Shape::line(pts, EStroke::new(1.0, Color32::from_rgba_unmultiplied(60, 130, 255, 110))));
            }
        }
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

/// Text inside shapes and numbered markers.
fn draw_labels(painter: &egui::Painter, page: &Page, ed: &Editor, view: &Affine, visible: Rect, dark: bool) {
    for s in &page.strokes {
        if s.style.label.is_empty() || !page.layers[(s.layer as usize).min(2)].visible {
            continue;
        }
        let bb = Rect::from_points(&s.points.iter().map(|p| p.pos()).collect::<Vec<_>>());
        if !bb.intersects(visible) {
            continue;
        }
        let c = view.apply(bb.center());
        let (size, col) = if s.style.marker {
            (13.0 * ed.zoom, Color32::WHITE)
        } else {
            let fill_dark = s.fill.is_some_and(|f| (f[0] as u32 + f[1] as u32 + f[2] as u32) < 300 && f[3] > 150);
            (14.0 * ed.zoom, if fill_dark || (dark && s.fill.is_none()) { Color32::from_gray(235) } else { Color32::from_gray(25) })
        };
        let galley = painter.layout(s.style.label.clone(), egui::FontId::proportional(size.max(6.0)), col, (bb.width() * ed.zoom - 8.0).max(20.0));
        painter.add(egui::epaint::TextShape::new(c - galley.size() * 0.5, galley, col).with_angle(ed.rot));
    }
}

fn draw_live(painter: &egui::Painter, ed: &Editor, view: &Affine, curve: f32, dark: bool) {
    let (o, _) = brush::Opts::for_zoom(ed.zoom, curve, dark);
    let Some(live) = &ed.live else {
        if ed.polyline.len() > 1 {
            let s = shape_stroke(ed.polyline.clone(), false, &ed.shape, 1, false, false);
            let mut out = Mesh::default();
            brush::append(&mut out, &brush::tessellate(&s, &o), view, 1.0);
            painter.add(Shape::mesh(out));
        }
        return;
    };
    let blue = Color32::from_rgb(60, 130, 255);
    match live.kind {
        LiveKind::Ink => {
            let mut out = Mesh::default();
            if let Some(parts) = &live.quick {
                let st = ShapeStyle { color: ed.pen.color, width: ed.pen.width, ..ShapeStyle::default() };
                for p in parts {
                    brush::append(&mut out, &brush::tessellate(&shape_stroke(p.pts.clone(), p.closed, &st, 1, false, false), &o), view, 1.0);
                }
            } else {
                let mut s = Stroke::new(live.points.clone(), ed.pen.color, ed.pen.width, ed.pen.brush());
                s.tilt = Arc::new(if live.tilt.len() == live.points.len() { live.tilt.clone() } else { Vec::new() });
                s.style = Arc::new(StrokeStyle { effect: ed.pen.effect, no_pressure: !ed.pen.pressure, ..Default::default() });
                s.id = 7;
                brush::append(&mut out, &brush::tessellate(&s, &o), view, 1.0);
            }
            painter.add(Shape::mesh(out));
        }
        LiveKind::Lasso => {
            let pts: Vec<Pos2> = live.points.iter().map(|p| view.apply(p.pos())).collect();
            painter.add(Shape::dashed_line(&pts, EStroke::new(1.5, blue), 6.0, 4.0));
        }
        LiveKind::Rect | LiveKind::Redact => {
            let a = live.points[0].pos();
            let b = live.points.last().unwrap().pos();
            let pts: Vec<Pos2> = [a, pos2(b.x, a.y), b, pos2(a.x, b.y), a].iter().map(|p| view.apply(*p)).collect();
            if live.kind == LiveKind::Redact {
                painter.add(Shape::convex_polygon(pts[..4].to_vec(), Color32::from_black_alpha(160), EStroke::NONE));
            } else {
                painter.add(Shape::dashed_line(&pts, EStroke::new(1.2, blue), 5.0, 3.0));
            }
        }
        LiveKind::Shape(k) => {
            if live.points.len() > 1 {
                let a = live.points[0].pos();
                let mut b = live.points.last().unwrap().pos();
                if live.add {
                    b = shapes::constrain(k, a, b);
                }
                let mut out = Mesh::default();
                for p in shapes::build(k, a, b) {
                    let s = shape_stroke(p.pts, p.closed, &ed.shape, 1, p.arrow_start, p.arrow_end);
                    brush::append(&mut out, &brush::tessellate(&s, &o), view, 1.0);
                }
                painter.add(Shape::mesh(out));
            }
        }
        _ => {}
    }
}

fn draw_laser(painter: &egui::Painter, ed: &mut Editor, view: &Affine, time: f64) {
    const LIFE: f64 = 1.6;
    let drawing = ed.live.as_ref().is_some_and(|l| l.kind == LiveKind::Laser);
    let n = ed.laser.len();
    let mut keep = Vec::with_capacity(n);
    for (i, t) in ed.laser.drain(..).enumerate() {
        if (drawing && i + 1 == n) || t.last().is_some_and(|p| time - p.1 < LIFE) {
            keep.push(t);
        }
    }
    ed.laser = keep;
    for trail in &ed.laser {
        for w in trail.windows(2) {
            let age = (time - w[1].1) / LIFE;
            if age >= 1.0 {
                continue;
            }
            let a = (1.0 - age) as f32;
            let (p, q) = (view.apply(w[0].0), view.apply(w[1].0));
            painter.line_segment([p, q], EStroke::new(9.0, Color32::from_rgba_unmultiplied(255, 40, 40, (60.0 * a) as u8)));
            painter.line_segment([p, q], EStroke::new(3.5, Color32::from_rgba_unmultiplied(255, 60, 60, (230.0 * a) as u8)));
        }
        if let Some(last) = trail.last() {
            if time - last.1 < 0.3 {
                let p = view.apply(last.0);
                painter.circle_filled(p, 9.0, Color32::from_rgba_unmultiplied(255, 0, 0, 70));
                painter.circle_filled(p, 4.5, Color32::from_rgb(255, 40, 40));
            }
        }
    }
}

fn draw_tool_overlays(painter: &egui::Painter, ed: &Editor, view: &Affine, ctx: &egui::Context) {
    let col = Color32::from_rgb(220, 90, 30);
    let font = egui::FontId::proportional(12.0);
    let mm = crate::settings::MM;
    if let Some(c) = &ed.compass {
        let center = view.apply(c.center);
        painter.circle_filled(center, 3.0, col);
        if let Some(a0) = c.start {
            let n = 64;
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let a = a0 + c.sweep * i as f32 / n as f32;
                    view.apply(c.center + vec2(a.cos(), a.sin()) * c.radius)
                })
                .collect();
            painter.add(Shape::line(pts, EStroke::new(2.0, col)));
        } else {
            painter.circle_stroke(center, c.radius * ed.zoom, EStroke::new(1.0, col.gamma_multiply(0.5)));
        }
        if let Some(p) = ctx.pointer_interact_pos() {
            painter.line_segment([center, p], EStroke::new(1.0, col));
            let hint = if c.start.is_none() { "  · Shift — начать дугу" } else { "" };
            painter.text(p + vec2(10.0, 10.0), egui::Align2::LEFT_TOP, format!("r = {:.1} мм{hint}", c.radius / mm), font.clone(), col);
        }
    }
    if let Some((a, b)) = ed.measure {
        let (pa, pb) = (view.apply(a), view.apply(b));
        painter.line_segment([pa, pb], EStroke::new(1.5, col));
        painter.circle_filled(pa, 3.0, col);
        painter.circle_filled(pb, 3.0, col);
        let d = (b - a).length();
        let len = d / mm;
        let ang = (-(b - a).y).atan2((b - a).x).to_degrees();
        let label = if len >= 10.0 { format!("{:.2} см · {:.0} px · {:.1}°", len / 10.0, d, ang) } else { format!("{:.1} мм · {:.0} px · {:.1}°", len, d, ang) };
        painter.text(pa + (pb - pa) * 0.5 + vec2(8.0, -16.0), egui::Align2::LEFT_BOTTOM, label, font.clone(), col);
    }
    if let (Some(r), Some(live)) = (ed.ruler, &ed.live) {
        if r.kind != RulerKind::Protractor && live.kind == LiveKind::Ink && live.points.len() > 1 {
            let a = live.points[0].pos();
            let b = live.points.last().unwrap().pos();
            let ang = (-(b - a).y).atan2((b - a).x).to_degrees();
            painter.text(view.apply(b) + vec2(12.0, 12.0), egui::Align2::LEFT_TOP, format!("{:.1} мм · {:.0}°", (b - a).length() / mm, ang), font, col);
        }
    }
}

fn cursor_preview(painter: &egui::Painter, ed: &Editor, ctx: &egui::Context, rect: Rect, is_mouse: bool, hovered: bool, pan_mode: bool) {
    if let Some((p, eraser)) = ed.hover {
        if rect.contains(p) && ed.live.is_none() {
            if eraser || ed.tool == Tool::Eraser {
                painter.circle_stroke(p, ed.eraser_radius, EStroke::new(1.0, Color32::from_gray(120)));
            } else if ed.tool == Tool::Draw {
                let r = (ed.pen.width * 0.5 * ed.zoom).max(1.5);
                painter.circle_stroke(p, r, EStroke::new(1.0, color32(ed.pen.color)));
            } else if ed.tool == Tool::Laser {
                painter.circle_filled(p, 4.0, Color32::from_rgb(255, 40, 40));
            }
        }
    }
    if ed.tool == Tool::Eraser && is_mouse {
        if let Some(p) = ctx.pointer_hover_pos() {
            if rect.contains(p) {
                painter.circle_stroke(p, ed.eraser_radius, EStroke::new(1.0, Color32::from_gray(120)));
            }
        }
    }
    if hovered {
        if pan_mode {
            ctx.set_cursor_icon(CursorIcon::Grab);
        } else if ed.tool == Tool::Laser && is_mouse {
            ctx.set_cursor_icon(CursorIcon::None);
            if let Some(p) = ctx.pointer_hover_pos() {
                painter.circle_filled(p, 9.0, Color32::from_rgba_unmultiplied(255, 0, 0, 60));
                painter.circle_filled(p, 4.0, Color32::from_rgb(255, 40, 40));
            }
        } else if matches!(ed.tool, Tool::Eyedropper | Tool::Fill) {
            ctx.set_cursor_icon(CursorIcon::Cell);
        } else if ed.tool.is_ink() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        }
    }
}

fn draw_ruler(painter: &egui::Painter, ed: &Editor, view: &Affine) {
    let Some(r) = ed.ruler else { return };
    let tc = |v: Vec2| view.apply(r.center + rot(v, r.angle));
    let fill = Color32::from_rgba_unmultiplied(200, 200, 215, 115);
    let edge = Color32::from_rgba_unmultiplied(40, 40, 60, 210);
    let small = egui::FontId::proportional((10.0 * ed.zoom).clamp(7.0, 14.0));
    let mm = crate::settings::MM;
    match r.kind {
        RulerKind::Protractor => {
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
                    painter.text(tc(d * 118.0), egui::Align2::CENTER_CENTER, format!("{deg}°"), small.clone(), edge);
                }
            }
            painter.circle_filled(tc(Vec2::ZERO), 3.0, edge);
        }
        RulerKind::Ruler => {
            let corners = [tc(vec2(-350.0, -30.0)), tc(vec2(350.0, -30.0)), tc(vec2(350.0, 30.0)), tc(vec2(-350.0, 30.0))];
            painter.add(Shape::convex_polygon(corners.to_vec(), fill, EStroke::new(1.0, edge)));
            let n = (700.0 / mm) as i32;
            for i in 0..=n {
                if ed.zoom < 0.6 && i % 5 != 0 {
                    continue;
                }
                let x = -350.0 + i as f32 * mm;
                let len = if i % 10 == 0 { 16.0 } else if i % 5 == 0 { 10.0 } else { 5.0 };
                painter.line_segment([tc(vec2(x, -30.0)), tc(vec2(x, -30.0 + len))], EStroke::new(1.0, edge));
                if i % 10 == 0 && ed.zoom > 0.5 {
                    painter.text(tc(vec2(x, -6.0)), egui::Align2::CENTER_CENTER, format!("{}", i / 10), small.clone(), edge);
                }
            }
            let deg = (-r.angle.to_degrees()).rem_euclid(360.0);
            painter.text(tc(vec2(0.0, 16.0)), egui::Align2::CENTER_CENTER, format!("{deg:.0}°  ·  Alt+колесо — поворот"), small, edge);
        }
        RulerKind::SetSquare => {
            let leg = 260.0;
            painter.add(Shape::convex_polygon(vec![tc(vec2(0.0, 0.0)), tc(vec2(leg, 0.0)), tc(vec2(0.0, -leg))], fill, EStroke::new(1.0, edge)));
            for i in 0..=((leg / mm) as i32) {
                let x = i as f32 * mm;
                let len = if i % 10 == 0 { 12.0 } else if i % 5 == 0 { 8.0 } else { 4.0 };
                painter.line_segment([tc(vec2(x, 0.0)), tc(vec2(x, -len))], EStroke::new(1.0, edge));
                painter.line_segment([tc(vec2(0.0, -x)), tc(vec2(len, -x))], EStroke::new(1.0, edge));
            }
            let deg = (-r.angle.to_degrees()).rem_euclid(360.0);
            painter.text(tc(vec2(70.0, -70.0)), egui::Align2::CENTER_CENTER, format!("90° · 45°\n{deg:.0}°"), small, edge);
        }
    }
}

/// Non-interactive drawing of a container (rotated views and underlays).
fn static_item(painter: &egui::Painter, it: &Item, view: &Affine, angle: f32, env: &mut Env, dark: bool) {
    let z = view.scale();
    let p = view.apply(it.pos2());
    match &it.kind {
        ItemKind::Text(tb) => {
            let none = |_: &str| false;
            let cx = crate::richtext::Ctx { dark, zoom: z, edit: false, highlights: &[], misspelled: &none, detect: false };
            let l = crate::richtext::layout(tb, &cx, it.width * z);
            let galley = painter.layout_job(l.job);
            if let Some(bg) = tb.bg {
                let r = Rect::from_min_size(it.pos2(), vec2(it.width, galley.size().y / z.max(0.01) + 8.0));
                let pts: Vec<Pos2> = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].iter().map(|q| view.apply(*q)).collect();
                painter.add(Shape::convex_polygon(pts, color32(bg), EStroke::NONE));
            }
            painter.add(egui::epaint::TextShape::new(p, galley, Color32::GRAY).with_angle(angle));
        }
        ItemKind::Image(img) => {
            let path = env.nb.asset_path(&img.asset);
            if let crate::textures::Tex::Ready(h, size) = env.textures.get(path, img.rotation, img.flip_h, img.flip_v) {
                let crop = Rect::from_min_max(pos2(img.crop[0], img.crop[1]), pos2(img.crop[2], img.crop[3]));
                let aspect = (crop.height() * size[1] as f32) / (crop.width() * size[0] as f32).max(1e-3);
                let r = Rect::from_min_size(it.pos2(), vec2(it.width, it.width * aspect));
                let mut m = Mesh::with_texture(h.id());
                for (c, uv) in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].iter().zip([crop.left_top(), crop.right_top(), crop.right_bottom(), crop.left_bottom()]) {
                    m.vertices.push(egui::epaint::Vertex { pos: view.apply(*c), uv, color: Color32::WHITE });
                }
                m.add_triangle(0, 1, 2);
                m.add_triangle(0, 2, 3);
                painter.add(Shape::mesh(m));
            }
        }
        _ => {
            let r = it.rect();
            let pts: Vec<Pos2> = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()].iter().map(|q| view.apply(*q)).collect();
            painter.add(Shape::line(pts, EStroke::new(1.0, Color32::from_gray(150))));
            let text: String = it.plain_text().chars().take(200).collect();
            let g = painter.layout(text, egui::FontId::proportional(12.0 * z), if dark { Color32::from_gray(220) } else { Color32::from_gray(40) }, it.width * z);
            painter.add(egui::epaint::TextShape::new(p, g, Color32::GRAY).with_angle(angle));
        }
    }
}

fn minimap(ui: &mut egui::Ui, page: &Page, ed: &mut Editor, rect: Rect) {
    let content = page.content_bounds();
    let vis = ed.visible(rect);
    if !content.is_positive() || vis.contains_rect(content) {
        return;
    }
    let world = content.union(vis).expand(40.0);
    let size = vec2(180.0, 130.0);
    let mr = Rect::from_min_size(rect.right_bottom() - size - vec2(14.0, 14.0), size);
    let k = (mr.width() / world.width()).min(mr.height() / world.height());
    let off = mr.center() - world.center().to_vec2() * k;
    let map = |p: Pos2| pos2(p.x * k + off.x, p.y * k + off.y);
    let painter = ui.painter_at(mr);
    painter.rect_filled(mr, 6.0, Color32::from_rgba_unmultiplied(120, 120, 130, 60));
    for it in &page.items {
        let r = it.rect();
        painter.rect_filled(Rect::from_min_max(map(r.min), map(r.max)), 1.0, Color32::from_rgba_unmultiplied(110, 140, 200, 120));
    }
    for s in &page.strokes {
        let step = (s.points.len() / 16).max(1);
        let pts: Vec<Pos2> = s.points.iter().step_by(step).map(|p| map(p.pos())).collect();
        if pts.len() > 1 {
            painter.add(Shape::line(pts, EStroke::new(1.0, Color32::from_gray(90))));
        }
    }
    painter.rect_stroke(Rect::from_min_max(map(vis.min), map(vis.max)), 2.0, EStroke::new(1.5, Color32::from_rgb(60, 130, 255)), egui::StrokeKind::Inside);
    let resp = ui.interact(mr, egui::Id::new(("minimap", ed.page_id)), Sense::click_and_drag());
    if resp.clicked() || resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let target = pos2((p.x - off.x) / k, (p.y - off.y) / k);
            let c = ed.view_center();
            ed.offset += target - c;
        }
    }
}

fn replay_bar(ui: &mut egui::Ui, ed: &mut Editor, page: &Page, rect: Rect, env: &mut Env, time: f64) {
    let bar = Rect::from_min_size(rect.center_top() + vec2(-210.0, 10.0), vec2(420.0, 34.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let mut stop = false;
    egui::Frame::popup(ui.style()).inner_margin(4.0).show(&mut child, |ui| {
        ui.horizontal(|ui| {
            let Some(r) = ed.replay.as_mut() else { return };
            match r.paused {
                Some(p) => {
                    if ui.button("▶").clicked() {
                        r.start += time - p;
                        r.paused = None;
                    }
                }
                None => {
                    if ui.button("⏸").clicked() {
                        r.paused = Some(time);
                    }
                }
            }
            if ui.button("⏮").clicked() {
                r.start = time;
                if r.paused.is_some() {
                    r.paused = Some(time);
                }
            }
            ui.add(egui::Slider::new(&mut r.speed, 0.25..=8.0).logarithmic(true).text("скорость"));
            if ui.button("GIF…").clicked() {
                env.actions.push(Action::ExportSelection("gif"));
            }
            if ui.button("⏹").clicked() {
                stop = true;
            }
        });
    });
    let total: usize = page.strokes.iter().map(|s| s.points.len().max(1)).sum();
    let finished = ed.replay.as_ref().is_some_and(|r| r.paused.is_none() && (time - r.start) * 140.0 * r.speed as f64 > total as f64 + 280.0);
    if stop || finished {
        ed.replay = None;
    }
}

fn guides_ui(ui: &mut egui::Ui, page: &mut Page, ed: &mut Editor, rect: Rect, view: &Affine, ctx: &egui::Context) -> bool {
    if page.guides.is_empty() {
        return false;
    }
    let painter = ui.painter_at(rect);
    let vis = ed.visible(rect);
    let col = Color32::from_rgb(0, 190, 220);
    let mut changed = false;
    let mut remove = None;
    let tool_free = !ed.tool.is_ink() || ed.tool == Tool::Pan;
    let inv = view.inverse();
    for (i, g) in page.guides.iter_mut().enumerate() {
        let (a, b) = if g.vertical { (pos2(g.at, vis.min.y), pos2(g.at, vis.max.y)) } else { (pos2(vis.min.x, g.at), pos2(vis.max.x, g.at)) };
        let (sa, sb) = (view.apply(a), view.apply(b));
        painter.add(Shape::dashed_line(&[sa, sb], EStroke::new(1.0, col), 8.0, 4.0));
        if !tool_free || ed.rotated() {
            continue;
        }
        let hit = Rect::from_two_pos(sa, sb).expand(4.0).intersect(rect);
        let r = ui.interact(hit, egui::Id::new(("guide", i, ed.page_id)), Sense::click_and_drag());
        if r.hovered() {
            ctx.set_cursor_icon(if g.vertical { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical });
        }
        if r.dragged() {
            let d = r.drag_delta();
            let dc = vec2(inv.m[0] * d.x + inv.m[1] * d.y, inv.m[2] * d.x + inv.m[3] * d.y);
            g.at += if g.vertical { dc.x } else { dc.y };
            changed = true;
        }
        r.context_menu(|ui| {
            if ui.button("Удалить направляющую").clicked() {
                remove = Some(i);
                ui.close();
            }
        });
    }
    if let Some(i) = remove {
        page.guides.remove(i);
        changed = true;
    }
    changed
}

fn canvas_menu(ui: &mut egui::Ui, page: &mut Page, ed: &mut Editor, env: &mut Env, rect: Rect, changed: &mut bool) {
    let at = ui.ctx().pointer_interact_pos().map(|p| ed.to_canvas(rect, p));
    if ui.button("Вставить").clicked() {
        if !env.clip.items.is_empty() || !env.clip.strokes.is_empty() {
            let clip = env.clip.clone();
            ed.paste(page, &clip, at);
            *changed = true;
        }
        ui.close();
    }
    if ui.button("Выделить всё   Ctrl+A").clicked() {
        ed.select_all(page);
        ui.close();
    }
    if !page.strokes.is_empty() {
        ui.separator();
        if ui.button("Рукопись в текст").clicked() {
            env.actions.push(Action::OcrInk);
            ui.close();
        }
        if ui.button("Воспроизвести рисование").clicked() {
            ed.replay = Some(Replay { start: ui.ctx().input(|i| i.time), speed: 1.0, paused: None });
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Горизонтальная направляющая").clicked() {
        if let Some(p) = at {
            page.guides.push(Guide { vertical: false, at: p.y });
            *changed = true;
        }
        ui.close();
    }
    if ui.button("Вертикальная направляющая").clicked() {
        if let Some(p) = at {
            page.guides.push(Guide { vertical: true, at: p.x });
            *changed = true;
        }
        ui.close();
    }
}

/// Three-finger tap = undo, four-finger tap = redo.
#[allow(clippy::too_many_arguments)]
fn track_gesture(ed: &mut Editor, page: &mut Page, env: &Env, id: u64, phase: egui::TouchPhase, pos: Pos2, time: f64, changed: &mut bool) {
    if !env.settings.touch_gestures {
        return;
    }
    match phase {
        egui::TouchPhase::Start => {
            if ed.touches.is_empty() {
                ed.touch_peak = 0;
                ed.touch_start = time;
            }
            ed.touches.insert(id, (pos, time, false));
            ed.touch_peak = ed.touch_peak.max(ed.touches.len());
        }
        egui::TouchPhase::Move => {
            if let Some(t) = ed.touches.get_mut(&id) {
                if (pos - t.0).length() > 25.0 {
                    t.2 = true;
                }
            }
        }
        egui::TouchPhase::End | egui::TouchPhase::Cancel => {
            let moved = ed.touches.get(&id).is_some_and(|t| t.2);
            ed.touches.remove(&id);
            if moved {
                ed.touch_peak = 0;
            }
            if ed.touches.is_empty() && time - ed.touch_start < 0.4 {
                if ed.touch_peak == 3 {
                    *changed |= ed.undo(page);
                    ed.live = None;
                } else if ed.touch_peak == 4 {
                    *changed |= ed.redo(page);
                    ed.live = None;
                }
                ed.touch_peak = 0;
            }
        }
    }
}

fn selection_handle_hit(ed: &Editor, page: &Page, view: &Affine, p: Pos2) -> bool {
    if ed.selection.is_empty() {
        return false;
    }
    let b = ed.selection_bounds(page);
    let corners = [b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom()].map(|c| view.apply(c));
    Rect::from_points(&corners).expand(14.0).contains(p) || (view.apply(b.center_top()) - p).length() < 40.0
}

fn selection_ui(ui: &mut egui::Ui, page: &mut Page, ed: &mut Editor, rect: Rect, view: &Affine, env: &mut Env) -> bool {
    let mut changed = false;
    ed.selection.strokes.retain(|&i| i < page.strokes.len());
    let b = ed.selection_bounds(page);
    if !b.is_positive() {
        ed.selection = Selection::default();
        return false;
    }
    let ctx = ui.ctx().clone();
    let blue = Color32::from_rgb(60, 130, 255);
    let painter = ui.painter_at(rect);
    let show = if ed.preview.angle == 0.0 { Rect::from_two_pos(ed.preview.apply(b.min), ed.preview.apply(b.max)) } else { b };
    let corners_c = [show.left_top(), show.right_top(), show.right_bottom(), show.left_bottom()];
    let corners: Vec<Pos2> = corners_c.iter().map(|c| view.apply(*c)).collect();
    let mut outline = corners.clone();
    outline.push(corners[0]);
    painter.add(Shape::line(outline, EStroke::new(1.0, blue)));
    let id = egui::Id::new(("sel", ed.page_id));
    let body_rect = Rect::from_points(&corners);
    let body = ui.interact(body_rect, id.with("body"), Sense::drag());
    if body.drag_started() {
        if let Some(p) = body.interact_pointer_pos() {
            ed.drag = Some((DragOp::Move, ed.to_canvas(rect, p)));
            ed.preview = Xform::default();
        }
    }
    if body.hovered() {
        ctx.set_cursor_icon(CursorIcon::Move);
    }
    for (k, c) in corners.iter().enumerate() {
        let hr = Rect::from_center_size(*c, vec2(12.0, 12.0));
        painter.rect_filled(hr, 2.0, Color32::WHITE);
        painter.rect_stroke(hr, 2.0, EStroke::new(1.0, blue), egui::StrokeKind::Inside);
        let h = ui.interact(hr.expand(4.0), id.with(("corner", k)), Sense::drag());
        if h.hovered() {
            ctx.set_cursor_icon(if k % 2 == 0 { CursorIcon::ResizeNwSe } else { CursorIcon::ResizeNeSw });
        }
        if h.drag_started() {
            ed.drag = Some((DragOp::Scale { anchor: corners_c[(k + 2) % 4], start: corners_c[k] }, corners_c[k]));
            ed.preview = Xform::default();
        }
    }
    let top = view.apply(show.center_top());
    let up = (top - view.apply(show.center())).normalized();
    let rh = top + if up.is_finite() { up * 26.0 } else { vec2(0.0, -26.0) };
    painter.line_segment([top, rh], EStroke::new(1.0, blue));
    painter.circle(rh, 6.0, Color32::WHITE, EStroke::new(1.0, blue));
    let rr = ui.interact(Rect::from_center_size(rh, vec2(18.0, 18.0)), id.with("rot"), Sense::drag());
    if rr.hovered() {
        ctx.set_cursor_icon(CursorIcon::Alias);
    }
    if rr.drag_started() {
        if let Some(p) = rr.interact_pointer_pos() {
            let center = b.center();
            let cp = ed.to_canvas(rect, p);
            ed.drag = Some((DragOp::Rotate { center, start_angle: (cp - center).angle() }, cp));
            ed.preview = Xform::default();
        }
    }
    if let Some((op, start)) = ed.drag {
        let shift = ctx.input(|i| i.modifiers.shift);
        if let Some(p) = ctx.pointer_interact_pos() {
            let cp = ed.to_canvas(rect, p);
            ed.preview = match op {
                DragOp::Move => {
                    let mut d = cp - start;
                    let moved = b.min + d;
                    d += snap_point(moved, page, ed, env.settings) - moved;
                    Xform { offset: d, ..Default::default() }
                }
                DragOp::Scale { anchor, start } => {
                    let d0 = start - anchor;
                    let d1 = cp - anchor;
                    let scale = if shift {
                        let k = if d0.length() > 1e-3 { d1.length() / d0.length() } else { 1.0 };
                        Vec2::splat(k.clamp(0.05, 20.0))
                    } else {
                        let sx = if d0.x.abs() > 1e-3 { d1.x / d0.x } else { 1.0 };
                        let sy = if d0.y.abs() > 1e-3 { d1.y / d0.y } else { 1.0 };
                        vec2(sx.clamp(-20.0, 20.0), sy.clamp(-20.0, 20.0))
                    };
                    Xform { pivot: anchor, scale, ..Default::default() }
                }
                DragOp::Rotate { center, start_angle } => {
                    let mut a = (cp - center).angle() - start_angle;
                    if shift {
                        let step = 15f32.to_radians();
                        a = (a / step).round() * step;
                    }
                    Xform { pivot: center, angle: a, ..Default::default() }
                }
            };
            if matches!(op, DragOp::Rotate { .. }) {
                let pts: Vec<Pos2> = [b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom(), b.left_top()].iter().map(|q| view.apply(ed.preview.apply(*q))).collect();
                painter.add(Shape::dashed_line(&pts, EStroke::new(1.0, blue), 5.0, 3.0));
            }
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            let x = ed.preview;
            if !x.is_identity() {
                ed.snapshot(page);
                apply_xform(page, ed, &x);
                changed = true;
            }
            ed.drag = None;
            ed.preview = Xform::default();
        }
    }
    if ed.drag.is_none() {
        changed |= crate::selection::bar(ui, page, ed, env, body_rect, rect);
    }
    changed
}

/// Applies a transform to the selection (strokes, items and their annotations).
pub fn apply_xform(page: &mut Page, ed: &Editor, x: &Xform) {
    for &i in &ed.selection.strokes {
        if let Some(s) = page.strokes.get_mut(i) {
            if !s.locked {
                *s = x.apply_stroke(s);
            }
        }
    }
    let ids = ed.selection.items.clone();
    let mut moved: Vec<(Id, Vec2)> = Vec::new();
    for it in page.items.iter_mut().filter(|it| ids.contains(&it.id) && !it.locked) {
        let old = it.pos2();
        let np = x.apply(old);
        let k = x.scale.x.abs();
        it.pos = [np.x, np.y];
        if k != 1.0 {
            it.width *= k;
            if let ItemKind::Text(t) = &mut it.kind {
                t.size = (t.size * k).clamp(4.0, 200.0);
            }
        }
        moved.push((it.id, np - old));
    }
    let sel: std::collections::HashSet<usize> = ed.selection.strokes.iter().copied().collect();
    for (id, d) in moved {
        let t = Xform { offset: d, ..Default::default() };
        for (i, s) in page.strokes.iter_mut().enumerate() {
            if s.anchor == Some(id) && !sel.contains(&i) {
                *s = t.apply_stroke(s);
            }
        }
    }
    update_connectors(page);
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
            let (o, z, r, m) = cap.saved_view;
            ed.offset = o;
            ed.zoom = z;
            ed.rot = r;
            ed.mirror = m;
            return true;
        }
    }
    if cap.idx < cap.tiles.len() {
        let t = cap.tiles[cap.idx];
        ed.offset = t.to_vec2();
        ed.zoom = cap.zoom;
        let cap = ed.capture.as_mut().unwrap();
        cap.waiting += 1;
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

pub type Highlights = HashMap<Id, Vec<(usize, usize)>>;
