//! Live input smoothing: a One-Euro filter (removes hand jitter at low speed
//! without adding lag at high speed), an optional pulled-string stabilizer,
//! and simulated pressure for mice and fingers.

use egui::{Pos2, Vec2};

struct LowPass {
    y: Option<f32>,
}

impl LowPass {
    fn filter(&mut self, x: f32, a: f32) -> f32 {
        let y = match self.y {
            Some(p) => p + a * (x - p),
            None => x,
        };
        self.y = Some(y);
        y
    }
}

fn alpha(cutoff: f32, dt: f32) -> f32 {
    let tau = 1.0 / (std::f32::consts::TAU * cutoff);
    1.0 / (1.0 + tau / dt)
}

/// One-Euro filter for a 2D point.
pub struct OneEuro {
    min_cutoff: f32,
    beta: f32,
    x: LowPass,
    y: LowPass,
    dx: LowPass,
    dy: LowPass,
    last: Option<(Pos2, f64)>,
}

impl OneEuro {
    /// `smoothing` in 0..1 (0 = off); speeds are measured in screen pixels.
    pub fn new(smoothing: f32) -> Self {
        let s = smoothing.clamp(0.0, 1.0);
        // Light smoothing by default: ~6 Hz cutoff when the hand is slow.
        let min_cutoff = 40.0 * (1.0 - s).powi(3) + 0.6;
        Self { min_cutoff, beta: 0.004 + 0.02 * (1.0 - s), x: LowPass { y: None }, y: LowPass { y: None }, dx: LowPass { y: None }, dy: LowPass { y: None }, last: None }
    }

    pub fn filter(&mut self, p: Pos2, t: f64, zoom: f32) -> Pos2 {
        let (prev, pt) = match self.last {
            Some(l) => l,
            None => {
                self.last = Some((p, t));
                self.x.filter(p.x, 1.0);
                self.y.filter(p.y, 1.0);
                return p;
            }
        };
        let dt = ((t - pt) as f32).clamp(0.001, 0.1);
        let ad = alpha(1.0, dt);
        let vx = self.dx.filter((p.x - prev.x) / dt, ad);
        let vy = self.dy.filter((p.y - prev.y) / dt, ad);
        let speed = Vec2::new(vx, vy).length() * zoom;
        let cutoff = self.min_cutoff + self.beta * speed;
        let a = alpha(cutoff, dt);
        let out = Pos2::new(self.x.filter(p.x, a), self.y.filter(p.y, a));
        self.last = Some((p, t));
        out
    }
}

/// Pulled-string stabilizer: the brush follows the pointer only when the
/// string is taut, which removes wobble from slow lines.
pub struct Stabilizer {
    len: f32,
    brush: Option<Pos2>,
}

impl Stabilizer {
    /// `amount` 0..1 → string length up to 40 screen pixels.
    pub fn new(amount: f32) -> Self {
        Self { len: amount.clamp(0.0, 1.0) * 40.0, brush: None }
    }
    pub fn active(&self) -> bool {
        self.len > 0.5
    }
    pub fn filter(&mut self, p: Pos2, zoom: f32) -> Option<Pos2> {
        let len = self.len / zoom.max(0.05);
        match self.brush {
            None => {
                self.brush = Some(p);
                Some(p)
            }
            Some(b) => {
                let d = p - b;
                if d.length() <= len {
                    return None;
                }
                let nb = p - d.normalized() * len;
                self.brush = Some(nb);
                Some(nb)
            }
        }
    }
}

/// Pressure for devices without it: faster movement gives a thinner line.
pub struct SimPressure {
    p: f32,
    last: Option<(Pos2, f64)>,
    pub count: usize,
}

impl SimPressure {
    pub fn new() -> Self {
        Self { p: 0.75, last: None, count: 0 }
    }
    pub fn next(&mut self, pos: Pos2, t: f64, zoom: f32) -> f32 {
        self.count += 1;
        if let Some((lp, lt)) = self.last {
            let dt = ((t - lt) as f32).max(0.004);
            let v = (pos - lp).length() * zoom / dt;
            let target = (1.0 - v / 2600.0).clamp(0.35, 1.0);
            self.p += (target - self.p) * 0.25;
        }
        self.last = Some((pos, t));
        // Taper in at the start of the stroke.
        let ramp = (self.count as f32 / 6.0).min(1.0);
        self.p * (0.45 + 0.55 * ramp)
    }
}

/// Tapers the last points of a stroke drawn without real pressure.
pub fn taper_end(points: &mut [crate::model::InkPoint]) {
    let n = points.len();
    let k = n.min(6);
    for i in 0..k {
        let idx = n - 1 - i;
        let f = 0.45 + 0.55 * (i as f32 / k as f32);
        points[idx].2 *= f;
    }
}

/// Detects a scratch-out zigzag: many direction reversals in a compact area.
pub fn is_scratch(points: &[Pos2]) -> bool {
    if points.len() < 12 {
        return false;
    }
    let mut bb = egui::Rect::NOTHING;
    for p in points {
        bb.extend_with(*p);
    }
    let diag = bb.size().length();
    let path: f32 = points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if diag < 8.0 || path < diag * 3.0 {
        return false;
    }
    // Count reversals along the main axis.
    let horizontal = bb.width() >= bb.height();
    let mut reversals = 0;
    let mut dir = 0.0f32;
    let mut acc = 0.0f32;
    for w in points.windows(2) {
        let d = if horizontal { w[1].x - w[0].x } else { w[1].y - w[0].y };
        acc += d;
        if acc.abs() > diag * 0.15 {
            let s = acc.signum();
            if dir != 0.0 && s != dir {
                reversals += 1;
            }
            dir = s;
            acc = 0.0;
        }
    }
    reversals >= 4
}
