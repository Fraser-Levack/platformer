//! One running level: fixed-timestep simulation, pause, restart, drawing.
//! Used by normal play and by the editor's playtest mode.

use crate::audio;
use crate::debug;
use crate::game::Game;
use crate::input::Input;
use crate::level::Level;
use crate::palette;
use crate::render;
use crate::speedometer;
use macroquad::prelude::*;

/// Physics runs at a fixed rate; rendering interpolates between steps,
/// so motion is smooth and identical at any monitor refresh rate.
const FIXED_DT: f32 = 1.0 / 120.0;

pub struct Session {
    pub game: Game,
    src: String,
    spawn: Option<Vec2>,
    accumulator: f32,
    pub paused: bool,
    /// Whether the controller's Back/Select button restarts (the editor uses it to exit).
    select_restarts: bool,
}

fn build(src: &str, spawn: Option<Vec2>) -> Game {
    let mut level = Level::parse(src);
    if let Some(s) = spawn {
        level.spawn = s;
    }
    Game::new(level)
}

impl Session {
    /// `spawn` overrides the level's start position (feet position).
    pub fn new(src: String, spawn: Option<Vec2>, select_restarts: bool) -> Self {
        speedometer::clear();
        Session {
            game: build(&src, spawn),
            src,
            spawn,
            accumulator: 0.0,
            paused: false,
            select_restarts,
        }
    }

    pub fn restart(&mut self) {
        audio::play(crate::synth::Sfx::Respawn);
        self.game = build(&self.src, self.spawn);
        self.accumulator = 0.0;
        self.paused = false;
    }

    /// Advance one rendered frame. Returns true when a finished level is confirmed.
    pub fn update(&mut self, input: &mut Input, frame_dt: f32) -> bool {
        let st = input.state;
        let restart = st.restart_pressed || (self.select_restarts && st.select_pressed);
        if self.game.finished {
            if st.confirm_pressed {
                input.consume_presses();
                audio::play(crate::synth::Sfx::Click);
                return true;
            }
            if restart {
                self.restart();
            }
        } else if restart {
            self.restart();
        } else if st.pause_pressed || (self.paused && st.confirm_pressed) {
            self.paused = !self.paused;
            audio::play(crate::synth::Sfx::Pause);
            input.consume_presses();
        }

        if self.paused {
            input.consume_presses();
        } else {
            self.accumulator += frame_dt;
            while self.accumulator >= FIXED_DT {
                self.game.step(&input.state, FIXED_DT);
                speedometer::record(&self.game.player);
                input.consume_presses();
                self.accumulator -= FIXED_DT;
            }
        }
        for (s, v) in self.game.sfx.drain(..) {
            audio::play_vol(s, v);
        }
        false
    }

    /// `finish_action` completes "Press A to ..." on the level-complete screen.
    pub fn draw(&mut self, input: &Input, frame_dt: f32, finish_action: &str) {
        let alpha = self.accumulator / FIXED_DT;
        palette::set_theme(self.game.level.theme);
        // Duck the music while paused and under the level-complete jingle.
        let level = if self.paused { 0.35 } else if self.game.finished { 0.3 } else { 1.0 };
        audio::music(self.game.level.theme, level);
        self.game.update_camera(frame_dt, alpha);
        render::draw_world(&self.game, alpha, self.game.current_zoom());
        render::draw_hud(&self.game, input, self.paused, finish_action);
        debug::draw(&self.game.player);
    }
}
