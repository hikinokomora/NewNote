//! Ink geometry: simplification, erasing, hit testing, transforms and
//! freehand shape recognition. Rendering lives in `brush.rs`.

use std::sync::Arc;

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::model::{InkKind, InkPoint, Stroke};

/// Ramer–Douglas–Peucker simplification keeping pressure.
pub fn simplify(points: &[InkPoint], eps: f32) -> Vec<InkPoint> {
    let keep = simplify_mask(points, eps);
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Which points survive simplification (to keep per-point data in sync).
pub fn simplify_mask(points: &[InkPoint], eps: f32) -> Vec<bool> {
    if points.len() < 3 {
        return vec![true; points.len()];
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
    keep
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
                tilt: Arc::new(Vec::new()),
                id: crate::model::rand_u64(),
                attach: [None, None],
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

pub fn make_shape_stroke(pts: Vec<Pos2>, closed: bool, color: [u8; 4], width: f32, fill: Option<[u8; 4]>) -> Stroke {
    let mut s = Stroke::new(pts.into_iter().map(|p| InkPoint(p.x, p.y, 1.0)).collect(), color, width, InkKind::Shape);
    s.fill = if closed { fill } else { None };
    s.closed = closed;
    s
}

pub fn stroke_in_rect(s: &Stroke, r: Rect) -> bool {
    !s.points.is_empty() && s.points.iter().all(|p| r.contains(p.pos()))
}

/// Hit test used for click selection.
pub fn stroke_at(strokes: &[Stroke], p: Pos2, tol: f32) -> Option<usize> {
    strokes.iter().enumerate().rev().find(|(_, s)| !s.locked && stroke_hits_circle(s, p, tol)).map(|(i, _)| i)
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
