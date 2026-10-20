//! Shape gallery: outlines for lines, polygons, flowchart blocks, symbols,
//! axes and grids, built from a drag rectangle.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Line,
    Polyline,
    Curve,
    Arc,
    Arrow,
    DoubleArrow,
    Rect,
    RoundRect,
    Square,
    Ellipse,
    Circle,
    IsoTriangle,
    RightTriangle,
    Pentagon,
    Hexagon,
    Star,
    Callout,
    Cloud,
    Process,
    Decision,
    Data,
    Terminator,
    Check,
    Cross,
    Heart,
    Lightning,
    Axes,
    Grid,
}

impl ShapeKind {
    pub const GROUPS: [(&'static str, &'static [ShapeKind]); 6] = [
        ("Линии", &[ShapeKind::Line, ShapeKind::Polyline, ShapeKind::Curve, ShapeKind::Arc, ShapeKind::Arrow, ShapeKind::DoubleArrow]),
        ("Основные", &[ShapeKind::Rect, ShapeKind::RoundRect, ShapeKind::Square, ShapeKind::Ellipse, ShapeKind::Circle]),
        ("Многоугольники", &[ShapeKind::IsoTriangle, ShapeKind::RightTriangle, ShapeKind::Pentagon, ShapeKind::Hexagon, ShapeKind::Star]),
        ("Выноски и блоки", &[ShapeKind::Callout, ShapeKind::Cloud, ShapeKind::Process, ShapeKind::Decision, ShapeKind::Data, ShapeKind::Terminator]),
        ("Символы", &[ShapeKind::Check, ShapeKind::Cross, ShapeKind::Heart, ShapeKind::Lightning]),
        ("Оси и сетки", &[ShapeKind::Axes, ShapeKind::Grid]),
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Line => "╱ Прямая",
            ShapeKind::Polyline => "⟋ Ломаная",
            ShapeKind::Curve => "∿ Кривая",
            ShapeKind::Arc => "◜ Дуга",
            ShapeKind::Arrow => "→ Стрелка",
            ShapeKind::DoubleArrow => "↔ Двойная стрелка",
            ShapeKind::Rect => "▭ Прямоугольник",
            ShapeKind::RoundRect => "▢ Скруглённый",
            ShapeKind::Square => "□ Квадрат",
            ShapeKind::Ellipse => "⬭ Овал",
            ShapeKind::Circle => "◯ Круг",
            ShapeKind::IsoTriangle => "△ Равнобедренный",
            ShapeKind::RightTriangle => "◺ Прямоугольный",
            ShapeKind::Pentagon => "⬠ Пятиугольник",
            ShapeKind::Hexagon => "⬡ Шестиугольник",
            ShapeKind::Star => "☆ Звезда",
            ShapeKind::Callout => "💬 Выноска",
            ShapeKind::Cloud => "☁ Облачко",
            ShapeKind::Process => "▭ Процесс",
            ShapeKind::Decision => "◇ Решение",
            ShapeKind::Data => "▱ Данные",
            ShapeKind::Terminator => "⬭ Начало/конец",
            ShapeKind::Check => "✓ Галочка",
            ShapeKind::Cross => "✗ Крестик",
            ShapeKind::Heart => "♡ Сердце",
            ShapeKind::Lightning => "⚡ Молния",
            ShapeKind::Axes => "┼ Оси координат",
            ShapeKind::Grid => "▦ Сетка",
        }
    }

    /// Lines that may connect two shapes.
    pub fn is_connector(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Arrow | ShapeKind::DoubleArrow | ShapeKind::Curve)
    }
}

pub struct Part {
    pub pts: Vec<Pos2>,
    pub closed: bool,
    pub arrow_start: bool,
    pub arrow_end: bool,
}

fn part(pts: Vec<Pos2>, closed: bool) -> Part {
    Part { pts, closed, arrow_start: false, arrow_end: false }
}

fn ellipse(c: Pos2, r: Vec2, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + a.cos() * r.x, c.y + a.sin() * r.y)
        })
        .collect()
}

fn regular(c: Pos2, r: Vec2, n: usize, rot: f32) -> Vec<Pos2> {
    (0..n)
        .map(|i| {
            let a = rot + i as f32 / n as f32 * std::f32::consts::TAU;
            pos2(c.x + a.cos() * r.x, c.y + a.sin() * r.y)
        })
        .collect()
}

/// Constrains a drag for Shift: equal sides, or 15° steps for lines.
pub fn constrain(kind: ShapeKind, a: Pos2, b: Pos2) -> Pos2 {
    if kind.is_connector() || kind == ShapeKind::Arc {
        let d = b - a;
        let len = d.length();
        let ang = d.y.atan2(d.x);
        let step = 15f32.to_radians();
        let s = (ang / step).round() * step;
        return a + vec2(s.cos(), s.sin()) * len;
    }
    let d = b - a;
    let m = d.x.abs().max(d.y.abs());
    a + vec2(m * d.x.signum(), m * d.y.signum())
}

/// Outline parts for a shape dragged from `a` to `b`.
pub fn build(kind: ShapeKind, a: Pos2, b: Pos2) -> Vec<Part> {
    let b = if matches!(kind, ShapeKind::Square | ShapeKind::Circle) { constrain(kind, a, b) } else { b };
    let r = Rect::from_two_pos(a, b);
    let c = r.center();
    let half = r.size() * 0.5;
    match kind {
        ShapeKind::Line | ShapeKind::Polyline => vec![part(vec![a, b], false)],
        ShapeKind::Curve => {
            // Gentle S-curve through the drag.
            let d = b - a;
            let n = vec2(-d.y, d.x) * 0.25;
            let p1 = a + d * 0.33 + n;
            let p2 = a + d * 0.66 - n;
            let pts = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    let u = 1.0 - t;
                    (a.to_vec2() * u * u * u + p1.to_vec2() * 3.0 * u * u * t + p2.to_vec2() * 3.0 * u * t * t + b.to_vec2() * t * t * t).to_pos2()
                })
                .collect();
            vec![part(pts, false)]
        }
        ShapeKind::Arc => {
            let rad = (b - a).length();
            let ang = (b - a).y.atan2((b - a).x);
            vec![part(ellipse(a, Vec2::splat(rad), ang - std::f32::consts::FRAC_PI_2, ang, 40), false)]
        }
        ShapeKind::Arrow => vec![Part { pts: vec![a, b], closed: false, arrow_start: false, arrow_end: true }],
        ShapeKind::DoubleArrow => vec![Part { pts: vec![a, b], closed: false, arrow_start: true, arrow_end: true }],
        ShapeKind::Rect | ShapeKind::Square | ShapeKind::Process => {
            vec![part(vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true)]
        }
        ShapeKind::RoundRect => {
            let rr = (r.width().min(r.height()) * 0.18).max(2.0);
            let mut pts = Vec::new();
            for (cc, a0) in [
                (pos2(r.max.x - rr, r.min.y + rr), -std::f32::consts::FRAC_PI_2),
                (pos2(r.max.x - rr, r.max.y - rr), 0.0),
                (pos2(r.min.x + rr, r.max.y - rr), std::f32::consts::FRAC_PI_2),
                (pos2(r.min.x + rr, r.min.y + rr), std::f32::consts::PI),
            ] {
                pts.extend(ellipse(cc, Vec2::splat(rr), a0, a0 + std::f32::consts::FRAC_PI_2, 8));
            }
            vec![part(pts, true)]
        }
        ShapeKind::Ellipse | ShapeKind::Circle => {
            let mut pts = ellipse(c, half, 0.0, std::f32::consts::TAU, 72);
            pts.pop();
            vec![part(pts, true)]
        }
        ShapeKind::IsoTriangle => vec![part(vec![pos2(c.x, r.min.y), r.right_bottom(), r.left_bottom()], true)],
        ShapeKind::RightTriangle => vec![part(vec![r.left_top(), r.right_bottom(), r.left_bottom()], true)],
        ShapeKind::Pentagon => vec![part(regular(c, half, 5, -std::f32::consts::FRAC_PI_2), true)],
        ShapeKind::Hexagon => vec![part(regular(c, half, 6, 0.0), true)],
        ShapeKind::Star => {
            let pts = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let k = if i % 2 == 0 { 1.0 } else { 0.42 };
                    pos2(c.x + a.cos() * half.x * k, c.y + a.sin() * half.y * k)
                })
                .collect();
            vec![part(pts, true)]
        }
        ShapeKind::Callout => {
            let body = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + r.height() * 0.75));
            let tx = body.left() + body.width() * 0.25;
            vec![part(
                vec![body.left_top(), body.right_top(), body.right_bottom(), pos2(tx + 24.0, body.bottom()), pos2(tx, r.bottom()), pos2(tx + 6.0, body.bottom()), body.left_bottom()],
                true,
            )]
        }
        ShapeKind::Cloud => {
            // Bumps around an ellipse.
            let n = 9;
            let mut pts = Vec::new();
            for i in 0..n {
                let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
                let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
                let p0 = pos2(c.x + a0.cos() * half.x * 0.82, c.y + a0.sin() * half.y * 0.82);
                let p1 = pos2(c.x + a1.cos() * half.x * 0.82, c.y + a1.sin() * half.y * 0.82);
                let mid = p0 + (p1 - p0) * 0.5;
                let rad = (p1 - p0).length() * 0.5;
                let base = (p1 - p0).y.atan2((p1 - p0).x);
                pts.extend(ellipse(mid, Vec2::splat(rad), base + std::f32::consts::PI, base + std::f32::consts::TAU, 8));
            }
            vec![part(pts, true)]
        }
        ShapeKind::Decision => vec![part(vec![pos2(c.x, r.min.y), pos2(r.max.x, c.y), pos2(c.x, r.max.y), pos2(r.min.x, c.y)], true)],
        ShapeKind::Data => {
            let k = r.width() * 0.18;
            vec![part(vec![pos2(r.min.x + k, r.min.y), r.right_top(), pos2(r.max.x - k, r.max.y), r.left_bottom()], true)]
        }
        ShapeKind::Terminator => {
            let rad = half.y;
            let mut pts = ellipse(pos2(r.max.x - rad, c.y), Vec2::splat(rad), -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2, 16);
            pts.extend(ellipse(pos2(r.min.x + rad, c.y), Vec2::splat(rad), std::f32::consts::FRAC_PI_2, 3.0 * std::f32::consts::FRAC_PI_2, 16));
            vec![part(pts, true)]
        }
        ShapeKind::Check => vec![part(vec![pos2(r.min.x, c.y), pos2(r.min.x + r.width() * 0.38, r.max.y), r.right_top()], false)],
        ShapeKind::Cross => vec![part(vec![r.left_top(), r.right_bottom()], false), part(vec![r.right_top(), r.left_bottom()], false)],
        ShapeKind::Heart => {
            let pts = (0..80)
                .map(|i| {
                    let t = i as f32 / 80.0 * std::f32::consts::TAU;
                    let x = 16.0 * t.sin().powi(3);
                    let y = -(13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos());
                    pos2(c.x + x / 17.0 * half.x, c.y + y / 17.0 * half.y + half.y * 0.1)
                })
                .collect();
            vec![part(pts, true)]
        }
        ShapeKind::Lightning => {
            let p = |x: f32, y: f32| pos2(r.min.x + x * r.width(), r.min.y + y * r.height());
            vec![part(vec![p(0.55, 0.0), p(0.15, 0.55), p(0.45, 0.55), p(0.3, 1.0), p(0.85, 0.38), p(0.55, 0.38), p(0.75, 0.0)], true)]
        }
        ShapeKind::Axes => {
            let mut v = vec![
                Part { pts: vec![pos2(r.min.x, c.y), pos2(r.max.x, c.y)], closed: false, arrow_start: false, arrow_end: true },
                Part { pts: vec![pos2(c.x, r.max.y), pos2(c.x, r.min.y)], closed: false, arrow_start: false, arrow_end: true },
            ];
            let step = 20.0;
            let mut x = c.x + step;
            while x < r.max.x - 14.0 {
                v.push(part(vec![pos2(x, c.y - 3.0), pos2(x, c.y + 3.0)], false));
                v.push(part(vec![pos2(2.0 * c.x - x, c.y - 3.0), pos2(2.0 * c.x - x, c.y + 3.0)], false));
                x += step;
            }
            let mut y = c.y + step;
            while y < r.max.y - 4.0 {
                v.push(part(vec![pos2(c.x - 3.0, y), pos2(c.x + 3.0, y)], false));
                if 2.0 * c.y - y > r.min.y + 14.0 {
                    v.push(part(vec![pos2(c.x - 3.0, 2.0 * c.y - y), pos2(c.x + 3.0, 2.0 * c.y - y)], false));
                }
                y += step;
            }
            v
        }
        ShapeKind::Grid => {
            let step = 24.0;
            let mut v = Vec::new();
            let mut x = r.min.x;
            while x <= r.max.x + 0.1 {
                v.push(part(vec![pos2(x, r.min.y), pos2(x, r.max.y)], false));
                x += step;
            }
            let mut y = r.min.y;
            while y <= r.max.y + 0.1 {
                v.push(part(vec![pos2(r.min.x, y), pos2(r.max.x, y)], false));
                y += step;
            }
            v
        }
    }
}
