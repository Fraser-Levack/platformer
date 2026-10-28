//! Jelly membrane on top of terrain: a 1D spring-wave simulation.
//!
//! Every walkable top edge (tissue tops and one-way membranes) is a "strip" of
//! points spaced `SPACING` apart. Each point springs back to rest and pulls on
//! its neighbours, so pushing one point sends ripples along the surface.
//! This is visual only; collision still uses the flat tile grid.

use crate::level::{Level, Slope, Tile, TILE};
use macroquad::prelude::*;
use std::collections::HashMap;

pub const SPACING: f32 = 8.0;
const STIFFNESS: f32 = 160.0;
const DAMPING: f32 = 6.0;
const SPREAD: f32 = 3500.0;
const MAX_OFFSET: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceKind {
    /// Top of solid tissue.
    Tissue,
    /// Floating one-way membrane.
    Membrane,
    /// Surface of an acid pool.
    Acid,
}

pub struct Strip {
    pub kind: SurfaceKind,
    pub row: i32,
    /// Left edge in world pixels.
    pub x0: f32,
    /// Right edge in world pixels.
    pub x1: f32,
    /// Whether each end is a free, rounded edge (vs. butting into a wall).
    pub cap_left: bool,
    pub cap_right: bool,
    /// Displacement per point; positive = pushed down.
    pub h: Vec<f32>,
    v: Vec<f32>,
    force: Vec<f32>,
}

impl Strip {
    pub fn top(&self) -> f32 {
        self.row as f32 * TILE
    }

    /// Interpolated displacement at world x.
    pub fn offset_at(&self, x: f32) -> f32 {
        let f = ((x - self.x0) / SPACING).clamp(0.0, (self.h.len() - 1) as f32);
        let i = (f.floor() as usize).min(self.h.len() - 2);
        let t = f - i as f32;
        self.h[i] + (self.h[i + 1] - self.h[i]) * t
    }

    /// Kick this strip directly (used for acid, which has no tile lookup by the player).
    pub fn kick(&mut self, x: f32, speed: f32, radius: f32) {
        let hits: Vec<_> = self.points_near(x, radius).collect();
        for (i, w) in hits {
            self.v[i] += speed * w;
        }
    }

    fn points_near(&self, x: f32, radius: f32) -> impl Iterator<Item = (usize, f32)> + '_ {
        let x0 = self.x0;
        (0..self.h.len()).filter_map(move |i| {
            let d = (x0 + i as f32 * SPACING - x).abs();
            (d < radius).then(|| (i, 1.0 - d / radius))
        })
    }
}

pub struct Surfaces {
    pub strips: Vec<Strip>,
    lookup: HashMap<(i32, i32), usize>,
}

fn kind_at(level: &Level, x: i32, y: i32) -> Option<SurfaceKind> {
    match level.tile(x, y) {
        Tile::Solid if !matches!(level.tile(x, y - 1), Tile::Solid | Tile::Bounce | Tile::Crumble | Tile::Slope(_)) => {
            Some(SurfaceKind::Tissue)
        }
        Tile::OneWay => Some(SurfaceKind::Membrane),
        Tile::Acid if level.tile(x, y - 1) != Tile::Acid => Some(SurfaceKind::Acid),
        _ => None,
    }
}

impl Surfaces {
    pub fn build(level: &Level) -> Self {
        let mut strips = Vec::new();
        let mut lookup = HashMap::new();
        for y in 0..level.height {
            let mut x = 0;
            while x < level.width {
                let Some(kind) = kind_at(level, x, y) else {
                    x += 1;
                    continue;
                };
                let start = x;
                while x < level.width && kind_at(level, x, y) == Some(kind) {
                    lookup.insert((x, y), strips.len());
                    x += 1;
                }
                let n = ((x - start) as f32 * TILE / SPACING) as usize + 1;
                let (cap_left, cap_right) = match kind {
                    SurfaceKind::Membrane => (true, true),
                    SurfaceKind::Acid => (false, false),
                    // No rounded cap where the strip meets the top of a ramp.
                    SurfaceKind::Tissue => (
                        !matches!(level.tile(start - 1, y), Tile::Solid | Tile::Slope(Slope { right: 2, .. })),
                        !matches!(level.tile(x, y), Tile::Solid | Tile::Slope(Slope { left: 2, .. })),
                    ),
                };
                strips.push(Strip {
                    kind,
                    row: y,
                    x0: start as f32 * TILE,
                    x1: x as f32 * TILE,
                    cap_left,
                    cap_right,
                    h: vec![0.0; n],
                    v: vec![0.0; n],
                    force: vec![0.0; n],
                });
            }
        }
        Surfaces { strips, lookup }
    }

    pub fn strip_at(&self, x: f32, row: i32) -> Option<&Strip> {
        let idx = self.lookup.get(&((x / TILE).floor() as i32, row))?;
        Some(&self.strips[*idx])
    }

    fn strip_at_mut(&mut self, x: f32, row: i32) -> Option<&mut Strip> {
        let idx = *self.lookup.get(&((x / TILE).floor() as i32, row))?;
        Some(&mut self.strips[idx])
    }

    /// Visual displacement of the surface whose top is `row` at world x (0 if none).
    pub fn offset(&self, x: f32, row: i32) -> f32 {
        self.strip_at(x, row).map_or(0.0, |s| s.offset_at(x))
    }

    /// Instant kick (e.g. landing). Positive pushes the surface down.
    pub fn impulse(&mut self, x: f32, row: i32, speed: f32, radius: f32) {
        if let Some(s) = self.strip_at_mut(x, row) {
            let hits: Vec<_> = s.points_near(x, radius).collect();
            for (i, w) in hits {
                s.v[i] += speed * w;
            }
        }
    }

    /// Continuous push for this step (e.g. something standing on it).
    pub fn press(&mut self, x: f32, row: i32, force: f32, radius: f32) {
        if let Some(s) = self.strip_at_mut(x, row) {
            let hits: Vec<_> = s.points_near(x, radius).collect();
            for (i, w) in hits {
                s.force[i] += force * w;
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
        for s in &mut self.strips {
            let n = s.h.len();
            for i in 0..n {
                let left = if i > 0 { s.h[i - 1] } else if s.cap_left { s.h[i] } else { 0.0 };
                let right = if i + 1 < n { s.h[i + 1] } else if s.cap_right { s.h[i] } else { 0.0 };
                let lap = left + right - 2.0 * s.h[i];
                let a = -STIFFNESS * s.h[i] - DAMPING * s.v[i] + SPREAD * lap + s.force[i];
                s.v[i] += a * dt;
            }
            for i in 0..n {
                s.h[i] = (s.h[i] + s.v[i] * dt).clamp(-MAX_OFFSET, MAX_OFFSET);
                s.force[i] = 0.0;
            }
        }
    }
}
