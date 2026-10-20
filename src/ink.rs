//! Ink geometry: smoothing, simplification, pressure-aware tessellation,
//! erasing, hit testing, transforms and freehand shape recognition.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Mesh, Pos2, Rect, Vec2, pos2, vec2};

use crate::model::{InkKind, InkPoint, Stroke};

/// Width multiplier from pressure.
pub fn pressure_width(base: f32, p: f32, curve: f32, kind: InkKind) -> f32 {
    match kind {
        InkKind::Pen => base * (0.2 + 0.8 * p.clamp(0.0, 1.0).powf(curve.max(0.1))),
        _ => base,
    }
}

/// Ramer–Douglas–Peucker simplification keeping pressure.
pub fn simplify(points: &[InkPoint], eps: f32) -> Vec<InkPoint> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (points[a].pos(), points[b].pos());
        let mut best = (0.0f32, 0usize);
        for i in a + 1..b {
            let mut d = seg_dist(points[i].pos(), pa, pb);
            // Preserve pressure changes as well as shape.
            let t = (i - a) as f32 / (b - a) as f32;
            let pi = points[a].2 + (points[b].2 - points[a].2) * t;
            d = d.max((points[i].2 - pi).abs() * 6.0);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > eps {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

pub fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-9 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Catmull–Rom densification so simplified strokes stay smooth.
fn densify(points: &[InkPoint], step: f32) -> Vec<InkPoint> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    let n = points.len();
    for i in 0..n - 1 {
        let p0 = points[i.saturating_sub(1)];
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = points[(i + 2).min(n - 1)];
        let len = (p2.pos() - p1.pos()).length();
        let segs = ((len / step).ceil() as usize).clamp(1, 16);
        for s in 0..segs {
            let t = s as f32 / segs as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let cr = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push(InkPoint(
                cr(p0.0, p1.0, p2.0, p3.0),
                cr(p0.1, p1.1, p2.1, p3.1),
                p1.2 + (p2.2 - p1.2) * t,
            ));
        }
    }
    out.push(points[n - 1]);
    out
}

/// Tessellates a stroke into a triangle mesh in canvas coordinates.
pub fn tessellate(stroke: &Stroke, curve: f32) -> Mesh {
    let mut mesh = Mesh::default();
    let color = stroke.color32();
    if let Some(fill) = stroke.fill {
        let fill = Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]);
        fill_polygon(&mut mesh, &stroke.points, fill);
    }
    let pts = if stroke.kind == InkKind::Shape {
        stroke.points.to_vec()
    } else {
        densify(&stroke.points, 2.0)
    };
    let mut pts = pts;
    if stroke.closed && pts.len() > 2 {
        pts.push(pts[0]);
        pts.push(pts[1]);
    }
    let pts = &pts;
    if pts.is_empty() {
        return mesh;
    }
    let widths: Vec<f32> = pts
        .iter()
        .map(|p| pressure_width(stroke.width, p.2, curve, stroke.kind) * 0.5)
        .collect();
    if pts.len() == 1 || (pts.len() == 2 && (pts[0].pos() - pts[1].pos()).length() < 0.01) {
        circle(&mut mesh, pts[0].pos(), widths[0], color);
        return mesh;
    }
    // Strip with averaged normals; round caps for pens.
    let n = pts.len();
    let base = mesh.vertices.len() as u32;
    for i in 0..n {
        let prev = pts[i.saturating_sub(1)].pos();
        let next = pts[(i + 1).min(n - 1)].pos();
        let mut dir = next - prev;
        if dir.length_sq() < 1e-12 {
            dir = vec2(1.0, 0.0);
        }
        let dir = dir.normalized();
        let normal = vec2(-dir.y, dir.x);
        let p = pts[i].pos();
        let w = widths[i];
        mesh.colored_vertex(p + normal * w, color);
        mesh.colored_vertex(p - normal * w, color);
    }
    for i in 0..(n as u32 - 1) {
        let a = base + i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    if stroke.kind != InkKind::Highlighter && !stroke.closed {
        half_circle(&mut mesh, pts[0].pos(), pts[0].pos() - pts[1].pos(), widths[0], color);
        half_circle(&mut mesh, pts[n - 1].pos(), pts[n - 1].pos() - pts[n - 2].pos(), widths[n - 1], color);
    }
    // Sharp corners in shapes need joins.
    if stroke.kind == InkKind::Shape {
        for (i, p) in pts.iter().enumerate().skip(1).take(n.saturating_sub(2)) {
            circle(&mut mesh, p.pos(), widths[i], color);
        }
    }
    mesh
}

fn circle(mesh: &mut Mesh, c: Pos2, r: f32, color: Color32) {
    let segs = ((r * 2.0) as u32).clamp(8, 32);
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(c, color);
    for i in 0..=segs {
        let a = i as f32 / segs as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r, color);
    }
    for i in 0..segs {
        mesh.add_triangle(base, base + 1 + i, base + 2 + i);
    }
}

fn half_circle(mesh: &mut Mesh, c: Pos2, outward: Vec2, r: f32, color: Color32) {
    if outward.length_sq() < 1e-12 {
        return;
    }
    let d = outward.normalized();
    let start = d.y.atan2(d.x) - std::f32::consts::FRAC_PI_2;
    let segs = ((r * 1.5) as u32).clamp(4, 16);
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(c, color);
    for i in 0..=segs {
        let a = start + i as f32 / segs as f32 * std::f32::consts::PI;
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r, color);
    }
    for i in 0..segs {
        mesh.add_triangle(base, base + 1 + i, base + 2 + i);
    }
}

/// Ear-free fan fill (fine for convex and mildly concave shapes).
fn fill_polygon(mesh: &mut Mesh, pts: &[InkPoint], color: Color32) {
    if pts.len() < 3 {
        return;
    }
    let c = pts.iter().fold(Vec2::ZERO, |a, p| a + p.pos().to_vec2()) / pts.len() as f32;
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(c.to_pos2(), color);
    for p in pts {
        mesh.colored_vertex(p.pos(), color);
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(base, base + 1 + i, base + 1 + (i + 1) % n);
    }
}

/// Appends `src` transformed by `scale` and `offset` into `dst`.
pub fn append_transformed(dst: &mut Mesh, src: &Mesh, scale: f32, offset: Vec2) {
    let base = dst.vertices.len() as u32;
    dst.vertices.reserve(src.vertices.len());
    for v in &src.vertices {
        let mut v = *v;
        v.pos = pos2(v.pos.x * scale + offset.x, v.pos.y * scale + offset.y);
        dst.vertices.push(v);
    }
    dst.indices.extend(src.indices.iter().map(|i| i + base));
}

/// Mesh cache keyed by the stroke point buffer, so unchanged strokes are
/// tessellated once and only transformed on pan/zoom.
#[derive(Default)]
pub struct MeshCache {
    map: HashMap<(usize, [u8; 4], u32, u32), (Arc<Mesh>, Rect, u64)>,
    frame: u64,
}

impl MeshCache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.frame % 600 == 0 {
            let f = self.frame;
            self.map.retain(|_, v| f - v.2 < 600);
        }
    }
    pub fn get(&mut self, s: &Stroke, curve: f32) -> (Arc<Mesh>, Rect) {
        let key = (
            Arc::as_ptr(&s.points) as usize,
            s.color,
            s.width.to_bits() ^ (s.closed as u32) ^ ((s.fill.is_some() as u32) << 1),
            curve.to_bits(),
        );
        let frame = self.frame;
        let e = self.map.entry(key).or_insert_with(|| {
            let m = tessellate(s, curve);
            let r = s.bbox();
            (Arc::new(m), r, frame)
        });
        e.2 = frame;
        (e.0.clone(), e.1)
    }
}

// ----------------------------------------------------------------------------
// Erasing and hit testing

pub fn stroke_hits_circle(s: &Stroke, c: Pos2, r: f32) -> bool {
    if !s.bbox().expand(r).contains(c) {
        return false;
    }
    let rr = r + s.width * 0.5;
    if s.points.len() == 1 {
        return (s.points[0].pos() - c).length() <= rr;
    }
    if s.fill.is_some() && point_in_polygon(c, &s.points.iter().map(|p| p.pos()).collect::<Vec<_>>()) {
        return true;
    }
    let mut it = s.points.windows(2);
    let hit = it.any(|w| seg_dist(c, w[0].pos(), w[1].pos()) <= rr);
    hit || (s.closed && s.points.len() > 2 && seg_dist(c, s.points[s.points.len() - 1].pos(), s.points[0].pos()) <= rr)
}

/// Point eraser: removes the parts of the stroke inside the circle, splitting it.
pub fn erase_points(s: &Stroke, c: Pos2, r: f32) -> Option<Vec<Stroke>> {
    if !stroke_hits_circle(s, c, r) {
        return None;
    }
    let mut pieces = Vec::new();
    let mut cur: Vec<InkPoint> = Vec::new();
    let pts = if s.points.len() > 1 { densify_linear(&s.points, (r * 0.5).max(0.5)) } else { s.points.to_vec() };
    for p in pts {
        if (p.pos() - c).length() <= r {
            if cur.len() > 1 {
                pieces.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        } else {
            cur.push(p);
        }
    }
    if cur.len() > 1 {
        pieces.push(cur);
    }
    Some(
        pieces
            .into_iter()
            .map(|pts| Stroke {
                points: Arc::new(simplify(&pts, 0.2)),
                closed: false,
                fill: None,
                ..s.clone()
            })
            .collect(),
    )
}

fn densify_linear(points: &[InkPoint], step: f32) -> Vec<InkPoint> {
    let mut out = Vec::with_capacity(points.len());
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b.pos() - a.pos()).length();
        let n = ((len / step).ceil() as usize).clamp(1, 512);
        for i in 0..n {
            let t = i as f32 / n as f32;
            out.push(InkPoint(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t));
        }
    }
    out.push(*points.last().unwrap());
    out
}

pub fn point_in_polygon(p: Pos2, poly: &[Pos2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Stroke is selected by lasso when most of its points are inside.
pub fn stroke_in_lasso(s: &Stroke, lasso: &[Pos2]) -> bool {
    let inside = s.points.iter().filter(|p| point_in_polygon(p.pos(), lasso)).count();
    inside * 10 >= s.points.len() * 6 && inside > 0
}

/// Affine transform: rotate by `angle` and scale by `scale` around `pivot`, then translate.
#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub pivot: Pos2,
    pub scale: Vec2,
    pub angle: f32,
    pub offset: Vec2,
}

impl Default for Xform {
    fn default() -> Self {
        Self { pivot: Pos2::ZERO, scale: Vec2::splat(1.0), angle: 0.0, offset: Vec2::ZERO }
    }
}

impl Xform {
    pub fn apply(&self, p: Pos2) -> Pos2 {
        let d = (p - self.pivot) * self.scale;
        let (s, c) = self.angle.sin_cos();
        self.pivot + vec2(d.x * c - d.y * s, d.x * s + d.y * c) + self.offset
    }
    pub fn is_identity(&self) -> bool {
        self.scale == Vec2::splat(1.0) && self.angle == 0.0 && self.offset == Vec2::ZERO
    }
    pub fn apply_stroke(&self, s: &Stroke) -> Stroke {
        let k = ((self.scale.x.abs() + self.scale.y.abs()) * 0.5).max(0.05);
        Stroke {
            points: Arc::new(
                s.points
                    .iter()
                    .map(|p| {
                        let q = self.apply(p.pos());
                        InkPoint(q.x, q.y, p.2)
                    })
                    .collect(),
            ),
            width: s.width * k,
            ..s.clone()
        }
    }
}

// ----------------------------------------------------------------------------
// Shape recognition for freehand drawing

pub enum Recognized {
    Line(Pos2, Pos2),
    Polygon(Vec<Pos2>),
    Ellipse(Rect),
}

pub fn recognize(points: &[InkPoint]) -> Option<Recognized> {
    if points.len() < 5 {
        return None;
    }
    let pts: Vec<Pos2> = points.iter().map(|p| p.pos()).collect();
    let mut bb = Rect::NOTHING;
    for p in &pts {
        bb.extend_with(*p);
    }
    let size = bb.size().length();
    if size < 12.0 {
        return None;
    }
    let (first, last) = (pts[0], *pts.last().unwrap());
    let path_len: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    // Straight line.
    let straight = pts.iter().map(|p| seg_dist(*p, first, last)).fold(0.0f32, f32::max);
    if straight < size * 0.06 && (last - first).length() > path_len * 0.9 {
        let mut b = last;
        // Snap to horizontal / vertical / 45°.
        let d = last - first;
        let ang = d.y.atan2(d.x).to_degrees();
        let snapped = (ang / 45.0).round() * 45.0;
        if (ang - snapped).abs() < 6.0 {
            let len = d.length();
            let r = snapped.to_radians();
            b = first + vec2(r.cos(), r.sin()) * len;
        }
        return Some(Recognized::Line(first, b));
    }
    let closed = (last - first).length() < size * 0.2;
    if !closed {
        return None;
    }
    // Ellipse: radial distance stays close to the fitted ellipse.
    let c = bb.center();
    let (rx, ry) = (bb.width() * 0.5, bb.height() * 0.5);
    if rx > 2.0 && ry > 2.0 {
        let err: f32 = pts
            .iter()
            .map(|p| {
                let d = vec2((p.x - c.x) / rx, (p.y - c.y) / ry).length();
                (d - 1.0).abs()
            })
            .sum::<f32>()
            / pts.len() as f32;
        let poly = simplify(points, size * 0.06);
        if err < 0.09 && poly.len() > 5 {
            return Some(Recognized::Ellipse(bb));
        }
    }
    let mut poly: Vec<Pos2> = simplify(points, size * 0.07).iter().map(|p| p.pos()).collect();
    if poly.len() > 2 && (poly[0] - *poly.last().unwrap()).length() < size * 0.2 {
        poly.pop();
    }
    match poly.len() {
        3 => Some(Recognized::Polygon(poly)),
        4 => {
            // Nearly axis-aligned quad → rectangle.
            let aligned = (0..4).all(|i| {
                let d = poly[(i + 1) % 4] - poly[i];
                let a = d.y.atan2(d.x).to_degrees().rem_euclid(90.0);
                !(12.0..=78.0).contains(&a)
            });
            if aligned {
                Some(Recognized::Polygon(vec![bb.left_top(), bb.right_top(), bb.right_bottom(), bb.left_bottom()]))
            } else {
                Some(Recognized::Polygon(poly))
            }
        }
        5 | 6 => Some(Recognized::Polygon(poly)),
        _ => None,
    }
}

pub fn ellipse_points(r: Rect, n: usize) -> Vec<Pos2> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            pos2(r.center().x + a.cos() * r.width() * 0.5, r.center().y + a.sin() * r.height() * 0.5)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Arrow,
    Rect,
    Ellipse,
    Triangle,
    Star,
    Callout,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 7] = [
        ShapeKind::Line,
        ShapeKind::Arrow,
        ShapeKind::Rect,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::Star,
        ShapeKind::Callout,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Line => "╱ Линия",
            ShapeKind::Arrow => "→ Стрелка",
            ShapeKind::Rect => "▭ Прямоугольник",
            ShapeKind::Ellipse => "◯ Овал",
            ShapeKind::Triangle => "△ Треугольник",
            ShapeKind::Star => "☆ Звезда",
            ShapeKind::Callout => "💬 Выноска",
        }
    }
}

/// Builds shape outline points from a drag `a` → `b`.
pub fn shape_points(kind: ShapeKind, a: Pos2, b: Pos2) -> (Vec<Pos2>, bool) {
    let r = Rect::from_two_pos(a, b);
    match kind {
        ShapeKind::Line => (vec![a, b], false),
        ShapeKind::Arrow => {
            let d = b - a;
            let len = d.length().max(1.0);
            let dir = d / len;
            let n = vec2(-dir.y, dir.x);
            let head = (len * 0.25).clamp(8.0, 28.0);
            let p1 = b - dir * head + n * head * 0.5;
            let p2 = b - dir * head - n * head * 0.5;
            (vec![a, b, p1, b, p2], false)
        }
        ShapeKind::Rect => (vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true),
        ShapeKind::Ellipse => (ellipse_points(r, 72), true),
        ShapeKind::Triangle => (vec![pos2(r.center().x, r.top()), r.right_bottom(), r.left_bottom()], true),
        ShapeKind::Star => {
            let c = r.center();
            let pts = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let k = if i % 2 == 0 { 1.0 } else { 0.42 };
                    pos2(c.x + a.cos() * r.width() * 0.5 * k, c.y + a.sin() * r.height() * 0.5 * k)
                })
                .collect();
            (pts, true)
        }
        ShapeKind::Callout => {
            let body = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + r.height() * 0.75));
            let tx = body.left() + body.width() * 0.25;
            (
                vec![
                    body.left_top(),
                    body.right_top(),
                    body.right_bottom(),
                    pos2(tx + 24.0, body.bottom()),
                    pos2(tx, r.bottom()),
                    pos2(tx + 6.0, body.bottom()),
                    body.left_bottom(),
                ],
                true,
            )
        }
    }
}

pub fn make_shape_stroke(pts: Vec<Pos2>, closed: bool, color: [u8; 4], width: f32, fill: Option<[u8; 4]>) -> Stroke {
    Stroke {
        points: Arc::new(pts.into_iter().map(|p| InkPoint(p.x, p.y, 1.0)).collect()),
        color,
        width,
        kind: InkKind::Shape,
        fill: if closed { fill } else { None },
        closed,
    }
}

/// Rasterizes strokes into a grayscale image (white background) for OCR of handwriting.
pub fn rasterize(strokes: &[Stroke], scale: f32) -> Option<(image::GrayImage, Rect)> {
    let mut bb = Rect::NOTHING;
    for s in strokes {
        bb = bb.union(s.bbox());
    }
    if !bb.is_positive() {
        return None;
    }
    let bb = bb.expand(10.0);
    let w = (bb.width() * scale).ceil() as u32;
    let h = (bb.height() * scale).ceil() as u32;
    if w == 0 || h == 0 || w > 8000 || h > 8000 {
        return None;
    }
    let mut img = image::GrayImage::from_pixel(w, h, image::Luma([255]));
    for s in strokes {
        if s.kind == InkKind::Highlighter {
            continue;
        }
        let r = (s.width * scale * 0.5).max(1.2);
        let pts = densify_linear(&s.points, 0.5);
        for p in pts {
            let cx = (p.0 - bb.min.x) * scale;
            let cy = (p.1 - bb.min.y) * scale;
            let (x0, x1) = ((cx - r).floor().max(0.0) as u32, ((cx + r).ceil() as u32).min(w - 1));
            let (y0, y1) = ((cy - r).floor().max(0.0) as u32, ((cy + r).ceil() as u32).min(h - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                    if d <= r {
                        img.put_pixel(x, y, image::Luma([0]));
                    }
                }
            }
        }
    }
    Some((img, bb))
}
