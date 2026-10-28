//! Movement debug overlay (F1): current move and raw speeds, shown above the
//! HUD speedometer. Also shows tuning-file notices.

use crate::player::Player;
use crate::speedometer::{self, move_color, ui};
use macroquad::prelude::*;
use std::cell::RefCell;

const NOTICE_TIME: f32 = 2.5;

struct Debug {
    on: bool,
    notice: Option<(String, f32)>,
}

thread_local! {
    static DEBUG: RefCell<Debug> = const { RefCell::new(Debug { on: false, notice: None }) };
}

pub fn toggle() {
    DEBUG.with(|d| d.borrow_mut().on ^= true);
}

pub fn notify(msg: String) {
    DEBUG.with(|d| d.borrow_mut().notice = Some((msg, NOTICE_TIME)));
}

pub fn draw(p: &Player) {
    if !DEBUG.with(|d| d.borrow().on) {
        return;
    }
    let k = ui();
    let below = speedometer::panel_rect();
    let (w, h) = (below.w, 92.0 * k);
    let (x, y) = (below.x, below.y - h - 8.0 * k);
    draw_rectangle(x, y, w, h, Color::new(0.03, 0.05, 0.08, 0.82));
    draw_rectangle_lines(x, y, w, h, 1.0, Color::new(1.0, 1.0, 1.0, 0.2));

    let line = |i: usize, text: &str, color: Color| {
        draw_text(text, x + 10.0 * k, y + (20.0 + i as f32 * 19.0) * k, 18.0 * k, color)
    };
    line(0, &format!("move  {}", p.state.name()), move_color(p.state));
    line(1, &format!("speed {:4.0}  ground {:5.0}  slope {:+.2}", p.speed(), p.gsp, p.gradient), WHITE);
    let dash = if p.has_dash { "ready" } else { "used" };
    line(2, &format!("vel ({:5.0}, {:5.0})  dash {dash}", p.vel.x, p.vel.y), WHITE);
    line(3, "F1 hide   edit assets/tuning.txt while playing", Color::new(1.0, 1.0, 1.0, 0.5));
}

/// Toast for tuning reloads. Call every frame.
pub fn draw_notice(dt: f32) {
    DEBUG.with(|d| {
        let mut d = d.borrow_mut();
        let Some((msg, time)) = &mut d.notice else { return };
        let msg: &str = msg;
        *time -= dt;
        let a = time.clamp(0.0, 1.0);
        let k = ui();
        let size = 24.0 * k;
        let dims = measure_text(msg, None, size as u16, 1.0);
        let x = (screen_width() - dims.width) / 2.0;
        let y = screen_height() - 70.0 * k;
        draw_rectangle(x - 12.0 * k, y - 26.0 * k, dims.width + 24.0 * k, 36.0 * k, Color::new(0.03, 0.05, 0.08, 0.8 * a));
        draw_text(msg, x, y, size, Color::new(1.0, 1.0, 1.0, a));
        if *time <= 0.0 {
            d.notice = None;
        }
    });
}
