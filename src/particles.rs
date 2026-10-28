//! Particle handler.
//!
//! - `Spec` describes *how* particles look and move; presets live in [`fx`].
//! - `Particles::emit` spawns a burst from a spec.
//! - `Emitter` spawns continuously at a given rate.
//! - Particles are drawn per `Layer`, so ambient motes sit behind the terrain
//!   and effects sit in front of everything.

use crate::palette::*;
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// Flat dot.
    Dot,
    /// Jelly droplet that stretches along its velocity.
    Blob,
    /// Hollow bubble that wobbles as it rises.
    Bubble,
    /// Expanding (or shrinking) membrane ring.
    Ring,
    /// Small bright spark.
    Spark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Back,
    Front,
}

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
    pub life: f32,
    pub max_life: f32,
    pub size: f32,
    pub size_end: f32,
    pub color: Color,
    pub gravity: f32,
    pub drag: f32,
    pub shape: Shape,
    pub layer: Layer,
    pub fade_in: f32,
    pub phase: f32,
}

/// Recipe for spawning particles. All ranges are (min, max).
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub shape: Shape,
    pub layer: Layer,
    /// Center direction in radians (0 = right, -PI/2 = up).
    pub angle: f32,
    /// Total cone width in radians (TAU = every direction).
    pub spread: f32,
    pub speed: (f32, f32),
    pub life: (f32, f32),
    pub size: (f32, f32),
    /// End size as a multiple of the start size.
    pub size_end: f32,
    pub colors: &'static [Paint],
    pub gravity: f32,
    pub drag: f32,
    /// Random spawn offset radius.
    pub jitter: f32,
    pub fade_in: f32,
}

/// Which theme colour a particle uses (resolved when it spawns, so themes apply).
#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Hazard,
    Pickup,
    Player,
    Terrain,
    Orb,
    Fixed,
}

#[derive(Clone, Copy, Debug)]
pub struct Paint {
    slot: Slot,
    rgb: Color,
    light: f32,
    alpha: f32,
}

impl Paint {
    pub const fn slot(slot: Slot) -> Self {
        Paint { slot, rgb: WHITE, light: 0.0, alpha: 1.0 }
    }
    pub const fn rgb(rgb: Color) -> Self {
        Paint { slot: Slot::Fixed, rgb, light: 0.0, alpha: 1.0 }
    }
    /// Blend toward white by `t`.
    pub const fn light(self, t: f32) -> Self {
        Paint { light: t, ..self }
    }
    pub const fn alpha(self, a: f32) -> Self {
        Paint { alpha: a, ..self }
    }
    pub fn resolve(&self) -> Color {
        let base = match self.slot {
            Slot::Hazard => hazard(),
            Slot::Pickup => pickup(),
            Slot::Player => player(),
            Slot::Terrain => terrain(),
            Slot::Orb => orb(),
            Slot::Fixed => self.rgb,
        };
        alpha(lighten(base, self.light), self.alpha)
    }
}

pub const P_HAZARD: Paint = Paint::slot(Slot::Hazard);
pub const P_PICKUP: Paint = Paint::slot(Slot::Pickup);
pub const P_PLAYER: Paint = Paint::slot(Slot::Player);
pub const P_TERRAIN: Paint = Paint::slot(Slot::Terrain);
pub const P_ORB: Paint = Paint::slot(Slot::Orb);
pub const P_WHITE: Paint = Paint::rgb(WHITE);

pub const BASE: Spec = Spec {
    shape: Shape::Dot,
    layer: Layer::Front,
    angle: 0.0,
    spread: TAU,
    speed: (40.0, 120.0),
    life: (0.4, 0.8),
    size: (2.0, 4.0),
    size_end: 0.2,
    colors: &[P_WHITE],
    gravity: 0.0,
    drag: 1.0,
    jitter: 0.0,
    fade_in: 0.0,
};

pub struct Particles {
    list: Vec<Particle>,
    cap: usize,
}

impl Particles {
    pub fn new(cap: usize) -> Self {
        Particles { list: Vec::with_capacity(cap), cap }
    }

    pub fn count_layer(&self, layer: Layer) -> usize {
        self.list.iter().filter(|p| p.layer == layer).count()
    }

    pub fn spawn(&mut self, p: Particle) {
        if self.list.len() >= self.cap {
            // Drop the oldest-ish particle rather than refusing new effects.
            self.list.swap_remove(0);
        }
        self.list.push(p);
    }

    pub fn emit(&mut self, spec: Spec, pos: Vec2, count: usize) {
        self.emit_with_vel(spec, pos, count, Vec2::ZERO);
    }

    /// Emit with an inherited base velocity (e.g. the player's motion).
    pub fn emit_with_vel(&mut self, spec: Spec, pos: Vec2, count: usize, base_vel: Vec2) {
        for _ in 0..count {
            let a = spec.angle + gen_range(-spec.spread * 0.5, spec.spread * 0.5);
            let speed = gen_range(spec.speed.0, spec.speed.1);
            let life = gen_range(spec.life.0, spec.life.1);
            let size = gen_range(spec.size.0, spec.size.1);
            let jitter = if spec.jitter > 0.0 {
                let ja = gen_range(0.0, TAU);
                vec2(ja.cos(), ja.sin()) * gen_range(0.0, spec.jitter)
            } else {
                Vec2::ZERO
            };
            let color = spec.colors[gen_range(0, spec.colors.len())].resolve();
            self.spawn(Particle {
                pos: pos + jitter,
                vel: vec2(a.cos(), a.sin()) * speed + base_vel,
                life,
                max_life: life,
                size,
                size_end: size * spec.size_end,
                color,
                gravity: spec.gravity,
                drag: spec.drag,
                shape: spec.shape,
                layer: spec.layer,
                fade_in: spec.fade_in,
                phase: gen_range(0.0, TAU),
            });
        }
    }

    pub fn update(&mut self, dt: f32) {
        for p in &mut self.list {
            p.vel.y += p.gravity * dt;
            p.vel *= (1.0 - p.drag * dt).max(0.0);
            p.pos += p.vel * dt;
            if p.shape == Shape::Bubble {
                p.pos.x += (p.phase + p.life * 7.0).sin() * 18.0 * dt;
            }
            p.life -= dt;
        }
        self.list.retain(|p| p.life > 0.0);
    }

    /// Draw one layer. `to_screen` maps world -> screen, `zoom` is pixels per world unit.
    pub fn draw(&self, layer: Layer, to_screen: impl Fn(Vec2) -> Vec2, zoom: f32) {
        for p in self.list.iter().filter(|p| p.layer == layer) {
            let t = (p.life / p.max_life).clamp(0.0, 1.0);
            let k = 1.0 - t;
            let mut a = t;
            if p.fade_in > 0.0 {
                a *= ((p.max_life - p.life) / p.fade_in).min(1.0);
            }
            let r = (p.size + (p.size_end - p.size) * k) * zoom;
            let s = to_screen(p.pos);
            match p.shape {
                Shape::Dot => {
                    draw_circle(s.x, s.y, r, alpha(p.color, a));
                }
                Shape::Spark => {
                    draw_circle(s.x, s.y, r, alpha(p.color, a));
                }
                Shape::Blob => {
                    let speed = p.vel.length();
                    let stretch = 1.0 + (speed / 300.0).min(1.2);
                    let ang = p.vel.y.atan2(p.vel.x).to_degrees();
                    draw_ellipse(s.x, s.y, r * stretch, r / stretch.sqrt(), ang, alpha(p.color, a * 0.9));
                }
                Shape::Bubble => {
                    draw_circle_lines(s.x, s.y, r, (1.2 * zoom).max(1.0), alpha(p.color, a * 0.8));
                }
                Shape::Ring => {
                    draw_circle_lines(s.x, s.y, r, (2.5 * zoom * t).max(1.0), alpha(p.color, a * 0.8));
                }
            }
        }
    }
}

/// Continuous emission at `rate` particles per second.
pub struct Emitter {
    pub spec: Spec,
    pub rate: f32,
    acc: f32,
}

impl Emitter {
    pub fn new(spec: Spec, rate: f32) -> Self {
        Emitter { spec, rate, acc: gen_range(0.0, 1.0) }
    }

    pub fn tick(&mut self, particles: &mut Particles, pos: Vec2, dt: f32) {
        self.acc += self.rate * dt;
        while self.acc >= 1.0 {
            particles.emit(self.spec, pos, 1);
            self.acc -= 1.0;
        }
    }
}

/// Effect presets.
pub mod fx {
    use super::*;

    const UP: f32 = -PI / 2.0;

    pub const MOTE: Spec = Spec {
        layer: Layer::Back,
        speed: (2.0, 14.0),
        life: (4.0, 8.0),
        size: (0.8, 2.2),
        size_end: 1.0,
        colors: &[
            P_TERRAIN.alpha(0.55),
            P_PLAYER.alpha(0.35),
            P_WHITE.alpha(0.3),
        ],
        gravity: -2.0,
        drag: 0.1,
        fade_in: 1.0,
        ..BASE
    };

    pub const LAND_SPLASH: Spec = Spec {
        shape: Shape::Blob,
        angle: UP,
        spread: 2.6,
        speed: (60.0, 230.0),
        life: (0.3, 0.6),
        size: (1.8, 4.0),
        size_end: 0.3,
        colors: &[P_TERRAIN, P_TERRAIN.light(0.4), P_PLAYER],
        gravity: 900.0,
        drag: 0.8,
        jitter: 4.0,
        ..BASE
    };

    pub const JUMP_BUBBLES: Spec = Spec {
        shape: Shape::Bubble,
        angle: UP,
        spread: 2.2,
        speed: (20.0, 70.0),
        life: (0.4, 0.9),
        size: (1.5, 3.5),
        size_end: 1.3,
        colors: &[P_PLAYER, P_PLAYER.light(0.6)],
        gravity: -80.0,
        drag: 2.0,
        jitter: 6.0,
        ..BASE
    };

    pub const DASH_SPARKS: Spec = Spec {
        shape: Shape::Spark,
        spread: 1.2,
        speed: (80.0, 260.0),
        life: (0.2, 0.45),
        size: (1.5, 3.0),
        colors: &[P_PLAYER, P_TERRAIN, P_WHITE],
        drag: 4.0,
        jitter: 5.0,
        ..BASE
    };

    pub const DASH_RING: Spec = Spec {
        shape: Shape::Ring,
        speed: (0.0, 0.0),
        life: (0.25, 0.25),
        size: (8.0, 8.0),
        size_end: 4.0,
        colors: &[P_PLAYER.light(0.6)],
        ..BASE
    };

    pub const DEATH_POP: Spec = Spec {
        shape: Shape::Blob,
        speed: (150.0, 480.0),
        life: (0.5, 1.0),
        size: (2.5, 6.5),
        size_end: 0.3,
        colors: &[P_PLAYER, P_PLAYER.light(0.5), P_TERRAIN],
        gravity: 700.0,
        drag: 1.2,
        jitter: 6.0,
        ..BASE
    };

    pub const DEATH_RING: Spec = Spec {
        shape: Shape::Ring,
        speed: (0.0, 0.0),
        life: (0.45, 0.45),
        size: (8.0, 8.0),
        size_end: 9.0,
        colors: &[P_HAZARD],
        ..BASE
    };

    pub const RESPAWN_RING: Spec = Spec {
        shape: Shape::Ring,
        speed: (0.0, 0.0),
        life: (0.35, 0.35),
        size: (48.0, 48.0),
        size_end: 0.25,
        colors: &[P_PLAYER],
        ..BASE
    };

    pub const NUTRIENT_BURST: Spec = Spec {
        shape: Shape::Spark,
        speed: (60.0, 200.0),
        life: (0.3, 0.6),
        size: (1.5, 3.0),
        colors: &[P_PICKUP, P_WHITE, P_PICKUP.light(0.5)],
        drag: 3.0,
        ..BASE
    };

    pub const NUTRIENT_RING: Spec = Spec {
        shape: Shape::Ring,
        speed: (0.0, 0.0),
        life: (0.3, 0.3),
        size: (6.0, 6.0),
        size_end: 4.0,
        colors: &[P_PICKUP],
        ..BASE
    };

    pub const WALL_DRIP: Spec = Spec {
        shape: Shape::Blob,
        angle: UP,
        spread: 1.0,
        speed: (10.0, 50.0),
        life: (0.3, 0.5),
        size: (1.5, 3.0),
        colors: &[P_TERRAIN, P_PLAYER],
        gravity: 400.0,
        ..BASE
    };

    pub const SKID: Spec = Spec {
        shape: Shape::Blob,
        angle: UP,
        spread: 1.4,
        speed: (30.0, 110.0),
        life: (0.2, 0.4),
        size: (1.2, 2.5),
        colors: &[P_TERRAIN, P_TERRAIN.light(0.4)],
        gravity: 700.0,
        ..BASE
    };

    pub const CHECKPOINT_BUBBLES: Spec = Spec {
        shape: Shape::Bubble,
        angle: UP,
        spread: 0.8,
        speed: (15.0, 40.0),
        life: (1.0, 1.8),
        size: (1.5, 3.5),
        size_end: 1.4,
        colors: &[P_PLAYER],
        drag: 0.3,
        jitter: 8.0,
        ..BASE
    };

    pub const CHECKPOINT_BURST: Spec = Spec {
        shape: Shape::Bubble,
        speed: (80.0, 220.0),
        life: (0.5, 1.0),
        size: (2.0, 5.0),
        colors: &[P_PLAYER, P_WHITE],
        drag: 2.5,
        gravity: -60.0,
        ..BASE
    };

    pub const GOAL_SWIRL: Spec = Spec {
        shape: Shape::Spark,
        speed: (10.0, 40.0),
        life: (0.8, 1.6),
        size: (1.2, 2.6),
        colors: &[P_PICKUP, P_HAZARD, P_WHITE],
        gravity: -40.0,
        drag: 0.5,
        jitter: 44.0,
        fade_in: 0.2,
        ..BASE
    };

    pub const GOAL_BURST: Spec = Spec {
        shape: Shape::Blob,
        speed: (120.0, 420.0),
        life: (0.8, 1.5),
        size: (2.5, 6.0),
        colors: &[P_PICKUP, P_HAZARD, P_PLAYER, P_WHITE],
        gravity: 250.0,
        drag: 1.0,
        jitter: 20.0,
        ..BASE
    };

    pub const HAZARD_SPORE: Spec = Spec {
        shape: Shape::Dot,
        angle: UP,
        spread: 1.6,
        speed: (8.0, 30.0),
        life: (0.8, 1.6),
        size: (1.0, 2.0),
        colors: &[P_HAZARD, P_HAZARD.light(0.4)],
        gravity: -15.0,
        drag: 0.5,
        jitter: 8.0,
        fade_in: 0.2,
        ..BASE
    };

    pub const SPRINT_TRAIL: Spec = Spec {
        shape: Shape::Blob,
        spread: 0.6,
        speed: (40.0, 120.0),
        life: (0.2, 0.4),
        size: (1.5, 3.0),
        colors: &[P_PLAYER, P_PLAYER.light(0.5)],
        gravity: 300.0,
        drag: 2.0,
        ..BASE
    };

    pub const CHARGE_READY: Spec = Spec {
        shape: Shape::Spark,
        angle: UP,
        spread: 2.4,
        speed: (30.0, 90.0),
        life: (0.25, 0.5),
        size: (1.2, 2.4),
        colors: &[P_PLAYER, P_PICKUP, P_WHITE],
        drag: 3.0,
        jitter: 10.0,
        ..BASE
    };

    pub const ORB_SPARKLE: Spec = Spec {
        shape: Shape::Spark,
        speed: (10.0, 30.0),
        life: (0.3, 0.6),
        size: (1.0, 2.0),
        colors: &[P_ORB, P_WHITE],
        drag: 2.0,
        jitter: 14.0,
        ..BASE
    };

    pub const ORB_BURST: Spec = Spec {
        shape: Shape::Spark,
        speed: (80.0, 240.0),
        life: (0.3, 0.6),
        size: (1.5, 3.2),
        colors: &[P_ORB, P_WHITE, P_TERRAIN],
        drag: 3.0,
        ..BASE
    };

    pub const ORB_RING: Spec = Spec {
        shape: Shape::Ring,
        speed: (0.0, 0.0),
        life: (0.35, 0.35),
        size: (10.0, 10.0),
        size_end: 3.5,
        colors: &[P_ORB],
        ..BASE
    };

    pub const BOUNCE_BURST: Spec = Spec {
        shape: Shape::Spark,
        angle: UP,
        spread: 1.8,
        speed: (120.0, 320.0),
        life: (0.25, 0.5),
        size: (1.5, 3.0),
        colors: &[P_PICKUP, P_WHITE, P_PLAYER],
        gravity: 400.0,
        drag: 2.0,
        jitter: 10.0,
        ..BASE
    };

    pub const BUBBLE_POP: Spec = Spec {
        shape: Shape::Bubble,
        speed: (60.0, 160.0),
        life: (0.3, 0.6),
        size: (1.5, 3.0),
        colors: &[P_TERRAIN.light(0.6), P_WHITE],
        drag: 3.0,
        ..BASE
    };

    pub const BUBBLE_RING: Spec = Spec {
        shape: Shape::Ring,
        speed: (0.0, 0.0),
        life: (0.3, 0.3),
        size: (10.0, 10.0),
        size_end: 3.0,
        colors: &[P_TERRAIN.light(0.6)],
        ..BASE
    };

    pub const CRUMBLE_DUST: Spec = Spec {
        shape: Shape::Blob,
        angle: PI / 2.0,
        spread: 2.0,
        speed: (20.0, 90.0),
        life: (0.4, 0.8),
        size: (2.0, 4.5),
        colors: &[P_TERRAIN, P_TERRAIN.light(0.3)],
        gravity: 600.0,
        jitter: 12.0,
        ..BASE
    };

    pub const STOMP_POP: Spec = Spec {
        shape: Shape::Blob,
        speed: (100.0, 300.0),
        life: (0.35, 0.7),
        size: (2.0, 5.0),
        colors: &[P_HAZARD, P_HAZARD.light(0.4), P_WHITE],
        gravity: 700.0,
        drag: 1.0,
        jitter: 6.0,
        ..BASE
    };

    pub const ACID_SPLASH: Spec = Spec {
        shape: Shape::Blob,
        angle: UP,
        spread: 1.6,
        speed: (100.0, 300.0),
        life: (0.4, 0.8),
        size: (2.0, 4.5),
        colors: &[P_HAZARD, P_HAZARD.light(0.3)],
        gravity: 900.0,
        jitter: 8.0,
        ..BASE
    };

    pub const ACID_BUBBLE: Spec = Spec {
        shape: Shape::Bubble,
        angle: UP,
        spread: 0.5,
        speed: (10.0, 30.0),
        life: (0.5, 1.0),
        size: (1.5, 3.0),
        size_end: 1.5,
        colors: &[P_HAZARD.light(0.4)],
        gravity: -30.0,
        drag: 0.5,
        ..BASE
    };
}
