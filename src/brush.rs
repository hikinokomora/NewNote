//! Brush engine: turns strokes into anti-aliased triangle meshes.
//!
//! Every stroke is resampled with a centripetal Catmull–Rom spline (no
//! overshoot or hooks), then built as a ribbon with a one-pixel feathered
//! edge, so lines stay smooth at any zoom without relying on MSAA. Meshes are
//! cached per stroke and zoom level and only transformed when the view moves.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Color32, Mesh, Pos2, Rect, Vec2, pos2, vec2};

use crate::model::{Dash, Effect, InkKind, InkPoint, Stroke};

/// Rendering parameters that depend on the view.
#[derive(Clone, Copy)]
pub struct Opts {
    /// Pressure response exponent.
    pub curve: f32,
    /// Width of the soft edge in canvas units (≈1 screen pixel).
    pub feather: f32,
    /// Resampling step in canvas units.
    pub step: f32,
    /// Page is dark: dark ink is shown light.
    pub dark: bool,
}

impl Opts {
    pub fn for_zoom(zoom: f32, curve: f32, dark: bool) -> (Self, i32) {
        let bucket = (zoom.max(0.05).log2() * 3.0).round() as i32;
        let z = 2f32.powf(bucket as f32 / 3.0);
        (Self { curve, feather: 1.1 / z, step: (1.4 / z).clamp(0.12, 4.0), dark }, bucket)
    }
}

/// Dark ink is shown light on a dark page (the stored colour does not change).
pub fn display_color(c: [u8; 4], dark: bool) -> [u8; 4] {
    let lum = 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
    if dark && lum < 70.0 {
        [235 - c[0] / 4, 235 - c[1] / 4, 235 - c[2] / 4, c[3]]
    } else {
        c
    }
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Deterministic noise in [0, 1).
fn hash01(a: u64, b: u64) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

// ----------------------------------------------------------------------------
// Resampling

#[derive(Clone, Copy)]
pub struct Sample {
    pub p: Pos2,
    pub pressure: f32,
    pub tilt: f32,
    /// Arc length from the start.
    pub s: f32,
}

/// Centripetal Catmull–Rom resampling (α = 0.5): smooth, never overshoots.
pub fn resample(points: &[InkPoint], tilt: &[u8], step: f32) -> Vec<Sample> {
    let n = points.len();
    let tilt_at = |i: usize| if tilt.len() == n { tilt[i] as f32 / 255.0 } else { 0.0 };
    let mut out: Vec<Sample> = Vec::with_capacity(n * 3);
    if n == 0 {
        return out;
    }
    if n == 1 {
        out.push(Sample { p: points[0].pos(), pressure: points[0].2, tilt: tilt_at(0), s: 0.0 });
        return out;
    }
    let mut s_acc = 0.0f32;
    let mut push = |out: &mut Vec<Sample>, p: Pos2, pr: f32, tl: f32| {
        if let Some(last) = out.last() {
            s_acc += (p - last.p).length();
        }
        out.push(Sample { p, pressure: pr, tilt: tl, s: s_acc });
    };
    for i in 0..n - 1 {
        let p1 = points[i].pos();
        let p2 = points[i + 1].pos();
        let p0 = if i == 0 { p1 + (p1 - p2) * 0.01 } else { points[i - 1].pos() };
        let p3 = if i + 2 < n { points[i + 2].pos() } else { p2 + (p2 - p1) * 0.01 };
        let seg = (p2 - p1).length();
        let k = ((seg / step).ceil() as usize).clamp(1, 64);
        // Knot intervals.
        let t01 = (p1 - p0).length().sqrt().max(1e-4);
        let t12 = seg.sqrt().max(1e-4);
        let t23 = (p3 - p2).length().sqrt().max(1e-4);
        // Tangents (Barry–Goldman form converted to Hermite).
        let m1 = ((p1 - p0) / t01 - (p2 - p0) / (t01 + t12) + (p2 - p1) / t12) * t12;
        let m2 = ((p2 - p1) / t12 - (p3 - p1) / (t12 + t23) + (p3 - p2) / t23) * t12;
        for j in 0..k {
            let t = j as f32 / k as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            let p = pos2(
                h00 * p1.x + h10 * m1.x + h01 * p2.x + h11 * m2.x,
                h00 * p1.y + h10 * m1.y + h01 * p2.y + h11 * m2.y,
            );
            let pr = points[i].2 + (points[i + 1].2 - points[i].2) * t;
            let tl = tilt_at(i) + (tilt_at(i + 1) - tilt_at(i)) * t;
            push(&mut out, p, pr, tl);
        }
    }
    push(&mut out, points[n - 1].pos(), points[n - 1].2, tilt_at(n - 1));
    out
}

fn polyline_samples(points: &[InkPoint], closed: bool) -> Vec<Sample> {
    let mut v: Vec<Pos2> = points.iter().map(|p| p.pos()).collect();
    if closed && v.len() > 2 {
        v.push(v[0]);
    }
    let mut out = Vec::with_capacity(v.len());
    let mut s = 0.0;
    for (i, p) in v.iter().enumerate() {
        if i > 0 {
            s += (*p - v[i - 1]).length();
        }
        out.push(Sample { p: *p, pressure: 1.0, tilt: 0.0, s });
    }
    out
}

// ----------------------------------------------------------------------------
// Geometry builders

#[derive(Clone, Copy, PartialEq)]
enum Cap {
    Round,
    Flat,
}

fn vtx(mesh: &mut Mesh, p: Pos2, c: Color32) -> u32 {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(p, c);
    i
}

/// Disc with a feathered rim.
fn disc(mesh: &mut Mesh, c: Pos2, r: f32, color: Color32, f: f32) {
    let (inner, a) = if r > f * 0.5 { (r - f * 0.5, 1.0) } else { (0.0, (2.0 * r / f).clamp(0.05, 1.0)) };
    let outer = r + f * 0.5;
    let col = color.gamma_multiply(a);
    let segs = ((outer * 1.2) as u32).clamp(10, 48) as usize;
    let center = vtx(mesh, c, col);
    let mut ring_in = Vec::with_capacity(segs);
    let mut ring_out = Vec::with_capacity(segs);
    for k in 0..segs {
        let ang = k as f32 / segs as f32 * std::f32::consts::TAU;
        let d = vec2(ang.cos(), ang.sin());
        ring_in.push(vtx(mesh, c + d * inner, col));
        ring_out.push(vtx(mesh, c + d * outer, Color32::TRANSPARENT));
    }
    for k in 0..segs {
        let k2 = (k + 1) % segs;
        mesh.add_triangle(center, ring_in[k], ring_in[k2]);
        mesh.add_triangle(ring_in[k], ring_out[k], ring_out[k2]);
        mesh.add_triangle(ring_in[k], ring_out[k2], ring_in[k2]);
    }
}

/// Half disc cap at `c` facing `dir`.
fn half_disc(mesh: &mut Mesh, c: Pos2, dir: Vec2, r: f32, color: Color32, f: f32) {
    if dir.length_sq() < 1e-12 {
        return;
    }
    let d = dir.normalized();
    let (inner, a) = if r > f * 0.5 { (r - f * 0.5, 1.0) } else { (0.0, (2.0 * r / f).clamp(0.05, 1.0)) };
    let outer = r + f * 0.5;
    let col = color.gamma_multiply(a);
    let start = d.y.atan2(d.x) - std::f32::consts::FRAC_PI_2;
    let segs = ((outer * 0.8) as usize).clamp(5, 24);
    let center = vtx(mesh, c, col);
    let mut prev: Option<(u32, u32)> = None;
    for k in 0..=segs {
        let ang = start + k as f32 / segs as f32 * std::f32::consts::PI;
        let v = vec2(ang.cos(), ang.sin());
        let i = vtx(mesh, c + v * inner, col);
        let o = vtx(mesh, c + v * outer, Color32::TRANSPARENT);
        if let Some((pi, po)) = prev {
            mesh.add_triangle(center, pi, i);
            mesh.add_triangle(pi, po, o);
            mesh.add_triangle(pi, o, i);
        }
        prev = Some((i, o));
    }
}

/// Variable-width ribbon with feathered edges. Splits at cusps so sharp
/// turns get a clean round join instead of a fold.
fn ribbon(mesh: &mut Mesh, pts: &[Pos2], hw: &[f32], cols: &[Color32], f: f32, cap: Cap) {
    let n = pts.len();
    if n == 0 {
        return;
    }
    if n == 1 || pts.iter().all(|p| (*p - pts[0]).length_sq() < 1e-6) {
        disc(mesh, pts[0], hw[0], cols[0], f);
        return;
    }
    let mut start = 0;
    for i in 1..n - 1 {
        let d0 = pts[i] - pts[i - 1];
        let d1 = pts[i + 1] - pts[i];
        if d0.length_sq() > 1e-9 && d1.length_sq() > 1e-9 && d0.normalized().dot(d1.normalized()) < -0.2 {
            ribbon_piece(mesh, &pts[start..=i], &hw[start..=i], &cols[start..=i], f, cap, start == 0, false);
            disc(mesh, pts[i], hw[i], cols[i], f);
            start = i;
        }
    }
    ribbon_piece(mesh, &pts[start..], &hw[start..], &cols[start..], f, cap, start == 0, true);
}

#[allow(clippy::too_many_arguments)]
fn ribbon_piece(mesh: &mut Mesh, pts: &[Pos2], hw: &[f32], cols: &[Color32], f: f32, cap: Cap, cap_start: bool, cap_end: bool) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let base = mesh.vertices.len() as u32;
    for i in 0..n {
        let a = if i == 0 { pts[0] } else { pts[i - 1] };
        let b = if i + 1 == n { pts[n - 1] } else { pts[i + 1] };
        let mut t = b - a;
        if t.length_sq() < 1e-12 {
            t = vec2(1.0, 0.0);
        }
        let t = t.normalized();
        let nrm = vec2(-t.y, t.x);
        // Miter compensation, limited so spikes cannot appear.
        let mut k = 1.0;
        if i > 0 && i + 1 < n {
            let d0 = (pts[i] - pts[i - 1]).normalized();
            let n0 = vec2(-d0.y, d0.x);
            let dot = nrm.dot(n0).abs();
            if dot > 0.3 {
                k = (1.0 / dot).min(2.0);
            }
        }
        let w = hw[i] * k;
        let (inner, alpha) = if w > f * 0.5 { (w - f * 0.5, 1.0) } else { (0.0, (2.0 * w / f).clamp(0.05, 1.0)) };
        let outer = w + f * 0.5;
        let col = cols[i].gamma_multiply(alpha);
        let p = pts[i];
        mesh.colored_vertex(p + nrm * outer, Color32::TRANSPARENT);
        mesh.colored_vertex(p + nrm * inner, col);
        mesh.colored_vertex(p - nrm * inner, col);
        mesh.colored_vertex(p - nrm * outer, Color32::TRANSPARENT);
    }
    for i in 0..(n as u32 - 1) {
        let a = base + i * 4;
        let b = a + 4;
        for q in 0..3 {
            mesh.add_triangle(a + q, a + q + 1, b + q);
            mesh.add_triangle(a + q + 1, b + q + 1, b + q);
        }
    }
    match cap {
        Cap::Round => {
            if cap_start {
                half_disc(mesh, pts[0], pts[0] - pts[1], hw[0], cols[0], f);
            }
            if cap_end {
                half_disc(mesh, pts[n - 1], pts[n - 1] - pts[n - 2], hw[n - 1], cols[n - 1], f);
            }
        }
        Cap::Flat => {
            // Feathered square end so marker strokes have soft chisel ends.
            for (end, prev, w, c, on) in [(pts[0], pts[1], hw[0], cols[0], cap_start), (pts[n - 1], pts[n - 2], hw[n - 1], cols[n - 1], cap_end)] {
                if !on {
                    continue;
                }
                let t = (end - prev).normalized();
                let nrm = vec2(-t.y, t.x);
                let i0 = vtx(mesh, end + nrm * w, c);
                let i1 = vtx(mesh, end - nrm * w, c);
                let o0 = vtx(mesh, end + nrm * w + t * f, Color32::TRANSPARENT);
                let o1 = vtx(mesh, end - nrm * w + t * f, Color32::TRANSPARENT);
                mesh.add_triangle(i0, i1, o1);
                mesh.add_triangle(i0, o1, o0);
            }
        }
    }
}

/// Ear-clipping triangulation for (possibly concave) simple polygons.
fn fill_polygon(mesh: &mut Mesh, poly: &[Pos2], color: Color32) {
    let n = poly.len();
    if n < 3 {
        return;
    }
    let area: f32 = (0..n).map(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        a.x * b.y - b.x * a.y
    }).sum::<f32>() * 0.5;
    let ccw = area > 0.0;
    let base = mesh.vertices.len() as u32;
    for p in poly {
        mesh.colored_vertex(*p, color);
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let cross = |o: Pos2, a: Pos2, b: Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            let cr = cross(a, b, c);
            if (cr > 0.0) != ccw || cr.abs() < 1e-9 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = poly[j];
                let d1 = cross(a, b, p);
                let d2 = cross(b, c, p);
                let d3 = cross(c, a, p);
                if ccw { d1 > 0.0 && d2 > 0.0 && d3 > 0.0 } else { d1 < 0.0 && d2 < 0.0 && d3 < 0.0 }
            });
            if inside {
                continue;
            }
            mesh.add_triangle(base + ia as u32, base + ib as u32, base + ic as u32);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        mesh.add_triangle(base + idx[0] as u32, base + idx[1] as u32, base + idx[2] as u32);
    } else if idx.len() > 3 {
        // Degenerate input: fall back to a fan.
        for k in 1..idx.len() - 1 {
            mesh.add_triangle(base + idx[0] as u32, base + idx[k] as u32, base + idx[k + 1] as u32);
        }
    }
}

fn arrow_head(mesh: &mut Mesh, tip: Pos2, from: Pos2, width: f32, color: Color32, f: f32) {
    let d = tip - from;
    if d.length_sq() < 1e-6 {
        return;
    }
    let d = d.normalized();
    let n = vec2(-d.y, d.x);
    let len = (width * 4.0).max(10.0);
    let a = tip - d * len + n * len * 0.45;
    let b = tip - d * len - n * len * 0.45;
    fill_polygon(mesh, &[tip + d * f * 0.5, a, b], color);
}

// ----------------------------------------------------------------------------
// Colour along the stroke

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn effect_color(e: Effect, base: [u8; 4], s: f32, total: f32) -> [u8; 4] {
    let a = base[3];
    let mix = |x: [f32; 3], y: [f32; 3], t: f32| [x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t];
    let to = |c: [f32; 3]| [(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, a];
    match e {
        Effect::None => base,
        Effect::Rainbow => to(hsv((s / 260.0).fract(), 0.8, 1.0)),
        Effect::Gold | Effect::Silver => {
            let (lo, hi) = if e == Effect::Gold { ([0.66, 0.50, 0.12], [1.0, 0.93, 0.62]) } else { ([0.55, 0.56, 0.60], [0.97, 0.97, 1.0]) };
            let sh = 0.5 + 0.5 * (s / 14.0).sin() * (s / 47.0).cos();
            to(mix(lo, hi, sh))
        }
        Effect::Gradient(c2) => {
            let t = if total > 0.0 { (s / total).clamp(0.0, 1.0) } else { 0.0 };
            let x = [base[0] as f32 / 255.0, base[1] as f32 / 255.0, base[2] as f32 / 255.0];
            let y = [c2[0] as f32 / 255.0, c2[1] as f32 / 255.0, c2[2] as f32 / 255.0];
            to(mix(x, y, t))
        }
    }
}

fn shade(c: [u8; 4], k: f32) -> [u8; 4] {
    let f = |v: u8| ((v as f32 * k).clamp(0.0, 255.0)) as u8;
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

/// Half width at a sample for each brush.
fn half_width(kind: InkKind, base: f32, smp: &Sample, dir: Vec2, curve: f32, no_pressure: bool) -> f32 {
    let p = if no_pressure { 1.0 } else { smp.pressure.clamp(0.0, 1.0) };
    let w = match kind {
        InkKind::Pen => base * (0.12 + 0.88 * p.powf(curve.max(0.1))),
        InkKind::Ballpoint => base * (0.85 + 0.15 * p),
        InkKind::Pencil => base * (0.55 + 0.45 * p) * (1.0 + smp.tilt * 1.6),
        InkKind::Calligraphy => {
            // Nib held at 40°: thin along the nib, wide across it.
            let nib = 40f32.to_radians();
            let ang = dir.y.atan2(dir.x);
            base * (0.12 + 0.88 * (ang - nib).sin().abs()) * (0.7 + 0.3 * p)
        }
        InkKind::Watercolor => base * (0.7 + 0.6 * p),
        InkKind::Oil => base * (0.75 + 0.35 * p),
        InkKind::Highlighter | InkKind::Shape | InkKind::Spray => base,
    };
    (w * 0.5).max(0.05)
}

/// Builds the mesh of a stroke in canvas coordinates.
pub fn tessellate(s: &Stroke, o: &Opts) -> Mesh {
    let mut mesh = Mesh::default();
    if s.points.is_empty() {
        return mesh;
    }
    let base = display_color(s.color, o.dark);
    let f = o.feather;
    let st = &s.style;

    // Redaction: an opaque box that hides what is under it.
    if st.redact {
        let bb = Rect::from_points(&s.points.iter().map(|p| p.pos()).collect::<Vec<_>>());
        let col = if o.dark { Color32::from_gray(10) } else { Color32::from_gray(35) };
        fill_polygon(&mut mesh, &[bb.left_top(), bb.right_top(), bb.right_bottom(), bb.left_bottom()], col);
        return mesh;
    }

    let samples = if s.kind == InkKind::Shape {
        let mut v = polyline_samples(&s.points, s.closed);
        // Smoothly resample shape edges only for dashes.
        if st.dash != Dash::Solid {
            v = densify_linear(&v, o.step * 2.0);
        }
        v
    } else {
        resample(&s.points, &s.tilt, o.step)
    };
    let total = samples.last().map(|x| x.s).unwrap_or(0.0);

    // Filled interior.
    if let Some(fc) = s.fill {
        let poly: Vec<Pos2> = s.points.iter().map(|p| p.pos()).collect();
        fill_polygon(&mut mesh, &poly, c32(display_color(fc, o.dark)));
    }

    let dirs: Vec<Vec2> = (0..samples.len())
        .map(|i| {
            let a = samples[i.saturating_sub(1)].p;
            let b = samples[(i + 1).min(samples.len() - 1)].p;
            let d = b - a;
            if d.length_sq() < 1e-12 { vec2(1.0, 0.0) } else { d.normalized() }
        })
        .collect();
    let hw: Vec<f32> = samples.iter().zip(&dirs).map(|(x, d)| half_width(s.kind, s.width, x, *d, o.curve, st.no_pressure)).collect();
    let pts: Vec<Pos2> = samples.iter().map(|x| x.p).collect();
    let colors: Vec<[u8; 4]> = samples.iter().map(|x| effect_color(st.effect, base, x.s, total)).collect();

    // Shadow and glow go under the stroke body.
    if st.shadow {
        let sp: Vec<Pos2> = pts.iter().map(|p| *p + vec2(3.0, 4.0)).collect();
        let sc: Vec<Color32> = vec![Color32::from_black_alpha(70); pts.len()];
        ribbon(&mut mesh, &sp, &hw, &sc, f + 5.0, Cap::Round);
    }
    if st.glow {
        let gw: Vec<f32> = hw.iter().map(|w| w + 4.0).collect();
        let gc: Vec<Color32> = colors.iter().map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 90)).collect();
        ribbon(&mut mesh, &pts, &gw, &gc, f + 8.0, Cap::Round);
    }

    match s.kind {
        InkKind::Spray => spray(&mut mesh, s, &samples, &colors, f),
        InkKind::Oil => oil(&mut mesh, s, &pts, &hw, &colors, f),
        InkKind::Pencil => {
            let cols: Vec<Color32> = samples
                .iter()
                .zip(&colors)
                .enumerate()
                .map(|(i, (x, c))| {
                    let grain = 0.55 + 0.45 * hash01(s.id, (i / 2) as u64);
                    let alpha = (0.3 + 0.7 * x.pressure) * (1.0 - x.tilt * 0.55) * grain;
                    c32(*c).gamma_multiply(alpha)
                })
                .collect();
            ribbon(&mut mesh, &pts, &hw, &cols, f * 1.3, Cap::Round);
            // A second, offset hairline gives graphite texture.
            let off: Vec<Pos2> = pts.iter().zip(&dirs).zip(&hw).map(|((p, d), w)| *p + vec2(-d.y, d.x) * *w * 0.45).collect();
            let thin: Vec<f32> = hw.iter().map(|w| w * 0.35).collect();
            let c2: Vec<Color32> = cols.iter().enumerate().map(|(i, c)| c.gamma_multiply(0.4 + 0.6 * hash01(s.id ^ 77, i as u64))).collect();
            ribbon(&mut mesh, &off, &thin, &c2, f, Cap::Round);
        }
        InkKind::Watercolor => {
            // Soft wide edge and pressure-driven transparency.
            let cols: Vec<Color32> = samples.iter().zip(&colors).map(|(x, c)| c32(*c).gamma_multiply(0.25 + 0.45 * x.pressure)).collect();
            let soft = hw.iter().copied().fold(0.0, f32::max) * 0.9 + f;
            ribbon(&mut mesh, &pts, &hw, &cols, soft, Cap::Round);
            // Pigment collects at the edges.
            let edge: Vec<Color32> = cols.iter().map(|c| c.gamma_multiply(0.5)).collect();
            let off: Vec<f32> = hw.iter().map(|w| w * 0.15).collect();
            for side in [1.0f32, -1.0] {
                let ep: Vec<Pos2> = pts.iter().zip(&dirs).zip(&hw).map(|((p, d), w)| *p + vec2(-d.y, d.x) * *w * 0.8 * side).collect();
                ribbon(&mut mesh, &ep, &off, &edge, f * 3.0, Cap::Round);
            }
        }
        _ => {
            let cols: Vec<Color32> = colors.iter().map(|c| c32(*c)).collect();
            let cap = if s.kind == InkKind::Highlighter { Cap::Flat } else { Cap::Round };
            if st.dash == Dash::Solid {
                ribbon(&mut mesh, &pts, &hw, &cols, f, cap);
            } else {
                dashes(&mut mesh, &samples, &hw, &cols, f, st.dash, s.width);
            }
        }
    }
    if st.arrow_end && pts.len() > 1 {
        let tip = pts[pts.len() - 1];
        let from = back_point(&pts, true, s.width * 3.0);
        arrow_head(&mut mesh, tip, from, s.width, c32(colors[colors.len() - 1]), f);
    }
    if st.arrow_start && pts.len() > 1 {
        let from = back_point(&pts, false, s.width * 3.0);
        arrow_head(&mut mesh, pts[0], from, s.width, c32(colors[0]), f);
    }
    mesh
}

/// Point some distance back along the polyline from an end (for arrow direction).
fn back_point(pts: &[Pos2], from_end: bool, dist: f32) -> Pos2 {
    let it: Box<dyn Iterator<Item = &Pos2>> = if from_end { Box::new(pts.iter().rev()) } else { Box::new(pts.iter()) };
    let mut prev: Option<Pos2> = None;
    let mut acc = 0.0;
    let mut last = pts[0];
    for p in it {
        if let Some(q) = prev {
            acc += (*p - q).length();
            if acc >= dist {
                return *p;
            }
        }
        prev = Some(*p);
        last = *p;
    }
    last
}

fn densify_linear(v: &[Sample], step: f32) -> Vec<Sample> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for w in v.windows(2) {
        let len = (w[1].p - w[0].p).length();
        let k = ((len / step).ceil() as usize).clamp(1, 2000);
        for j in 0..k {
            let t = j as f32 / k as f32;
            out.push(Sample { p: w[0].p + (w[1].p - w[0].p) * t, pressure: 1.0, tilt: 0.0, s: w[0].s + (w[1].s - w[0].s) * t });
        }
    }
    if let Some(l) = v.last() {
        out.push(*l);
    }
    out
}

fn dashes(mesh: &mut Mesh, samples: &[Sample], hw: &[f32], cols: &[Color32], f: f32, dash: Dash, width: f32) {
    let w = width.max(1.0);
    let pattern: &[(bool, f32)] = match dash {
        Dash::Dashed => &[(true, 4.0), (false, 2.5)],
        Dash::Dotted => &[(true, 0.01), (false, 2.2)],
        Dash::DashDot => &[(true, 5.0), (false, 1.8), (true, 0.01), (false, 1.8)],
        Dash::Solid => &[(true, 1e9)],
    };
    let period: f32 = pattern.iter().map(|p| p.1 * w).sum();
    let on_at = |s: f32| {
        let mut m = s % period;
        for (on, len) in pattern {
            let l = len * w;
            if m < l.max(0.01) {
                return *on;
            }
            m -= l;
        }
        false
    };
    let mut start = None;
    for i in 0..samples.len() {
        let on = on_at(samples[i].s);
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(a)) => {
                if i - a >= 1 {
                    let pts: Vec<Pos2> = samples[a..i].iter().map(|x| x.p).collect();
                    if pts.len() == 1 {
                        disc(mesh, pts[0], hw[a], cols[a], f);
                    } else {
                        ribbon(mesh, &pts, &hw[a..i], &cols[a..i], f, Cap::Round);
                    }
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(a) = start {
        let pts: Vec<Pos2> = samples[a..].iter().map(|x| x.p).collect();
        ribbon(mesh, &pts, &hw[a..], &cols[a..], f, Cap::Round);
    }
}

fn spray(mesh: &mut Mesh, s: &Stroke, samples: &[Sample], colors: &[[u8; 4]], f: f32) {
    let radius = s.width * 1.8;
    let dot = (s.width * 0.09).clamp(0.35, 2.5);
    let mut last_s = -1e9;
    let gap = (radius * 0.18).max(0.6);
    for (i, smp) in samples.iter().enumerate() {
        if smp.s - last_s < gap {
            continue;
        }
        last_s = smp.s;
        let count = (4.0 + radius * 0.35 * smp.pressure.max(0.2)) as u64;
        for k in 0..count {
            let r = radius * hash01(s.id ^ i as u64, k * 2).powf(0.7);
            let a = hash01(s.id ^ i as u64, k * 2 + 1) * std::f32::consts::TAU;
            let p = smp.p + vec2(a.cos(), a.sin()) * r;
            let c = c32(colors[i]).gamma_multiply(0.55 + 0.45 * hash01(s.id, i as u64 * 31 + k));
            disc(mesh, p, dot * (0.6 + 0.8 * hash01(s.id ^ 5, i as u64 + k)), c, f);
        }
    }
}

fn oil(mesh: &mut Mesh, s: &Stroke, pts: &[Pos2], hw: &[f32], colors: &[[u8; 4]], f: f32) {
    let n = pts.len();
    if n < 2 {
        let c = c32(colors[0]);
        disc(mesh, pts[0], hw[0], c, f);
        return;
    }
    let normals: Vec<Vec2> = (0..n)
        .map(|i| {
            let d = pts[(i + 1).min(n - 1)] - pts[i.saturating_sub(1)];
            if d.length_sq() < 1e-12 { vec2(0.0, 1.0) } else { vec2(-d.y, d.x).normalized() }
        })
        .collect();
    // Body.
    let body: Vec<Color32> = colors.iter().map(|c| c32(*c)).collect();
    let bw: Vec<f32> = hw.iter().map(|w| w * 0.8).collect();
    ribbon(mesh, pts, &bw, &body, f, Cap::Round);
    // Bristles with their own shade and length.
    let bristles = 7;
    for b in 0..bristles {
        let off = (b as f32 / (bristles - 1) as f32) * 2.0 - 1.0;
        let k = 0.82 + 0.36 * hash01(s.id, b as u64);
        let a = (hash01(s.id ^ 3, b as u64) * 0.12 * n as f32) as usize;
        let e = n - (hash01(s.id ^ 9, b as u64) * 0.15 * n as f32) as usize;
        if e <= a + 1 {
            continue;
        }
        let bp: Vec<Pos2> = (a..e).map(|i| pts[i] + normals[i] * hw[i] * off * 0.85).collect();
        let bwid: Vec<f32> = (a..e).map(|i| hw[i] * 0.22).collect();
        let bc: Vec<Color32> = (a..e).map(|i| c32(shade(colors[i], k))).collect();
        ribbon(mesh, &bp, &bwid, &bc, f, Cap::Round);
    }
}

// ----------------------------------------------------------------------------
// View transform and caching

/// Affine map canvas → screen: p' = M·p + t (M = [a b; c d]).
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub m: [f32; 4],
    pub t: Vec2,
}

impl Affine {
    pub fn apply(&self, p: Pos2) -> Pos2 {
        pos2(self.m[0] * p.x + self.m[1] * p.y + self.t.x, self.m[2] * p.x + self.m[3] * p.y + self.t.y)
    }
    pub fn inverse(&self) -> Affine {
        let [a, b, c, d] = self.m;
        let det = a * d - b * c;
        let inv = if det.abs() < 1e-12 { [1.0, 0.0, 0.0, 1.0] } else { [d / det, -b / det, -c / det, a / det] };
        let t = vec2(-(inv[0] * self.t.x + inv[1] * self.t.y), -(inv[2] * self.t.x + inv[3] * self.t.y));
        Affine { m: inv, t }
    }
    pub fn scale(&self) -> f32 {
        (self.m[0] * self.m[3] - self.m[1] * self.m[2]).abs().sqrt()
    }
}

pub fn append(dst: &mut Mesh, src: &Mesh, a: &Affine, alpha: f32) {
    let base = dst.vertices.len() as u32;
    dst.vertices.reserve(src.vertices.len());
    for v in &src.vertices {
        let mut v = *v;
        v.pos = a.apply(v.pos);
        if alpha < 1.0 {
            v.color = v.color.gamma_multiply(alpha);
        }
        dst.vertices.push(v);
    }
    dst.indices.extend(src.indices.iter().map(|i| i + base));
}

fn stroke_key(s: &Stroke, bucket: i32, curve: f32, dark: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (Arc::as_ptr(&s.points) as usize).hash(&mut h);
    (Arc::as_ptr(&s.tilt) as usize).hash(&mut h);
    s.color.hash(&mut h);
    s.width.to_bits().hash(&mut h);
    s.kind.hash(&mut h);
    s.fill.hash(&mut h);
    s.closed.hash(&mut h);
    s.style.hash(&mut h);
    s.id.hash(&mut h);
    bucket.hash(&mut h);
    curve.to_bits().hash(&mut h);
    dark.hash(&mut h);
    h.finish()
}

/// Tessellated strokes, reused until the stroke or the zoom level changes.
#[derive(Default)]
pub struct Cache {
    map: HashMap<u64, (Arc<Mesh>, u64)>,
    frame: u64,
}

impl Cache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.frame % 300 == 0 {
            let f = self.frame;
            self.map.retain(|_, v| f - v.1 < 300);
        }
    }
    pub fn get(&mut self, s: &Stroke, zoom: f32, curve: f32, dark: bool) -> Arc<Mesh> {
        let (o, bucket) = Opts::for_zoom(zoom, curve, dark);
        let key = stroke_key(s, bucket, curve, dark);
        let frame = self.frame;
        let e = self.map.entry(key).or_insert_with(|| (Arc::new(tessellate(s, &o)), frame));
        e.1 = frame;
        e.0.clone()
    }
    pub fn clear(&mut self) {
        self.map.clear();
    }
}
