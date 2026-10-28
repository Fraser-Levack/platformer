//! Speed history for the HUD speedometer: the last few seconds of speed,
//! coloured by move, with run/top speed guides.

use crate::palette::*;
use crate::player::{Move, Player};
use crate::tuning;
use macroquad::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;

/// Physics steps kept in the graph (3 s at 120 Hz).
const HISTORY: usize = 360;

thread_local! {
    static HISTORY_BUF: RefCell<VecDeque<(f32, Move)>> = const { RefCell::new(VecDeque::new()) };
}

/// Call once per physics step.
pub fn record(p: &Player) {
    HISTORY_BUF.with(|h| {
        let mut h = h.borrow_mut();
        if h.len() >= HISTORY {
            h.pop_front();
        }
        h.push_back((p.speed(), p.state));
    });
}

/// Forget the graph (new level).
pub fn clear() {
    HISTORY_BUF.with(|h| h.borrow_mut().clear());
}

/// UI scale: designed for a 720 px tall window.
pub fn ui() -> f32 {
    (screen_height() / 720.0).clamp(0.5, 2.0)
}

/// Where the HUD speedometer panel sits (bottom left).
pub fn panel_rect() -> Rect {
    let k = ui();
    let (w, h) = (330.0 * k, 128.0 * k);
    Rect::new(16.0 * k, screen_height() - h - 16.0 * k, w, h)
}

pub fn move_color(m: Move) -> Color {
    match m {
        Move::Run => Color::from_hex(0xf2f6fa),
        Move::Slide => Color::from_hex(0xffa64d),
        Move::Crouch => Color::from_hex(0x9aa4ad),
        Move::Air => Color::from_hex(0x5cd6ff),
        Move::WallRun => Color::from_hex(0xff5cc8),
        Move::WallSlide => Color::from_hex(0xb07cff),
        Move::Dash => orb(),
    }
}

/// Draw the graph into a screen rect. `k` is the UI scale.
pub fn draw_graph(r: Rect, k: f32) {
    let t = tuning::get();
    let max = (t.top_speed * 1.45).max(1.0);
    let to_y = |s: f32| r.y + r.h - (s / max).min(1.0) * r.h;
    for (s, label) in [(t.run_speed, "run"), (t.top_speed, "top")] {
        let ly = to_y(s);
        let mut x = r.x;
        // Dashed guide.
        while x < r.x + r.w - 22.0 * k {
            draw_line(x, ly, (x + 4.0 * k).min(r.x + r.w), ly, 1.0, alpha(WHITE, 0.22));
            x += 8.0 * k;
        }
        draw_text(label, r.x + r.w - 18.0 * k, ly + 4.0 * k, 13.0 * k, alpha(WHITE, 0.45));
    }
    HISTORY_BUF.with(|h| {
        let h = h.borrow();
        let n = h.len();
        let w = r.w - 22.0 * k;
        let x_at = |i: usize| r.x + w * (HISTORY - n + i) as f32 / HISTORY as f32;
        // Soft fill under the line, then the line itself.
        for (i, (a, b)) in h.iter().zip(h.iter().skip(1)).enumerate() {
            let (x0, x1) = (x_at(i), x_at(i + 1));
            let c = alpha(move_color(b.1), 0.16);
            let (y0, y1) = (to_y(a.0), to_y(b.0));
            draw_triangle(vec2(x0, y0), vec2(x1, y1), vec2(x1, r.y + r.h), c);
            draw_triangle(vec2(x0, y0), vec2(x1, r.y + r.h), vec2(x0, r.y + r.h), c);
        }
        for (i, (a, b)) in h.iter().zip(h.iter().skip(1)).enumerate() {
            draw_line(x_at(i), to_y(a.0), x_at(i + 1), to_y(b.0), 2.0 * k, move_color(b.1));
        }
        if let Some(last) = h.back() {
            let p = vec2(x_at(n - 1), to_y(last.0));
            draw_circle(p.x, p.y, 3.0 * k, move_color(last.1));
        }
    });
}
