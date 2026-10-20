//! Line icons drawn with the painter (Fluent-like, 1.4 px strokes) and the
//! pen illustrations used in the drawing gallery.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::model::{Effect, InkKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Undo,
    Redo,
    Search,
    Settings,
    Notebooks,
    Recent,
    Select,
    Lasso,
    Space,
    Eraser,
    Shapes,
    ToShape,
    ToText,
    Math,
    Ruler,
    Plus,
    Sort,
    Hand,
    Table,
    Image,
    Attach,
    Link,
    Calendar,
    Code,
    Screenshot,
    Pdf,
    Diagram,
    ZoomIn,
    ZoomOut,
    Fit,
    Focus,
    Present,
    Pin,
    Speaker,
    Mic,
    Tag,
    Find,
    Paste,
    Cut,
    Copy,
    Brush,
    Bullets,
    Numbers,
    Checkbox,
    Outdent,
    Indent,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Justify,
    Page,
    Section,
    Close,
    Maximize,
    Restore,
    Minimize,
    Chevron,
    ChevronUp,
    Layers,
    Laser,
    Help,
    Text,
    Printer,
    Lock,
    Sticky,
    Chart,
}

fn line(p: &Painter, pts: &[Pos2], s: Stroke) {
    p.add(Shape::line(pts.to_vec(), s));
}

fn circle_pts(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Paints `icon` centred in `rect` (designed on a 20×20 grid).
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let k = rect.width().min(rect.height()) / 20.0;
    let o = rect.center() - vec2(10.0, 10.0) * k;
    let q = |x: f32, y: f32| pos2(o.x + x * k, o.y + y * k);
    let s = Stroke::new(1.4 * k.max(0.8), color);
    let thin = Stroke::new(1.0 * k.max(0.8), color);
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    match icon {
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let qx = |x: f32, y: f32| q(10.0 + (x - 10.0) * flip, y);
            let arc: Vec<Pos2> = circle_pts(pos2(0.0, 0.0), 5.5 * k, -FRAC_PI_2, FRAC_PI_2, 16)
                .iter()
                .map(|v| qx(11.0 + v.x / k, 11.0 + v.y / k))
                .collect();
            let mut pts = vec![qx(4.0, 5.5)];
            pts.extend(arc);
            pts.push(qx(7.0, 16.5));
            line(p, &pts, s);
            line(p, &[qx(7.0, 2.5), qx(4.0, 5.5), qx(7.0, 8.5)], s);
        }
        Icon::Search | Icon::Find => {
            p.circle_stroke(q(8.5, 8.5), 5.0 * k, s);
            line(p, &[q(12.2, 12.2), q(17.0, 17.0)], s);
        }
        Icon::Settings => {
            let c = q(10.0, 10.0);
            let mut pts = Vec::new();
            for i in 0..=48 {
                let a = i as f32 / 48.0 * TAU;
                let r = if (i / 3) % 2 == 0 { 7.5 } else { 6.0 };
                pts.push(c + vec2(a.cos(), a.sin()) * r * k);
            }
            line(p, &pts, s);
            p.circle_stroke(c, 2.6 * k, s);
        }
        Icon::Notebooks => {
            for (x, h) in [(3.5, 14.0), (8.0, 14.0)] {
                p.rect_stroke(Rect::from_min_max(q(x, 17.0 - h), q(x + 3.6, 17.0)), 0.8 * k, s, egui::StrokeKind::Middle);
            }
            // Leaning book.
            line(p, &[q(12.8, 4.2), q(16.3, 3.4), q(18.8, 16.0), q(15.3, 16.8), q(12.8, 4.2)], s);
        }
        Icon::Recent => {
            p.circle_stroke(q(10.0, 10.0), 7.0 * k, s);
            line(p, &[q(10.0, 5.5), q(10.0, 10.0), q(13.0, 12.0)], s);
        }
        Icon::Select => {
            // Arrow + I-beam.
            line(p, &[q(3.0, 2.5), q(3.0, 14.0), q(6.0, 11.2), q(8.0, 15.5), q(9.6, 14.8), q(7.6, 10.6), q(11.5, 10.6), q(3.0, 2.5)], s);
            line(p, &[q(14.5, 4.0), q(14.5, 16.0)], s);
            line(p, &[q(12.8, 4.0), q(16.2, 4.0)], s);
            line(p, &[q(12.8, 16.0), q(16.2, 16.0)], s);
        }
        Icon::Lasso => {
            let mut pts = Vec::new();
            for i in 0..=40 {
                let a = PI * 0.75 + i as f32 / 40.0 * TAU * 0.9;
                pts.push(q(10.5 + a.cos() * 7.0, 8.0 + a.sin() * 5.0));
            }
            p.add(Shape::dashed_line(&pts, s, 2.2 * k, 1.4 * k));
            line(p, &[q(5.2, 11.5), q(4.0, 14.5), q(6.5, 17.0)], s);
            line(p, &[q(14.5, 13.0), q(14.5, 18.5)], thin);
            line(p, &[q(11.8, 15.7), q(17.2, 15.7)], thin);
        }
        Icon::Space => {
            line(p, &[q(3.0, 10.0), q(17.0, 10.0)], Stroke::new(1.0 * k, color));
            line(p, &[q(10.0, 2.5), q(10.0, 8.0)], s);
            line(p, &[q(7.5, 5.5), q(10.0, 8.0), q(12.5, 5.5)], s);
            line(p, &[q(10.0, 12.0), q(10.0, 17.5)], s);
            line(p, &[q(7.5, 14.5), q(10.0, 12.0), q(12.5, 14.5)], s);
        }
        Icon::Eraser => {
            line(p, &[q(7.0, 17.0), q(3.0, 13.0), q(11.0, 5.0), q(16.5, 10.5), q(10.0, 17.0), q(7.0, 17.0)], s);
            line(p, &[q(7.0, 9.0), q(12.5, 14.5)], s);
            line(p, &[q(10.0, 17.0), q(17.0, 17.0)], s);
        }
        Icon::Shapes => {
            p.circle_stroke(q(7.0, 7.0), 4.5 * k, s);
            p.rect_stroke(Rect::from_min_max(q(9.5, 9.5), q(17.0, 17.0)), 1.0 * k, s, egui::StrokeKind::Middle);
        }
        Icon::ToShape => {
            let wob: Vec<Pos2> = (0..=24).map(|i| {
                let t = i as f32 / 24.0;
                q(2.5 + t * 7.0, 6.0 + (t * 9.0).sin() * 1.2)
            }).collect();
            line(p, &wob, s);
            line(p, &[q(10.5, 9.0), q(13.0, 9.0)], thin);
            line(p, &[q(12.0, 8.0), q(13.0, 9.0), q(12.0, 10.0)], thin);
            p.rect_stroke(Rect::from_min_max(q(11.5, 12.0), q(17.5, 17.5)), 0.5 * k, s, egui::StrokeKind::Middle);
            let wob2: Vec<Pos2> = circle_pts(pos2(0.0, 0.0), 1.0, 0.0, TAU, 20).iter().enumerate().map(|(i, v)| q(6.0 + v.x * (3.5 + (i as f32).sin() * 0.4), 14.5 + v.y * (3.0 + (i as f32 * 1.3).cos() * 0.4))).collect();
            line(p, &wob2, s);
        }
        Icon::ToText => {
            let wob: Vec<Pos2> = (0..=24).map(|i| {
                let t = i as f32 / 24.0;
                q(2.0 + t * 7.0, 12.0 + (t * 14.0).sin() * 2.0 - t * 3.0)
            }).collect();
            line(p, &wob, s);
            p.text(q(15.0, 11.0), egui::Align2::CENTER_CENTER, "A", egui::FontId::proportional(11.0 * k), color);
            line(p, &[q(9.5, 16.0), q(12.0, 16.0)], thin);
        }
        Icon::Math => {
            line(p, &[q(2.5, 11.0), q(4.5, 10.0), q(6.5, 16.5), q(9.0, 4.0), q(17.5, 4.0)], s);
            line(p, &[q(10.5, 8.5), q(16.0, 15.0)], s);
            line(p, &[q(16.0, 8.5), q(10.5, 15.0)], s);
        }
        Icon::Ruler => {
            let pts = [q(2.0, 13.5), q(13.5, 2.0), q(18.0, 6.5), q(6.5, 18.0), q(2.0, 13.5)];
            line(p, &pts, s);
            for i in 1..5 {
                let t = i as f32 / 5.0;
                let a = q(2.0 + 11.5 * t, 13.5 - 11.5 * t);
                let len = if i % 2 == 0 { 3.0 } else { 1.8 };
                line(p, &[a, a + vec2(1.0, 1.0) * len * k * 0.7], thin);
            }
        }
        Icon::Plus => {
            line(p, &[q(10.0, 3.5), q(10.0, 16.5)], s);
            line(p, &[q(3.5, 10.0), q(16.5, 10.0)], s);
        }
        Icon::Sort => {
            for (y, w) in [(5.0, 13.0), (10.0, 9.5), (15.0, 6.0)] {
                line(p, &[q(3.0, y), q(3.0 + w, y)], s);
            }
            line(p, &[q(16.0, 4.0), q(16.0, 16.0)], s);
            line(p, &[q(13.8, 13.8), q(16.0, 16.0), q(18.2, 13.8)], s);
        }
        Icon::Hand => {
            line(p, &[q(6.0, 11.0), q(6.0, 5.0), q(8.0, 5.0), q(8.0, 10.0)], s);
            line(p, &[q(8.0, 4.0), q(10.0, 4.0), q(10.0, 10.0)], s);
            line(p, &[q(10.0, 4.5), q(12.0, 4.5), q(12.0, 10.0)], s);
            line(p, &[q(12.0, 6.0), q(14.0, 6.0), q(14.0, 12.0), q(12.5, 16.5), q(7.5, 16.5), q(4.0, 12.0), q(4.5, 10.5), q(6.0, 11.0)], s);
        }
        Icon::Table => {
            p.rect_stroke(Rect::from_min_max(q(3.0, 4.0), q(17.0, 16.0)), 1.0 * k, s, egui::StrokeKind::Middle);
            line(p, &[q(3.0, 8.0), q(17.0, 8.0)], thin);
            line(p, &[q(3.0, 12.0), q(17.0, 12.0)], thin);
            line(p, &[q(8.0, 4.0), q(8.0, 16.0)], thin);
            line(p, &[q(12.5, 4.0), q(12.5, 16.0)], thin);
        }
        Icon::Image | Icon::Screenshot => {
            p.rect_stroke(Rect::from_min_max(q(2.5, 4.0), q(17.5, 16.0)), 1.5 * k, s, egui::StrokeKind::Middle);
            p.circle_stroke(q(7.0, 8.0), 1.4 * k, thin);
            line(p, &[q(3.0, 14.5), q(8.0, 10.5), q(11.0, 13.0), q(13.5, 11.0), q(17.0, 14.0)], s);
            if icon == Icon::Screenshot {
                line(p, &[q(14.0, 1.5), q(18.5, 1.5), q(18.5, 6.0)], thin);
            }
        }
        Icon::Attach => {
            let pts = [q(13.0, 6.0), q(7.0, 12.0), q(7.5, 14.5), q(10.0, 15.0), q(16.0, 9.0), q(16.0, 5.0), q(13.0, 3.0), q(9.5, 3.5), q(4.0, 9.0), q(3.5, 14.0), q(6.5, 17.5), q(11.5, 17.0), q(16.0, 12.5)];
            line(p, &pts, s);
        }
        Icon::Link => {
            p.rect_stroke(Rect::from_min_max(q(2.0, 7.5), q(10.0, 12.5)), 2.5 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(10.0, 7.5), q(18.0, 12.5)), 2.5 * k, s, egui::StrokeKind::Middle);
            line(p, &[q(7.0, 10.0), q(13.0, 10.0)], s);
        }
        Icon::Calendar => {
            p.rect_stroke(Rect::from_min_max(q(3.0, 4.5), q(17.0, 17.0)), 1.5 * k, s, egui::StrokeKind::Middle);
            line(p, &[q(3.0, 8.0), q(17.0, 8.0)], s);
            line(p, &[q(7.0, 2.5), q(7.0, 6.0)], s);
            line(p, &[q(13.0, 2.5), q(13.0, 6.0)], s);
            for (x, y) in [(6.5, 11.0), (10.0, 11.0), (13.5, 11.0), (6.5, 14.0), (10.0, 14.0)] {
                p.circle_filled(q(x, y), 0.8 * k, color);
            }
        }
        Icon::Code => {
            line(p, &[q(7.0, 5.0), q(2.5, 10.0), q(7.0, 15.0)], s);
            line(p, &[q(13.0, 5.0), q(17.5, 10.0), q(13.0, 15.0)], s);
            line(p, &[q(11.2, 4.0), q(8.8, 16.0)], s);
        }
        Icon::Pdf | Icon::Page => {
            line(p, &[q(4.5, 2.5), q(12.0, 2.5), q(15.5, 6.0), q(15.5, 17.5), q(4.5, 17.5), q(4.5, 2.5)], s);
            line(p, &[q(12.0, 2.5), q(12.0, 6.0), q(15.5, 6.0)], thin);
            if icon == Icon::Pdf {
                p.text(q(10.0, 12.0), egui::Align2::CENTER_CENTER, "PDF", egui::FontId::proportional(4.6 * k), color);
            } else {
                for y in [9.5, 12.0, 14.5] {
                    line(p, &[q(7.0, y), q(13.0, y)], thin);
                }
            }
        }
        Icon::Diagram => {
            p.rect_stroke(Rect::from_min_max(q(7.0, 2.5), q(13.0, 6.5)), 1.0 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(2.0, 13.0), q(8.0, 17.0)), 1.0 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(12.0, 13.0), q(18.0, 17.0)), 1.0 * k, s, egui::StrokeKind::Middle);
            line(p, &[q(10.0, 6.5), q(10.0, 10.0), q(5.0, 10.0), q(5.0, 13.0)], thin);
            line(p, &[q(10.0, 10.0), q(15.0, 10.0), q(15.0, 13.0)], thin);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            p.circle_stroke(q(8.5, 8.5), 5.5 * k, s);
            line(p, &[q(12.5, 12.5), q(17.0, 17.0)], s);
            line(p, &[q(6.0, 8.5), q(11.0, 8.5)], s);
            if icon == Icon::ZoomIn {
                line(p, &[q(8.5, 6.0), q(8.5, 11.0)], s);
            }
        }
        Icon::Fit => {
            for (a, b, c) in [((3.0, 7.0), (3.0, 3.0), (7.0, 3.0)), ((13.0, 3.0), (17.0, 3.0), (17.0, 7.0)), ((17.0, 13.0), (17.0, 17.0), (13.0, 17.0)), ((7.0, 17.0), (3.0, 17.0), (3.0, 13.0))] {
                line(p, &[q(a.0, a.1), q(b.0, b.1), q(c.0, c.1)], s);
            }
        }
        Icon::Focus => {
            p.circle_stroke(q(10.0, 10.0), 6.5 * k, s);
            p.circle_filled(q(10.0, 10.0), 2.4 * k, color);
        }
        Icon::Present => {
            p.rect_stroke(Rect::from_min_max(q(2.5, 3.0), q(17.5, 13.0)), 1.0 * k, s, egui::StrokeKind::Middle);
            line(p, &[q(10.0, 13.0), q(10.0, 15.5)], s);
            line(p, &[q(6.0, 17.5), q(10.0, 15.5), q(14.0, 17.5)], s);
            line(p, &[q(8.5, 6.0), q(12.5, 8.0), q(8.5, 10.0), q(8.5, 6.0)], thin);
        }
        Icon::Pin => {
            line(p, &[q(7.0, 2.5), q(13.0, 2.5), q(12.0, 4.0), q(12.0, 9.0), q(15.0, 12.0), q(5.0, 12.0), q(8.0, 9.0), q(8.0, 4.0), q(7.0, 2.5)], s);
            line(p, &[q(10.0, 12.0), q(10.0, 18.0)], s);
        }
        Icon::Speaker => {
            line(p, &[q(3.0, 7.5), q(6.5, 7.5), q(10.5, 4.0), q(10.5, 16.0), q(6.5, 12.5), q(3.0, 12.5), q(3.0, 7.5)], s);
            line(p, &circle_pts(q(10.5, 10.0), 3.5 * k, -0.9, 0.9, 10), s);
            line(p, &circle_pts(q(10.5, 10.0), 6.5 * k, -0.9, 0.9, 12), s);
        }
        Icon::Mic => {
            p.rect_stroke(Rect::from_min_max(q(7.5, 2.5), q(12.5, 12.0)), 2.5 * k, s, egui::StrokeKind::Middle);
            line(p, &circle_pts(q(10.0, 9.0), 5.0 * k, 0.1, PI - 0.1, 14), s);
            line(p, &[q(10.0, 14.0), q(10.0, 17.5)], s);
        }
        Icon::Tag => {
            line(p, &[q(3.0, 3.0), q(10.0, 3.0), q(17.0, 10.0), q(10.0, 17.0), q(3.0, 10.0), q(3.0, 3.0)], s);
            p.circle_stroke(q(6.8, 6.8), 1.3 * k, thin);
        }
        Icon::Paste => {
            p.rect_stroke(Rect::from_min_max(q(4.0, 4.0), q(14.0, 17.5)), 1.5 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(7.0, 2.5), q(11.0, 5.5)), 1.0 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(10.0, 9.0), q(17.0, 18.0)), 1.0 * k, s, egui::StrokeKind::Middle);
        }
        Icon::Cut => {
            p.circle_stroke(q(6.0, 14.5), 2.5 * k, s);
            p.circle_stroke(q(14.0, 14.5), 2.5 * k, s);
            line(p, &[q(7.5, 12.5), q(14.0, 2.5)], s);
            line(p, &[q(12.5, 12.5), q(6.0, 2.5)], s);
        }
        Icon::Copy => {
            p.rect_stroke(Rect::from_min_max(q(7.0, 6.0), q(16.5, 17.5)), 1.5 * k, s, egui::StrokeKind::Middle);
            line(p, &[q(4.0, 14.0), q(4.0, 3.5), q(12.5, 3.5)], s);
        }
        Icon::Brush => {
            line(p, &[q(9.0, 11.0), q(16.5, 3.5), q(18.0, 5.0), q(10.5, 12.5)], s);
            line(p, &[q(9.0, 11.0), q(5.0, 12.5), q(3.0, 17.5), q(8.0, 16.0), q(10.5, 12.5)], s);
        }
        Icon::Bullets | Icon::Numbers | Icon::Checkbox => {
            for (i, y) in [5.0f32, 10.0, 15.0].iter().enumerate() {
                match icon {
                    Icon::Bullets => {
                        p.circle_filled(q(4.0, *y), 1.3 * k, color);
                    }
                    Icon::Numbers => {
                        p.text(q(4.0, *y), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(5.5 * k), color);
                    }
                    _ => {
                        p.rect_stroke(Rect::from_center_size(q(4.0, *y), Vec2::splat(3.4 * k)), 0.6 * k, thin, egui::StrokeKind::Middle);
                    }
                }
                line(p, &[q(8.0, *y), q(17.5, *y)], s);
            }
        }
        Icon::Outdent | Icon::Indent => {
            for y in [3.5, 16.5] {
                line(p, &[q(2.5, y), q(17.5, y)], s);
            }
            for y in [8.0, 12.0] {
                line(p, &[q(9.0, y), q(17.5, y)], s);
            }
            if icon == Icon::Indent {
                line(p, &[q(2.5, 7.5), q(5.5, 10.0), q(2.5, 12.5)], s);
            } else {
                line(p, &[q(5.5, 7.5), q(2.5, 10.0), q(5.5, 12.5)], s);
            }
        }
        Icon::AlignLeft | Icon::AlignCenter | Icon::AlignRight | Icon::Justify => {
            for (i, y) in [4.0, 8.0, 12.0, 16.0].iter().enumerate() {
                let w = if icon == Icon::Justify || i % 2 == 0 { 15.0 } else { 9.0 };
                let x0 = match icon {
                    Icon::AlignCenter => 10.0 - w / 2.0,
                    Icon::AlignRight => 17.5 - w,
                    _ => 2.5,
                };
                line(p, &[q(x0, *y), q(x0 + w, *y)], s);
            }
        }
        Icon::Section => {
            line(p, &[q(3.0, 4.0), q(14.0, 4.0), q(17.0, 7.0), q(17.0, 16.0), q(3.0, 16.0), q(3.0, 4.0)], s);
            line(p, &[q(3.0, 8.0), q(17.0, 8.0)], thin);
        }
        Icon::Close => {
            line(p, &[q(5.0, 5.0), q(15.0, 15.0)], thin);
            line(p, &[q(15.0, 5.0), q(5.0, 15.0)], thin);
        }
        Icon::Maximize => {
            p.rect_stroke(Rect::from_min_max(q(5.0, 5.0), q(15.0, 15.0)), 0.0, thin, egui::StrokeKind::Middle);
        }
        Icon::Restore => {
            p.rect_stroke(Rect::from_min_max(q(5.0, 7.0), q(13.0, 15.0)), 0.0, thin, egui::StrokeKind::Middle);
            line(p, &[q(7.0, 7.0), q(7.0, 5.0), q(15.0, 5.0), q(15.0, 13.0), q(13.0, 13.0)], thin);
        }
        Icon::Minimize => {
            line(p, &[q(5.0, 10.0), q(15.0, 10.0)], thin);
        }
        Icon::Chevron => {
            line(p, &[q(5.5, 8.0), q(10.0, 12.5), q(14.5, 8.0)], s);
        }
        Icon::ChevronUp => {
            line(p, &[q(5.5, 12.0), q(10.0, 7.5), q(14.5, 12.0)], s);
        }
        Icon::Layers => {
            line(p, &[q(10.0, 3.0), q(18.0, 7.0), q(10.0, 11.0), q(2.0, 7.0), q(10.0, 3.0)], s);
            line(p, &[q(2.0, 10.5), q(10.0, 14.5), q(18.0, 10.5)], s);
            line(p, &[q(2.0, 14.0), q(10.0, 18.0), q(18.0, 14.0)], s);
        }
        Icon::Laser => {
            p.circle_filled(q(10.0, 10.0), 3.0 * k, Color32::from_rgb(235, 50, 50));
            p.circle_stroke(q(10.0, 10.0), 6.0 * k, Stroke::new(1.0 * k, Color32::from_rgba_unmultiplied(235, 50, 50, 120)));
        }
        Icon::Help => {
            p.circle_stroke(q(10.0, 10.0), 7.5 * k, s);
            p.text(q(10.0, 10.3), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(10.0 * k), color);
        }
        Icon::Text => {
            line(p, &[q(4.0, 4.0), q(16.0, 4.0)], s);
            line(p, &[q(10.0, 4.0), q(10.0, 16.5)], s);
            line(p, &[q(7.5, 16.5), q(12.5, 16.5)], s);
        }
        Icon::Printer => {
            p.rect_stroke(Rect::from_min_max(q(2.5, 7.0), q(17.5, 14.0)), 1.5 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(5.5, 2.5), q(14.5, 7.0)), 0.5 * k, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(q(5.5, 11.5), q(14.5, 17.5)), 0.5 * k, s, egui::StrokeKind::Middle);
        }
        Icon::Lock => {
            p.rect_stroke(Rect::from_min_max(q(4.5, 9.0), q(15.5, 17.5)), 1.5 * k, s, egui::StrokeKind::Middle);
            line(p, &circle_pts(q(10.0, 9.0), 3.5 * k, PI, TAU, 12), s);
        }
        Icon::Sticky => {
            line(p, &[q(3.0, 3.0), q(17.0, 3.0), q(17.0, 12.0), q(12.0, 17.0), q(3.0, 17.0), q(3.0, 3.0)], s);
            line(p, &[q(12.0, 17.0), q(12.0, 12.0), q(17.0, 12.0)], thin);
        }
        Icon::Chart => {
            line(p, &[q(3.0, 2.5), q(3.0, 17.0), q(17.5, 17.0)], s);
            for (x, h) in [(6.5, 6.0), (10.5, 10.0), (14.5, 4.0)] {
                p.rect_filled(Rect::from_min_max(q(x - 1.3, 17.0 - h), q(x + 1.3, 17.0)), 0.0, color);
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Pens

fn lighten(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (v as f32 + (255.0 - v as f32) * k) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

fn darken(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (v as f32 * (1.0 - k)) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// OneNote-style pen illustration: body, coloured cap band and tip. The
/// selected pen is raised a little. `dark` is the UI theme.
pub fn pen(p: &Painter, rect: Rect, kind: InkKind, color: [u8; 4], effect: Effect, selected: bool, dark: bool) {
    let c = Color32::from_rgb(color[0], color[1], color[2]);
    let lift = if selected { -4.0 } else { 0.0 };
    let cx = rect.center().x;
    let top = rect.top() + 4.0 + lift;
    let bottom = rect.bottom() + 6.0 + lift;
    let body_col = if dark { Color32::from_gray(238) } else { Color32::from_gray(245) };
    let outline = if dark { Color32::from_gray(25) } else { Color32::from_gray(120) };
    let tip_color = |i: usize, n: usize| -> Color32 {
        match effect {
            Effect::Rainbow => {
                let h = i as f32 / n.max(1) as f32;
                let rgb = egui::ecolor::Hsva::new(h, 0.85, 1.0, 1.0);
                Color32::from(rgb)
            }
            Effect::Gold => Color32::from_rgb(212, 175, 55),
            Effect::Silver => Color32::from_rgb(200, 200, 208),
            Effect::Gradient(g) => {
                let t = i as f32 / n.max(1) as f32;
                Color32::from_rgb(
                    (color[0] as f32 + (g[0] as f32 - color[0] as f32) * t) as u8,
                    (color[1] as f32 + (g[1] as f32 - color[1] as f32) * t) as u8,
                    (color[2] as f32 + (g[2] as f32 - color[2] as f32) * t) as u8,
                )
            }
            Effect::None => c,
        }
    };
    let painter = p.with_clip_rect(rect.expand2(vec2(2.0, 0.0)));
    match kind {
        InkKind::Highlighter => {
            // Wide chisel marker.
            let w = 15.0;
            let body = Rect::from_min_max(pos2(cx - w / 2.0, top + 9.0), pos2(cx + w / 2.0, bottom));
            painter.rect_filled(body, 2.0, body_col);
            painter.rect_stroke(body, 2.0, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
            let band = Rect::from_min_max(pos2(cx - w / 2.0, top + 9.0), pos2(cx + w / 2.0, top + 13.0));
            painter.rect_filled(band, 0.0, c);
            let tip = vec![pos2(cx - w / 2.0 + 2.0, top + 9.0), pos2(cx - w / 2.0 + 2.0, top + 3.0), pos2(cx + w / 2.0 - 2.0, top), pos2(cx + w / 2.0 - 2.0, top + 9.0)];
            painter.add(Shape::convex_polygon(tip, lighten(c, 0.1), Stroke::new(1.0, outline)));
        }
        InkKind::Pencil => {
            let w = 9.0;
            let wood_top = top + 10.0;
            let body = Rect::from_min_max(pos2(cx - w / 2.0, wood_top), pos2(cx + w / 2.0, bottom));
            painter.rect_filled(body, 0.0, Color32::from_rgb(240, 200, 70));
            painter.line_segment([pos2(cx, wood_top), pos2(cx, bottom)], Stroke::new(1.0, Color32::from_rgb(200, 160, 40)));
            let cone = vec![pos2(cx - w / 2.0, wood_top), pos2(cx, top), pos2(cx + w / 2.0, wood_top)];
            painter.add(Shape::convex_polygon(cone, Color32::from_rgb(235, 205, 160), Stroke::new(1.0, outline)));
            let lead = vec![pos2(cx - 1.8, top + 3.6), pos2(cx, top), pos2(cx + 1.8, top + 3.6)];
            painter.add(Shape::convex_polygon(lead, c, Stroke::NONE));
            painter.rect_stroke(body, 0.0, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
        }
        _ => {
            // Pen / ballpoint / calligraphy / brushes: body with a cone tip.
            let w = 12.0;
            let cone_top = top + 2.0;
            let cone_bottom = top + 11.0;
            let body = Rect::from_min_max(pos2(cx - w / 2.0, cone_bottom), pos2(cx + w / 2.0, bottom));
            painter.rect_filled(body, 1.5, body_col);
            let band = Rect::from_min_max(pos2(cx - w / 2.0, cone_bottom), pos2(cx + w / 2.0, cone_bottom + 5.0));
            // Coloured band (rainbow/gradient for effects).
            let n = 6;
            for i in 0..n {
                let x0 = band.left() + band.width() * i as f32 / n as f32;
                let x1 = band.left() + band.width() * (i + 1) as f32 / n as f32;
                painter.rect_filled(Rect::from_min_max(pos2(x0, band.top()), pos2(x1, band.bottom())), 0.0, tip_color(i, n - 1));
            }
            painter.rect_stroke(body, 1.5, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
            let cone_col = if dark { Color32::from_gray(60) } else { Color32::from_gray(70) };
            let cone = vec![pos2(cx - w / 2.0, cone_bottom), pos2(cx - 1.6, cone_top + 2.0), pos2(cx + 1.6, cone_top + 2.0), pos2(cx + w / 2.0, cone_bottom)];
            painter.add(Shape::convex_polygon(cone, cone_col, Stroke::new(1.0, outline)));
            let nib = match kind {
                InkKind::Calligraphy => vec![pos2(cx - 2.6, cone_top + 2.5), pos2(cx - 1.0, cone_top - 2.0), pos2(cx + 2.6, cone_top + 1.0)],
                InkKind::Watercolor | InkKind::Oil => {
                    painter.add(Shape::ellipse_filled(pos2(cx, cone_top), vec2(2.6, 4.0), tip_color(0, 1)));
                    vec![]
                }
                InkKind::Spray => {
                    for i in 0..6 {
                        let a = i as f32 * 1.1;
                        painter.circle_filled(pos2(cx + a.cos() * 3.0, cone_top - 1.0 + a.sin() * 2.0), 0.9, c);
                    }
                    vec![]
                }
                _ => vec![pos2(cx - 1.6, cone_top + 2.0), pos2(cx, cone_top - 2.0), pos2(cx + 1.6, cone_top + 2.0)],
            };
            if !nib.is_empty() {
                painter.add(Shape::convex_polygon(nib, tip_color(0, 1), Stroke::NONE));
            }
            if kind == InkKind::Ballpoint {
                painter.line_segment([pos2(cx + w / 2.0 - 1.5, cone_bottom + 7.0), pos2(cx + w / 2.0 - 1.5, bottom - 2.0)], Stroke::new(1.2, darken(c, 0.2)));
            }
        }
    }
}
