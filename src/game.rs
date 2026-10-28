use crate::enemies::{Contact, Enemy};
use crate::input::InputState;
use crate::level::{Level, Tile, TILE};
use crate::particles::{fx, Emitter, Layer, Particles, Spec};
use crate::player::{JumpKind, Move, Player, PlayerEvent, W};
use crate::surface::{SurfaceKind, Surfaces};
use crate::synth::Sfx;
use crate::tuning;
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::f32::consts::PI;

/// How many tiles tall the view is; the zoom adapts to the window size.
pub const VIEW_TILES_TALL: f32 = 13.0;
pub const GHOST_LIFE: f32 = 0.25;
const RESPAWN_DELAY: f32 = 0.6;
const AMBIENT_MOTES: usize = 160;
const MAX_PARTICLES: usize = 3000;
/// Distance walked between footstep sounds.
const STEP_DIST: f32 = 30.0;
/// Ground distance for one radian of the robot's run cycle.
const STRIDE_LEN: f32 = 5.0;

/// How the robot looks at one moment: enough to draw it, or an afterimage of it.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    /// Bottom centre, where the feet touch down.
    pub feet: Vec2,
    pub vel: Vec2,
    pub facing: f32,
    pub state: Move,
    pub crouching: bool,
    pub wall_dir: i32,
    pub gradient: f32,
    pub scale: Vec2,
    /// Run-cycle phase (radians).
    pub stride: f32,
}

/// Afterimage of the robot while dashing or at top speed.
pub struct Ghost {
    pub pose: Pose,
    pub life: f32,
}

/// Collectible nutrient. It jiggles on a spring and gets pulled toward the player.
pub struct Nutrient {
    pub home: Vec2,
    pub off: Vec2,
    vel: Vec2,
    pub taken: bool,
}

/// Touch to store one dash. Regrows after being used.
pub struct DashOrb {
    pub pos: Vec2,
    /// Seconds until available again (0 = available).
    pub regrow: f32,
    pub wobble: f32,
}

pub const ORB_REGROW_TIME: f32 = 3.0;

pub struct Checkpoint {
    pub pos: Vec2,
    pub active: bool,
    pub wobble: f32,
    emitter: Emitter,
}

/// Touch to store one extra mid-air jump. Regrows after being used.
pub struct JumpBubble {
    pub pos: Vec2,
    pub regrow: f32,
    pub wobble: f32,
}

pub const BUBBLE_REGROW_TIME: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CrumbleState {
    Solid,
    /// Stood on: shaking, seconds left before it dissolves.
    Shaking(f32),
    /// Dissolved: seconds left before it regrows.
    Gone(f32),
}

pub struct Crumble {
    pub x: i32,
    pub y: i32,
    pub state: CrumbleState,
}

/// A gate cell. Opens for good once enough coins are collected.
pub struct Gate {
    pub x: i32,
    pub y: i32,
    pub need: u32,
    /// Opening animation, 0 = shut, 1 = fully retracted.
    pub open: f32,
    pub opening: bool,
}

/// How close (in pixels, from the sign's cell centre) the player must be to read a sign.
const SIGN_RANGE: Vec2 = vec2(TILE * 2.0, TILE * 2.5);

pub const CRUMBLE_DELAY: f32 = 0.45;
pub const CRUMBLE_REGROW: f32 = 2.5;

pub struct Game {
    pub level: Level,
    pub player: Player,
    pub surfaces: Surfaces,
    pub particles: Particles,
    pub nutrients: Vec<Nutrient>,
    pub dash_orbs: Vec<DashOrb>,
    pub bubbles: Vec<JumpBubble>,
    pub enemies: Vec<Enemy>,
    pub crumbles: Vec<Crumble>,
    /// Bounce block squish animation, 1 = just hit, decays to 0.
    pub bounce_anim: std::collections::HashMap<(i32, i32), f32>,
    pub checkpoints: Vec<Checkpoint>,
    pub gates: Vec<Gate>,
    /// Fade (0..1) of each sign's text, parallel to `level.signs`.
    pub sign_alpha: Vec<f32>,
    pub goal_wobble: f32,
    goal_emitter: Emitter,
    spikes: Vec<Vec2>,
    respawn_point: Vec2,
    pub deaths: u32,
    pub time: f32,
    pub finished: bool,
    respawn_timer: f32,
    pub ghosts: Vec<Ghost>,
    pub cam: Vec2,
    look_ahead: f32,
    /// Smoothed zoom-out from speed (0 = normal view).
    zoom_out: f32,
    pub shake: f32,
    pub anim_t: f32,
    ghost_tick: u32,
    /// Editor preview: drawn but not simulated, player hidden.
    pub preview: bool,
    /// Zoom used for camera/ambient effects when not the default.
    pub view_zoom: Option<f32>,
    /// Sounds triggered this step, with relative volume; the session plays them.
    pub sfx: Vec<(Sfx, f32)>,
    step_dist: f32,
    /// Run-cycle phase for the robot's legs (radians).
    stride: f32,
}

pub fn zoom() -> f32 {
    screen_height() / (VIEW_TILES_TALL * TILE)
}

impl Game {
    /// Zoom for drawing: the editor's, or the default pulled back by speed.
    pub fn current_zoom(&self) -> f32 {
        self.view_zoom.unwrap_or_else(|| zoom() / (1.0 + self.zoom_out))
    }

    pub fn new(level: Level) -> Self {
        crate::palette::set_theme(level.theme);
        let player = Player::new(level.spawn);
        let cam = player.center();
        let mut spikes = Vec::new();
        for y in 0..level.height {
            for x in 0..level.width {
                if level.tile(x, y) == Tile::Spike {
                    spikes.push(vec2(x as f32 * TILE + TILE / 2.0, y as f32 * TILE + TILE * 0.6));
                }
            }
        }
        let mut level = level;
        // Gates that need nothing start open.
        let gates: Vec<Gate> = level
            .gates
            .iter()
            .map(|g| {
                let open = g.need == 0;
                Gate { x: g.x, y: g.y, need: g.need, open: if open { 1.0 } else { 0.0 }, opening: open }
            })
            .collect();
        for g in gates.iter().filter(|g| g.opening) {
            level.set_gone(g.x, g.y, true);
        }
        Game {
            gates,
            sign_alpha: vec![0.0; level.signs.len()],
            surfaces: Surfaces::build(&level),
            particles: Particles::new(MAX_PARTICLES),
            nutrients: level
                .coins
                .iter()
                .map(|c| Nutrient { home: *c, off: Vec2::ZERO, vel: Vec2::ZERO, taken: false })
                .collect(),
            bubbles: level
                .bubbles
                .iter()
                .map(|p| JumpBubble { pos: *p, regrow: 0.0, wobble: 0.0 })
                .collect(),
            enemies: level.enemies.iter().map(|(k, p)| Enemy::new(*k, *p)).collect(),
            crumbles: level
                .cells(Tile::Crumble)
                .into_iter()
                .map(|(x, y)| Crumble { x, y, state: CrumbleState::Solid })
                .collect(),
            bounce_anim: Default::default(),
            dash_orbs: level
                .dash_orbs
                .iter()
                .map(|p| DashOrb { pos: *p, regrow: 0.0, wobble: 0.0 })
                .collect(),
            checkpoints: level
                .checkpoints
                .iter()
                .map(|c| Checkpoint {
                    pos: *c,
                    active: false,
                    wobble: 0.0,
                    emitter: Emitter::new(fx::CHECKPOINT_BUBBLES, 4.0),
                })
                .collect(),
            goal_wobble: 0.0,
            goal_emitter: Emitter::new(fx::GOAL_SWIRL, 18.0),
            spikes,
            respawn_point: level.spawn,
            player,
            level,
            deaths: 0,
            time: 0.0,
            finished: false,
            respawn_timer: 0.0,
            ghosts: Vec::new(),
            cam,
            look_ahead: 0.0,
            zoom_out: 0.0,
            shake: 0.0,
            anim_t: 0.0,
            ghost_tick: 0,
            preview: false,
            view_zoom: None,
            sfx: Vec::new(),
            step_dist: 0.0,
            stride: 0.0,
        }
    }

    fn sound(&mut self, s: Sfx, volume: f32) {
        self.sfx.push((s, volume));
    }

    pub fn goal_center(&self) -> Vec2 {
        self.level.goal - vec2(0.0, 50.0)
    }

    pub fn checkpoint_cell(cp: &Checkpoint) -> Vec2 {
        cp.pos - vec2(0.0, 46.0)
    }

    /// Animate an editor preview: ambient effects only, no gameplay.
    pub fn tick_preview(&mut self, dt: f32) {
        self.anim_t += dt;
        self.particles.update(dt);
        self.surfaces.update(dt);
        self.update_ambient(dt);
    }

    /// One fixed-timestep simulation tick.
    pub fn step(&mut self, input: &InputState, dt: f32) {
        self.anim_t += dt;
        self.particles.update(dt);
        self.surfaces.update(dt);
        self.shake = (self.shake - dt * 2.5).max(0.0);
        for g in &mut self.ghosts {
            g.life -= dt;
        }
        self.ghosts.retain(|g| g.life > 0.0);
        self.update_ambient(dt);
        if self.finished {
            return;
        }
        if !self.player.dead {
            self.time += dt;
        }

        self.update_crumbles(dt);
        let prev_bottom = self.player.prev_pos.y + self.player.height();
        self.player.step(input, &self.level, dt);

        // Standing on a crumble block starts it shaking.
        for (x, y) in self.player.standing_on(&self.level, Tile::Crumble) {
            if let Some(c) = self.crumbles.iter_mut().find(|c| c.x == x && c.y == y) {
                if c.state == CrumbleState::Solid {
                    c.state = CrumbleState::Shaking(CRUMBLE_DELAY);
                    self.sfx.push((Sfx::CrumbleShake, 0.8));
                }
            }
        }

        self.update_enemies(input.jump_held, prev_bottom, dt);

        let feet = self.player.feet();
        let feet_row = (feet.y / TILE).round() as i32;
        let center = self.player.center();
        let events: Vec<PlayerEvent> = self.player.events.drain(..).collect();
        let max_fall = tuning::get().max_fall;
        for ev in events {
            match ev {
                PlayerEvent::Jump(kind) => {
                    let fast = self.player.speed01();
                    self.particles.emit(fx::JUMP_BUBBLES, feet, 6 + (fast * 6.0) as usize);
                    let sound = match kind {
                        JumpKind::Normal if fast < 0.5 => Sfx::Jump,
                        _ => Sfx::JumpLong,
                    };
                    self.sound(sound, 1.0);
                    self.surfaces.impulse(feet.x, feet_row, 140.0 + fast * 120.0, 18.0);
                    if kind != JumpKind::Normal {
                        let back = if self.player.vel.x > 0.0 { PI } else { 0.0 };
                        self.particles.emit(Spec { angle: back, ..fx::DASH_SPARKS }, feet, 10);
                    }
                }
                PlayerEvent::WallJump(dir) => {
                    let side = center + vec2(-dir as f32 * W / 2.0, 0.0);
                    let angle = if dir > 0 { -0.4 } else { PI + 0.4 };
                    self.particles.emit(Spec { angle, ..fx::SKID }, side, 10);
                    self.particles.emit(fx::JUMP_BUBBLES, side, 3);
                    self.sound(Sfx::WallJump, 1.0);
                }
                PlayerEvent::Perfect => {
                    self.sound(Sfx::Perfect, 1.0);
                    self.particles.emit(fx::CHARGE_READY, feet, 10);
                    self.particles.emit(Spec { size_end: 2.5, ..fx::DASH_RING }, feet, 1);
                }
                PlayerEvent::WallRun(dir) => {
                    let side = center + vec2(dir as f32 * W / 2.0, 6.0);
                    self.sound(Sfx::WallRun, 1.0);
                    self.particles.emit(Spec { angle: PI / 2.0, ..fx::SKID }, side, 8);
                }
                PlayerEvent::Vault => {
                    self.sound(Sfx::Jump, 0.6);
                    self.particles.emit(fx::JUMP_BUBBLES, feet, 4);
                }
                PlayerEvent::Mantle => {
                    self.sound(Sfx::Land, 0.5);
                    self.particles.emit(fx::JUMP_BUBBLES, feet, 5);
                }
                PlayerEvent::Launch => {
                    self.sound(Sfx::JumpBig, 0.7);
                    let back = if self.player.vel.x > 0.0 { PI + 0.5 } else { -0.5 };
                    self.particles.emit(Spec { angle: back, ..fx::DASH_SPARKS }, feet, 12);
                    self.shake = self.shake.max(0.06);
                }
                PlayerEvent::Land(speed) => {
                    self.surfaces.impulse(feet.x, feet_row, speed * 0.35, 22.0);
                    if speed > 120.0 {
                        self.sound(Sfx::Land, (speed / max_fall).clamp(0.25, 1.0));
                    }
                    if speed > 200.0 {
                        let n = (speed / max_fall * 14.0) as usize + 3;
                        self.particles.emit(fx::LAND_SPLASH, feet, n);
                    }
                    if speed > 0.9 * max_fall {
                        self.shake = self.shake.max(0.15);
                    }
                }
                PlayerEvent::Bounce(top) => {
                    let cell = ((top.x / TILE).floor() as i32, (top.y / TILE).round() as i32);
                    self.bounce_anim.insert(cell, 1.0);
                    self.sound(Sfx::Bounce, 1.0);
                    self.particles.emit(fx::BOUNCE_BURST, top, 14);
                    self.particles.emit(Spec { size_end: 3.5, ..fx::DASH_RING }, top, 1);
                    self.shake = self.shake.max(0.08);
                }
                PlayerEvent::AirJump => {
                    self.sound(Sfx::AirJump, 1.0);
                    self.particles.emit(fx::BUBBLE_POP, feet, 10);
                    self.particles.emit(fx::BUBBLE_RING, feet, 1);
                }
                PlayerEvent::Dash(dir) => {
                    self.shake = self.shake.max(0.12);
                    self.sound(Sfx::Dash, 1.0);
                    let back = (-dir.y).atan2(-dir.x);
                    self.particles.emit(Spec { angle: back, ..fx::DASH_SPARKS }, center, 14);
                    self.particles.emit(fx::DASH_RING, center, 1);
                }
                PlayerEvent::SlideStart => {
                    self.sound(Sfx::Slide, 1.0);
                    let back = if self.player.vel.x > 0.0 { PI + 0.4 } else { -0.4 };
                    self.particles.emit(Spec { angle: back, ..fx::SKID }, feet, 8);
                    self.surfaces.impulse(feet.x, feet_row, 120.0, 18.0);
                }
                PlayerEvent::DropThrough => {
                    self.sound(Sfx::DropThrough, 1.0);
                    self.surfaces.impulse(feet.x, feet_row, 260.0, 20.0);
                    self.particles.emit(fx::JUMP_BUBBLES, feet, 5);
                }
                PlayerEvent::Died => {
                    self.deaths += 1;
                    self.sound(Sfx::Death, 1.0);
                    self.respawn_timer = RESPAWN_DELAY;
                    self.shake = 0.45;
                    self.particles.emit_with_vel(fx::DEATH_POP, center, 40, self.player.vel * 0.2);
                    self.particles.emit(fx::DEATH_RING, center, 1);
                    self.particles.emit(fx::HAZARD_SPORE, center, 12);
                    self.splash_acid(center, 380.0);
                }
            }
        }

        if self.player.dead {
            self.respawn_timer -= dt;
            if self.respawn_timer <= 0.0 {
                let facing = self.player.facing;
                self.player = Player::new(self.respawn_point);
                self.player.facing = facing;
                self.reset_world();
                let c = self.player.center();
                self.particles.emit(fx::RESPAWN_RING, c, 1);
                self.sound(Sfx::Respawn, 0.8);
                self.particles.emit(fx::JUMP_BUBBLES, c, 10);
            }
            return;
        }

        // Footsteps: longer strides at speed.
        let fast = self.player.speed01();
        if self.player.state == Move::Run && self.player.gsp.abs() > 40.0 {
            self.step_dist += self.player.gsp.abs() * dt;
            if self.step_dist >= STEP_DIST * (1.0 + fast) {
                self.step_dist = 0.0;
                self.sound(Sfx::Step, 0.55 + 0.3 * fast);
            }
        } else {
            self.step_dist = STEP_DIST * 0.6; // first step comes quickly
        }

        // Ambient player effects.
        let p = &self.player;
        if p.wall_sliding && p.vel.y > 60.0 && gen_range(0.0, 1.0) < 0.35 {
            let side = center + vec2(p.wall_dir as f32 * W / 2.0, 4.0);
            self.particles.emit(fx::WALL_DRIP, side, 1);
        }
        let skidding = p.grounded && p.vel.x.abs() > 150.0 && input.move_x * p.vel.x < 0.0;
        if (skidding || p.sliding) && gen_range(0.0, 1.0) < 0.6 {
            let angle = if p.vel.x > 0.0 { -0.5 } else { PI + 0.5 };
            self.particles.emit(Spec { angle, ..fx::SKID }, feet, 1);
        }
        if p.grounded && fast > 0.0 && gen_range(0.0, 1.0) < 0.15 + 0.4 * fast {
            let back = if p.vel.x > 0.0 { PI + 0.3 } else { -0.3 };
            self.particles.emit(Spec { angle: back, ..fx::SPRINT_TRAIL }, feet - vec2(0.0, 2.0), 1);
        }
        if p.state == Move::WallRun && gen_range(0.0, 1.0) < 0.5 {
            let side = center + vec2(p.wall_dir as f32 * W / 2.0, 8.0);
            self.particles.emit(Spec { angle: PI / 2.0, ..fx::SKID }, side, 1);
        }
        if p.has_dash && !p.grounded && gen_range(0.0, 1.0) < 0.15 {
            self.particles.emit(fx::ORB_SPARKLE, center, 1);
        }
        // Afterimages while dashing and at top speed.
        if p.state == Move::Dash || fast > 0.85 {
            self.ghost_tick += 1;
            if self.ghost_tick % 3 == 0 {
                self.ghosts.push(Ghost { pose: self.pose(1.0), life: GHOST_LIFE });
            }
        }

        // Standing on a platform presses it down; the legs cycle with ground speed.
        if self.player.grounded && self.player.gradient == 0.0 {
            self.surfaces.press(feet.x, feet_row, 500.0, 16.0);
        }
        match self.player.state {
            Move::Run => self.stride += self.player.gsp.abs() * dt / STRIDE_LEN,
            Move::WallRun => self.stride += self.player.vel.y.abs() * dt / STRIDE_LEN,
            _ => {}
        }

        self.update_pickups(dt);
        self.update_gates(dt);
        self.update_signs(dt);
    }

    /// The robot's pose, interpolated `alpha` of the way through the current step.
    pub fn pose(&self, alpha: f32) -> Pose {
        let p = &self.player;
        let mut feet = p.render_pos(alpha) + vec2(W / 2.0, p.height() + p.visual_drop(&self.level));
        if p.grounded && p.gradient == 0.0 {
            let row = (p.feet().y / TILE).round() as i32;
            feet.y += self.surfaces.offset(feet.x, row).max(0.0);
        }
        Pose {
            feet,
            vel: p.vel,
            facing: p.facing,
            state: p.state,
            crouching: p.crouching,
            wall_dir: p.wall_dir,
            gradient: if p.grounded { p.gradient } else { 0.0 },
            scale: p.scale,
            stride: self.stride,
        }
    }

    pub fn coins_collected(&self) -> u32 {
        self.nutrients.iter().filter(|n| n.taken).count() as u32
    }

    fn update_gates(&mut self, dt: f32) {
        let have = self.coins_collected();
        let mut opened = false;
        for g in &mut self.gates {
            if !g.opening && have >= g.need {
                g.opening = true;
                opened = true;
                self.level.set_gone(g.x, g.y, true);
                let c = vec2((g.x as f32 + 0.5) * TILE, (g.y as f32 + 0.5) * TILE);
                self.particles.emit(fx::CRUMBLE_DUST, c, 10);
            }
            if g.opening {
                g.open = (g.open + dt * 2.5).min(1.0);
            }
        }
        if opened {
            self.sound(Sfx::GateOpen, 1.0);
            self.shake = self.shake.max(0.15);
        }
    }

    fn update_signs(&mut self, dt: f32) {
        let pc = self.player.center();
        for (s, a) in self.level.signs.iter().zip(&mut self.sign_alpha) {
            let c = vec2((s.x as f32 + 0.5) * TILE, (s.y as f32 + 0.5) * TILE);
            let d = (pc - c).abs();
            let near = !self.player.dead && d.x < SIGN_RANGE.x && d.y < SIGN_RANGE.y;
            let target = if near { 1.0 } else { 0.0 };
            *a += (target - *a).clamp(-dt * 5.0, dt * 5.0);
        }
    }

    fn update_pickups(&mut self, dt: f32) {
        let pr = self.player.rect();
        let pc = self.player.center();

        for n in self.nutrients.iter_mut().filter(|n| !n.taken) {
            let pos = n.home + n.off;
            let to_player = pc - pos;
            let dist = to_player.length();
            let mut acc = -n.off * 60.0 - n.vel * 7.0;
            if dist < 70.0 && dist > 0.1 {
                acc += to_player / dist * 1400.0 * (1.0 - dist / 70.0);
            }
            n.vel += acc * dt;
            n.off += n.vel * dt;
            n.off = n.off.clamp_length_max(24.0);

            let pos = n.home + n.off;
            let closest = vec2(pos.x.clamp(pr.x, pr.x + pr.w), pos.y.clamp(pr.y, pr.y + pr.h));
            if closest.distance(pos) < 10.0 {
                n.taken = true;
                self.sfx.push((Sfx::Nutrient, 1.0));
                self.particles.emit(fx::NUTRIENT_BURST, pos, 14);
                self.particles.emit(fx::NUTRIENT_RING, pos, 1);
            }
        }

        for b in &mut self.bubbles {
            if b.regrow > 0.0 || pc.distance(b.pos) > 20.0 {
                continue;
            }
            if self.player.give_air_jump() {
                b.regrow = BUBBLE_REGROW_TIME;
                self.sfx.push((Sfx::Bubble, 1.0));
                self.particles.emit(fx::BUBBLE_POP, b.pos, 14);
                self.particles.emit(fx::BUBBLE_RING, b.pos, 1);
            } else {
                b.wobble = b.wobble.max(0.5);
            }
        }

        for orb in &mut self.dash_orbs {
            if orb.regrow > 0.0 || pc.distance(orb.pos) > 20.0 {
                continue;
            }
            if self.player.give_dash() {
                orb.regrow = ORB_REGROW_TIME;
                self.sfx.push((Sfx::Orb, 1.0));
                self.particles.emit(fx::ORB_BURST, orb.pos, 18);
                self.particles.emit(fx::ORB_RING, orb.pos, 1);
            } else {
                // Already holding a dash: the orb just jiggles.
                orb.wobble = orb.wobble.max(0.5);
            }
        }

        for cp in &mut self.checkpoints {
            let zone = Rect::new(cp.pos.x - 16.0, cp.pos.y - 64.0, 32.0, 64.0);
            if pr.overlaps(&zone) {
                if !cp.active {
                    cp.active = true;
                    self.sfx.push((Sfx::Checkpoint, 1.0));
                    self.respawn_point = cp.pos;
                    self.particles.emit(fx::CHECKPOINT_BURST, Self::checkpoint_cell(cp), 24);
                    self.particles.emit(Spec { colors: &[crate::particles::P_PLAYER], ..fx::NUTRIENT_RING }, Self::checkpoint_cell(cp), 1);
                }
                cp.wobble = cp.wobble.max(0.6);
            }
        }

        let g = self.level.goal;
        if pr.overlaps(&Rect::new(g.x - 24.0, g.y - 96.0, 48.0, 96.0)) {
            self.finished = true;
            self.goal_wobble = 1.0;
            self.sound(Sfx::Goal, 1.0);
            self.particles.emit(fx::GOAL_BURST, self.goal_center(), 90);
            self.particles.emit(Spec { size_end: 12.0, life: (0.7, 0.7), ..fx::NUTRIENT_RING }, self.goal_center(), 1);
        }
    }

    fn update_crumbles(&mut self, dt: f32) {
        let pr = self.player.rect();
        for c in &mut self.crumbles {
            let cell = Rect::new(c.x as f32 * TILE, c.y as f32 * TILE, TILE, TILE);
            let center = cell.center();
            match c.state {
                CrumbleState::Solid => {}
                CrumbleState::Shaking(t) if t - dt > 0.0 => c.state = CrumbleState::Shaking(t - dt),
                CrumbleState::Shaking(_) => {
                    c.state = CrumbleState::Gone(CRUMBLE_REGROW);
                    self.level.set_gone(c.x, c.y, true);
                    self.particles.emit(fx::CRUMBLE_DUST, center, 14);
                    self.sfx.push((Sfx::Crumble, 0.8));
                }
                CrumbleState::Gone(t) if t - dt > 0.0 => c.state = CrumbleState::Gone(t - dt),
                CrumbleState::Gone(_) => {
                    // Don't regrow inside the player or an enemy.
                    let blocked = (!self.player.dead && pr.overlaps(&cell))
                        || self.enemies.iter().any(|e| e.alive && e.rect().overlaps(&cell));
                    if !blocked {
                        c.state = CrumbleState::Solid;
                        self.level.set_gone(c.x, c.y, false);
                        self.particles.emit(Spec { size_end: 2.0, ..fx::BUBBLE_RING }, center, 1);
                    }
                }
            }
        }
    }

    fn update_enemies(&mut self, jump_held: bool, player_prev_bottom: f32, dt: f32) {
        let mut stomped = false;
        for e in &mut self.enemies {
            if e.update(&self.level, dt) {
                self.particles.emit(fx::STOMP_POP, e.pos, 16);
                self.sfx.push((Sfx::Splash, 0.7));
                let tile_y = (e.pos.y / TILE).floor();
                for s in self.surfaces.strips.iter_mut().filter(|s| s.kind == SurfaceKind::Acid) {
                    if (s.row as f32 - tile_y).abs() <= 1.0 && e.pos.x >= s.x0 && e.pos.x <= s.x1 {
                        s.kick(e.pos.x, 300.0, 24.0);
                    }
                }
            }
            if self.player.dead || stomped {
                continue;
            }
            match e.contact(self.player.rect(), player_prev_bottom, self.player.vel.y) {
                Contact::None => {}
                Contact::Stomp => {
                    e.squash();
                    stomped = true;
                    self.sfx.push((Sfx::Stomp, 1.0));
                    self.player.stomp_bounce(jump_held);
                    self.particles.emit(fx::STOMP_POP, e.pos, 22);
                    self.particles.emit(Spec { size_end: 4.0, ..fx::DEATH_RING }, e.pos, 1);
                    self.shake = self.shake.max(0.15);
                }
                Contact::Hurt => self.player.kill(),
            }
        }
    }

    /// Kick acid surfaces near a point (things falling in).
    fn splash_acid(&mut self, p: Vec2, strength: f32) {
        let row = (p.y / TILE).floor() as i32;
        for s in self.surfaces.strips.iter_mut().filter(|s| s.kind == SurfaceKind::Acid) {
            if (s.row - row).abs() <= 1 && p.x >= s.x0 - 16.0 && p.x <= s.x1 + 16.0 {
                s.kick(p.x, strength, 28.0);
                self.particles.emit(fx::ACID_SPLASH, vec2(p.x, s.top()), 14);
            }
        }
    }

    /// After a respawn, enemies and crumble blocks come back.
    fn reset_world(&mut self) {
        self.enemies = self.level.enemies.iter().map(|(k, p)| Enemy::new(*k, *p)).collect();
        for c in &mut self.crumbles {
            c.state = CrumbleState::Solid;
            self.level.set_gone(c.x, c.y, false);
        }
    }

    fn update_ambient(&mut self, dt: f32) {
        for v in self.bounce_anim.values_mut() {
            *v = (*v - dt * 2.5).max(0.0);
        }
        self.bounce_anim.retain(|_, v| *v > 0.0);
        for b in &mut self.bubbles {
            b.wobble *= (-4.0 * dt).exp();
            if b.regrow > 0.0 {
                b.regrow -= dt;
                if b.regrow <= 0.0 {
                    b.regrow = 0.0;
                    b.wobble = 1.0;
                }
            }
        }
        // Acid pools gently churn and bubble.
        let view_half = vec2(screen_width(), screen_height()) / self.current_zoom() / 2.0;
        for s in self.surfaces.strips.iter_mut().filter(|s| s.kind == SurfaceKind::Acid) {
            if s.x1 < self.cam.x - view_half.x || s.x0 > self.cam.x + view_half.x {
                continue;
            }
            let width = s.x1 - s.x0;
            if gen_range(0.0, 1.0) < width / TILE * 0.02 {
                s.kick(gen_range(s.x0, s.x1), gen_range(-40.0, 40.0), 16.0);
            }
            if gen_range(0.0, 1.0) < width / TILE * 0.008 {
                self.particles.emit(fx::ACID_BUBBLE, vec2(gen_range(s.x0 + 4.0, s.x1 - 4.0), s.top() + 6.0), 1);
            }
        }

        let z = self.current_zoom();
        let half = vec2(screen_width(), screen_height()) / z / 2.0 + vec2(60.0, 60.0);
        let missing = AMBIENT_MOTES.saturating_sub(self.particles.count_layer(Layer::Back));
        for _ in 0..missing.min(4) {
            let p = self.cam + vec2(gen_range(-half.x, half.x), gen_range(-half.y, half.y));
            self.particles.emit(fx::MOTE, p, 1);
        }

        for cp in &mut self.checkpoints {
            cp.wobble *= (-3.0 * dt).exp();
            if cp.active {
                cp.emitter.tick(&mut self.particles, cp.pos - vec2(0.0, 46.0), dt);
            }
        }
        for orb in &mut self.dash_orbs {
            orb.wobble *= (-4.0 * dt).exp();
            if orb.regrow > 0.0 {
                orb.regrow -= dt;
                if orb.regrow <= 0.0 {
                    orb.regrow = 0.0;
                    orb.wobble = 1.0;
                    self.particles.emit(fx::ORB_RING, orb.pos, 1);
                }
            }
        }
        self.goal_wobble *= (-2.0 * dt).exp();
        let gc = self.goal_center();
        self.goal_emitter.tick(&mut self.particles, gc, dt);

        for s in &self.spikes {
            if (s.x - self.cam.x).abs() < half.x && gen_range(0.0, 1.0) < 0.002 {
                self.particles.emit(fx::HAZARD_SPORE, *s, 1);
            }
        }
    }

    /// Smooth camera, run once per rendered frame using the interpolated player position.
    pub fn update_camera(&mut self, frame_dt: f32, alpha: f32) {
        let z = self.current_zoom();
        let view = vec2(screen_width(), screen_height()) / z;
        let p = self.player.render_pos(alpha) + vec2(W / 2.0, self.player.height() / 2.0);

        let t = tuning::get();
        let look_target =
            (self.player.facing * 48.0 + self.player.vel.x * t.look_ahead_time).clamp(-t.look_ahead_max, t.look_ahead_max);
        self.look_ahead += (look_target - self.look_ahead) * (1.0 - (-3.0 * frame_dt).exp());
        // Pull the view back as you speed up; ease back in slowly so it doesn't pump.
        let zoom_target = if self.player.dead { self.zoom_out } else { self.player.speed01() * t.zoom_out };
        let rate = if zoom_target > self.zoom_out { 2.0 } else { 0.8 };
        self.zoom_out += (zoom_target - self.zoom_out) * (1.0 - (-rate * frame_dt).exp());

        let target = p + vec2(self.look_ahead, -24.0);
        self.cam.x += (target.x - self.cam.x) * (1.0 - (-8.0 * frame_dt).exp());
        self.cam.y += (target.y - self.cam.y) * (1.0 - (-5.0 * frame_dt).exp());

        let (lw, lh) = (self.level.pixel_width(), self.level.pixel_height());
        self.cam.x = if lw <= view.x { lw / 2.0 } else { self.cam.x.clamp(view.x / 2.0, lw - view.x / 2.0) };
        self.cam.y = if lh <= view.y { lh / 2.0 } else { self.cam.y.clamp(view.y / 2.0, lh - view.y / 2.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gates_open_once_enough_coins_are_taken() {
        let mut g = Game::new(Level::parse("@gate 1 0 2\noLo\n###\n"));
        assert!(g.level.is_solid(1, 0));
        g.nutrients[0].taken = true;
        g.update_gates(0.01);
        assert!(g.level.is_solid(1, 0), "one coin is not enough");
        g.nutrients[1].taken = true;
        g.update_gates(0.01);
        assert!(!g.level.is_solid(1, 0));
        assert!(g.sfx.iter().any(|(s, _)| *s == Sfx::GateOpen));
    }

    #[test]
    fn gates_needing_nothing_start_open() {
        let g = Game::new(Level::parse("@gate 1 0 0\n.L.\n###\n"));
        assert!(!g.level.is_solid(1, 0));
    }
}
