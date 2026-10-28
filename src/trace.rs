//! Player traces for the editor: records where the player went during playtests
//! so the paths can be drawn over the level afterwards.

use crate::game::Game;
use crate::level::TILE;
use crate::palette::*;
use macroquad::prelude::*;
use std::collections::VecDeque;

/// How many playtest runs are kept; older ones fade out and are dropped.
const MAX_RUNS: usize = 8;
/// Minimum distance between recorded points, in world pixels.
const MIN_STEP: f32 = 3.0;
/// A jump bigger than this between samples is a teleport (respawn), not movement.
const TELEPORT: f32 = TILE * 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Motion {
    Ground,
    Air,
    Dash,
}

impl Motion {
    fn color(self) -> Color {
        match self {
            Motion::Ground => player(),
            Motion::Air => lighten(player(), 0.5),
            Motion::Dash => orb(),
        }
    }
}

/// One playtest attempt, from (re)start until exit or restart.
struct Run {
    /// Continuous stretches of movement; a death or respawn starts a new one.
    segments: Vec<Vec<(Vec2, Motion)>>,
    deaths: Vec<Vec2>,
    finish: Option<Vec2>,
    time: f32,
    was_dead: bool,
}

impl Run {
    fn new() -> Self {
        Run { segments: Vec::new(), deaths: Vec::new(), finish: None, time: 0.0, was_dead: false }
    }

    fn is_empty(&self) -> bool {
        self.segments.iter().all(|s| s.len() < 2)
    }

    fn start(&self) -> Option<Vec2> {
        self.segments.first().and_then(|s| s.first()).map(|p| p.0)
    }

    fn end(&self) -> Option<Vec2> {
        self.segments.last().and_then(|s| s.last()).map(|p| p.0)
    }
}

pub struct Traces {
    runs: VecDeque<Run>,
    pub visible: bool,
}

impl Traces {
    pub fn new() -> Self {
        Traces { runs: VecDeque::new(), visible: true }
    }

    pub fn clear(&mut self) {
        self.runs.clear();
    }

    pub fn count(&self) -> usize {
        self.runs.iter().filter(|r| !r.is_empty()).count()
    }

    /// Deaths and time of the most recent run, if any.
    pub fn last_summary(&self) -> Option<(usize, f32, bool)> {
        self.runs.back().filter(|r| !r.is_empty()).map(|r| (r.deaths.len(), r.time, r.finish.is_some()))
    }

    /// Begin recording a new run (a playtest started).
    pub fn begin(&mut self) {
        if self.runs.back().is_some_and(|r| r.is_empty()) {
            self.runs.pop_back();
        }
        self.runs.push_back(Run::new());
        while self.runs.len() > MAX_RUNS {
            self.runs.pop_front();
        }
    }

    /// Sample the game once per rendered frame while playtesting.
    pub fn record(&mut self, game: &Game) {
        // The timer going backwards means the player restarted (R): that's a new run.
        if self.runs.back().is_some_and(|r| game.time < r.time) {
            self.begin();
        }
        let Some(run) = self.runs.back_mut() else { return };
        run.time = game.time;

        let p = &game.player;
        let pos = p.center();
        if p.dead {
            if !run.was_dead {
                run.was_dead = true;
                run.deaths.push(pos);
            }
            return;
        }
        if run.was_dead {
            run.was_dead = false;
            run.segments.push(Vec::new()); // respawned somewhere else
        }
        if game.finished {
            run.finish.get_or_insert(pos);
        }

        let motion = if p.dash_timer > 0.0 {
            Motion::Dash
        } else if p.grounded {
            Motion::Ground
        } else {
            Motion::Air
        };
        let last = run.segments.last().and_then(|s| s.last()).copied();
        match last {
            Some((q, _)) if q.distance(pos) > TELEPORT => run.segments.push(vec![(pos, motion)]),
            Some((q, m)) if q.distance(pos) < MIN_STEP && m == motion => {}
            Some(_) => run.segments.last_mut().unwrap().push((pos, motion)),
            None => run.segments.push(vec![(pos, motion)]),
        }
    }

    /// Draw every kept run; the newest is brightest. `to_screen` maps world to screen.
    pub fn draw(&self, to_screen: impl Fn(Vec2) -> Vec2, zoom: f32, ui: f32) {
        if !self.visible {
            return;
        }
        let n = self.runs.len();
        for (i, run) in self.runs.iter().enumerate() {
            let newest = i + 1 == n;
            let a = if newest { 0.9 } else { 0.15 + 0.4 * (i + 1) as f32 / n as f32 };
            let thick = (if newest { 3.0 } else { 2.0 }) * zoom.clamp(0.5, 1.5);

            for seg in &run.segments {
                for w in seg.windows(2) {
                    let (p0, p1) = (to_screen(w[0].0), to_screen(w[1].0));
                    draw_line(p0.x, p0.y, p1.x, p1.y, thick, alpha(w[1].1.color(), a));
                }
            }

            let r = 5.0 * ui;
            if let Some(s) = run.start() {
                let s = to_screen(s);
                draw_circle_lines(s.x, s.y, r, 2.0, alpha(WHITE, a));
            }
            if let (Some(e), None) = (run.end(), run.finish) {
                let e = to_screen(e);
                draw_circle(e.x, e.y, r * 0.7, alpha(player(), a));
            }
            if let Some(f) = run.finish {
                let f = to_screen(f);
                draw_circle(f.x, f.y, r * 1.2, alpha(pickup(), a));
                draw_circle_lines(f.x, f.y, r * 1.8, 2.0, alpha(pickup(), a));
            }
            for d in &run.deaths {
                let d = to_screen(*d);
                let k = r * 1.3;
                draw_line(d.x - k, d.y - k, d.x + k, d.y + k, 3.0, alpha(hazard(), a));
                draw_line(d.x + k, d.y - k, d.x - k, d.y + k, 3.0, alpha(hazard(), a));
            }
        }
    }
}
