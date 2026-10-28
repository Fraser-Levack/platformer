//! Enemies. Crawlers patrol the ground, floaters hover in place.
//! Landing on top of one squashes it; touching it from the side or below hurts.

use crate::level::{EnemyKind, Level, Tile, TILE};
use macroquad::prelude::*;

const CRAWLER_SIZE: Vec2 = Vec2::new(26.0, 18.0);
const FLOATER_SIZE: Vec2 = Vec2::new(24.0, 22.0);
const CRAWL_SPEED: f32 = 60.0;
const GRAVITY: f32 = 1800.0;
const MAX_FALL: f32 = 700.0;
/// How far below an enemy's top the player's feet may have been last step and still count as a stomp.
const STOMP_TOLERANCE: f32 = 10.0;
/// Squashed enemies linger this long for their death animation.
pub const DEATH_TIME: f32 = 0.35;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Contact {
    None,
    /// Player landed on top: enemy dies, player bounces.
    Stomp,
    /// Player touched it from the side or below.
    Hurt,
}

pub struct Enemy {
    pub kind: EnemyKind,
    /// Centre of the hitbox.
    pub pos: Vec2,
    pub prev_pos: Vec2,
    vel: Vec2,
    /// Walking direction (crawlers), -1 or 1.
    pub dir: f32,
    home: Vec2,
    pub phase: f32,
    pub alive: bool,
    /// Counts down after death; the enemy is removed from drawing at 0.
    pub death_t: f32,
    pub grounded: bool,
    pub t: f32,
}

impl Enemy {
    /// `cell_center` is the centre of the level cell it was placed in.
    pub fn new(kind: EnemyKind, cell_center: Vec2) -> Self {
        let pos = match kind {
            // Crawlers stand on the bottom of their cell.
            EnemyKind::Crawler => vec2(cell_center.x, cell_center.y + TILE / 2.0 - CRAWLER_SIZE.y / 2.0),
            EnemyKind::Floater => cell_center,
        };
        Enemy {
            kind,
            pos,
            prev_pos: pos,
            vel: Vec2::ZERO,
            dir: -1.0,
            home: pos,
            phase: (cell_center.x * 0.37 + cell_center.y * 0.11) % std::f32::consts::TAU,
            alive: true,
            death_t: 0.0,
            grounded: false,
            t: 0.0,
        }
    }

    pub fn size(&self) -> Vec2 {
        match self.kind {
            EnemyKind::Crawler => CRAWLER_SIZE,
            EnemyKind::Floater => FLOATER_SIZE,
        }
    }

    pub fn rect(&self) -> Rect {
        let s = self.size();
        Rect::new(self.pos.x - s.x / 2.0, self.pos.y - s.y / 2.0, s.x, s.y)
    }

    pub fn squash(&mut self) {
        self.alive = false;
        self.death_t = DEATH_TIME;
    }

    /// Advance one step. Returns true if it died to a hazard this step.
    pub fn update(&mut self, level: &Level, dt: f32) -> bool {
        self.prev_pos = self.pos;
        self.t += dt;
        if !self.alive {
            self.death_t = (self.death_t - dt).max(0.0);
            return false;
        }
        match self.kind {
            EnemyKind::Floater => {
                self.pos = self.home
                    + vec2((self.t * 1.1 + self.phase).sin() * 5.0, (self.t * 2.0 + self.phase).sin() * 7.0);
                false
            }
            EnemyKind::Crawler => self.crawl(level, dt),
        }
    }

    fn crawl(&mut self, level: &Level, dt: f32) -> bool {
        let half = CRAWLER_SIZE / 2.0;
        self.vel.x = self.dir * CRAWL_SPEED;
        self.vel.y = (self.vel.y + GRAVITY * dt).min(MAX_FALL);

        // Horizontal: turn around at walls.
        self.pos.x += self.vel.x * dt;
        if level.rect_hits_solid(self.rect()) {
            self.pos.x = self.prev_pos.x;
            self.dir = -self.dir;
        }

        // Vertical: land on solids and one-way platforms.
        let prev_bottom = self.pos.y + half.y;
        self.pos.y += self.vel.y * dt;
        let r = self.rect();
        let row = ((r.y + r.h - 0.01) / TILE).floor() as i32;
        let top = row as f32 * TILE;
        let x0 = (r.x / TILE).floor() as i32;
        let x1 = ((r.x + r.w - 0.01) / TILE).floor() as i32;
        let lands = (x0..=x1).any(|x| {
            level.is_solid(x, row) || (level.tile(x, row) == Tile::OneWay && prev_bottom <= top + 0.5)
        });
        self.grounded = self.vel.y >= 0.0 && lands;
        if self.grounded {
            self.pos.y = top - half.y;
            self.vel.y = 0.0;
        }

        // Turn around at ledges rather than walking off.
        if self.grounded {
            let probe = vec2(self.pos.x + self.dir * (half.x + 2.0), self.pos.y + half.y + 2.0);
            let (tx, ty) = ((probe.x / TILE).floor() as i32, (probe.y / TILE).floor() as i32);
            let no_floor = !(level.is_solid(tx, ty) || level.tile(tx, ty) == Tile::OneWay);
            // ...or at hazards right in front of them.
            let ahead = level.tile(tx, (self.pos.y / TILE).floor() as i32);
            if no_floor || matches!(ahead, Tile::Spike | Tile::Acid) {
                self.dir = -self.dir;
            }
        }

        let body = Rect::new(r.x + 3.0, r.y + 3.0, r.w - 6.0, r.h - 3.0);
        if level.rect_hits_hazard(body) || self.pos.y > level.pixel_height() + 64.0 {
            self.squash();
            return true;
        }
        false
    }

    /// How the player is touching this enemy, if at all.
    pub fn contact(&self, player: Rect, player_prev_bottom: f32, player_vel_y: f32) -> Contact {
        if !self.alive {
            return Contact::None;
        }
        let r = self.rect();
        if !player.overlaps(&r) {
            return Contact::None;
        }
        if player_vel_y >= 0.0 && player_prev_bottom <= r.y + STOMP_TOLERANCE {
            return Contact::Stomp;
        }
        // Slightly forgiving hurtbox for side/bottom touches.
        let hurt = Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0);
        if player.overlaps(&hurt) {
            Contact::Hurt
        } else {
            Contact::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crawler_in(src: &str) -> (Level, Enemy) {
        let level = Level::parse(src);
        let (kind, pos) = level.enemies[0];
        (level, Enemy::new(kind, pos))
    }

    #[test]
    fn crawler_turns_at_ledges_instead_of_falling() {
        let (level, mut e) = crawler_in(
            "\
..........
...E......
..####....
..........
..........
##########",
        );
        let start_y = e.pos.y;
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        for _ in 0..1200 {
            e.update(&level, 1.0 / 120.0);
            min_x = min_x.min(e.pos.x);
            max_x = max_x.max(e.pos.x);
        }
        assert!(e.alive);
        assert!((e.pos.y - start_y).abs() < 0.5, "fell off: {}", e.pos.y);
        assert!(min_x >= 2.0 * TILE && max_x <= 6.0 * TILE, "left the platform: {min_x}..{max_x}");
        assert!(max_x - min_x > TILE, "should patrol, moved only {}", max_x - min_x);
    }

    #[test]
    fn crawler_turns_at_walls() {
        let (level, mut e) = crawler_in(
            "\
#......#
#..E...#
########",
        );
        let mut turned = 0;
        let mut last = e.dir;
        for _ in 0..1200 {
            e.update(&level, 1.0 / 120.0);
            if e.dir != last {
                turned += 1;
                last = e.dir;
            }
            assert!(e.rect().x >= TILE - 0.01 && e.rect().right() <= 7.0 * TILE + 0.01);
        }
        assert!(turned >= 2);
    }

    #[test]
    fn crawler_treats_acid_as_a_ledge() {
        let (level, mut e) = crawler_in(
            "\
.E....
###~~#
######",
        );
        for _ in 0..1200 {
            assert!(!e.update(&level, 1.0 / 120.0), "walked into the acid");
        }
        assert!(e.alive && e.rect().right() <= 3.0 * TILE + 0.01);
    }

    #[test]
    fn crawler_turns_before_spikes() {
        let (level, mut e) = crawler_in(
            "\n#..E..^.#
#########",
        );
        e.dir = 1.0;
        for _ in 0..1200 {
            assert!(!e.update(&level, 1.0 / 120.0), "walked into the spikes");
        }
        assert!(e.alive);
    }

    #[test]
    fn crawler_dies_when_falling_into_acid() {
        let (level, mut e) = crawler_in(
            "\
#E.#
#..#
#~~#
####",
        );
        let mut died = false;
        for _ in 0..240 {
            died |= e.update(&level, 1.0 / 120.0);
        }
        assert!(died && !e.alive);
    }

    #[test]
    fn stomp_from_above_hurt_from_side_and_below() {
        let e = Enemy::new(EnemyKind::Floater, vec2(100.0, 100.0));
        let r = e.rect();
        // Falling onto the top.
        let p = Rect::new(r.x, r.y - 26.0, 22.0, 28.0);
        assert_eq!(e.contact(p, r.y - 1.0, 300.0), Contact::Stomp);
        // Walking into the side.
        let p = Rect::new(r.x - 20.0, r.y - 4.0, 22.0, 28.0);
        assert_eq!(e.contact(p, p.bottom(), 0.0), Contact::Hurt);
        // Jumping into it from below.
        let p = Rect::new(r.x, r.bottom() - 6.0, 22.0, 28.0);
        assert_eq!(e.contact(p, p.bottom() + 5.0, -400.0), Contact::Hurt);
        // Not touching.
        let p = Rect::new(r.x + 100.0, r.y, 22.0, 28.0);
        assert_eq!(e.contact(p, p.bottom(), 0.0), Contact::None);
    }
}
