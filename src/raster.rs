//! CPU rasterizer for brush meshes: used for PNG export with transparency,
//! copying drawings as images and rendering stroke animations to GIF.

use egui::{Mesh, Rect};

use crate::brush::{self, Affine};
use crate::model::Stroke;

/// Premultiplied RGBA float canvas.
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    pub fn new(w: u32, h: u32, bg: Option<[u8; 4]>) -> Self {
        let c = bg.map(|b| {
            let a = b[3] as f32 / 255.0;
            [b[0] as f32 / 255.0 * a, b[1] as f32 / 255.0 * a, b[2] as f32 / 255.0 * a, a]
        });
        Self { w, h, px: vec![c.unwrap_or([0.0; 4]); (w * h) as usize] }
    }

    pub fn draw_mesh(&mut self, mesh: &Mesh, a: &Affine) {
        let v = &mesh.vertices;
        for tri in mesh.indices.chunks_exact(3) {
            let (p0, p1, p2) = (v[tri[0] as usize], v[tri[1] as usize], v[tri[2] as usize]);
            let (a0, a1, a2) = (a.apply(p0.pos), a.apply(p1.pos), a.apply(p2.pos));
            let c = |c: egui::Color32| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0];
            let (c0, c1, c2) = (c(p0.color), c(p1.color), c(p2.color));
            let area = (a1.x - a0.x) * (a2.y - a0.y) - (a1.y - a0.y) * (a2.x - a0.x);
            if area.abs() < 1e-8 {
                continue;
            }
            let minx = a0.x.min(a1.x).min(a2.x).floor().max(0.0) as i32;
            let maxx = a0.x.max(a1.x).max(a2.x).ceil().min(self.w as f32 - 1.0) as i32;
            let miny = a0.y.min(a1.y).min(a2.y).floor().max(0.0) as i32;
            let maxy = a0.y.max(a1.y).max(a2.y).ceil().min(self.h as f32 - 1.0) as i32;
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((a1.x - px) * (a2.y - py) - (a1.y - py) * (a2.x - px)) / area;
                    let w1 = ((a2.x - px) * (a0.y - py) - (a2.y - py) * (a0.x - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let src = [
                        c0[0] * w0 + c1[0] * w1 + c2[0] * w2,
                        c0[1] * w0 + c1[1] * w1 + c2[1] * w2,
                        c0[2] * w0 + c1[2] * w1 + c2[2] * w2,
                        c0[3] * w0 + c1[3] * w1 + c2[3] * w2,
                    ];
                    let d = &mut self.px[(y as u32 * self.w + x as u32) as usize];
                    let k = 1.0 - src[3];
                    for i in 0..4 {
                        d[i] = src[i] + d[i] * k;
                    }
                }
            }
        }
    }

    pub fn to_image(&self) -> image::RgbaImage {
        let mut img = image::RgbaImage::new(self.w, self.h);
        for (i, p) in self.px.iter().enumerate() {
            let a = p[3].clamp(0.0, 1.0);
            let un = |v: f32| if a > 1e-5 { ((v / a).clamp(0.0, 1.0) * 255.0) as u8 } else { 0 };
            img.put_pixel(i as u32 % self.w, i as u32 / self.w, image::Rgba([un(p[0]), un(p[1]), un(p[2]), (a * 255.0) as u8]));
        }
        img
    }

    pub fn copy_from(&mut self, other: &Canvas) {
        self.px.copy_from_slice(&other.px);
    }
}

/// Renders strokes to an image with `scale` pixels per canvas unit.
pub fn render_strokes(strokes: &[Stroke], scale: f32, bg: Option<[u8; 4]>, curve: f32) -> Option<(image::RgbaImage, Rect)> {
    let mut bb = Rect::NOTHING;
    for s in strokes {
        bb = bb.union(s.bbox());
    }
    if !bb.is_positive() {
        return None;
    }
    let bb = bb.expand(6.0);
    let w = (bb.width() * scale).ceil() as u32;
    let h = (bb.height() * scale).ceil() as u32;
    if w == 0 || h == 0 || w > 12000 || h > 12000 {
        return None;
    }
    let mut c = Canvas::new(w, h, bg);
    let aff = Affine { m: [scale, 0.0, 0.0, scale], t: -bb.min.to_vec2() * scale };
    let (o, _) = brush::Opts::for_zoom(scale, curve, false);
    for s in strokes {
        c.draw_mesh(&brush::tessellate(s, &o), &aff);
    }
    Some((c.to_image(), bb))
}

/// Frames of a drawing animation in stroke order, as a GIF.
pub fn animation_gif(strokes: &[Stroke], max_side: u32, bg: [u8; 4], curve: f32, seconds: f32, path: &std::path::Path) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let mut bb = Rect::NOTHING;
    for s in strokes {
        bb = bb.union(s.bbox());
    }
    if !bb.is_positive() {
        return Err("Нет рисунков".into());
    }
    let bb = bb.expand(8.0);
    let scale = (max_side as f32 / bb.width().max(bb.height())).min(2.0);
    let (w, h) = ((bb.width() * scale).ceil() as u32, (bb.height() * scale).ceil() as u32);
    let aff = Affine { m: [scale, 0.0, 0.0, scale], t: -bb.min.to_vec2() * scale };
    let (o, _) = brush::Opts::for_zoom(scale, curve, false);
    let total_pts: usize = strokes.iter().map(|s| s.points.len().max(1)).sum();
    let fps = 15u32;
    let frames = ((seconds * fps as f32) as usize).clamp(2, 600);
    let per_frame = (total_pts as f32 / frames as f32).max(1.0);
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new_with_speed(file, 10);
    enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
    let mut done = Canvas::new(w, h, Some(bg));
    let mut frame = Canvas::new(w, h, Some(bg));
    let delay = image::Delay::from_numer_denom_ms(1000, fps);
    let mut drawn = 0usize;
    let mut f = 1usize;
    loop {
        // Strokes fully shown and the point count of the current one.
        let mut budget = (per_frame * f as f32) as usize;
        let mut complete = strokes.len();
        let mut partial = 0;
        for (i, s) in strokes.iter().enumerate() {
            let n = s.points.len().max(1);
            if budget >= n {
                budget -= n;
            } else {
                complete = i;
                partial = budget;
                break;
            }
        }
        for s in &strokes[drawn..complete] {
            done.draw_mesh(&brush::tessellate(s, &o), &aff);
        }
        drawn = complete;
        frame.copy_from(&done);
        if complete < strokes.len() && partial > 1 {
            let mut part = strokes[complete].clone();
            part.points = std::sync::Arc::new(part.points[..partial].to_vec());
            part.tilt = if part.tilt.len() >= partial { std::sync::Arc::new(part.tilt[..partial].to_vec()) } else { Default::default() };
            frame.draw_mesh(&brush::tessellate(&part, &o), &aff);
        }
        enc.encode_frame(image::Frame::from_parts(frame.to_image(), 0, 0, delay)).map_err(|e| e.to_string())?;
        if complete >= strokes.len() {
            break;
        }
        f += 1;
    }
    Ok(())
}

/// SVG with the drawing as filled outlines of the rendered meshes.
pub fn strokes_svg(strokes: &[Stroke], curve: f32) -> String {
    let mut bb = Rect::NOTHING;
    for s in strokes {
        bb = bb.union(s.bbox());
    }
    if !bb.is_positive() {
        return String::new();
    }
    let (o, _) = brush::Opts::for_zoom(1.0, curve, false);
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"{:.1} {:.1} {:.1} {:.1}\">\n",
        bb.width(), bb.height(), bb.min.x, bb.min.y, bb.width(), bb.height()
    );
    for s in strokes {
        let o = brush::Opts { feather: 0.0, ..o };
        let m = brush::tessellate(s, &o);
        // Group triangles by colour into paths.
        let mut by_color: std::collections::BTreeMap<u32, String> = Default::default();
        for t in m.indices.chunks_exact(3) {
            let v = [m.vertices[t[0] as usize], m.vertices[t[1] as usize], m.vertices[t[2] as usize]];
            let c = v[0].color;
            if c.a() == 0 && v[1].color.a() == 0 && v[2].color.a() == 0 {
                continue;
            }
            let key = u32::from_be_bytes([c.r(), c.g(), c.b(), c.a().max(v[1].color.a()).max(v[2].color.a())]);
            by_color.entry(key).or_default().push_str(&format!(
                "M{:.2} {:.2}L{:.2} {:.2}L{:.2} {:.2}Z",
                v[0].pos.x, v[0].pos.y, v[1].pos.x, v[1].pos.y, v[2].pos.x, v[2].pos.y
            ));
        }
        for (k, d) in by_color {
            let [r, g, b, a] = k.to_be_bytes();
            let af = a as f32 / 255.0;
            let un = |v: u8| if af > 0.0 { ((v as f32 / af).min(255.0)) as u8 } else { 0 };
            out.push_str(&format!("<path fill=\"rgb({},{},{})\" fill-opacity=\"{:.3}\" d=\"{d}\"/>\n", un(r), un(g), un(b), af));
        }
    }
    out.push_str("</svg>\n");
    out
}
