//! Level editor. Launch with `cargo run --release -- --editor`.
//!
//! - Two layers: Tiles and Decor (Tab or the tabs above the palette). Decorations
//!   never collide and draw over blocks; signs live on the decor layer.
//! - Pick a tool from the palette (click it, press 1-9, or drag it onto the grid).
//! - Gate tool: [ and ] (or the -/+ buttons) set how many coins the gate needs.
//! - Sign tool: click to place a sign or edit one, then type. Enter finishes,
//!   Shift+Enter starts a new line.
//! - Left mouse paints, right mouse erases. Middle mouse / Space+drag / WASD pans, wheel zooms.
//! - F5 playtests from the spawn, F6 from the cursor; Esc/F5/Back returns to editing.
//! - Player traces from playtests are drawn over the level: L toggles them, Shift+L clears.
//! - M / N toggle music / sound effects (everywhere, not just the editor).
//! - Changes autosave to `assets/levels/`.

use crate::audio;
use crate::game::Game;
use crate::input::Input;
use crate::level::{encode_sign_text, DecorKind, Half, Level, WallKind, TILE};
use crate::levels;
use crate::palette::*;
use crate::player::{H, W};
use crate::render;
use crate::session::Session;
use crate::synth::Sfx;
use crate::trace::Traces;
use macroquad::prelude::*;
use std::f32::consts::TAU;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// UI scale: the layout is designed for a 1280x720 window and scales with it
/// (this also keeps it usable on high-DPI displays, where the logical size is smaller).
fn ui() -> f32 {
    (screen_width() / 1280.0).min(screen_height() / 720.0).clamp(0.45, 2.0)
}
fn top_h() -> f32 {
    86.0 * ui()
}
fn left_w() -> f32 {
    224.0 * ui()
}
fn bottom_h() -> f32 {
    30.0 * ui()
}
fn font() -> f32 {
    20.0 * ui()
}
const MIN_W: usize = 12;
const MIN_H: usize = 8;
const NEW_W: usize = 48;
const NEW_H: usize = 16;
const AUTOSAVE_DELAY: f32 = 0.6;
const UNDO_LIMIT: usize = 200;
const SIGN_MAX_LEN: usize = 200;
const DEFAULT_GATE_NEED: u32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layer {
    Tiles,
    Walls,
    /// Walls in front of the player.
    Front,
    Decor,
}

impl Layer {
    const ALL: [Layer; 4] = [Layer::Tiles, Layer::Walls, Layer::Front, Layer::Decor];

    fn tools(self) -> &'static [Tool] {
        match self {
            Layer::Tiles => &Tool::TILES,
            Layer::Walls | Layer::Front => &Tool::WALLS,
            Layer::Decor => &Tool::DECOR,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Layer::Tiles => "Tiles",
            Layer::Walls => "Walls",
            Layer::Front => "Front",
            Layer::Decor => "Decor",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Block,
    Slick,
    Ramp,
    GentleRamp,
    Half,
    Platform,
    Bounce,
    Crumble,
    Spikes,
    Acid,
    Crawler,
    Floater,
    Coin,
    DashOrb,
    Bubble,
    Gate,
    Checkpoint,
    Spawn,
    Goal,
    Cilia,
    Tendril,
    Eye,
    Cysts,
    Scar,
    Veins,
    Sign,
    Screen,
    Neon,
    Grass,
    Railing,
    Ladder,
    Floodlight,
    Skylight,
    Crates,
    Wall,
    Window,
    LowWall,
    Pillar,
    GlassWall,
    Shutter,
    AccentWall,
    Eraser,
}

impl Tool {
    /// Palette order: terrain, hazards, enemies, pickups, markers.
    const TILES: [Tool; 20] = [
        Tool::Block,
        Tool::Slick,
        Tool::Ramp,
        Tool::GentleRamp,
        Tool::Half,
        Tool::Platform,
        Tool::Bounce,
        Tool::Crumble,
        Tool::Spikes,
        Tool::Acid,
        Tool::Crawler,
        Tool::Floater,
        Tool::Coin,
        Tool::DashOrb,
        Tool::Bubble,
        Tool::Gate,
        Tool::Checkpoint,
        Tool::Spawn,
        Tool::Goal,
        Tool::Eraser,
    ];

    const WALLS: [Tool; 8] = [
        Tool::Wall,
        Tool::Window,
        Tool::LowWall,
        Tool::Pillar,
        Tool::GlassWall,
        Tool::Shutter,
        Tool::AccentWall,
        Tool::Eraser,
    ];

    const DECOR: [Tool; 16] = [
        Tool::Screen,
        Tool::Neon,
        Tool::Grass,
        Tool::Railing,
        Tool::Ladder,
        Tool::Floodlight,
        Tool::Skylight,
        Tool::Crates,
        Tool::Cilia,
        Tool::Tendril,
        Tool::Eye,
        Tool::Cysts,
        Tool::Scar,
        Tool::Veins,
        Tool::Sign,
        Tool::Eraser,
    ];

    fn ch(self) -> char {
        match self {
            Tool::Block => '#',
            Tool::Slick => '%',
            Tool::Ramp => '/',
            Tool::GentleRamp => '1',
            Tool::Half => '_',
            Tool::Platform => '=',
            Tool::Bounce => 'B',
            Tool::Crumble => 'X',
            Tool::Spikes => '^',
            Tool::Acid => '~',
            Tool::Crawler => 'E',
            Tool::Floater => 'F',
            Tool::Coin => 'o',
            Tool::DashOrb => 'D',
            Tool::Bubble => 'J',
            Tool::Gate => 'L',
            Tool::Checkpoint => 'C',
            Tool::Spawn => 'P',
            Tool::Goal => 'G',
            Tool::Cilia => 'c',
            Tool::Tendril => 't',
            Tool::Eye => 'e',
            Tool::Cysts => 'u',
            Tool::Scar => 'x',
            Tool::Veins => 'v',
            Tool::Sign => 'S',
            Tool::Screen => 'A',
            Tool::Neon => 'N',
            Tool::Grass => 'g',
            Tool::Railing => 'r',
            Tool::Ladder => 'l',
            Tool::Floodlight => 'f',
            Tool::Skylight => 'k',
            Tool::Crates => 'b',
            Tool::Wall => 'w',
            Tool::Window => 'n',
            Tool::LowWall => 'h',
            Tool::Pillar => 'p',
            Tool::GlassWall => 'g',
            Tool::Shutter => 'd',
            Tool::AccentWall => 'a',
            Tool::Eraser => '.',
        }
    }

    /// Tools placed in one of four rotations (R / the Rotate button).
    fn rotates(self) -> bool {
        matches!(self, Tool::Half | Tool::Eye | Tool::Scar)
    }

    /// Tools whose cells carry typed text.
    fn has_text(self) -> bool {
        matches!(self, Tool::Sign | Tool::Neon)
    }

    fn label(self) -> &'static str {
        match self {
            Tool::Block => "Block",
            Tool::Slick => "Slick",
            Tool::Ramp => "Ramp",
            Tool::GentleRamp => "Gentle",
            Tool::Half => "Half",
            Tool::Platform => "Platform",
            Tool::Bounce => "Bounce",
            Tool::Crumble => "Crumble",
            Tool::Spikes => "Spikes",
            Tool::Acid => "Acid",
            Tool::Crawler => "Crawler",
            Tool::Floater => "Floater",
            Tool::Coin => "Coin",
            Tool::DashOrb => "Dash orb",
            Tool::Bubble => "Bubble",
            Tool::Gate => "Gate",
            Tool::Checkpoint => "Checkpt",
            Tool::Spawn => "Spawn",
            Tool::Goal => "Goal",
            Tool::Cilia => "Plant",
            Tool::Tendril => "Vines",
            Tool::Eye => "Camera",
            Tool::Cysts => "AC unit",
            Tool::Scar => "Pipe",
            Tool::Veins => "Antenna",
            Tool::Sign => "Sign",
            Tool::Screen => "Screen",
            Tool::Neon => "Neon",
            Tool::Grass => "Grass",
            Tool::Railing => "Railing",
            Tool::Ladder => "Ladder",
            Tool::Floodlight => "Light",
            Tool::Skylight => "Skylight",
            Tool::Crates => "Crates",
            Tool::Wall => "Wall",
            Tool::Window => "Window",
            Tool::LowWall => "Low wall",
            Tool::Pillar => "Pillar",
            Tool::GlassWall => "Glass",
            Tool::Shutter => "Shutter",
            Tool::AccentWall => "Accent",
            Tool::Eraser => "Eraser",
        }
    }

    /// Only one per level; placing it moves the existing one.
    fn unique(self) -> bool {
        matches!(self, Tool::Spawn | Tool::Goal)
    }
}

// ---------- Grid model ----------

#[derive(Clone, PartialEq)]
struct Grid {
    rows: Vec<Vec<char>>,
    /// Decoration layer, same size as `rows`.
    decor: Vec<Vec<char>>,
    /// Wall layer (half-depth walls behind the decorations), same size as `rows`.
    walls: Vec<Vec<char>>,
    /// Front wall layer (in front of the player), same size as `rows`.
    front: Vec<Vec<char>>,
    /// Coins needed by each gate cell.
    gates: BTreeMap<(usize, usize), u32>,
    /// Text of each sign (keyed by its decor cell).
    signs: BTreeMap<(usize, usize), String>,
    /// Quarter turns of rotated decorations (keyed by decor cell; absent = 0).
    rots: BTreeMap<(usize, usize), u8>,
    theme: usize,
}

const VALID: &[char] = &[
    '#', '%', '=', 'B', 'X', '^', '~', 'E', 'F', 'o', 'D', 'J', 'L', 'C', 'P', 'G', '/', '\\', '1', '2', '3', '4', '_', '[', 'T', ']',
];

impl Grid {
    fn from_text(src: &str) -> Self {
        let level = Level::parse(src); // reuse its metadata parsing
        let mut rows: Vec<Vec<char>> = Vec::new();
        let mut decor: Vec<Vec<char>> = Vec::new();
        let mut walls: Vec<Vec<char>> = Vec::new();
        let mut front: Vec<Vec<char>> = Vec::new();
        let mut decor_src: Vec<&str> = Vec::new();
        let mut section = 0; // 0 tiles, 1 decor, 2 walls, 3 front walls
        for l in src.lines().map(|l| l.trim_end()).filter(|l| !l.is_empty()) {
            if l.trim_start().starts_with('@') {
                match l.trim() {
                    "@decor" => section = 1,
                    "@walls" => section = 2,
                    "@front" => section = 3,
                    _ => {}
                }
                continue;
            }
            match section {
                1 => {
                    decor_src.push(l);
                    decor.push(l.chars().map(|c| if DecorKind::from_char(c).is_some() { c } else { '.' }).collect());
                }
                2 | 3 => {
                    let row = l.chars().map(|c| if WallKind::from_char(c).is_some() { c } else { '.' }).collect();
                    if section == 2 { walls.push(row) } else { front.push(row) }
                }
                _ => rows.push(l.chars().map(|c| if VALID.contains(&c) { c } else { '.' }).collect()),
            }
        }
        if walls.is_empty() {
            // Older levels kept walls and windows on the decor layer: move them over.
            walls = decor_src.iter().map(|l| l.chars().map(|c| if matches!(c, 'w' | 'n') { c } else { '.' }).collect()).collect();
        }
        if rows.is_empty() {
            return Grid::blank(level.theme);
        }
        let w = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(MIN_W);
        for layer in [&mut decor, &mut walls, &mut front] {
            layer.resize(rows.len(), Vec::new());
            layer.truncate(rows.len());
        }
        for r in rows.iter_mut().chain(decor.iter_mut()).chain(walls.iter_mut()).chain(front.iter_mut()) {
            r.resize(w, '.');
        }
        let pad = MIN_H.saturating_sub(rows.len());
        for _ in 0..pad {
            rows.insert(0, vec!['.'; w]);
            decor.insert(0, vec!['.'; w]);
            walls.insert(0, vec!['.'; w]);
            front.insert(0, vec!['.'; w]);
        }
        let gates = level.gates.iter().map(|g| ((g.x as usize, g.y as usize + pad), g.need)).collect();
        let signs = level.signs.iter().chain(&level.neons).map(|s| ((s.x as usize, s.y as usize + pad), s.text.clone())).collect();
        let rots = level.decor.iter().filter(|d| d.rot != 0).map(|d| ((d.x as usize, d.y as usize + pad), d.rot)).collect();
        Grid { rows, decor, walls, front, gates, signs, rots, theme: level.theme }
    }

    /// A fresh level: empty grid with a floor, a spawn and a goal.
    fn blank(theme: usize) -> Self {
        let mut rows = vec![vec!['.'; NEW_W]; NEW_H];
        rows[NEW_H - 1] = vec!['#'; NEW_W];
        rows[NEW_H - 2][2] = 'P';
        rows[NEW_H - 2][NEW_W - 3] = 'G';
        let decor = vec![vec!['.'; NEW_W]; NEW_H];
        let walls = decor.clone();
        let front = decor.clone();
        Grid { rows, decor, walls, front, gates: BTreeMap::new(), signs: BTreeMap::new(), rots: BTreeMap::new(), theme }
    }

    fn w(&self) -> usize {
        self.rows[0].len()
    }

    fn h(&self) -> usize {
        self.rows.len()
    }

    fn to_text(&self) -> String {
        let mut out = format!("@theme {}\n", THEMES[self.theme].name);
        for ((x, y), n) in &self.gates {
            out.push_str(&format!("@gate {x} {y} {n}\n"));
        }
        for ((x, y), text) in &self.signs {
            out.push_str(&format!("@sign {x} {y} {}\n", encode_sign_text(text)));
        }
        for ((x, y), n) in &self.rots {
            out.push_str(&format!("@rot {x} {y} {n}\n"));
        }
        for r in &self.rows {
            out.extend(r.iter());
            out.push('\n');
        }
        for (name, layer) in [("@decor", &self.decor), ("@walls", &self.walls), ("@front", &self.front)] {
            if layer.iter().flatten().any(|&c| c != '.') {
                out.push_str(name);
                out.push('\n');
                for r in layer {
                    out.extend(r.iter());
                    out.push('\n');
                }
            }
        }
        out
    }

    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w() && (y as usize) < self.h()
    }

    fn layer(&self, layer: Layer) -> &Vec<Vec<char>> {
        match layer {
            Layer::Tiles => &self.rows,
            Layer::Walls => &self.walls,
            Layer::Front => &self.front,
            Layer::Decor => &self.decor,
        }
    }

    fn layer_mut(&mut self, layer: Layer) -> &mut Vec<Vec<char>> {
        match layer {
            Layer::Tiles => &mut self.rows,
            Layer::Walls => &mut self.walls,
            Layer::Front => &mut self.front,
            Layer::Decor => &mut self.decor,
        }
    }

    fn find(&self, c: char) -> Option<(usize, usize)> {
        self.rows
            .iter()
            .enumerate()
            .find_map(|(y, r)| r.iter().position(|&k| k == c).map(|x| (x, y)))
    }

    fn count(&self, c: char) -> usize {
        self.rows.iter().flatten().filter(|&&k| k == c).count()
    }

    fn resize(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            for r in self.rows.iter_mut().chain(self.decor.iter_mut()).chain(self.walls.iter_mut()).chain(self.front.iter_mut()) {
                r.extend(std::iter::repeat('.').take(dw as usize));
            }
        } else if dw < 0 {
            let w = (self.w() as i32 + dw).max(MIN_W as i32) as usize;
            for r in self.rows.iter_mut().chain(self.decor.iter_mut()).chain(self.walls.iter_mut()).chain(self.front.iter_mut()) {
                r.truncate(w);
            }
            self.gates.retain(|&(x, _), _| x < w);
            self.rots.retain(|&(x, _), _| x < w);
            self.signs.retain(|&(x, _), _| x < w);
        }
        // Height changes happen at the top, so the floor stays put.
        // Gate and sign positions shift with the rows.
        let mut moved = 0i32;
        if dh > 0 {
            for _ in 0..dh {
                self.rows.insert(0, vec!['.'; self.w()]);
                self.decor.insert(0, vec!['.'; self.w()]);
                self.walls.insert(0, vec!['.'; self.w()]);
                self.front.insert(0, vec!['.'; self.w()]);
            }
            moved = dh;
        } else if dh < 0 {
            let remove = (-dh as usize).min(self.h() - MIN_H);
            self.rows.drain(0..remove);
            self.decor.drain(0..remove);
            self.walls.drain(0..remove);
            self.front.drain(0..remove);
            moved = -(remove as i32);
        }
        if moved != 0 {
            fn shift<V>(map: &mut BTreeMap<(usize, usize), V>, dy: i32) {
                *map = std::mem::take(map)
                    .into_iter()
                    .filter_map(|((x, y), v)| {
                        let ny = y as i32 + dy;
                        (ny >= 0).then_some(((x, ny as usize), v))
                    })
                    .collect();
            }
            shift(&mut self.gates, moved);
            shift(&mut self.signs, moved);
            shift(&mut self.rots, moved);
        }
    }
}

// ---------- Tiny immediate-mode UI helpers ----------

fn button(x: &mut f32, y: f32, label: &str, highlight: bool) -> bool {
    let s = ui();
    let w = measure_text(label, None, font() as u16, 1.0).width + 18.0 * s;
    let r = Rect::new(*x, y, w, 32.0 * s);
    *x += w + 6.0 * s;
    let (mx, my) = mouse_position();
    let hover = r.contains(vec2(mx, my));
    let base = if highlight { alpha(player(), 0.35) } else { alpha(WHITE, 0.06) };
    draw_rectangle(r.x, r.y, r.w, r.h, if hover { alpha(WHITE, 0.16) } else { base });
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, alpha(WHITE, if hover { 0.5 } else { 0.18 }));
    draw_text(label, r.x + 9.0 * s, r.y + 22.0 * s, font(), WHITE);
    let clicked = hover && is_mouse_button_pressed(MouseButton::Left);
    if clicked {
        audio::play(Sfx::Click);
    }
    clicked
}

fn label(x: &mut f32, y: f32, text: &str, color: Color) {
    draw_text(text, *x, y + 22.0 * ui(), font(), color);
    *x += measure_text(text, None, font() as u16, 1.0).width + 8.0 * ui();
}

fn separator(x: &mut f32, y: f32) {
    let s = ui();
    *x += 4.0 * s;
    draw_line(*x, y + 4.0 * s, *x, y + 28.0 * s, 1.0, alpha(WHITE, 0.2));
    *x += 10.0 * s;
}

/// Draws a tool's icon centred at `c` with roughly `s` pixels of size.
/// `flip` mirrors ramps (they slope down to the right).
fn draw_tool_icon(tool: Tool, c: Vec2, s: f32, a: f32, flip: bool, turn: u8) {
    let h = s / 2.0;
    let metal = mix(terrain(), deep(), 0.45);
    let leaf = Color::from_hex(0x46c84a);
    match tool {
        Tool::Ramp | Tool::GentleRamp => {
            let rise = if tool == Tool::Ramp { s } else { s * 0.5 };
            let d = if flip { -1.0 } else { 1.0 };
            let (bl, br) = (vec2(c.x - h * d, c.y + h), vec2(c.x + h * d, c.y + h));
            let tr = vec2(c.x + h * d, c.y + h - rise);
            draw_triangle(bl, br, tr, alpha(terrain(), a));
            draw_line(bl.x, bl.y, tr.x, tr.y, 3.0, alpha(lighten(terrain(), 0.35), a));
        }
        Tool::Slick => {
            // A block clad in polished glass along its edges.
            let glass = Color::from_hex(0x9fe4ff);
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(terrain(), a));
            for x in [c.x - h, c.x + h - s * 0.2] {
                draw_rectangle(x, c.y - h, s * 0.2, s, alpha(glass, a));
            }
            draw_rectangle(c.x - h, c.y - h, s, s * 0.16, alpha(lighten(glass, 0.4), a));
            for k in 0..3 {
                let y = c.y - h * 0.4 + k as f32 * h * 0.5;
                draw_line(c.x - h, y + s * 0.15, c.x - h + s * 0.2, y, 1.2, alpha(WHITE, 0.8 * a));
                draw_line(c.x + h - s * 0.2, y + s * 0.15, c.x + h, y, 1.2, alpha(WHITE, 0.8 * a));
            }
        }
        Tool::Block => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(lighten(terrain(), 0.35), a));
            draw_rectangle(c.x - h + 2.0, c.y - h + s * 0.3, s - 4.0, s * 0.7 - 2.0, alpha(terrain(), a));
            draw_line(c.x - h + 4.0, c.y - h + s * 0.45, c.x, c.y - h + s * 0.45, 2.0, alpha(WHITE, 0.35 * a));
        }
        Tool::Platform => {
            draw_rectangle(c.x - h, c.y - s * 0.15, s, s * 0.3, alpha(lighten(terrain(), 0.35), 0.7 * a));
            draw_rectangle_lines(c.x - h, c.y - s * 0.15, s, s * 0.3, 1.5, alpha(lighten(terrain(), 0.6), a));
        }
        Tool::Spikes => {
            for k in 0..6 {
                let ang = k as f32 / 6.0 * TAU;
                let d = vec2(ang.cos(), ang.sin());
                let p = vec2(-d.y, d.x) * s * 0.12;
                draw_triangle(c + d * s * 0.2 + p, c + d * s * 0.2 - p, c + d * s * 0.48, alpha(darken(hazard(), 0.1), a));
            }
            draw_circle(c.x, c.y, s * 0.26, alpha(hazard(), a));
        }
        Tool::Coin => {
            draw_circle(c.x, c.y, s * 0.42, alpha(pickup(), 0.18 * a));
            draw_circle(c.x, c.y, s * 0.28, alpha(pickup(), a));
            draw_circle(c.x - s * 0.08, c.y - s * 0.1, s * 0.07, alpha(WHITE, 0.8 * a));
        }
        Tool::DashOrb => {
            let pts = [c + vec2(0.0, -h * 0.9), c + vec2(h * 0.7, 0.0), c + vec2(0.0, h * 0.9), c + vec2(-h * 0.7, 0.0)];
            draw_triangle(pts[0], pts[1], pts[2], alpha(orb(), a));
            draw_triangle(pts[0], pts[2], pts[3], alpha(orb(), a));
            draw_circle(c.x, c.y, s * 0.12, alpha(WHITE, a));
        }
        Tool::Checkpoint => {
            draw_line(c.x, c.y + h, c.x, c.y - h * 0.2, 2.5, alpha(darken(player(), 0.3), a));
            draw_circle(c.x, c.y - h * 0.4, s * 0.26, alpha(player(), a));
        }
        Tool::Spawn => {
            draw_rectangle(c.x - s * 0.32, c.y - s * 0.4, s * 0.64, s * 0.8, alpha(player(), a));
            for dx in [-0.12, 0.12] {
                draw_circle(c.x + dx * s, c.y - s * 0.12, s * 0.07, alpha(WHITE, a));
            }
        }
        Tool::Bounce => {
            draw_rectangle(c.x - h, c.y - h * 0.3, s, s * 0.65, alpha(mix(terrain(), deep(), 0.3), a));
            draw_rectangle(c.x - h, c.y - h, s, s * 0.32, alpha(pickup(), a));
            draw_line(c.x - h * 0.4, c.y + h * 0.35, c.x, c.y + h * 0.05, 2.0, alpha(pickup(), 0.7 * a));
            draw_line(c.x, c.y + h * 0.05, c.x + h * 0.4, c.y + h * 0.35, 2.0, alpha(pickup(), 0.7 * a));
        }
        Tool::Crumble => {
            let fill = lighten(mix(terrain(), deep(), 0.15), 0.12);
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(fill, a));
            let d = darken(fill, 0.45);
            draw_line(c.x - h * 0.1, c.y - h * 0.1, c.x - h * 0.7, c.y - h * 0.7, 1.5, alpha(d, a));
            draw_line(c.x - h * 0.1, c.y - h * 0.1, c.x + h * 0.6, c.y - h * 0.3, 1.5, alpha(d, a));
            draw_line(c.x - h * 0.1, c.y - h * 0.1, c.x + h * 0.2, c.y + h * 0.7, 1.5, alpha(d, a));
        }
        Tool::Acid => {
            draw_rectangle(c.x - h, c.y - h * 0.2, s, s * 0.6, alpha(mix(hazard(), deep(), 0.25), 0.8 * a));
            // Wavy surface.
            let top = c.y - h * 0.2;
            for k in 0..4 {
                let xa = c.x - h + k as f32 * s / 4.0;
                let (ya, yb) = if k % 2 == 0 { (top, top - 2.0) } else { (top - 2.0, top) };
                draw_line(xa, ya, xa + s / 4.0, yb, 2.0, alpha(lighten(hazard(), 0.35), a));
            }
            draw_circle_lines(c.x - h * 0.3, c.y - h * 0.6, s * 0.08, 1.0, alpha(lighten(hazard(), 0.4), a));
        }
        Tool::Crawler => {
            let base = c + vec2(0.0, h * 0.5);
            let pts: Vec<Vec2> = (0..=12)
                .map(|i| {
                    let ang = std::f32::consts::PI + i as f32 / 12.0 * std::f32::consts::PI;
                    base + vec2(ang.cos() * h * 0.9, ang.sin() * h * 0.95)
                })
                .collect();
            for i in 0..pts.len() - 1 {
                draw_triangle(base, pts[i], pts[i + 1], alpha(hazard(), a));
            }
            for dx in [-0.25, 0.25] {
                draw_circle(c.x + dx * s, c.y, s * 0.08, alpha(WHITE, a));
            }
        }
        Tool::Floater => {
            for f in [-0.5, -0.15, 0.2, 0.55] {
                draw_line(c.x + f * h, c.y, c.x + f * h + 1.5, c.y + h * 0.9, 1.5, alpha(darken(hazard(), 0.15), a));
            }
            draw_circle(c.x, c.y - h * 0.15, s * 0.36, alpha(hazard(), a));
            draw_rectangle(c.x - s * 0.36, c.y - h * 0.15, s * 0.72, s * 0.14, alpha(hazard(), a));
            for dx in [-0.15, 0.15] {
                draw_circle(c.x + dx * s, c.y - h * 0.25, s * 0.07, alpha(WHITE, a));
            }
        }
        Tool::Bubble => {
            let col = lighten(terrain(), 0.6);
            draw_circle(c.x, c.y, s * 0.36, alpha(col, 0.18 * a));
            draw_circle_lines(c.x, c.y, s * 0.36, 1.5, alpha(col, a));
            draw_line(c.x - s * 0.12, c.y + s * 0.06, c.x, c.y - s * 0.07, 1.6, alpha(WHITE, a));
            draw_line(c.x, c.y - s * 0.07, c.x + s * 0.12, c.y + s * 0.06, 1.6, alpha(WHITE, a));
        }
        Tool::Goal => {
            draw_circle(c.x, c.y, s * 0.42, alpha(pickup(), 0.45 * a));
            draw_circle_lines(c.x, c.y, s * 0.42, 2.5, alpha(hazard(), a));
        }
        Tool::Gate => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(mix(deep(), pickup(), 0.1), a));
            for k in [-0.55, 0.0, 0.55] {
                draw_rectangle(c.x + k * h - s * 0.06, c.y - h, s * 0.12, s, alpha(mix(pickup(), terrain(), 0.45), a));
            }
            draw_circle(c.x, c.y, s * 0.2, alpha(deep(), a));
            draw_circle_lines(c.x, c.y, s * 0.2, 1.5, alpha(pickup(), a));
        }
        Tool::LowWall => {
            draw_rectangle(c.x - h, c.y, s, h, alpha(mix(terrain(), deep(), 0.15), a));
            draw_rectangle(c.x - h, c.y, s, s * 0.08, alpha(lighten(terrain(), 0.4), a));
        }
        Tool::Pillar => {
            draw_rectangle(c.x - s * 0.18, c.y - h, s * 0.36, s, alpha(mix(terrain(), deep(), 0.15), a));
        }
        Tool::GlassWall => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(Color::from_hex(0x7cc4f0), 0.7 * a));
            draw_line(c.x, c.y - h, c.x, c.y + h, 1.5, alpha(metal, a));
            draw_line(c.x - h, c.y, c.x + h, c.y, 1.0, alpha(metal, a));
            draw_rectangle_lines(c.x - h, c.y - h, s, s, 1.5, alpha(metal, a));
        }
        Tool::Shutter => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(mix(terrain(), deep(), 0.35), a));
            for k in 1..6 {
                let y = c.y - h + s * k as f32 / 6.0;
                draw_line(c.x - h, y, c.x + h, y, 1.0, alpha(deep(), 0.5 * a));
            }
        }
        Tool::AccentWall => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(Color::from_hex(0xff6a1a), a));
            draw_rectangle(c.x - h, c.y - s * 0.05, s, s * 0.1, alpha(WHITE, 0.8 * a));
        }
        Tool::Screen => {
            draw_rectangle(c.x - h, c.y - h * 0.7, s, s * 0.7, alpha(deep(), a));
            draw_rectangle(c.x - h * 0.85, c.y - h * 0.55, s * 0.85, s * 0.55, alpha(Color::from_hex(0xff3ca8), a));
            draw_circle(c.x + h * 0.4, c.y - h * 0.1, s * 0.14, alpha(Color::from_hex(0xffe14d), a));
            draw_line(c.x - h * 0.6, c.y - h * 0.3, c.x, c.y - h * 0.3, 2.0, alpha(WHITE, a));
        }
        Tool::Neon => {
            let col = Color::from_hex(0xff3ca8);
            draw_circle(c.x, c.y, s * 0.45, alpha(col, 0.2 * a));
            let t = "N";
            let size = s * 1.1;
            let d = measure_text(t, None, size as u16, 1.0);
            draw_text(t, c.x - d.width / 2.0, c.y + d.offset_y / 2.0, size, alpha(lighten(col, 0.5), a));
        }
        Tool::Grass => {
            draw_rectangle(c.x - h, c.y + h * 0.4, s, h * 0.6, alpha(darken(leaf, 0.2), a));
            for k in 0..6 {
                let x = c.x - h + (k as f32 + 0.5) * s / 6.0;
                draw_triangle(vec2(x - 2.0, c.y + h * 0.45), vec2(x + 2.0, c.y + h * 0.45), vec2(x, c.y - h * 0.2), alpha(leaf, a));
            }
        }
        Tool::Railing => {
            let col = alpha(lighten(metal, 0.3), a);
            draw_line(c.x - h, c.y - h * 0.4, c.x + h, c.y - h * 0.4, 2.0, col);
            draw_line(c.x - h, c.y + h * 0.2, c.x + h, c.y + h * 0.2, 1.5, col);
            for x in [c.x - h * 0.8, c.x, c.x + h * 0.8] {
                draw_line(x, c.y - h * 0.4, x, c.y + h, 2.0, col);
            }
        }
        Tool::Ladder => {
            let col = alpha(Color::from_hex(0xe8232b), a);
            for x in [c.x - h * 0.5, c.x + h * 0.5] {
                draw_line(x, c.y - h, x, c.y + h, 2.0, col);
            }
            for k in 0..4 {
                let y = c.y - h * 0.7 + k as f32 * h * 0.5;
                draw_line(c.x - h * 0.5, y, c.x + h * 0.5, y, 1.5, col);
            }
        }
        Tool::Floodlight => {
            draw_triangle(vec2(c.x + h * 0.2, c.y - h * 0.5), vec2(c.x + h, c.y - h), vec2(c.x + h, c.y + h * 0.1), alpha(Color::from_hex(0xfff3c4), 0.5 * a));
            draw_line(c.x - h * 0.3, c.y + h, c.x - h * 0.3, c.y - h * 0.4, 2.0, alpha(metal, a));
            draw_rectangle(c.x - h * 0.5, c.y - h * 0.7, s * 0.4, s * 0.25, alpha(metal, a));
        }
        Tool::Skylight => {
            draw_rectangle(c.x - h, c.y + h * 0.4, s, h * 0.6, alpha(lighten(terrain(), 0.2), a));
            draw_triangle(vec2(c.x - h * 0.9, c.y + h * 0.4), vec2(c.x + h * 0.9, c.y + h * 0.4), vec2(c.x, c.y - h * 0.5), alpha(Color::from_hex(0x7cc4f0), 0.8 * a));
        }
        Tool::Crates => {
            draw_rectangle(c.x - h, c.y - h * 0.2, s * 0.6, s * 0.6, alpha(Color::from_hex(0xff6a1a), a));
            draw_rectangle(c.x + h * 0.25, c.y + h * 0.3, s * 0.35, s * 0.35, alpha(Color::from_hex(0x1f9fe8), a));
            draw_rectangle_lines(c.x - h, c.y - h * 0.2, s * 0.6, s * 0.6, 1.0, alpha(deep(), 0.4 * a));
        }
        Tool::Wall => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(mix(terrain(), deep(), 0.15), a));
            draw_line(c.x, c.y - h, c.x, c.y + h, 1.0, alpha(deep(), 0.3 * a));
            draw_rectangle(c.x - h, c.y + h - s * 0.14, s, s * 0.14, alpha(mix(terrain(), deep(), 0.4), a));
        }
        Tool::Window => {
            draw_rectangle(c.x - h, c.y - h, s, s, alpha(mix(terrain(), deep(), 0.15), a));
            draw_rectangle(c.x - h * 0.7, c.y - h * 0.7, s * 0.7, s * 0.6, alpha(metal, a));
            draw_rectangle(c.x - h * 0.6, c.y - h * 0.6, s * 0.6, s * 0.5, alpha(Color::from_hex(0x7cc4f0), a));
            draw_line(c.x, c.y - h * 0.6, c.x, c.y + h * 0.4, 1.5, alpha(metal, a));
        }
        Tool::Cilia => {
            // Potted plant.
            for k in 0..5 {
                let ang = -std::f32::consts::FRAC_PI_2 + (k as f32 / 4.0 - 0.5) * 1.4;
                let root = vec2(c.x, c.y + h * 0.35);
                let tip = root + Vec2::from_angle(ang) * s * 0.55;
                draw_line(root.x, root.y, tip.x, tip.y, 2.5, alpha(leaf, a));
            }
            draw_rectangle(c.x - h * 0.5, c.y + h * 0.35, s * 0.5, h * 0.6, alpha(lighten(terrain(), 0.3), a));
        }
        Tool::Tendril => {
            // Hanging vines.
            for kx in [-0.3, 0.3] {
                let x = c.x + kx * s;
                draw_line(x, c.y - h, x, c.y + h * 0.7, 1.5, alpha(darken(leaf, 0.2), a));
                for ky in [-0.4, 0.1, 0.6] {
                    draw_circle(x + if ky < 0.0 { 2.5 } else { -2.5 }, c.y + ky * h, 2.2, alpha(leaf, a));
                }
            }
        }
        Tool::Eye => {
            // Security camera on a mount, turned to its mounting surface.
            let ang = turn as f32 * std::f32::consts::FRAC_PI_2;
            let r = |dx: f32, dy: f32| c + Vec2::from_angle(ang).rotate(vec2(dx, dy) * h);
            let (m0, m1) = (r(-0.6, -0.8), r(-0.6, -0.2));
            draw_line(m0.x, m0.y, m1.x, m1.y, 2.5, alpha(metal, a));
            let body = [r(-0.8, -0.35), r(0.7, -0.1), r(0.7, 0.4), r(-0.8, 0.3)];
            draw_triangle(body[0], body[1], body[2], alpha(lighten(terrain(), 0.3), a));
            draw_triangle(body[0], body[2], body[3], alpha(lighten(terrain(), 0.3), a));
            let lens = r(0.75, 0.15);
            draw_circle(lens.x, lens.y, s * 0.1, alpha(deep(), a));
            let led = r(-0.4, 0.0);
            draw_circle(led.x, led.y, s * 0.06, alpha(hazard(), a));
        }
        Tool::Half => {
            let col = alpha(lighten(terrain(), 0.35), a);
            match turn % 4 {
                0 => draw_rectangle(c.x - h, c.y, s, h, col),
                1 => draw_rectangle(c.x - h, c.y - h, h, s, col),
                2 => draw_rectangle(c.x - h, c.y - h, s, h, col),
                _ => draw_rectangle(c.x, c.y - h, h, s, col),
            }
            draw_rectangle_lines(c.x - h, c.y - h, s, s, 1.0, alpha(WHITE, 0.25 * a));
        }
        Tool::Cysts => {
            // Rooftop AC unit.
            draw_rectangle(c.x - h, c.y - h * 0.6, s, s * 0.8, alpha(lighten(terrain(), 0.25), a));
            for k in 0..3 {
                let y = c.y - h * 0.3 + k as f32 * h * 0.3;
                draw_line(c.x - h * 0.85, y, c.x - h * 0.2, y, 1.0, alpha(metal, a));
            }
            draw_circle(c.x + h * 0.45, c.y - h * 0.2 + h * 0.2, s * 0.22, alpha(metal, a));
            draw_line(c.x + h * 0.45 - s * 0.18, c.y, c.x + h * 0.45 + s * 0.18, c.y, 1.5, alpha(lighten(terrain(), 0.3), a));
        }
        Tool::Scar => {
            // Pipe with a flange, across or up and down.
            if turn % 2 == 0 {
                draw_rectangle(c.x - h, c.y - s * 0.12, s, s * 0.24, alpha(lighten(terrain(), 0.2), a));
                draw_rectangle(c.x - s * 0.06, c.y - s * 0.2, s * 0.12, s * 0.4, alpha(metal, a));
            } else {
                draw_rectangle(c.x - s * 0.12, c.y - h, s * 0.24, s, alpha(lighten(terrain(), 0.2), a));
                draw_rectangle(c.x - s * 0.2, c.y - s * 0.06, s * 0.4, s * 0.12, alpha(metal, a));
            }
        }
        Tool::Veins => {
            // Antenna mast with a dish and a red light.
            draw_line(c.x, c.y + h, c.x, c.y - h * 0.8, 2.0, alpha(metal, a));
            for k in 0..2 {
                let y = c.y + h * 0.4 - k as f32 * h * 0.5;
                let w = h * (0.5 - k as f32 * 0.15);
                draw_line(c.x - w, y, c.x + w, y, 1.5, alpha(metal, a));
            }
            draw_ellipse(c.x + h * 0.35, c.y - h * 0.1, h * 0.2, h * 0.35, 0.0, alpha(lighten(terrain(), 0.3), a));
            draw_circle(c.x, c.y - h * 0.85, s * 0.08, alpha(hazard(), a));
        }
        Tool::Sign => {
            // Holo sign on a post.
            draw_line(c.x, c.y, c.x, c.y + h, 2.5, alpha(metal, a));
            draw_rectangle(c.x - h * 0.85, c.y - h * 0.8, s * 0.85, s * 0.45, alpha(deep(), 0.9 * a));
            for k in 0..2 {
                let y = c.y - h * 0.5 + k as f32 * h * 0.3;
                draw_line(c.x - h * 0.6, y, c.x + h * (0.5 - k as f32 * 0.4), y, 1.5, alpha(player(), a));
            }
        }
        Tool::Eraser => {
            draw_rectangle_lines(c.x - h * 0.8, c.y - h * 0.8, s * 0.8, s * 0.8, 2.0, alpha(hazard(), a));
            draw_line(c.x - h * 0.6, c.y - h * 0.6, c.x + h * 0.6, c.y + h * 0.6, 2.0, alpha(hazard(), a));
            draw_line(c.x + h * 0.6, c.y - h * 0.6, c.x - h * 0.6, c.y + h * 0.6, 2.0, alpha(hazard(), a));
        }
    }
}

// ---------- Editor ----------

/// A sign whose text is being typed; `before` is the grid when editing began (for undo).
struct SignEdit {
    cell: (usize, usize),
    before: Grid,
}

struct Stroke {
    button: MouseButton,
    erase: bool,
    last: (i32, i32),
    changed: bool,
}

pub struct Editor {
    files: Vec<PathBuf>,
    current: usize,
    grid: Grid,
    layer: Layer,
    tool: Tool,
    /// The selected tool on the layer that isn't active.
    /// The tool last used on each layer, restored when switching back.
    layer_tool: [Tool; 4],
    gate_need: u32,
    /// Ramps go down to the right instead of up.
    ramp_flip: bool,
    /// Quarter turns for half blocks, pipes and cameras.
    rotation: u8,
    sign_edit: Option<SignEdit>,
    cam: Vec2,
    zoom: f32,
    preview: Game,
    preview_dirty: bool,
    save_in: Option<f32>,
    undo: Vec<Grid>,
    redo: Vec<Grid>,
    stroke: Option<Stroke>,
    palette_drag: Option<(Tool, Vec2)>,
    pan: Option<(Vec2, Vec2)>,
    toast: Option<(String, f32)>,
    delete_armed: f32,
    playtest: Option<Session>,
    traces: Traces,
}

impl Editor {
    pub fn new() -> Self {
        let files = levels::list();
        let grid = Grid::from_text(&levels::load(&files[0]));
        let preview = Game::new(Level::parse(&grid.to_text()));
        let mut ed = Editor {
            files,
            current: 0,
            grid,
            layer: Layer::Tiles,
            tool: Tool::Block,
            layer_tool: [Tool::Block, Tool::Wall, Tool::Pillar, Tool::Cilia],
            gate_need: DEFAULT_GATE_NEED,
            ramp_flip: false,
            rotation: 0,
            sign_edit: None,
            cam: Vec2::ZERO,
            zoom: 1.0,
            preview,
            preview_dirty: true,
            save_in: None,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke: None,
            palette_drag: None,
            pan: None,
            toast: None,
            delete_armed: 0.0,
            playtest: None,
            traces: Traces::new(),
        };
        ed.frame_level();
        ed
    }

    fn path(&self) -> &PathBuf {
        &self.files[self.current]
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), 2.2));
    }

    // ----- Level files -----

    fn load_current(&mut self) {
        self.sign_edit = None;
        self.grid = Grid::from_text(&levels::load(self.path()));
        self.undo.clear();
        self.redo.clear();
        self.stroke = None;
        self.preview_dirty = true;
        self.traces.clear();
        self.frame_level();
    }

    fn save_now(&mut self) {
        match levels::save(self.path(), &self.grid.to_text()) {
            Ok(()) => self.save_in = None,
            Err(e) => self.notify(format!("Save failed: {e}")),
        }
    }

    /// Write any pending autosave immediately.
    fn flush(&mut self) {
        if self.save_in.is_some() {
            self.save_now();
        }
    }

    fn select_file(&mut self, path: &PathBuf) {
        self.files = levels::list();
        self.current = self.files.iter().position(|p| p == path).unwrap_or(0);
        self.load_current();
    }

    fn switch_level(&mut self, delta: i32) {
        self.flush();
        self.files = levels::list();
        let n = self.files.len() as i32;
        self.current = (self.current as i32 + delta).rem_euclid(n) as usize;
        self.load_current();
        self.notify(format!("Editing {}", levels::display_name(self.path())));
    }

    fn new_level(&mut self, duplicate: bool) {
        self.flush();
        let path = levels::next_free_path();
        let grid = if duplicate { self.grid.clone() } else { Grid::blank(self.grid.theme) };
        if let Err(e) = levels::save(&path, &grid.to_text()) {
            self.notify(format!("Could not create level: {e}"));
            return;
        }
        self.select_file(&path);
        let verb = if duplicate { "Duplicated to" } else { "Created" };
        self.notify(format!("{verb} {}", levels::display_name(&path)));
    }

    fn delete_level(&mut self) {
        if self.files.len() <= 1 {
            self.notify("Can't delete the only level");
            return;
        }
        if self.delete_armed <= 0.0 {
            self.delete_armed = 2.5;
            self.notify("Click Delete again to confirm");
            return;
        }
        self.delete_armed = 0.0;
        let name = levels::display_name(self.path());
        if let Err(e) = levels::delete(self.path()) {
            self.notify(format!("Delete failed: {e}"));
            return;
        }
        self.save_in = None;
        self.files = levels::list();
        self.current = self.current.min(self.files.len() - 1);
        self.load_current();
        self.notify(format!("Deleted {name}"));
    }

    // ----- Editing -----

    fn changed(&mut self) {
        self.preview_dirty = true;
        self.save_in = Some(AUTOSAVE_DELAY);
    }

    fn push_undo(&mut self) {
        self.undo.push(self.grid.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(g) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.grid, g));
            self.changed();
            audio::play(Sfx::Undo);
            self.notify("Undo");
        }
    }

    fn redo(&mut self) {
        if let Some(g) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.grid, g));
            self.changed();
            audio::play(Sfx::Undo);
            self.notify("Redo");
        }
    }

    /// Put the current tool (or empty) in one cell of the active layer. Returns whether anything changed.
    fn apply(&mut self, cell: (i32, i32), erase: bool) -> bool {
        if !self.grid.in_bounds(cell.0, cell.1) {
            return false;
        }
        let (x, y) = (cell.0 as usize, cell.1 as usize);
        let tool = if erase { Tool::Eraser } else { self.tool };
        if matches!(tool, Tool::Ramp | Tool::GentleRamp) {
            return self.apply_ramp(cell, tool);
        }
        let c = if tool == Tool::Half { Half::ALL[self.rotation as usize % 4].0 } else { tool.ch() };
        let layer = self.layer;
        let old = self.grid.layer(layer)[y][x];
        let same_gate = tool == Tool::Gate && self.grid.gates.get(&(x, y)) == Some(&self.gate_need);
        let same_turn = !(layer == Layer::Decor && tool.rotates()) || self.grid.rots.get(&(x, y)).copied().unwrap_or(0) == self.rotation;
        if old == c && (tool != Tool::Gate || same_gate) && same_turn {
            return false;
        }
        if tool.unique() {
            if let Some((ox, oy)) = self.grid.find(c) {
                self.grid.rows[oy][ox] = '.';
            }
        }
        self.grid.layer_mut(layer)[y][x] = c;
        match layer {
            Layer::Tiles if tool == Tool::Gate => {
                self.grid.gates.insert((x, y), self.gate_need);
            }
            Layer::Tiles => {
                self.grid.gates.remove(&(x, y));
            }
            Layer::Decor if tool.rotates() && self.rotation != 0 => {
                self.grid.rots.insert((x, y), self.rotation);
                self.grid.signs.remove(&(x, y));
            }
            Layer::Decor if tool.has_text() => {
                self.grid.rots.remove(&(x, y));
                self.grid.signs.entry((x, y)).or_default();
            }
            Layer::Decor => {
                self.grid.signs.remove(&(x, y));
                self.grid.rots.remove(&(x, y));
            }
            Layer::Walls | Layer::Front => {}
        }
        audio::play_vol(if erase { Sfx::Erase } else { Sfx::Paint }, 0.7);
        true
    }

    /// Level characters for the selected ramp, left to right (gentle ramps take two cells).
    fn ramp_chars(&self, tool: Tool) -> &'static [char] {
        match (tool, self.ramp_flip) {
            (Tool::Ramp, false) => &['/'],
            (Tool::Ramp, true) => &['\\'],
            (_, false) => &['1', '2'],
            (_, true) => &['3', '4'],
        }
    }

    fn apply_ramp(&mut self, cell: (i32, i32), tool: Tool) -> bool {
        let mut changed = false;
        for (i, &c) in self.ramp_chars(tool).iter().enumerate() {
            let (x, y) = (cell.0 + i as i32, cell.1);
            if !self.grid.in_bounds(x, y) {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            if self.grid.rows[y][x] != c {
                self.grid.rows[y][x] = c;
                self.grid.gates.remove(&(x, y));
                changed = true;
            }
        }
        if changed {
            audio::play_vol(Sfx::Paint, 0.7);
        }
        changed
    }

    fn apply_line(&mut self, from: (i32, i32), to: (i32, i32), erase: bool) -> bool {
        // Signs and gentle ramps (two cells wide) are placed one click at a time.
        if (self.tool.has_text() || self.tool == Tool::GentleRamp) && !erase {
            return false;
        }
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).max(1);
        let mut changed = false;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = (from.0 as f32 + (to.0 - from.0) as f32 * t).round() as i32;
            let y = (from.1 as f32 + (to.1 - from.1) as f32 * t).round() as i32;
            changed |= self.apply((x, y), erase);
        }
        changed
    }

    fn set_layer(&mut self, layer: Layer) {
        if layer != self.layer {
            self.finish_sign_edit();
            self.layer_tool[self.layer.index()] = self.tool;
            self.layer = layer;
            self.tool = self.layer_tool[layer.index()];
            audio::play(Sfx::Click);
        }
    }

    fn select_tool(&mut self, tool: Tool) {
        self.tool = tool;
        audio::play(Sfx::Click);
    }

    fn adjust_gate_need(&mut self, delta: i32) {
        self.gate_need = (self.gate_need as i32 + delta).clamp(0, 999) as u32;
        audio::play(Sfx::Click);
    }

    // ----- Signs -----

    /// Start typing into the sign at `cell`, if there is one.
    fn begin_sign_edit(&mut self, cell: (i32, i32)) {
        if !self.grid.in_bounds(cell.0, cell.1) {
            return;
        }
        let key = (cell.0 as usize, cell.1 as usize);
        if self.grid.signs.contains_key(&key) {
            self.finish_sign_edit();
            while get_char_pressed().is_some() {} // drop keys typed before editing began
            self.sign_edit = Some(SignEdit { cell: key, before: self.grid.clone() });
        }
    }

    fn finish_sign_edit(&mut self) {
        if let Some(edit) = self.sign_edit.take() {
            if edit.before != self.grid {
                self.undo.push(edit.before);
                self.redo.clear();
            }
        }
    }

    /// Typing into a sign. Returns true while editing (other shortcuts are off).
    fn update_sign_edit(&mut self) -> bool {
        let Some(edit) = &self.sign_edit else { return false };
        let cell = edit.cell;
        if !self.grid.signs.contains_key(&cell) {
            self.sign_edit = None;
            return false;
        }
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let mut done = is_key_pressed(KeyCode::Escape);
        let mut changed = false;
        let text = self.grid.signs.get_mut(&cell).unwrap();
        while let Some(c) = get_char_pressed() {
            if !c.is_control() && text.chars().count() < SIGN_MAX_LEN {
                text.push(c);
                changed = true;
            }
        }
        if is_key_pressed(KeyCode::Backspace) && text.pop().is_some() {
            changed = true;
        }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
            if shift {
                text.push('\n');
                changed = true;
            } else {
                done = true;
            }
        }
        if changed {
            self.changed();
        }
        if done {
            self.finish_sign_edit();
            audio::play(Sfx::Click);
        }
        true
    }

    pub fn typing(&self) -> bool {
        self.sign_edit.is_some() && self.playtest.is_none()
    }

    fn resize(&mut self, dw: i32, dh: i32) {
        let before = self.grid.clone();
        self.grid.resize(dw, dh);
        if self.grid != before {
            let before_h = before.h();
            self.undo.push(before);
            self.redo.clear();
            // Rows were added/removed at the top: keep the view on the same content.
            if dh != 0 {
                self.cam.y += (self.grid.h() as f32 - before_h as f32) * TILE;
            }
            self.changed();
        }
    }

    fn set_theme_index(&mut self, delta: i32) {
        self.push_undo();
        let n = THEMES.len() as i32;
        self.grid.theme = (self.grid.theme as i32 + delta).rem_euclid(n) as usize;
        self.changed();
    }

    // ----- View -----

    fn frame_level(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        let view_h = sh - top_h() - bottom_h();
        let lh = self.grid.h() as f32 * TILE;
        self.zoom = (view_h * 0.92 / lh).clamp(0.25, 2.0);
        // Left edge of the level just right of the palette, vertically centred in the view area.
        self.cam.x = (sw / 2.0 - left_w() - 24.0 * ui()) / self.zoom;
        let view_center_y = top_h() + view_h / 2.0;
        self.cam.y = lh / 2.0 - (view_center_y - sh / 2.0) / self.zoom;
    }

    fn to_world(&self, screen: Vec2) -> Vec2 {
        (screen - vec2(screen_width(), screen_height()) / 2.0) / self.zoom + self.cam
    }

    fn to_screen(&self, world: Vec2) -> Vec2 {
        (world - self.cam) * self.zoom + vec2(screen_width(), screen_height()) / 2.0
    }

    fn cell_at(&self, screen: Vec2) -> (i32, i32) {
        let w = self.to_world(screen);
        ((w.x / TILE).floor() as i32, (w.y / TILE).floor() as i32)
    }

    fn over_ui(&self, p: Vec2) -> bool {
        let (sw, sh) = (screen_width(), screen_height());
        p.y < top_h() || p.y > sh - bottom_h() || p.x < left_w() || p.x > sw
    }

    fn palette_rect(i: usize) -> Rect {
        let (col, row) = (i % 3, i / 3);
        let s = ui();
        Rect::new(
            (8.0 + col as f32 * 70.0) * s,
            top_h() + (48.0 + row as f32 * 56.0) * s,
            66.0 * s,
            52.0 * s,
        )
    }

    fn layer_tab_rect(layer: Layer) -> Rect {
        let s = ui();
        let i = layer.index() as f32;
        Rect::new((8.0 + i * 52.5) * s, top_h() + 8.0 * s, 49.5 * s, 32.0 * s)
    }

    /// The -/+ buttons for the gate's coin count, shown under the palette.
    /// Whether the current tool can be flipped or rotated.
    fn tool_has_options(&self) -> bool {
        matches!(self.tool, Tool::Ramp | Tool::GentleRamp) || self.tool.rotates()
    }

    /// Flip a ramp, or turn a rotating tool a quarter turn.
    fn turn_tool(&mut self) {
        if self.tool.rotates() {
            self.rotation = (self.rotation + 1) % 4;
        } else {
            self.ramp_flip = !self.ramp_flip;
        }
        audio::play(Sfx::Click);
    }

    /// The ramp "Flip" / "Rotate" button under the palette.
    fn flip_button_rect(&self) -> Rect {
        let s = ui();
        let n = self.layer.tools().len();
        let y = Self::palette_rect(n - 1).bottom() + 18.0 * s;
        Rect::new(12.0 * s, y, 196.0 * s, 28.0 * s)
    }

    fn gate_button_rects(&self) -> (Rect, Rect) {
        let s = ui();
        let n = self.layer.tools().len();
        let y = Self::palette_rect(n - 1).bottom() + 30.0 * s;
        (Rect::new(120.0 * s, y, 30.0 * s, 28.0 * s), Rect::new(184.0 * s, y, 30.0 * s, 28.0 * s))
    }

    // ----- Playtest -----

    fn start_playtest(&mut self, from_cursor: bool) {
        self.finish_sign_edit();
        self.flush();
        let spawn = if from_cursor {
            let (cx, cy) = self.cell_at(mouse_position().into());
            if !self.grid.in_bounds(cx, cy) {
                self.notify("Move the cursor over the level to play from there");
                return;
            }
            Some(vec2((cx as f32 + 0.5) * TILE, (cy as f32 + 1.0) * TILE))
        } else {
            if self.grid.find('P').is_none() {
                self.notify("Place a Spawn first (or press F6 to play from the cursor)");
                return;
            }
            None
        };
        self.playtest = Some(Session::new(self.grid.to_text(), spawn, false));
        audio::play(Sfx::Respawn);
        self.traces.begin();
    }

    fn toggle_traces(&mut self) {
        self.traces.visible = !self.traces.visible;
        self.notify(if self.traces.visible { "Traces shown" } else { "Traces hidden" });
    }

    fn clear_traces(&mut self) {
        self.traces.clear();
        self.notify("Traces cleared");
    }

    // ----- Frame -----

    pub fn frame(&mut self, input: &mut Input, dt: f32) {
        // Playtest runs the real game until the player returns.
        if let Some(session) = &mut self.playtest {
            let exit = is_key_pressed(KeyCode::Escape)
                || is_key_pressed(KeyCode::F5)
                || is_key_pressed(KeyCode::Tab)
                || input.state.select_pressed;
            let completed = !exit && session.update(input, dt);
            self.traces.record(&session.game);
            if !exit && !completed {
                session.draw(input, dt, "return to the editor");
                draw_playtest_banner();
                return;
            }
            self.playtest = None;
            input.consume_presses();
            audio::play(Sfx::Click);
            self.notify(if completed { "Level cleared! Back to editing" } else { "Back to editing" });
            // Skip editor input this frame so the exit key (Tab, Esc...) doesn't also act here.
            while get_char_pressed().is_some() {}
            self.draw();
            return;
        }

        self.update(dt);
        self.draw();
    }

    fn shortcuts(&mut self, ctrl: bool, shift: bool) {
        if ctrl && is_key_pressed(KeyCode::Z) {
            if shift {
                self.redo();
            } else {
                self.undo();
            }
        }
        if ctrl && is_key_pressed(KeyCode::Y) {
            self.redo();
        }
        if ctrl && is_key_pressed(KeyCode::S) {
            self.save_now();
            self.notify(format!("Saved {}", levels::display_name(self.path())));
        }
        if is_key_pressed(KeyCode::F5) {
            self.start_playtest(false);
        }
        if is_key_pressed(KeyCode::F6) {
            self.start_playtest(true);
        }
        if is_key_pressed(KeyCode::PageUp) {
            self.switch_level(-1);
        }
        if is_key_pressed(KeyCode::PageDown) {
            self.switch_level(1);
        }
        if is_key_pressed(KeyCode::F) {
            self.frame_level();
        }
        if is_key_pressed(KeyCode::L) {
            if shift {
                self.clear_traces();
            } else {
                self.toggle_traces();
            }
        }
        if is_key_pressed(KeyCode::T) {
            self.set_theme_index(if shift { -1 } else { 1 });
        }
        const DIGITS: [KeyCode; 9] = [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
            KeyCode::Key8,
            KeyCode::Key9,
        ];
        for (i, k) in DIGITS.iter().enumerate() {
            if let (true, Some(&tool)) = (is_key_pressed(*k), self.layer.tools().get(i)) {
                self.select_tool(tool);
            }
        }
        if is_key_pressed(KeyCode::Tab) {
            self.set_layer(Layer::ALL[(self.layer.index() + 1) % Layer::ALL.len()]);
        }
        if is_key_pressed(KeyCode::R) && self.tool_has_options() {
            self.turn_tool();
        }
        if self.tool == Tool::Gate {
            if is_key_pressed(KeyCode::LeftBracket) {
                self.adjust_gate_need(if shift { -5 } else { -1 });
            }
            if is_key_pressed(KeyCode::RightBracket) {
                self.adjust_gate_need(if shift { 5 } else { 1 });
            }
        }
    }

    fn update(&mut self, dt: f32) {
        // Timers.
        if let Some((_, t)) = &mut self.toast {
            *t -= dt;
            if *t <= 0.0 {
                self.toast = None;
            }
        }
        self.delete_armed = (self.delete_armed - dt).max(0.0);
        if let Some(t) = &mut self.save_in {
            *t -= dt;
            if *t <= 0.0 {
                self.save_now();
            }
        }

        let mouse: Vec2 = mouse_position().into();
        let over_ui = self.over_ui(mouse);
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

        let typing = self.update_sign_edit();
        if !typing {
            while get_char_pressed().is_some() {} // typed characters only matter for signs
            self.shortcuts(ctrl, shift);
        }

        // ---- Camera ----
        if !ctrl && !typing {
            let mut pan = Vec2::ZERO;
            if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
                pan.x -= 1.0;
            }
            if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
                pan.x += 1.0;
            }
            if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
                pan.y -= 1.0;
            }
            if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
                pan.y += 1.0;
            }
            self.cam += pan * 700.0 / self.zoom * dt;
        }
        let (_, wheel) = mouse_wheel();
        if wheel != 0.0 && !over_ui {
            let before = self.to_world(mouse);
            self.zoom = (self.zoom * 1.15f32.powf(wheel.signum())).clamp(0.2, 3.0);
            self.cam += before - self.to_world(mouse);
        }
        let space = is_key_down(KeyCode::Space);
        if !over_ui
            && (is_mouse_button_pressed(MouseButton::Middle) || (space && is_mouse_button_pressed(MouseButton::Left)))
        {
            self.pan = Some((mouse, self.cam));
        }
        if let Some((start, cam)) = self.pan {
            if is_mouse_button_down(MouseButton::Middle) || is_mouse_button_down(MouseButton::Left) {
                self.cam = cam - (mouse - start) / self.zoom;
            } else {
                self.pan = None;
            }
        }

        // ---- Palette: layer tabs, gate count, tools (click or drag) ----
        if is_mouse_button_pressed(MouseButton::Left) {
            if let Some(i) = (0..self.layer.tools().len()).find(|&i| Self::palette_rect(i).contains(mouse)) {
                self.select_tool(self.layer.tools()[i]);
                self.palette_drag = Some((self.tool, mouse));
            }
            for layer in Layer::ALL {
                if Self::layer_tab_rect(layer).contains(mouse) {
                    self.set_layer(layer);
                }
            }
            if self.tool_has_options() && self.flip_button_rect().contains(mouse) {
                self.turn_tool();
            }
            if self.tool == Tool::Gate {
                let (minus, plus) = self.gate_button_rects();
                let step = if shift { 5 } else { 1 };
                if minus.contains(mouse) {
                    self.adjust_gate_need(-step);
                }
                if plus.contains(mouse) {
                    self.adjust_gate_need(step);
                }
            }
            // Clicking anywhere else ends sign editing (a Sign-tool click on the map starts the next one).
            let sign_click = self.tool.has_text() && !over_ui;
            if self.sign_edit.is_some() && !sign_click {
                self.finish_sign_edit();
            }
        }
        if let Some((tool, start)) = self.palette_drag {
            if !is_mouse_button_down(MouseButton::Left) {
                self.palette_drag = None;
                if mouse.distance(start) > 6.0 && !over_ui {
                    let cell = self.cell_at(mouse);
                    self.push_undo();
                    self.tool = tool;
                    if self.apply(cell, false) {
                        self.changed();
                    } else {
                        self.undo.pop();
                    }
                    if tool.has_text() {
                        self.begin_sign_edit(cell);
                    }
                }
            }
        }

        // ---- Paint / erase strokes ----
        if self.pan.is_none() && self.palette_drag.is_none() && self.stroke.is_none() && !over_ui {
            let start = if is_mouse_button_pressed(MouseButton::Left) && !space {
                Some((MouseButton::Left, self.tool == Tool::Eraser))
            } else if is_mouse_button_pressed(MouseButton::Right) {
                Some((MouseButton::Right, true))
            } else {
                None
            };
            if let Some((button, erase)) = start {
                self.push_undo();
                let cell = self.cell_at(mouse);
                let changed = self.apply(cell, erase);
                self.stroke = Some(Stroke { button, erase, last: cell, changed });
                if self.tool.has_text() && !erase {
                    self.begin_sign_edit(cell);
                }
            }
        }
        if let Some(mut stroke) = self.stroke.take() {
            if is_mouse_button_down(stroke.button) {
                let cell = self.cell_at(mouse);
                if cell != stroke.last {
                    stroke.changed |= self.apply_line(stroke.last, cell, stroke.erase);
                    stroke.last = cell;
                }
                self.stroke = Some(stroke);
            } else if stroke.changed {
                self.changed();
            } else {
                self.undo.pop(); // nothing happened; don't leave an empty undo step
            }
            if self.stroke.as_ref().is_some_and(|s| s.changed) {
                self.preview_dirty = true;
            }
        }

        // ---- Live preview ----
        if self.preview_dirty {
            self.preview_dirty = false;
            let mut g = Game::new(Level::parse(&self.grid.to_text()));
            g.preview = true;
            // Keep ambient particles and animation time so edits don't flicker.
            std::mem::swap(&mut g.particles, &mut self.preview.particles);
            g.anim_t = self.preview.anim_t;
            self.preview = g;
        }
        set_theme(self.grid.theme);
        audio::music(self.grid.theme, 0.4); // quieter while editing
        self.preview.cam = self.cam;
        self.preview.view_zoom = Some(self.zoom);
        self.preview.tick_preview(dt);
    }

    fn draw(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        let mouse: Vec2 = mouse_position().into();
        render::draw_world(&self.preview, 1.0, self.zoom);

        // ---- Grid overlay ----
        let (gw, gh) = (self.grid.w() as f32 * TILE, self.grid.h() as f32 * TILE);
        let tl = self.to_screen(Vec2::ZERO);
        let br = self.to_screen(vec2(gw, gh));
        let shade = alpha(BLACK, 0.45);
        draw_rectangle(0.0, 0.0, sw, tl.y.max(0.0), shade);
        draw_rectangle(0.0, br.y, sw, (sh - br.y).max(0.0), shade);
        draw_rectangle(0.0, tl.y, tl.x.max(0.0), br.y - tl.y, shade);
        draw_rectangle(br.x, tl.y, (sw - br.x).max(0.0), br.y - tl.y, shade);
        let step = TILE * self.zoom;
        if step > 6.0 {
            for x in 0..=self.grid.w() {
                let sx = tl.x + x as f32 * step;
                let a = if x % 5 == 0 { 0.14 } else { 0.06 };
                draw_line(sx, tl.y, sx, br.y, 1.0, alpha(WHITE, a));
            }
            for y in 0..=self.grid.h() {
                let sy = tl.y + y as f32 * step;
                let a = if (self.grid.h() - y) % 5 == 0 { 0.14 } else { 0.06 };
                draw_line(tl.x, sy, br.x, sy, 1.0, alpha(WHITE, a));
            }
        }
        draw_rectangle_lines(tl.x, tl.y, br.x - tl.x, br.y - tl.y, 2.0, alpha(WHITE, 0.4));

        // ---- Spawn marker (the player is hidden in the preview) ----
        if let Some((x, y)) = self.grid.find('P') {
            let feet = vec2((x as f32 + 0.5) * TILE, (y as f32 + 1.0) * TILE);
            let a = self.to_screen(feet - vec2(W / 2.0, H));
            draw_rectangle(a.x, a.y, W * self.zoom, H * self.zoom, alpha(player(), 0.75));
            draw_rectangle_lines(a.x, a.y, W * self.zoom, H * self.zoom, 2.0, lighten(player(), 0.5));
            let t = "START";
            let size = 16.0 * ui();
            let tw = measure_text(t, None, size as u16, 1.0).width;
            draw_text(t, a.x + W * self.zoom / 2.0 - tw / 2.0, a.y - 6.0 * ui(), size, WHITE);
        }

        // ---- Playtest traces ----
        self.traces.draw(|p| self.to_screen(p), self.zoom, ui());

        // ---- Decor layer: mark decorated cells (some decorations are subtle) ----
        if self.layer != Layer::Tiles && step > 6.0 {
            for (y, row) in self.grid.layer(self.layer).iter().enumerate() {
                for (x, &c) in row.iter().enumerate() {
                    if c != '.' {
                        let p = self.to_screen(vec2(x as f32 * TILE, y as f32 * TILE));
                        draw_rectangle_lines(p.x + 1.0, p.y + 1.0, step - 2.0, step - 2.0, 1.0, alpha(pickup(), 0.35));
                    }
                }
            }
        }
        if let Some(edit) = &self.sign_edit {
            let p = self.to_screen(vec2(edit.cell.0 as f32 * TILE, edit.cell.1 as f32 * TILE));
            draw_rectangle_lines(p.x, p.y, step, step, 3.0, pickup());
        }

        // ---- Hover / brush preview ----
        if self.sign_edit.is_none() && !self.over_ui(mouse) && self.pan.is_none() && !is_key_down(KeyCode::Space) {
            let (cx, cy) = self.cell_at(mouse);
            if self.grid.in_bounds(cx, cy) {
                let p = self.to_screen(vec2(cx as f32 * TILE, cy as f32 * TILE));
                let erasing = is_mouse_button_down(MouseButton::Right) || self.tool == Tool::Eraser;
                let outline = if erasing { hazard() } else { WHITE };
                draw_rectangle(p.x, p.y, step, step, alpha(outline, 0.12));
                draw_rectangle_lines(p.x, p.y, step, step, 2.0, alpha(outline, 0.8));
                if !erasing && self.palette_drag.is_none() {
                    draw_tool_icon(self.tool, p + Vec2::splat(step / 2.0), step * 0.8, 0.55, self.ramp_flip, self.rotation);
                }
            }
        }

        self.draw_top_bar();
        self.draw_palette();
        self.draw_status_bar();
        self.draw_sign_input();

        if let Some((tool, start)) = self.palette_drag {
            if mouse.distance(start) > 6.0 {
                draw_tool_icon(tool, mouse, 40.0 * ui(), 0.85, self.ramp_flip, self.rotation);
            }
        }

        if let Some((msg, t)) = &self.toast {
            let s = ui();
            let a = (t / 0.4).min(1.0);
            let size = 24.0 * s;
            let dims = measure_text(msg, None, size as u16, 1.0);
            let x = left_w() + (sw - left_w() - dims.width) / 2.0;
            let y = top_h() + 14.0 * s;
            draw_rectangle(x - 14.0 * s, y, dims.width + 28.0 * s, 38.0 * s, alpha(deep(), 0.85 * a));
            draw_rectangle_lines(x - 14.0 * s, y, dims.width + 28.0 * s, 38.0 * s, 2.0, alpha(player(), a));
            draw_text(msg, x, y + 26.0 * s, size, alpha(WHITE, a));
        }
    }

    /// Two rows: level management + Play on top, theme + size below.
    fn draw_top_bar(&mut self) {
        let sw = screen_width();
        let s = ui();
        draw_rectangle(0.0, 0.0, sw, top_h(), alpha(darken(deep(), 0.5), 0.95));
        draw_line(0.0, top_h(), sw, top_h(), 1.0, alpha(WHITE, 0.15));
        let dim = alpha(WHITE, 0.6);

        // Row 1: level selector.
        let y = 6.0 * s;
        let mut x = 10.0 * s;
        label(&mut x, y, "Level", dim);
        if button(&mut x, y, "<", false) {
            self.switch_level(-1);
        }
        let name = format!("{} ({}/{})", levels::display_name(self.path()), self.current + 1, self.files.len());
        label(&mut x, y, &name, WHITE);
        if button(&mut x, y, ">", false) {
            self.switch_level(1);
        }
        separator(&mut x, y);
        if button(&mut x, y, "New", false) {
            self.new_level(false);
        }
        if button(&mut x, y, "Copy", false) {
            self.new_level(true);
        }
        let armed = self.delete_armed > 0.0;
        if button(&mut x, y, if armed { "Confirm delete" } else { "Delete" }, armed) {
            self.delete_level();
        }
        let play = "Play (F5)";
        let mut px = (sw - measure_text(play, None, font() as u16, 1.0).width - 28.0 * s).max(x);
        if button(&mut px, y, play, true) {
            self.start_playtest(false);
        }

        // Row 2: theme and size.
        let y = 44.0 * s;
        let mut x = 10.0 * s;
        label(&mut x, y, "Theme", dim);
        if button(&mut x, y, "<", false) {
            self.set_theme_index(-1);
        }
        for (i, c) in [hazard(), pickup(), player(), terrain()].iter().enumerate() {
            draw_rectangle(x + i as f32 * 10.0 * s, y + 10.0 * s, 9.0 * s, 12.0 * s, *c);
        }
        x += 44.0 * s;
        label(&mut x, y, THEMES[self.grid.theme].name, WHITE);
        if button(&mut x, y, ">", false) {
            self.set_theme_index(1);
        }
        separator(&mut x, y);

        let step = if is_key_down(KeyCode::LeftShift) { 5 } else { 1 };
        label(&mut x, y, "Size", dim);
        label(&mut x, y, &format!("{}x{}", self.grid.w(), self.grid.h()), WHITE);
        if button(&mut x, y, "W-", false) {
            self.resize(-step, 0);
        }
        if button(&mut x, y, "W+", false) {
            self.resize(step, 0);
        }
        if button(&mut x, y, "H-", false) {
            self.resize(0, -step);
        }
        if button(&mut x, y, "H+", false) {
            self.resize(0, step);
        }
        label(&mut x, y, "(Shift: 5)", alpha(WHITE, 0.35));
        separator(&mut x, y);

        if button(&mut x, y, "Traces (L)", self.traces.visible) {
            self.toggle_traces();
        }
        if button(&mut x, y, "Clear", false) {
            self.clear_traces();
        }
        label(&mut x, y, &format!("{} runs", self.traces.count()), alpha(WHITE, 0.6));
    }

    fn draw_palette(&mut self) {
        let sh = screen_height();
        let s = ui();
        draw_rectangle(0.0, top_h(), left_w(), sh - top_h() - bottom_h(), alpha(darken(deep(), 0.5), 0.95));
        draw_line(left_w(), top_h(), left_w(), sh - bottom_h(), 1.0, alpha(WHITE, 0.15));
        let mouse: Vec2 = mouse_position().into();
        for layer in Layer::ALL {
            let r = Self::layer_tab_rect(layer);
            let active = layer == self.layer;
            let bg = if active { alpha(player(), 0.3) } else if r.contains(mouse) { alpha(WHITE, 0.12) } else { alpha(WHITE, 0.05) };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, if active { 2.0 } else { 1.0 }, alpha(WHITE, if active { 0.8 } else { 0.15 }));
            let name = layer.name();
            let size = 16.0 * s;
            let tw = measure_text(name, None, size as u16, 1.0).width;
            draw_text(name, r.x + (r.w - tw) / 2.0, r.y + 21.0 * s, size, alpha(WHITE, if active { 1.0 } else { 0.6 }));
        }
        for (i, tool) in self.layer.tools().iter().enumerate() {
            let r = Self::palette_rect(i);
            let selected = *tool == self.tool;
            let hover = r.contains(mouse);
            let bg = if selected {
                alpha(player(), 0.3)
            } else if hover {
                alpha(WHITE, 0.12)
            } else {
                alpha(WHITE, 0.05)
            };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            let (thick, edge) = if selected { (2.0, 0.8) } else { (1.0, 0.15) };
            draw_rectangle_lines(r.x, r.y, r.w, r.h, thick, alpha(WHITE, edge));
            draw_tool_icon(*tool, vec2(r.x + r.w / 2.0, r.y + 21.0 * s), 22.0 * s, 1.0, self.ramp_flip, self.rotation);
            let name = tool.label();
            let size = 13.0 * s;
            let tw = measure_text(name, None, size as u16, 1.0).width;
            draw_text(name, r.x + (r.w - tw) / 2.0, r.y + r.h - 6.0 * s, size, WHITE);
            if i < 9 {
                draw_text(&format!("{}", i + 1), r.x + 4.0 * s, r.y + 13.0 * s, 14.0 * s, alpha(WHITE, 0.4));
            }
        }

        // Tool options.
        let mut top = Self::palette_rect(self.layer.tools().len() - 1).bottom() + 18.0 * s;
        if self.tool == Tool::Gate {
            let (minus, plus) = self.gate_button_rects();
            draw_text("Coins needed", 12.0 * s, minus.y + 19.0 * s, 16.0 * s, alpha(WHITE, 0.7));
            for (r, t) in [(minus, "-"), (plus, "+")] {
                let bg = if r.contains(mouse) { alpha(WHITE, 0.16) } else { alpha(WHITE, 0.06) };
                draw_rectangle(r.x, r.y, r.w, r.h, bg);
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, alpha(WHITE, 0.25));
                draw_text(t, r.x + 10.0 * s, r.y + 20.0 * s, font(), WHITE);
            }
            let n = self.gate_need.to_string();
            let tw = measure_text(&n, None, font() as u16, 1.0).width;
            draw_text(&n, (minus.right() + plus.x - tw) / 2.0, minus.y + 20.0 * s, font(), pickup());
            draw_text("[ / ]  change   (Shift: 5)", 12.0 * s, minus.bottom() + 16.0 * s, 14.0 * s, alpha(WHITE, 0.45));
            top = minus.bottom() + 40.0 * s;
        } else if self.tool_has_options() {
            let r = self.flip_button_rect();
            let bg = if r.contains(mouse) { alpha(WHITE, 0.16) } else { alpha(WHITE, 0.06) };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, alpha(WHITE, 0.25));
            let (label, hint) = match self.tool {
                Tool::Half => (["Rotate: lower half", "Rotate: left half", "Rotate: upper half", "Rotate: right half"][self.rotation as usize % 4], "Crouch or slide under half gaps"),
                Tool::Eye => (["Rotate: on ceiling", "Rotate: on right wall", "Rotate: on floor", "Rotate: on left wall"][self.rotation as usize % 4], ""),
                Tool::Scar => (if self.rotation % 2 == 0 { "Rotate: across" } else { "Rotate: up and down" }, ""),
                _ => (if self.ramp_flip { "Flip: down to the right" } else { "Flip: up to the right" }, "Put solid blocks under ramps"),
            };
            draw_text(label, r.x + 8.0 * s, r.y + 19.0 * s, 15.0 * s, WHITE);
            draw_text("R also does this", 12.0 * s, r.bottom() + 16.0 * s, 15.0 * s, alpha(WHITE, 0.45));
            draw_text(hint, 12.0 * s, r.bottom() + 34.0 * s, 15.0 * s, alpha(WHITE, 0.45));
            top = r.bottom() + 56.0 * s;
        } else if self.tool.has_text() {
            draw_text("Click to place or edit,", 12.0 * s, top + 4.0 * s, 15.0 * s, alpha(WHITE, 0.7));
            draw_text("then type. Enter: done", 12.0 * s, top + 22.0 * s, 15.0 * s, alpha(WHITE, 0.7));
            draw_text("Shift+Enter: new line", 12.0 * s, top + 40.0 * s, 15.0 * s, alpha(WHITE, 0.7));
            top += 62.0 * s;
        }

        let help = [
            "Tab  next layer",
            "LMB  paint / drag tool",
            "RMB  erase",
            "MMB / Space  pan",
            "Wheel  zoom   F  frame",
            "T / Shift+T  theme",
            "Ctrl+Z / Y  undo / redo",
            "PgUp / PgDn  level",
            "F5  play from spawn",
            "F6  play from cursor",
            "L / Shift+L  traces / clear",
            "M / N  music / sounds",
        ];
        for (i, line) in help.iter().enumerate() {
            let y = top + i as f32 * 18.0 * s;
            if y < sh - bottom_h() - 6.0 * s {
                draw_text(line, 12.0 * s, y, 15.0 * s, alpha(WHITE, 0.5));
            }
        }
    }

    /// Text box at the bottom of the view while typing into a sign.
    fn draw_sign_input(&self) {
        let Some(edit) = &self.sign_edit else { return };
        let text = self.grid.signs.get(&edit.cell).map(|t| t.as_str()).unwrap_or("");
        let s = ui();
        let size = 20.0 * s;
        let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "_" } else { " " };
        let lines: Vec<String> = format!("{text}{caret}").split('\n').map(str::to_owned).collect();
        let (x, w) = (left_w() + 20.0 * s, screen_width() - left_w() - 40.0 * s);
        let h = (lines.len() as f32 + 1.6) * size * 1.2;
        let y = screen_height() - bottom_h() - h - 14.0 * s;
        draw_rectangle(x, y, w, h, alpha(darken(deep(), 0.4), 0.95));
        draw_rectangle_lines(x, y, w, h, 2.0, pickup());
        draw_text("Sign text   (Enter: done   Shift+Enter: new line   Backspace: delete)", x + 12.0 * s, y + size, 15.0 * s, alpha(WHITE, 0.55));
        for (i, l) in lines.iter().enumerate() {
            draw_text(l, x + 12.0 * s, y + size * (2.2 + i as f32 * 1.2), size, WHITE);
        }
    }

    fn draw_status_bar(&self) {
        let (sw, sh) = (screen_width(), screen_height());
        let s = ui();
        let y = sh - bottom_h();
        draw_rectangle(0.0, y, sw, bottom_h(), alpha(darken(deep(), 0.5), 0.95));
        draw_line(0.0, y, sw, y, 1.0, alpha(WHITE, 0.15));
        let (cx, cy) = self.cell_at(mouse_position().into());
        let mut cell = if self.grid.in_bounds(cx, cy) { format!("cell {cx},{cy}") } else { "outside".into() };
        if self.grid.in_bounds(cx, cy) {
            let key = (cx as usize, cy as usize);
            if let Some(n) = self.grid.gates.get(&key) {
                cell.push_str(&format!(" (gate: {n} coins)"));
            }
            if let Some(t) = self.grid.signs.get(&key) {
                let short: String = t.chars().take(24).collect();
                cell.push_str(&format!(" (sign: \"{}\")", short.replace('\n', " ")));
            }
        }
        let saved = if self.save_in.is_some() { "saving..." } else { "saved" };
        let mut text = format!(
            "{cell}   |   {}: {}   |   {} coins  {} orbs  {} checkpoints   |   {saved}",
            self.layer.name(),
            self.tool.label(),
            self.grid.count('o'),
            self.grid.count('D'),
            self.grid.count('C'),
        );
        if let Some((deaths, time, finished)) = self.traces.last_summary() {
            let end = if finished { "  finished" } else { "" };
            text.push_str(&format!("   |   last run {time:.1}s  {deaths} deaths{end}"));
        }
        if self.grid.find('P').is_none() {
            text.push_str("   |   no spawn!");
        }
        if self.grid.find('G').is_none() {
            text.push_str("   |   no goal!");
        }
        let coins = self.grid.count('o') as u32;
        if self.grid.gates.values().any(|&n| n > coins) {
            text.push_str("   |   a gate needs more coins than the level has!");
        }
        draw_text(&text, 12.0 * s, y + 21.0 * s, 18.0 * s, alpha(WHITE, 0.75));
    }
}

fn draw_playtest_banner() {
    let s = ui();
    let size = 18.0 * s;
    let text = "PLAYTEST   Esc / F5 / Back: return to editor   R: restart";
    let w = measure_text(text, None, size as u16, 1.0).width + 24.0 * s;
    let x = (screen_width() - w) / 2.0;
    let y = 8.0 * s;
    draw_rectangle(x, y, w, 26.0 * s, alpha(BLACK, 0.5));
    draw_text(text, x + 12.0 * s, y + 18.0 * s, size, alpha(WHITE, 0.85));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_round_trips_through_text() {
        let src = "@theme Daybreak\n#..P\n#==o\n####\n";
        let g = Grid::from_text(src);
        assert_eq!(THEMES[g.theme].name, "Daybreak");
        // Levels saved with older theme names still load.
        assert_eq!(Grid::from_text("@theme Ichor\n#..P\n").theme, g.theme);
        assert_eq!(Grid::from_text("@theme Sunset\n#..P\n").theme, g.theme);
        let again = Grid::from_text(&g.to_text());
        assert!(g == again);
        let level = Level::parse(&g.to_text());
        assert_eq!(level.theme, g.theme);
        assert_eq!(level.coins.len(), 1);
    }

    #[test]
    fn resize_keeps_floor_at_bottom() {
        let mut g = Grid::blank(0);
        let floor = g.rows.last().unwrap().clone();
        g.resize(3, 2);
        assert_eq!(g.w(), NEW_W + 3);
        assert_eq!(g.h(), NEW_H + 2);
        assert!(g.rows.last().unwrap().starts_with(&floor));
        g.resize(-100, -100);
        assert_eq!((g.w(), g.h()), (MIN_W, MIN_H));
    }

    #[test]
    fn gates_signs_and_decor_round_trip() {
        let mut g = Grid::blank(0);
        g.rows[10][5] = 'L';
        g.gates.insert((5, 10), 4);
        g.decor[3][7] = 'S';
        g.signs.insert((7, 3), "Do not\nlook down".into());
        g.decor[14][1] = 'c';
        let text = g.to_text();
        assert!(text.contains("@gate 5 10 4\n"));
        assert!(text.contains("@sign 7 3 Do not\\nlook down\n"));
        assert!(text.contains("@decor\n"));
        let again = Grid::from_text(&text);
        assert!(g == again);

        let level = Level::parse(&text);
        assert_eq!(level.gates[0].need, 4);
        assert_eq!(level.signs[0].text, "Do not\nlook down");
        assert_eq!(level.decor.len(), 2);

        // No decorations: no decor section at all.
        assert!(!Grid::blank(0).to_text().contains("@decor"));
    }

    #[test]
    fn walls_and_neon_round_trip() {
        let mut g = Grid::blank(0);
        g.walls[5][3] = 'h';
        g.walls[5][4] = 'g';
        g.front[2][8] = 'p';
        g.rows[NEW_H - 2][9] = '%';
        g.decor[5][3] = 'N';
        g.signs.insert((3, 5), "OPEN 24/7".into());
        g.decor[6][2] = 'e';
        g.rots.insert((2, 6), 3);
        g.rows[NEW_H - 3][6] = 'T';
        let text = g.to_text();
        assert!(text.contains("@walls\n"));
        assert!(text.contains("@front\n"));
        assert!(text.contains("@rot 2 6 3\n"));
        let again = Grid::from_text(&text);
        assert!(g == again);
        let level = Level::parse(&text);
        assert_eq!(level.walls.len(), 2);
        assert_eq!(level.front_walls.len(), 1);
        assert!(level.is_slick(9, NEW_H as i32 - 2));
        assert_eq!(level.neons[0].text, "OPEN 24/7");
        assert_eq!(level.decor.iter().find(|d| d.kind == DecorKind::Eye).map(|d| d.rot), Some(3));
        assert_eq!(level.tile(6, NEW_H as i32 - 3), crate::level::Tile::Half(Half::Upper));
    }

    #[test]
    fn walls_move_off_an_old_decor_layer() {
        let g = Grid::from_text("#..P\n####\n@decor\n.wnc\n....\n");
        let pad = MIN_H - 2;
        assert_eq!(&g.walls[pad][1..3], &['w', 'n']);
        assert_eq!(g.decor[pad][1], '.');
        assert_eq!(g.decor[pad][3], 'c');
    }

    #[test]
    fn resize_moves_gates_and_signs_with_their_rows() {
        let mut g = Grid::blank(0);
        g.rows[10][5] = 'L';
        g.gates.insert((5, 10), 2);
        g.decor[1][40] = 'S';
        g.signs.insert((40, 1), "hi".into());
        g.resize(0, 3);
        assert_eq!(g.gates.get(&(5, 13)), Some(&2));
        assert_eq!(g.rows[13][5], 'L');
        assert_eq!(g.signs.get(&(40, 4)).map(String::as_str), Some("hi"));
        g.resize(0, -5); // removes the top 5 rows: the sign (now row 4) goes with them
        assert_eq!(g.gates.get(&(5, 8)), Some(&2));
        assert!(g.signs.is_empty());
        g.resize(-20, 0); // width 48 -> 28: x=5 survives
        assert_eq!(g.gates.len(), 1);
        assert_eq!(g.decor[0].len(), g.w());
    }
}
