//! Player controller: momentum-based movement.
//!
//! Every tuning value lives in `tuning.rs` (hot-reloaded from `assets/tuning.txt`).
//!
//! On the ground the player moves along the surface with one signed ground speed
//! (`gsp`), so slopes speed you up or slow you down, and running off a ramp
//! launches you along it. Nothing stops you dead: landings keep your speed,
//! fast wall hits turn into wall-runs, small steps are vaulted, ledges mantled.

use crate::input::InputState;
use crate::level::{tile_span, Floor, Level, Tile, TILE};
use crate::tuning::{self, Tuning};
use macroquad::prelude::*;

pub const W: f32 = 22.0;
pub const H: f32 = 28.0;
/// Low enough to fit under a half block (half a tile), which standing can't.
pub const CROUCH_H: f32 = 14.0;

/// Extra distance the ground sticks to below you, beyond what the slope drops.
const GROUND_STICK: f32 = 4.0;
/// Longest distance moved in one collision sub-step (well under a tile).
const MAX_SUBSTEP: f32 = 8.0;
const CORNER_CORRECTION: i32 = 8;
const ONE_WAY_DROP_TIME: f32 = 0.18;
const DROP_HOLD_TIME: f32 = 0.12;
/// How fast the visual offset from vaults and mantles catches up (per second).
const VIS_CATCH_UP: f32 = 28.0;
/// Stick deflection that counts as holding a direction.
const DEADZONE: f32 = 0.15;
/// Let go of down slightly before jumping and still get the long jump.
const SLIDE_JUMP_GRACE: f32 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Run,
    Slide,
    Crouch,
    Air,
    WallRun,
    WallSlide,
    Dash,
}

impl Move {
    pub fn name(self) -> &'static str {
        match self {
            Move::Run => "run",
            Move::Slide => "slide",
            Move::Crouch => "crouch",
            Move::Air => "air",
            Move::WallRun => "wall-run",
            Move::WallSlide => "wall-slide",
            Move::Dash => "dash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpKind {
    Normal,
    /// Out of a slide: low and fast.
    Long,
    /// Out of a dash near the ground: keeps the dash speed.
    Dash,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerEvent {
    Jump(JumpKind),
    /// Kicked off a wall; carries the direction you were pushed.
    WallJump(i32),
    /// Jumped right as you landed: speed boost.
    Perfect,
    Land(f32),
    Dash(Vec2),
    /// Launched by a bounce block; carries the top-centre of the block.
    Bounce(Vec2),
    AirJump,
    SlideStart,
    /// Started running up a wall on this side.
    WallRun(i32),
    Vault,
    Mantle,
    /// Left a ramp moving upward fast.
    Launch,
    DropThrough,
    Died,
}

pub struct Player {
    pub pos: Vec2, // top-left of hitbox
    pub prev_pos: Vec2,
    pub vel: Vec2,
    /// Speed along the ground (signed, + is right). Only meaningful on the ground.
    pub gsp: f32,
    /// dy/dx of the floor under you (y down).
    pub gradient: f32,
    pub state: Move,
    pub grounded: bool,
    pub facing: f32,
    pub scale: Vec2, // squash & stretch
    /// Drawn offset that eases away, so vaults and mantles don't pop.
    pub vis_offset: Vec2,
    pub dead: bool,
    pub has_dash: bool,
    /// Stored mid-air jumps from jump bubbles (max 1).
    pub air_jumps: u8,
    pub dash_timer: f32,
    pub wall_sliding: bool,
    pub wall_dir: i32,
    pub crouching: bool,
    pub sliding: bool,
    pub events: Vec<PlayerEvent>,

    input_x: f32,
    coyote: f32,
    jump_buffer: f32,
    /// Rising from a jump with the button held (for jump cut and apex hang).
    jumping: bool,
    last_wall_dir: i32,
    wall_coyote: f32,
    wall_kick_lock: f32,
    /// Horizontal speed you hit the current wall with.
    wall_speed: f32,
    /// Side of the last wall-run since touching the ground (0 = none): you can't
    /// run up the same side twice in a row.
    last_wallrun_dir: i32,
    dash_dir: Vec2,
    dash_speed: f32,
    dash_cooldown: f32,
    drop_timer: f32,
    drop_hold: f32,
    land_speed: f32,
    /// Seconds since touching down (for perfect landings).
    since_land: f32,
    slide_cooldown: f32,
    /// Seconds left in which a jump still counts as out of a slide.
    slide_grace: f32,
}

fn approach(current: f32, target: f32, step: f32) -> f32 {
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

fn snap(v: f32) -> f32 {
    if v > 0.4 {
        1.0
    } else if v < -0.4 {
        -1.0
    } else {
        0.0
    }
}

/// Unit vector along a floor with this gradient, pointing right.
pub fn tangent(gradient: f32) -> Vec2 {
    vec2(1.0, gradient).normalize()
}

impl Player {
    pub fn new(feet: Vec2) -> Self {
        let pos = vec2(feet.x - W / 2.0, feet.y - H);
        Player {
            pos,
            prev_pos: pos,
            vel: Vec2::ZERO,
            gsp: 0.0,
            gradient: 0.0,
            state: Move::Air,
            grounded: false,
            facing: 1.0,
            scale: vec2(0.6, 1.4),
            vis_offset: Vec2::ZERO,
            dead: false,
            has_dash: false,
            air_jumps: 0,
            dash_timer: 0.0,
            wall_sliding: false,
            wall_dir: 0,
            crouching: false,
            sliding: false,
            events: Vec::new(),
            input_x: 0.0,
            coyote: 0.0,
            jump_buffer: 0.0,
            jumping: false,
            last_wall_dir: 0,
            wall_coyote: 0.0,
            wall_kick_lock: 0.0,
            wall_speed: 0.0,
            last_wallrun_dir: 0,
            dash_dir: Vec2::ZERO,
            dash_speed: 0.0,
            dash_cooldown: 0.0,
            drop_timer: 0.0,
            drop_hold: 0.0,
            land_speed: 0.0,
            since_land: f32::INFINITY,
            slide_cooldown: 0.0,
            slide_grace: 0.0,
        }
    }

    pub fn height(&self) -> f32 {
        if self.crouching {
            CROUCH_H
        } else {
            H
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.pos.x, self.pos.y, W, self.height())
    }

    /// Position interpolated between the last two physics steps (plus the visual offset).
    pub fn render_pos(&self, alpha: f32) -> Vec2 {
        self.prev_pos.lerp(self.pos, alpha) + self.vis_offset
    }

    pub fn center(&self) -> Vec2 {
        self.pos + vec2(W / 2.0, self.height() / 2.0)
    }

    pub fn feet(&self) -> Vec2 {
        self.pos + vec2(W / 2.0, self.height())
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }

    /// 0 at run speed (or slower), 1 at top speed (or faster).
    pub fn speed01(&self) -> f32 {
        let t = tuning::get();
        ((self.speed() - t.run_speed) / (t.top_speed - t.run_speed).max(1.0)).clamp(0.0, 1.0)
    }

    fn on_ground(&self) -> bool {
        matches!(self.state, Move::Run | Move::Slide | Move::Crouch)
    }

    /// Give the player an extra mid-air jump from a bubble. Returns false if one is already stored.
    pub fn give_air_jump(&mut self) -> bool {
        if self.air_jumps > 0 {
            return false;
        }
        self.air_jumps = 1;
        true
    }

    /// Give the player a dash from an orb. Returns false if one is already stored.
    pub fn give_dash(&mut self) -> bool {
        if self.has_dash {
            return false;
        }
        self.has_dash = true;
        true
    }

    /// Bounce off an enemy you landed on.
    pub fn stomp_bounce(&mut self, jump_held: bool) {
        let t = tuning::get();
        let h = if jump_held { t.stomp_high_height } else { t.stomp_height };
        self.vel.y = -t.jump_speed(h);
        self.become_airborne();
        self.jumping = jump_held;
        self.scale = vec2(1.3, 0.75);
    }

    /// Killed by something outside the player (enemies).
    pub fn kill(&mut self) {
        if !self.dead {
            self.dead = true;
            self.events.push(PlayerEvent::Died);
        }
    }

    /// Tiles of `kind` directly under the player's feet (while standing on them).
    pub fn standing_on(&self, level: &Level, kind: Tile) -> Vec<(i32, i32)> {
        let bottom = self.pos.y + self.height();
        let row = (bottom / TILE).round() as i32;
        if !self.grounded || (row as f32 * TILE - bottom).abs() > 0.5 {
            return Vec::new();
        }
        let (x0, x1, _, _) = tile_span(self.rect());
        (x0..=x1).filter(|&x| level.tile(x, row) == kind).map(|x| (x, row)).collect()
    }

    /// The shape the body relaxes toward.
    fn rest_scale(&self) -> Vec2 {
        match self.state {
            Move::Slide => vec2(1.35, 0.62),
            Move::Crouch => vec2(1.28, 0.64),
            Move::WallRun => vec2(0.85, 1.15),
            Move::Dash => vec2(1.15, 0.88),
            _ => Vec2::ONE,
        }
    }

    pub fn step(&mut self, input: &InputState, level: &Level, dt: f32) {
        self.prev_pos = self.pos;
        if self.dead {
            return;
        }
        let t = tuning::get();

        self.jump_buffer -= dt;
        self.coyote -= dt;
        self.wall_coyote -= dt;
        self.wall_kick_lock -= dt;
        self.dash_cooldown -= dt;
        self.drop_timer -= dt;
        self.slide_cooldown -= dt;
        self.slide_grace -= dt;
        self.since_land += dt;
        self.vis_offset *= (-VIS_CATCH_UP * dt).exp();

        let mx = input.move_x;
        self.input_x = mx;
        if input.jump_pressed {
            self.jump_buffer = t.jump_buffer;
        }
        let steering = !matches!(self.state, Move::Dash | Move::Slide | Move::WallRun);
        if mx.abs() > DEADZONE && self.wall_kick_lock <= 0.0 && steering {
            self.facing = mx.signum();
        }

        if input.dash_pressed && self.has_dash && self.dash_cooldown <= 0.0 && !self.on_ground() {
            self.start_dash(mx, input.move_y, &t);
        }

        match self.state {
            Move::Run | Move::Slide | Move::Crouch => self.ground_step(input, level, &t, dt),
            Move::Dash => self.dash_step(level, &t, dt),
            Move::Air | Move::WallRun | Move::WallSlide => self.air_step(input, level, &t, dt),
        }

        self.grounded = self.on_ground();
        if self.grounded && t.dash_ground_refill > 0.5 {
            self.has_dash = true;
        }
        self.sliding = self.state == Move::Slide;
        self.update_walls(level, mx);

        // Bounce blocks launch you the moment you stand on them.
        if let Some(&(bx, by)) = self.standing_on(level, Tile::Bounce).first() {
            if self.crouching {
                self.try_stand(level);
            }
            let h = if input.jump_held { t.bounce_high_height } else { t.bounce_height };
            self.vel = vec2(self.gsp * tangent(self.gradient).x, -t.jump_speed(h));
            self.become_airborne();
            self.jump_buffer = 0.0;
            self.scale = vec2(0.6, 1.5);
            self.events.push(PlayerEvent::Bounce(vec2((bx as f32 + 0.5) * TILE, by as f32 * TILE)));
        }

        // Ease squash & stretch toward the resting shape (frame-rate independent).
        self.scale = self.scale.lerp(self.rest_scale(), 1.0 - (-14.0 * dt).exp());

        // ---- Hazards ----
        let r = self.rect();
        let hurtbox = Rect::new(r.x + 3.0, r.y + 3.0, r.w - 6.0, r.h - 4.0);
        if level.rect_hits_hazard(hurtbox) || self.pos.y > level.pixel_height() + 100.0 {
            self.kill();
        }
    }

    // ---------- Ground ----------

    fn ground_step(&mut self, input: &InputState, level: &Level, t: &Tuning, dt: f32) {
        let (mx, my) = (input.move_x, input.move_y);
        let want_down = my > 0.5;

        // Hold down on a one-way platform to drop through it.
        if want_down && self.on_one_way_only(level) {
            self.drop_hold += dt;
            if self.drop_hold >= DROP_HOLD_TIME {
                self.drop_hold = 0.0;
                self.drop_timer = ONE_WAY_DROP_TIME;
                self.pos.y += 1.0;
                self.leave_ground(t);
                self.coyote = 0.0;
                self.scale = vec2(0.8, 1.25);
                self.events.push(PlayerEvent::DropThrough);
                self.move_air(level, t, dt);
                return;
            }
        } else {
            self.drop_hold = 0.0;
        }

        // Crouch, slide, stand.
        if want_down && !self.crouching {
            self.crouch();
            if self.gsp.abs() >= t.slide_min_speed || self.gradient != 0.0 {
                self.start_slide(t);
            } else {
                self.state = Move::Crouch;
            }
        } else if !want_down && self.crouching && self.try_stand(level) {
            if self.state == Move::Slide {
                self.slide_grace = SLIDE_JUMP_GRACE;
            }
            self.state = Move::Run; // the slide's speed carries straight into the run
        }

        // Ground speed. Slope pull first, so friction can hold you still on a ramp.
        let slope_pull = t.gravity() * tangent(self.gradient).y * dt;
        match self.state {
            Move::Slide => {
                self.gsp += slope_pull * t.slope_slide_factor;
                let braking = mx.abs() > 0.3 && mx.signum() != self.gsp.signum();
                let friction = if braking { t.slide_brake } else { t.slide_friction };
                self.gsp = approach(self.gsp, 0.0, friction * dt);
                if self.gsp.abs() < t.slide_stop_speed && self.gradient == 0.0 {
                    self.state = Move::Crouch;
                }
            }
            Move::Crouch => {
                if self.gradient != 0.0 {
                    self.start_slide(t);
                } else {
                    let target = if mx.abs() > DEADZONE { mx.signum() * t.crawl_speed } else { 0.0 };
                    self.gsp = approach(self.gsp, target, t.ground_friction * dt);
                }
            }
            _ => {
                self.gsp += slope_pull * t.slope_run_factor;
                self.gsp = run_speed_step(self.gsp, mx, t, dt);
            }
        }
        self.gsp = self.gsp.clamp(-t.hard_cap, t.hard_cap);

        if self.jump_buffer > 0.0 && self.ground_jump(level, t) {
            self.move_air(level, t, dt);
            return;
        }
        self.move_ground(level, t, dt);
    }

    fn start_slide(&mut self, t: &Tuning) {
        self.state = Move::Slide;
        if self.gsp == 0.0 {
            return; // crouched on a ramp: just start slipping
        }
        if self.slide_cooldown <= 0.0 {
            self.gsp += self.gsp.signum() * t.slide_boost;
            self.slide_cooldown = t.slide_boost_cooldown;
        }
        self.events.push(PlayerEvent::SlideStart);
    }

    /// Jump from the ground. Returns false if there's no room to stand up.
    fn ground_jump(&mut self, level: &Level, t: &Tuning) -> bool {
        let from_slide = self.state == Move::Slide || self.slide_grace > 0.0;
        if self.crouching && !self.try_stand(level) {
            return false;
        }
        let mut vel = self.gsp * tangent(self.gradient);
        // Uphill speed adds to the jump; running downhill doesn't take away from it.
        vel.y = vel.y.min(0.0) * t.slope_jump_keep;
        let kind = if from_slide {
            let dir = if self.gsp != 0.0 { self.gsp.signum() } else { self.facing };
            vel.x = dir * (self.gsp.abs() * t.long_jump_mult).max(t.long_jump_min);
            vel.y -= t.jump_speed(t.long_jump_height);
            self.scale = vec2(1.45, 0.7);
            JumpKind::Long
        } else {
            let fast = ((self.gsp.abs() - t.run_speed) / (t.top_speed - t.run_speed).max(1.0)).clamp(0.0, 1.0);
            vel.y -= t.jump_speed(t.jump_height + t.speed_jump_bonus * fast);
            self.scale = vec2(0.7, 1.35);
            JumpKind::Normal
        };
        if self.since_land <= t.perfect_window && self.gsp.abs() >= t.perfect_min_speed {
            let cap = (t.top_speed * t.perfect_cap_mult).max(vel.x.abs());
            vel.x = vel.x.signum() * (vel.x.abs() + t.perfect_boost).min(cap);
            self.events.push(PlayerEvent::Perfect);
        }
        self.vel = vel;
        self.become_airborne();
        self.jumping = true;
        self.events.push(PlayerEvent::Jump(kind));
        true
    }

    fn move_ground(&mut self, level: &Level, t: &Tuning, dt: f32) {
        let total = self.gsp * tangent(self.gradient).x * dt;
        let steps = (total.abs() / MAX_SUBSTEP).ceil().max(1.0) as i32;
        for _ in 0..steps {
            if !self.ground_substep(level, t, total / steps as f32) {
                break;
            }
        }
        if self.on_ground() {
            self.vel = self.gsp * tangent(self.gradient);
        }
    }

    /// Move along the ground by `dx`. Returns false once something stops the move.
    fn ground_substep(&mut self, level: &Level, t: &Tuning, dx: f32) -> bool {
        if dx == 0.0 {
            // Standing still: still check the floor hasn't gone (crumble blocks, gates).
            if self.probe(level, self.bottom() - 0.5, self.bottom() + 0.5, self.bottom() - 0.5).is_none() {
                self.leave_ground(t);
            }
            return false;
        }
        let h = self.height();
        let old = self.pos;
        let dir = dx.signum();
        self.pos.x += dx;
        // Follow the floor up and down ramps first, so the block under the next
        // ramp tile isn't mistaken for a wall.
        let bottom = old.y + h;
        let floor = self.probe(level, bottom - (dx.abs() + 1.0), bottom + dx.abs() + GROUND_STICK, bottom - 0.5);
        if let Some(f) = floor {
            self.pos.y = f.y - h;
        }
        let solid = level.rect_hits_solid(self.rect());
        if solid || self.ramp_wall(level, dx.abs() + 1.0) {
            self.pos = old;
            if solid {
                self.flush_to_wall(level, dir);
            }
            if self.try_vault(level, t, dir) {
                return true;
            }
            if self.state != Move::Crouch {
                if let Some(top) = self.ledge_top(level, dir, TILE / 2.0 + 0.5) {
                    // A half-height step: just step up it.
                    self.climb_onto(top);
                    return true;
                }
            }
            let toward = self.input_x * dir > 0.3;
            if t.ground_wallrun > 0.5
                && self.state == Move::Run
                && toward
                && self.gsp.abs() >= t.wallrun_min_speed
                && self.last_wallrun_dir != dir as i32
                && !self.no_grip(level, dir)
            {
                self.start_wallrun(dir as i32, self.gsp.abs(), t);
            } else {
                self.gsp = 0.0;
            }
            return false;
        }

        match floor {
            Some(f) => {
                self.gradient = f.gradient;
                true
            }
            None => {
                self.leave_ground(t);
                false
            }
        }
    }

    /// Running into a step no taller than `vault_max_height`: hop onto it at full speed.
    fn try_vault(&mut self, level: &Level, t: &Tuning, dir: f32) -> bool {
        if self.gsp.abs() < t.vault_min_speed || self.state != Move::Run {
            return false;
        }
        let Some(top) = self.ledge_top(level, dir, t.vault_max_height * TILE + 0.5) else {
            return false;
        };
        self.climb_onto(top);
        self.scale = vec2(0.85, 1.2);
        self.events.push(PlayerEvent::Vault);
        true
    }

    /// Top of the wall just ahead, if it is at most `max_rise` above your feet and
    /// there's room to stand on it.
    fn ledge_top(&self, level: &Level, dir: f32, max_rise: f32) -> Option<f32> {
        let r = self.rect();
        let x = if dir > 0.0 { r.x + r.w + 0.5 } else { r.x - 0.5 };
        let col = (x / TILE).floor() as i32;
        let bottom = r.y + r.h;
        if col < 0 || col >= level.width {
            return None;
        }
        let top = level.solid_top_at(x, bottom - 0.01)?;
        let rise = bottom - top;
        if rise <= 0.0 || rise > max_rise {
            return None;
        }
        let standing = Rect::new(r.x + dir, top - H, W, H);
        (!level.rect_hits_solid(standing)).then_some(top)
    }

    /// Put the player standing at `top`, keeping the old height on screen for a moment.
    fn climb_onto(&mut self, top: f32) {
        let before = self.pos.y + self.vis_offset.y;
        self.crouching = false;
        self.pos.y = top - H;
        self.vis_offset.y = before - self.pos.y;
        self.prev_pos.y = self.pos.y;
        self.gradient = 0.0;
    }

    fn leave_ground(&mut self, t: &Tuning) {
        self.vel = self.gsp * tangent(self.gradient);
        if self.vel.y < -150.0 {
            self.events.push(PlayerEvent::Launch);
        }
        self.become_airborne();
        self.coyote = t.coyote_time;
    }

    fn become_airborne(&mut self) {
        self.state = Move::Air;
        self.grounded = false;
        self.gradient = 0.0;
        self.coyote = 0.0;
        self.jump_buffer = 0.0;
        self.jumping = false;
        self.dash_timer = 0.0;
        self.since_land = f32::INFINITY;
        self.slide_grace = 0.0;
    }

    // ---------- Air ----------

    fn air_step(&mut self, input: &InputState, level: &Level, t: &Tuning, dt: f32) {
        let (mx, my) = (input.move_x, input.move_y);
        if self.crouching {
            self.try_stand(level);
        }

        // ---- Jumps: coyote, wall, stored air jump ----
        if self.jump_buffer > 0.0 {
            if self.coyote > 0.0 {
                self.vel.y = self.vel.y.min(0.0) - t.jump_speed(t.jump_height);
                self.become_airborne();
                self.jumping = true;
                self.scale = vec2(0.7, 1.35);
                self.events.push(PlayerEvent::Jump(JumpKind::Normal));
            } else if self.wall_coyote > 0.0 {
                self.wall_jump(t);
            } else if input.jump_pressed && self.air_jumps > 0 {
                self.air_jumps -= 1;
                self.vel.y = -t.jump_speed(t.air_jump_height);
                self.state = Move::Air;
                self.jumping = true;
                self.jump_buffer = 0.0;
                self.scale = vec2(0.7, 1.35);
                self.events.push(PlayerEvent::AirJump);
            }
        }

        // ---- Horizontal ----
        if self.state == Move::WallRun {
            self.vel.x = 0.0;
        } else {
            let control = if self.wall_kick_lock > 0.0 { 0.2 } else { 1.0 };
            self.vel.x = air_speed_step(self.vel.x, mx, control, t, dt);
        }

        // ---- Vertical ----
        let mut g = t.gravity();
        if self.state == Move::WallRun {
            g *= t.wallrun_gravity_mult;
        } else {
            if self.vel.y > 0.0 {
                g *= t.fall_gravity_mult;
            }
            if self.jumping && input.jump_held && self.vel.y.abs() < t.apex_threshold {
                g *= t.apex_gravity_mult;
            }
        }
        self.vel.y += g * dt;
        if self.jumping && !input.jump_held && self.vel.y < 0.0 {
            self.vel.y *= t.jump_cut;
            self.jumping = false;
        }
        if self.vel.y > t.apex_threshold {
            self.jumping = false;
        }
        let max_fall = match self.state {
            Move::WallSlide => t.wall_slide_speed,
            _ if my > 0.6 => t.fast_fall,
            _ => t.max_fall,
        };
        if self.vel.y > max_fall {
            self.vel.y = if self.state == Move::WallSlide { approach(self.vel.y, max_fall, 3000.0 * dt) } else { max_fall };
        }
        if self.state == Move::WallRun && self.vel.y >= 0.0 {
            self.state = Move::Air;
        }
        self.vel = self.vel.clamp_length_max(t.hard_cap);

        if self.state == Move::WallRun {
            // Running off the top of the wall: pull up onto the ledge.
            let dir = self.last_wall_dir as f32;
            if let Some(top) = self.ledge_top(level, dir, t.mantle_reach) {
                self.mantle(top, dir, self.wall_speed, t);
                return;
            }
        }
        self.move_air(level, t, dt);
    }

    fn move_air(&mut self, level: &Level, t: &Tuning, dt: f32) {
        let total = self.vel * dt;
        let steps = (total.abs().max_element() / MAX_SUBSTEP).ceil().max(1.0) as i32;
        let d = total / steps as f32;
        for _ in 0..steps {
            self.air_substep_x(level, t, d.x);
            if self.on_ground() {
                break;
            }
            self.air_substep_y(level, t, d.y);
            if self.on_ground() {
                break;
            }
        }
    }

    fn air_substep_x(&mut self, level: &Level, t: &Tuning, dx: f32) {
        if dx == 0.0 || self.on_ground() {
            return;
        }
        let old = self.pos;
        self.pos.x += dx;
        // Moving sideways into a ramp (or just past a corner): ride up onto it.
        let bottom = self.bottom();
        let ramp = self.probe(level, bottom - (dx.abs() + 1.0), bottom - 0.01, f32::INFINITY);
        if let Some(f) = ramp {
            self.pos.y = f.y - self.height();
        }
        let solid = level.rect_hits_solid(self.rect());
        if solid || self.ramp_wall(level, dx.abs() + 1.0) {
            self.pos = old;
            let dir = dx.signum();
            if solid {
                self.flush_to_wall(level, dir);
            }
            self.hit_wall_in_air(level, t, dir);
            return;
        }
        if let Some(f) = ramp.filter(|_| self.vel.y >= 0.0) {
            self.land(f, t);
        }
    }

    fn air_substep_y(&mut self, level: &Level, t: &Tuning, dy: f32) {
        if dy == 0.0 {
            return;
        }
        let h = self.height();
        let prev_bottom = self.pos.y + h;
        self.pos.y += dy;
        if dy > 0.0 {
            let one_way = if self.drop_timer > 0.0 { f32::INFINITY } else { prev_bottom - 0.5 };
            if let Some(f) = self.probe(level, prev_bottom - 0.5, self.pos.y + h, one_way) {
                self.pos.y = f.y - h;
                self.land(f, t);
            }
        } else if level.rect_hits_solid(self.rect()) {
            // Corner correction: slide around ceiling corners instead of bonking.
            let r = self.rect();
            for i in 1..=CORNER_CORRECTION {
                for s in [-1.0, 1.0] {
                    let off = i as f32 * s;
                    if !level.rect_hits_solid(Rect::new(r.x + off, r.y, r.w, r.h)) {
                        self.pos.x += off;
                        return;
                    }
                }
            }
            self.pos.y = level.solid_bottom_in(r).unwrap_or(((r.y / TILE).floor() + 1.0) * TILE);
            self.vel.y = 0.0;
            self.jumping = false;
            if self.state == Move::WallRun {
                self.state = Move::Air;
            }
        }
    }

    fn hit_wall_in_air(&mut self, level: &Level, t: &Tuning, dir: f32) {
        if self.state == Move::WallRun {
            self.vel.x = 0.0; // already running up it; keep the stored entry speed
            return;
        }
        let impact = self.vel.x.abs();
        if self.no_grip(level, dir) {
            // The level's side edges and slick blocks are a plain barrier: nothing to run up or grab.
            self.vel.x = 0.0;
            if self.state == Move::Dash {
                self.state = Move::Air;
                self.dash_timer = 0.0;
            }
            return;
        }
        self.wall_speed = impact;
        self.last_wall_dir = dir as i32;
        if let Some(top) = self.ledge_top(level, dir, t.mantle_reach) {
            self.mantle(top, dir, impact, t);
            return;
        }
        let holding_away = self.input_x * dir < -DEADZONE;
        if impact >= t.wallrun_min_speed && !holding_away && self.last_wallrun_dir != dir as i32 {
            self.start_wallrun(dir as i32, impact, t);
        } else {
            self.vel.x = 0.0;
            if self.state == Move::Dash {
                self.state = Move::Air;
                self.dash_timer = 0.0;
            }
        }
    }

    fn start_wallrun(&mut self, dir: i32, impact: f32, t: &Tuning) {
        let up = (impact * t.wallrun_conversion).clamp(t.wallrun_min_up, t.wallrun_max_up);
        self.vel = vec2(0.0, self.vel.y.min(-up));
        self.state = Move::WallRun;
        self.grounded = false;
        self.gradient = 0.0;
        self.jumping = false;
        self.dash_timer = 0.0;
        self.wall_speed = impact;
        self.wall_dir = dir;
        self.last_wall_dir = dir;
        self.last_wallrun_dir = dir;
        self.facing = dir as f32;
        self.scale = vec2(0.7, 1.35);
        self.events.push(PlayerEvent::WallRun(dir));
    }

    fn mantle(&mut self, top: f32, dir: f32, speed: f32, t: &Tuning) {
        self.climb_onto(top);
        self.state = Move::Run;
        self.grounded = true;
        self.gsp = dir * speed.max(t.mantle_speed);
        self.vel = vec2(self.gsp, 0.0);
        self.jumping = false;
        self.dash_timer = 0.0;
        self.last_wallrun_dir = 0;
        self.since_land = 0.0;
        self.scale = vec2(1.2, 0.85);
        self.events.push(PlayerEvent::Mantle);
    }

    fn wall_jump(&mut self, t: &Tuning) {
        let dir = -self.last_wall_dir;
        self.vel.x = dir as f32 * (t.wall_kick_x + self.wall_speed * t.wall_kick_keep);
        self.vel.y = self.vel.y.min(-t.wall_kick_y);
        self.state = Move::Air;
        self.dash_timer = 0.0;
        self.wall_kick_lock = t.wall_kick_lock;
        self.wall_coyote = 0.0;
        self.wall_speed = 0.0;
        self.jump_buffer = 0.0;
        self.jumping = true;
        self.facing = dir as f32;
        self.scale = vec2(0.75, 1.3);
        self.events.push(PlayerEvent::WallJump(dir));
    }

    fn land(&mut self, f: Floor, t: &Tuning) {
        let tan = tangent(f.gradient);
        if self.state == Move::Dash {
            // Dashing into the ground: turn the dash into ground speed (a wavedash).
            let dir = if self.dash_dir.x != 0.0 { self.dash_dir.x.signum() } else { 0.0 };
            self.gsp = dir * self.vel.x.abs() * t.wavedash_mult;
        } else {
            // Keep the speed along the floor; landing on a downhill adds to it.
            let along = self.vel.dot(tan);
            self.gsp = if along * self.vel.x < 0.0 { self.vel.x * tan.x } else { along };
        }
        self.gsp = self.gsp.clamp(-t.hard_cap, t.hard_cap);
        self.land_speed = self.vel.y.max(0.0);
        self.gradient = f.gradient;
        self.state = Move::Run;
        self.grounded = true;
        self.jumping = false;
        self.dash_timer = 0.0;
        self.since_land = 0.0;
        self.last_wallrun_dir = 0;
        self.vel = self.gsp * tan;
        let k = (self.land_speed / t.max_fall).clamp(0.0, 1.0);
        if self.land_speed > 100.0 {
            self.scale = vec2(1.0 + 0.4 * k, 1.0 - 0.35 * k);
        }
        self.events.push(PlayerEvent::Land(self.land_speed));
    }

    // ---------- Dash ----------

    fn start_dash(&mut self, mx: f32, my: f32, t: &Tuning) {
        let mut dir = vec2(snap(mx), snap(my));
        if dir == Vec2::ZERO {
            dir.x = self.facing;
        }
        let dir = dir.normalize();
        // Never slower than you already were going that way.
        self.dash_speed = t.dash_speed.max(self.vel.dot(dir));
        self.dash_dir = dir;
        self.dash_timer = t.dash_time;
        self.dash_cooldown = t.dash_cooldown;
        self.has_dash = false;
        self.jumping = false;
        self.state = Move::Dash;
        self.vel = dir * self.dash_speed;
        if dir.x != 0.0 {
            self.facing = dir.x.signum();
        }
        self.scale = if dir.y.abs() > dir.x.abs() { vec2(0.65, 1.4) } else { vec2(1.4, 0.7) };
        self.events.push(PlayerEvent::Dash(dir));
    }

    fn dash_step(&mut self, level: &Level, t: &Tuning, dt: f32) {
        self.dash_timer -= dt;
        self.vel = self.dash_dir * self.dash_speed;
        if self.jump_buffer > 0.0 && self.coyote > 0.0 {
            // Dash-jump: cancel into a jump and keep all the horizontal speed.
            let vx = self.vel.x;
            self.become_airborne();
            self.vel = vec2(vx, -t.jump_speed(t.jump_height));
            self.jumping = true;
            self.scale = vec2(1.3, 0.8);
            self.events.push(PlayerEvent::Jump(JumpKind::Dash));
        } else if self.jump_buffer > 0.0 && self.wall_coyote > 0.0 {
            self.wall_jump(t);
        } else if self.dash_timer <= 0.0 {
            self.state = Move::Air;
            self.vel.x *= t.dash_end_keep;
            if self.vel.y < 0.0 {
                self.vel.y *= 0.45;
            }
        }
        self.move_air(level, t, dt);
    }

    // ---------- Walls ----------

    fn update_walls(&mut self, level: &Level, mx: f32) {
        if self.on_ground() || self.state == Move::Dash {
            self.wall_dir = 0;
            self.wall_sliding = false;
            return;
        }
        self.wall_dir = if self.touching_wall(level, 1) {
            1
        } else if self.touching_wall(level, -1) {
            -1
        } else {
            0
        };
        let t = tuning::get();
        if self.wall_dir != 0 {
            self.wall_coyote = t.wall_coyote;
            self.last_wall_dir = self.wall_dir;
        }
        let toward = self.wall_dir != 0 && mx * self.wall_dir as f32 > 0.3;
        self.state = match self.state {
            Move::WallRun if self.wall_dir == 0 => Move::Air,
            Move::Air | Move::WallSlide if toward && self.vel.y > 0.0 => Move::WallSlide,
            Move::WallSlide => Move::Air,
            s => s,
        };
        self.wall_sliding = self.state == Move::WallSlide;
    }

    fn touching_wall(&self, level: &Level, dir: i32) -> bool {
        if self.no_grip(level, dir as f32) {
            return false;
        }
        let r = self.rect();
        level.rect_hits_solid(Rect::new(r.x + dir as f32, r.y + 2.0, r.w, r.h - 4.0))
    }

    /// Whether the wall just ahead is the level's side edge (outside the map) or
    /// nothing but slick blocks, which block you but can't be run up, slid down,
    /// kicked off or grabbed.
    fn no_grip(&self, level: &Level, dir: f32) -> bool {
        let r = self.rect();
        let col = if dir > 0.0 { ((r.x + r.w + 0.5) / TILE).floor() } else { ((r.x - 0.5) / TILE).floor() } as i32;
        if col < 0 || col >= level.width {
            return true;
        }
        let (_, _, y0, y1) = tile_span(Rect::new(r.x, r.y + 2.0, r.w, r.h - 4.0));
        !(y0..=y1).any(|y| level.solid_rect(col, y).is_some() && !level.is_slick(col, y))
    }

    /// After a blocked move, close any gap to the wall so you're touching it.
    fn flush_to_wall(&mut self, level: &Level, dir: f32) {
        for _ in 0..8 {
            let r = self.rect();
            if level.rect_hits_solid(Rect::new(r.x + dir, r.y, r.w, r.h)) {
                break;
            }
            self.pos.x += dir;
        }
        // Snap the last sub-pixel so the edge sits exactly on the tile boundary.
        let r = self.rect();
        let edge = if dir > 0.0 { ((r.x + r.w) / TILE).ceil() * TILE - r.w } else { (r.x / TILE).floor() * TILE };
        if !level.rect_hits_solid(Rect::new(edge, r.y, r.w, r.h)) && (edge - r.x).abs() < 1.0 {
            self.pos.x = edge;
        }
    }

    // ---------- Floors ----------

    fn bottom(&self) -> f32 {
        self.pos.y + self.height()
    }

    /// The highest floor under either bottom corner with `min_y <= y <= max_y`.
    fn probe(&self, level: &Level, min_y: f32, max_y: f32, one_way_min_y: f32) -> Option<Floor> {
        let a = level.floor_at(self.pos.x, min_y, max_y, one_way_min_y);
        let b = level.floor_at(self.pos.x + W - 0.01, min_y, max_y, one_way_min_y);
        match (a, b) {
            (Some(a), Some(b)) => Some(if b.y < a.y { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    /// A ramp surface somewhere between your head and `step` above your feet: too
    /// steep to walk onto from here, so it acts as a wall.
    fn ramp_wall(&self, level: &Level, step: f32) -> bool {
        let bottom = self.bottom();
        self.probe(level, self.pos.y, bottom - step, f32::INFINITY).is_some()
    }

    /// How far below the hitbox the floor is under its centre (for drawing on ramps).
    pub fn visual_drop(&self, level: &Level) -> f32 {
        if !self.grounded || self.gradient == 0.0 {
            return 0.0;
        }
        let bottom = self.bottom();
        level
            .floor_at(self.pos.x + W / 2.0, bottom, bottom + W, f32::INFINITY)
            .map_or(0.0, |f| (f.y - bottom).clamp(0.0, W / 2.0))
    }

    fn ground_tiles(&self, level: &Level) -> Option<(bool, bool)> {
        let bottom = self.bottom();
        let row = (bottom / TILE).round() as i32;
        if (row as f32 * TILE - bottom).abs() > 0.5 {
            return None;
        }
        let (x0, x1, _, _) = tile_span(self.rect());
        let mut solid = false;
        let mut one_way = false;
        for x in x0..=x1 {
            if level.solid_rect(x, row).is_some_and(|s| (s.y - bottom).abs() < 0.5) {
                solid = true;
            } else if level.tile(x, row) == Tile::OneWay && self.drop_timer <= 0.0 {
                one_way = true;
            }
        }
        Some((solid, one_way))
    }

    fn on_one_way_only(&self, level: &Level) -> bool {
        self.gradient == 0.0 && matches!(self.ground_tiles(level), Some((false, true)))
    }

    // ---------- Crouching ----------

    fn crouch(&mut self) {
        self.crouching = true;
        self.pos.y += H - CROUCH_H;
        self.prev_pos.y += H - CROUCH_H; // the visual body handles the squish; avoid a pop
    }

    /// Stand up from a crouch if there's headroom.
    fn try_stand(&mut self, level: &Level) -> bool {
        let tall = Rect::new(self.pos.x, self.pos.y - (H - CROUCH_H), W, H);
        if level.rect_hits_solid(tall) {
            return false;
        }
        self.pos.y = tall.y;
        self.prev_pos.y -= H - CROUCH_H;
        self.crouching = false;
        true
    }
}

/// One step of running: quick to run_speed, then a slow build up to top_speed.
fn run_speed_step(gsp: f32, mx: f32, t: &Tuning, dt: f32) -> f32 {
    if mx.abs() <= DEADZONE {
        return approach(gsp, 0.0, t.ground_friction * dt);
    }
    let dir = mx.signum();
    if gsp * dir < 0.0 {
        return approach(gsp, dir * t.run_speed, t.turn_accel * dt); // skid
    }
    let speed = gsp.abs();
    let walk = t.run_speed * mx.abs().min(1.0);
    let new = if speed < walk {
        (speed + t.run_accel * dt).min(walk)
    } else if mx.abs() < 0.9 {
        approach(speed, walk, t.ground_friction * dt) // stick tilted gently: walk
    } else if speed < t.top_speed {
        (speed + t.build_accel * dt).min(t.top_speed)
    } else {
        (speed - t.overspeed_drag * dt).max(t.top_speed)
    };
    dir * new
}

/// One step of air steering: full control up to run_speed, momentum kept above it.
fn air_speed_step(vx: f32, mx: f32, control: f32, t: &Tuning, dt: f32) -> f32 {
    if mx.abs() <= DEADZONE {
        return approach(vx, 0.0, t.air_drag * dt);
    }
    let dir = mx.signum();
    if vx * dir > t.run_speed {
        approach(vx, dir * t.run_speed, t.air_overspeed_drag * dt)
    } else {
        approach(vx, dir * t.run_speed * mx.abs().min(1.0), t.air_accel * control * dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    fn parse(rows: &[&str]) -> Level {
        Level::parse(&rows.join("\n"))
    }

    fn flat_level() -> Level {
        let mut rows = vec![format!("#{}#", ".".repeat(200)); 14];
        rows[12] = format!("#.P{}#", ".".repeat(198));
        rows.push("#".repeat(202));
        Level::parse(&rows.join("\n"))
    }

    fn run(p: &mut Player, level: &Level, input: InputState, frames: usize) {
        for _ in 0..frames {
            p.step(&input, level, DT);
            p.events.clear();
        }
    }

    fn settle(p: &mut Player, level: &Level) {
        run(p, level, InputState::default(), 60);
        assert!(p.grounded, "didn't land: {:?}", p.state);
    }

    fn right() -> InputState {
        InputState { move_x: 1.0, ..Default::default() }
    }

    /// Press jump once (holding it), returning (peak height, horizontal distance until landing).
    fn jump_arc(p: &mut Player, level: &Level, mut input: InputState) -> (f32, f32) {
        let (start_y, start_x) = (p.bottom(), p.pos.x);
        input.jump_pressed = true;
        input.jump_held = true;
        let mut best = 0.0f32;
        for frame in 0..400 {
            p.step(&input, level, DT);
            p.events.clear();
            input.jump_pressed = false;
            best = best.max(start_y - p.bottom());
            if frame > 5 && p.grounded {
                break;
            }
        }
        (best, p.pos.x - start_x)
    }

    #[test]
    fn full_jump_clears_three_tiles_and_tap_is_shorter() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let (height, _) = jump_arc(&mut p, &level, InputState::default());
        assert!(height > 3.1 * TILE && height < 3.8 * TILE, "jump height {height}");

        let start = p.pos.y;
        p.step(&InputState { jump_pressed: true, jump_held: true, ..Default::default() }, &level, DT);
        let mut best = 0.0f32;
        for _ in 0..240 {
            p.step(&InputState::default(), &level, DT);
            best = best.max(start - p.pos.y);
        }
        assert!(best < 1.5 * TILE, "tap height {best}");
    }

    #[test]
    fn speed_builds_past_run_speed_while_you_keep_running() {
        let t = Tuning::default();
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 20);
        assert!((p.gsp - t.run_speed).abs() < 30.0, "run speed after 0.17s: {}", p.gsp);
        run(&mut p, &level, right(), 360);
        assert!(p.gsp > t.run_speed + 150.0, "speed didn't build: {}", p.gsp);
        assert!(p.gsp <= t.top_speed + 0.01);
    }

    #[test]
    fn landing_keeps_horizontal_speed() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 300);
        let before = p.gsp;
        jump_arc(&mut p, &level, right());
        assert!(p.grounded);
        assert!(p.gsp > before * 0.92, "lost speed on landing: {before} -> {}", p.gsp);
    }

    #[test]
    fn faster_jumps_go_higher() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 20);
        let (slow_h, slow_d) = jump_arc(&mut p, &level, right());

        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 400);
        let (fast_h, fast_d) = jump_arc(&mut p, &level, right());
        assert!(fast_h > slow_h + 10.0, "fast {fast_h} vs slow {slow_h}");
        assert!(fast_d > slow_d * 1.3, "fast dist {fast_d} vs slow {slow_d}");
    }

    #[test]
    fn perfect_landing_jump_boosts_speed() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 60);
        // Jump, then buffer the next jump just before landing.
        let mut input = InputState { move_x: 1.0, jump_pressed: true, jump_held: true, ..Default::default() };
        let mut perfect = false;
        let mut landed_at = 0.0;
        for frame in 0..400 {
            if frame > 10 && !p.grounded && p.vel.y > 0.0 && p.bottom() > 14.0 * TILE - 30.0 {
                input.jump_pressed = true;
            }
            p.step(&input, &level, DT);
            input.jump_pressed = false;
            if p.events.iter().any(|e| matches!(e, PlayerEvent::Land(_))) {
                landed_at = p.gsp;
            }
            if p.events.contains(&PlayerEvent::Perfect) {
                perfect = true;
                assert!(p.vel.x > landed_at + 20.0, "no boost: {landed_at} -> {}", p.vel.x);
                break;
            }
            p.events.clear();
        }
        assert!(perfect, "never got a perfect landing");
    }

    /// `n` copies of a string.
    fn r(s: &str, n: usize) -> String {
        s.repeat(n)
    }

    fn ramp_level() -> Level {
        // Flat run-up, a two-tile steep ramp up, a plateau, then a ramp back down.
        let open = format!("#{}#", r(".", 46));
        let mut rows = vec![open; 7];
        rows.push(format!("#{}/{}\\{}#", r(".", 12), r("#", 6), r(".", 26)));
        rows.push(format!("#.P{}/{}\\{}#", r(".", 9), r("#", 8), r(".", 25)));
        rows.push(r("#", 48));
        Level::parse(&rows.join("\n"))
    }

    #[test]
    fn runs_up_and_down_ramps() {
        let level = ramp_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let floor = p.bottom();
        let mut highest = floor;
        let mut gradients = (false, false);
        for _ in 0..500 {
            p.step(&right(), &level, DT);
            p.events.clear();
            highest = highest.min(p.bottom());
            gradients.0 |= p.gradient < 0.0;
            gradients.1 |= p.gradient > 0.0;
            assert!(p.grounded || p.state == Move::Air, "{:?} at {:?}", p.state, p.pos);
            if p.pos.x > 30.0 * TILE {
                break; // well past the ramps, before the far wall
            }
        }
        assert!(floor - highest >= 2.0 * TILE - 0.5, "never reached the plateau: {}", floor - highest);
        assert!(gradients.0 && gradients.1, "didn't use both ramps");
        assert!(p.pos.x > 30.0 * TILE, "got stuck at {}", p.pos.x);
    }

    #[test]
    fn sliding_downhill_gains_speed() {
        let level = parse(&[
            "#P.............................#",
            "#\\.............................#",
            "##\\............................#",
            "###\\...........................#",
            "####\\..........................#",
            "#####\\.........................#",
            "######\\........................#",
            "#######\\.......................#",
            "################################",
        ]);
        let mut p = Player::new(level.spawn);
        run(&mut p, &level, InputState::default(), 30);
        let slide = InputState { move_y: 1.0, ..Default::default() };
        let mut best = 0.0f32;
        for _ in 0..90 {
            p.step(&slide, &level, DT);
            p.events.clear();
            best = best.max(p.gsp);
        }
        assert!(best > 500.0, "slide only reached {best}");
    }

    #[test]
    fn ramp_lip_launches_you_upward() {
        // A three-tile ramp that ends in mid-air.
        let open = format!("#{}#", r(".", 38));
        let mut rows = vec![open; 5];
        rows.push(format!("#{}/{}#", r(".", 15), r(".", 22)));
        rows.push(format!("#{}/#{}#", r(".", 14), r(".", 22)));
        rows.push(format!("#.P{}/##{}#", r(".", 11), r(".", 22)));
        rows.push(r("#", 40));
        let level = Level::parse(&rows.join("\n"));
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let floor = p.bottom();
        let mut launched = false;
        let mut highest = floor;
        for _ in 0..300 {
            p.step(&right(), &level, DT);
            launched |= p.events.contains(&PlayerEvent::Launch);
            p.events.clear();
            highest = highest.min(p.bottom());
        }
        assert!(launched, "no launch event");
        assert!(floor - highest > 3.3 * TILE, "launch only reached {}", floor - highest);
    }

    #[test]
    fn slide_keeps_momentum_and_long_jumps_far() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 60);
        let speed = p.gsp;
        let slide = InputState { move_x: 1.0, move_y: 1.0, ..Default::default() };
        run(&mut p, &level, slide, 20);
        assert!(p.sliding && p.crouching);
        assert!(p.gsp > speed * 0.85, "slide lost too much: {speed} -> {}", p.gsp);
        let (h, d) = jump_arc(&mut p, &level, right());
        let mut q = Player::new(level.spawn);
        settle(&mut q, &level);
        run(&mut q, &level, right(), 60);
        let (normal_h, normal_d) = jump_arc(&mut q, &level, right());
        assert!(h < normal_h, "long jump {h} should be lower than {normal_h}");
        assert!(d > normal_d, "long jump dist {d} vs {normal_d}");

        // Letting go of down carries the slide's speed into a run.
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 60);
        run(&mut p, &level, slide, 10);
        let sliding_speed = p.gsp;
        run(&mut p, &level, right(), 1);
        assert!(!p.crouching && p.state == Move::Run);
        assert!(p.gsp >= sliding_speed - 1.0);
    }

    #[test]
    fn crouch_shrinks_hitbox_and_stands_back_up() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let feet = p.feet();
        run(&mut p, &level, InputState { move_y: 1.0, ..Default::default() }, 10);
        assert!(p.crouching && p.grounded && p.state == Move::Crouch);
        assert_eq!(p.rect().h, CROUCH_H);
        assert!((p.feet().y - feet.y).abs() < 0.01);
        run(&mut p, &level, InputState::default(), 5);
        assert!(!p.crouching && p.grounded);
        assert!((p.feet().y - feet.y).abs() < 0.01);
    }

    #[test]
    fn crouching_on_semisolid_drops_through() {
        let level = parse(&["#......#", "#..P...#", "#.====.#", "#......#", "#......#", "########"]);
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let platform_feet = p.feet().y;
        run(&mut p, &level, InputState { move_y: 1.0, ..Default::default() }, 40);
        run(&mut p, &level, InputState::default(), 60);
        assert!(p.grounded && p.feet().y > platform_feet + TILE, "still on platform: {:?}", p.feet());
    }

    #[test]
    fn vaults_a_one_tile_step_without_losing_speed() {
        let open = format!("#{}#", r(".", 62));
        let rows = [open.clone(), open.clone(), open, format!("#.P{}#{}#", r(".", 9), r(".", 51)), r("#", 64)];
        let level = Level::parse(&rows.join("
"));
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, right(), 40);
        let speed = p.gsp;
        let mut vaulted = false;
        for _ in 0..120 {
            p.step(&right(), &level, DT);
            vaulted |= p.events.contains(&PlayerEvent::Vault);
            p.events.clear();
        }
        assert!(vaulted, "never vaulted, stuck at {}", p.pos.x);
        assert!(p.pos.x > 14.0 * TILE);
        assert!(p.gsp >= speed - 1.0, "lost speed: {speed} -> {}", p.gsp);
    }

    #[test]
    fn fast_wall_hit_becomes_a_wallrun_that_reaches_the_ledge() {
        // Run-up, then a three-tile wall with a long plateau on top.
        let open = format!("#{}#", r(".", 58));
        let wall = format!("#{}{}#", r(".", 17), r("#", 41));
        let mut rows = vec![open; 6];
        rows.extend([wall.clone(), wall, format!("#.P{}{}#", r(".", 15), r("#", 41)), r("#", 60)]);
        let level = Level::parse(&rows.join("
"));
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let mut wallran = false;
        let mut mantled = 0;
        for _ in 0..400 {
            p.step(&right(), &level, DT);
            wallran |= p.events.iter().any(|e| matches!(e, PlayerEvent::WallRun(1)));
            if mantled > 0 || p.events.contains(&PlayerEvent::Mantle) {
                mantled += 1;
            }
            p.events.clear();
            if mantled > 10 {
                break;
            }
        }
        assert!(wallran, "no wall-run");
        assert!(mantled > 0, "never pulled up onto the ledge, ended at {:?}", p.pos);
        assert!(p.grounded && (p.bottom() - 6.0 * TILE).abs() < 0.5, "ended at {:?}", p.pos);
        assert!(p.gsp > 300.0, "mantle lost speed: {}", p.gsp);
    }

    #[test]
    fn half_block_gaps_only_fit_crouching() {
        // Upper halves at head height leave a half-tile gap over the floor.
        let level = parse(&["#....................#", "#....................#", "#.P.....TTTT.........#", "######################"]);
        // Walking slowly (no wall-run up onto them).
        let walk = InputState { move_x: 0.3, ..Default::default() };
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, walk, 240);
        assert!(p.pos.x + W <= 8.0 * TILE + 0.5, "walked under standing: {}", p.pos.x);

        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        run(&mut p, &level, InputState { move_y: 1.0, ..walk }, 600);
        assert!(p.pos.x > 12.0 * TILE, "couldn't crawl under: {}", p.pos.x);
    }

    #[test]
    fn half_steps_are_stepped_onto_and_stood_on() {
        let level = parse(&["#....................#", "#....................#", "#.P....__________....#", "######################"]);
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        // Walk slowly onto the step: no vault speed needed.
        let mut on_step = false;
        for _ in 0..240 {
            p.step(&InputState { move_x: 0.4, ..Default::default() }, &level, DT);
            p.events.clear();
            if p.grounded && (p.feet().y - (2.5 * TILE)).abs() < 0.5 {
                on_step = true;
                break;
            }
        }
        assert!(on_step, "never stood on the half step: feet {}", p.feet().y);
    }

    #[test]
    fn level_edges_block_but_cannot_be_climbed() {
        // No walls of its own: the map's sides are the only barrier.
        let level = parse(&["......", "......", "......", "..P...", "######"]);
        for dir in [1.0, -1.0] {
            let mut p = Player::new(level.spawn);
            settle(&mut p, &level);
            p.gsp = dir * 900.0;
            let input = InputState { move_x: dir, ..Default::default() };
            for frame in 0..240 {
                let mut i = input;
                i.jump_pressed = frame == 20;
                i.jump_held = frame >= 20;
                p.step(&i, &level, DT);
                assert!(p.state != Move::WallRun && p.state != Move::WallSlide, "climbed the edge: {:?}", p.state);
                p.events.clear();
            }
            assert!(p.pos.x >= 0.0 && p.pos.x + W <= level.pixel_width(), "left the map: {}", p.pos.x);
        }
    }

    #[test]
    fn slick_walls_cannot_be_run_kicked_or_mantled() {
        // A slick tower with a ledge in reach, and a slick wall behind.
        let level = parse(&[
            "%.........",
            "%.........",
            "%.......%%",
            "%.......%%",
            "%.......%%",
            "%.......%%",
            "%.......%%",
            "%...P...%%",
            "##########",
        ]);
        for dir in [1.0, -1.0] {
            let mut p = Player::new(level.spawn);
            settle(&mut p, &level);
            p.gsp = dir * 900.0;
            for frame in 0..240 {
                let input = InputState { move_x: dir, jump_pressed: frame % 20 == 5, jump_held: true, ..Default::default() };
                p.step(&input, &level, DT);
                assert!(p.state != Move::WallRun && p.state != Move::WallSlide, "gripped the slick wall: {:?}", p.state);
                assert!(!p.events.iter().any(|e| matches!(e, PlayerEvent::WallJump(_) | PlayerEvent::Mantle)), "{:?}", p.events);
                assert!(!(p.grounded && p.bottom() < 7.0 * TILE), "climbed onto the slick ledge");
                p.events.clear();
            }
        }
    }

    #[test]
    fn slow_wall_hit_does_not_wallrun() {
        let level = parse(&["#......#", "#....###", "#....###", "#.P..###", "########"]);
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        let walk = InputState { move_x: 0.5, ..Default::default() };
        run(&mut p, &level, walk, 120);
        assert!(p.grounded && p.state == Move::Run && p.gsp == 0.0);
    }

    #[test]
    fn wall_kicks_climb_the_chimney() {
        let level = parse(&[
            "..........",
            "..........",
            ".....#####",
            ".#...#####",
            ".#...#####",
            ".#...#####",
            ".#...#####",
            ".#...#####",
            ".#...#####",
            ".#...#####",
            ".....#####",
            "...P.#####",
            "##########",
        ]);
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        // Simple bot: hold toward the cliff, and wall-jump whenever touching a wall.
        let mut toward = 1.0;
        for frame in 0..1200 {
            let jump = (p.grounded && frame % 30 == 0) || (p.wall_dir != 0 && p.vel.y > -50.0);
            if p.wall_dir != 0 {
                toward = -p.wall_dir as f32;
            }
            let inp = InputState {
                move_x: if p.pos.y < 2.0 * TILE { 1.0 } else { toward },
                jump_pressed: jump,
                jump_held: true,
                ..Default::default()
            };
            p.step(&inp, &level, DT);
            p.events.clear();
            if p.grounded && p.bottom() <= 2.0 * TILE + 0.5 {
                return; // standing on top of the cliff
            }
        }
        panic!("never reached the top, ended at {:?}", p.pos);
    }

    #[test]
    fn dash_refills_on_the_ground_and_from_orbs() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        assert!(p.has_dash);
        let air_dash = InputState { move_x: 1.0, dash_pressed: true, ..Default::default() };
        p.step(&InputState { jump_pressed: true, jump_held: true, ..Default::default() }, &level, DT);
        run(&mut p, &level, InputState { jump_held: true, ..Default::default() }, 10);
        p.step(&air_dash, &level, DT);
        assert!(p.state == Move::Dash && !p.has_dash);
        run(&mut p, &level, InputState { jump_held: true, ..Default::default() }, 30);
        p.step(&air_dash, &level, DT);
        assert!(p.state != Move::Dash, "dashed twice without a refill");
        assert!(p.give_dash() && !p.give_dash());
        run(&mut p, &level, InputState::default(), 200);
        assert!(p.grounded && p.has_dash);
    }

    #[test]
    fn dash_keeps_existing_speed() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        p.step(&InputState { jump_pressed: true, jump_held: true, ..Default::default() }, &level, DT);
        p.vel.x = 1000.0;
        p.step(&InputState { move_x: 1.0, dash_pressed: true, ..Default::default() }, &level, DT);
        assert!(p.state == Move::Dash && p.vel.x >= 1000.0, "dash slowed you: {}", p.vel.x);
    }

    #[test]
    fn bounce_block_launches_higher_than_a_jump() {
        let level = parse(&[
            "#........#",
            "#........#",
            "#........#",
            "#........#",
            "#........#",
            "#........#",
            "#........#",
            "#........#",
            "#..P.....#",
            "#..B.....#",
            "##########",
        ]);
        let mut p = Player::new(level.spawn);
        let start = p.feet().y;
        let mut best = 0.0f32;
        for _ in 0..240 {
            p.step(&InputState::default(), &level, DT);
            best = best.max(start - p.feet().y);
        }
        assert!(best > 4.5 * TILE, "bounce only reached {best}");
    }

    #[test]
    fn jump_bubble_gives_one_air_jump() {
        let level = flat_level();
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        p.step(&InputState { jump_pressed: true, jump_held: true, ..Default::default() }, &level, DT);
        run(&mut p, &level, InputState { jump_held: true, ..Default::default() }, 20);
        let vy = p.vel.y;
        p.step(&InputState { jump_pressed: true, jump_held: true, ..Default::default() }, &level, DT);
        assert!(p.vel.y >= vy, "air jump without a bubble");
        assert!(p.give_air_jump() && !p.give_air_jump());
        p.step(&InputState { jump_pressed: true, jump_held: true, ..Default::default() }, &level, DT);
        assert!(p.vel.y < -400.0 && p.air_jumps == 0);
    }

    #[test]
    fn dissolved_crumble_block_is_not_solid() {
        let mut level = parse(&["#....#", "#.P..#", "#.X..#", "#....#", "######"]);
        let mut p = Player::new(level.spawn);
        settle(&mut p, &level);
        assert_eq!(p.standing_on(&level, Tile::Crumble), vec![(2, 2)]);
        level.set_gone(2, 2, true);
        run(&mut p, &level, InputState::default(), 60);
        assert!(p.grounded && p.feet().y > 3.0 * TILE + 1.0, "should have fallen through");
    }
}
