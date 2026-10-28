//! All drawing: a glossy future city seen from its rooftops (Frutiger Aero).
//!
//! Everything sits in one perspective view: the level's buildings are boxes
//! reaching back from the play plane, in front of a city of towers standing in
//! the haze far below. Back to front: sky, clouds, distant towers, the level's
//! buildings and objects, the robot, then dark foreground silhouettes that move
//! faster than the camera so speed reads at a glance.

use crate::enemies::DEATH_TIME;
use crate::game::{CrumbleState, Game, Pose, BUBBLE_REGROW_TIME, CRUMBLE_DELAY, CRUMBLE_REGROW, GHOST_LIFE, ORB_REGROW_TIME};
use crate::input::{Device, Input};
use crate::level::{DecorKind, EnemyKind, Half, Level, Slope, Tile, WallCell, WallKind, TILE};
use crate::palette::*;
use crate::particles::Layer;
use crate::player::Move;
use crate::speedometer::{self, move_color, ui};
use crate::surface::SurfaceKind;
use crate::tuning;
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::f32::consts::{PI, TAU};

/// Roof ledge thickness.
const TRIM: f32 = 5.0;
/// How deep the robot's body is (world px back from the play plane).
const ROBOT_DEPTH: f32 = 16.0;
/// Where the sky gradient reaches its horizon colour (fraction of the screen).
const SKY_SPLIT: f32 = 0.7;

/// World -> screen transform for the current frame.
#[derive(Clone, Copy)]
struct View {
    cam: Vec2,
    zoom: f32,
    half: Vec2,
    shake: Vec2,
    t: f32,
    /// Camera height at its lowest (view resting on the level's bottom edge).
    ground_ref: f32,
    /// The level's bottom edge (world y).
    ground: f32,
}

impl View {
    fn p(&self, w: Vec2) -> Vec2 {
        (w - self.cam) * self.zoom + self.half + self.shake
    }
    fn s(&self, v: f32) -> f32 {
        v * self.zoom
    }
    /// Project a point `z` world px behind the play plane.
    fn p3(&self, w: Vec2, z: f32) -> Vec2 {
        let k = depth_scale(z);
        (w - self.cam) * self.zoom * k + self.half + self.shake * k
    }
    /// The same view for things standing `z` behind the play plane: everything
    /// drawn through it (including its own `p3` depths) is pushed back by `z`.
    fn at_depth(&self, z: f32) -> View {
        let k = depth_scale(z);
        View { zoom: self.zoom * k, shake: self.shake * k, ..*self }
    }
    /// Visible world rect (min, max) with a margin.
    fn world_bounds(&self, margin: f32) -> (Vec2, Vec2) {
        let half = self.half / self.zoom + Vec2::splat(margin);
        (self.cam - half, self.cam + half)
    }
    /// Screen pixels the camera is above its lowest point (for parallax height).
    fn rise(&self) -> f32 {
        ((self.ground_ref - self.cam.y) * self.zoom).max(0.0)
    }
}

// ---------- Small geometry helpers ----------

fn hash(a: i32, b: i32, seed: u32) -> f32 {
    let mut x = (a as u32).wrapping_mul(2_654_435_761)
        ^ (b as u32).wrapping_mul(2_246_822_519)
        ^ seed.wrapping_mul(0x9E37_79B9);
    x ^= x >> 15;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 16;
    (x % 10_000) as f32 / 10_000.0
}

fn centroid(pts: &[Vec2]) -> Vec2 {
    pts.iter().copied().sum::<Vec2>() / pts.len() as f32
}

fn fill_poly(pts: &[Vec2], color: Color) {
    let c = centroid(pts);
    for i in 0..pts.len() {
        draw_triangle(c, pts[i], pts[(i + 1) % pts.len()], color);
    }
}

fn outline_poly(pts: &[Vec2], thickness: f32, color: Color) {
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        draw_line(a.x, a.y, b.x, b.y, thickness, color);
        draw_circle(a.x, a.y, thickness * 0.5, color); // round joins
    }
}

fn quad(a: Vec2, b: Vec2, c: Vec2, d: Vec2, color: Color) {
    draw_triangle(a, b, c, color);
    draw_triangle(a, c, d, color);
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

fn ellipse_pts(c: Vec2, rx: f32, ry: f32, n: usize) -> Vec<Vec2> {
    (0..n).map(|i| c + Vec2::from_angle(i as f32 / n as f32 * TAU) * vec2(rx, ry)).collect()
}

fn vert(p: Vec2, c: Color) -> Vertex {
    Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, c)
}

/// Quad with a colour per corner.
fn grad_quad(p: [Vec2; 4], c: [Color; 4]) {
    draw_mesh(&Mesh {
        vertices: (0..4).map(|i| vert(p[i], c[i])).collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    });
}

/// Rectangle with a vertical gradient.
fn grad_rect(x: f32, y: f32, w: f32, h: f32, top: Color, bottom: Color) {
    grad_quad([vec2(x, y), vec2(x + w, y), vec2(x + w, y + h), vec2(x, y + h)], [top, top, bottom, bottom]);
}

/// Convex polygon filled from its centre, coloured per point.
fn fan(pts: &[Vec2], color: impl Fn(Vec2) -> Color) {
    let c = centroid(pts);
    let mut vertices = Vec::with_capacity(pts.len() + 1);
    vertices.push(vert(c, color(c)));
    vertices.extend(pts.iter().map(|p| vert(*p, color(*p))));
    let n = pts.len() as u16;
    let indices = (0..n).flat_map(|i| [0, i + 1, (i + 1) % n + 1]).collect();
    draw_mesh(&Mesh { vertices, indices, texture: None });
}

/// Convex polygon with a vertical gradient between its top and bottom.
fn grad_poly(pts: &[Vec2], top: Color, bottom: Color) {
    let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    fan(pts, |p| mix(top, bottom, ((p.y - y0) / (y1 - y0).max(0.001)).clamp(0.0, 1.0)));
}

/// Soft round glow that fades to nothing at radius `r`.
fn glow(c: Vec2, r: f32, color: Color) {
    const RINGS: [f32; 4] = [0.0, 0.3, 0.6, 1.0];
    const SEGS: u16 = 24;
    let mut vertices = vec![vert(c, color)];
    for &f in &RINGS[1..] {
        let a = (1.0 - f) * (1.0 - f);
        for i in 0..SEGS {
            let d = Vec2::from_angle(i as f32 / SEGS as f32 * TAU);
            vertices.push(vert(c + d * r * f, alpha(color, a)));
        }
    }
    let mut indices = Vec::new();
    for i in 0..SEGS {
        let j = (i + 1) % SEGS;
        indices.extend([0, 1 + i, 1 + j]);
        for ring in 0..2u16 {
            let (a, b) = (1 + ring * SEGS, 1 + (ring + 1) * SEGS);
            indices.extend([a + i, b + i, b + j, a + i, b + j, a + j]);
        }
    }
    draw_mesh(&Mesh { vertices, indices, texture: None });
}

fn round_rect_pts(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Vec2> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let corners = [
        (vec2(x + w - r, y + r), -PI / 2.0),
        (vec2(x + w - r, y + h - r), 0.0),
        (vec2(x + r, y + h - r), PI / 2.0),
        (vec2(x + r, y + r), PI),
    ];
    let mut pts = Vec::with_capacity(28);
    for (c, a0) in corners {
        for k in 0..=6 {
            pts.push(c + Vec2::from_angle(a0 + k as f32 / 6.0 * PI / 2.0) * r);
        }
    }
    pts
}

/// Rounded rectangle with a vertical gradient.
fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32, top: Color, bottom: Color) {
    grad_poly(&round_rect_pts(x, y, w, h, r), top, bottom);
}

/// Frosted glass panel: dark tint, a glossy top half and a light rim.
fn glass_panel(x: f32, y: f32, w: f32, h: f32, a: f32) {
    let k = ui();
    let r = 12.0 * k;
    let d = deep();
    round_rect(x, y, w, h, r, alpha(d, 0.55 * a), alpha(d, 0.75 * a));
    round_rect(x + 2.0, y + 2.0, w - 4.0, h * 0.48, r - 2.0, alpha(WHITE, 0.16 * a), alpha(WHITE, 0.02 * a));
    outline_poly(&round_rect_pts(x, y, w, h, r), 1.2 * k, alpha(WHITE, 0.3 * a));
}

/// Glossy sphere: shaded body, a white highlight and a soft rim light.
fn gloss_ball(c: Vec2, r: f32, color: Color, a: f32) {
    fan(&ellipse_pts(c, r, r, 20), |p| {
        let t = ((p.y - (c.y - r)) / (2.0 * r)).clamp(0.0, 1.0);
        alpha(mix(lighten(color, 0.35), darken(color, 0.25), t), a)
    });
    fill_poly(&ellipse_pts(c + vec2(0.0, r * 0.45), r * 0.55, r * 0.3, 12), alpha(lighten(color, 0.4), 0.5 * a));
    fill_poly(&ellipse_pts(c + vec2(-r * 0.12, -r * 0.5), r * 0.55, r * 0.28, 12), alpha(WHITE, 0.85 * a));
}

// ---------- Entry points ----------

pub fn draw_world(g: &Game, alpha_t: f32, zoom: f32) {
    let shake = if g.shake > 0.0 {
        let m = g.shake * g.shake * 40.0;
        vec2(gen_range(-m, m), gen_range(-m, m))
    } else {
        Vec2::ZERO
    };
    let half = vec2(screen_width(), screen_height()) / 2.0;
    let v = View {
        cam: g.cam,
        zoom,
        half,
        shake,
        t: g.anim_t,
        ground_ref: g.level.pixel_height() - half.y / zoom,
        ground: g.level.pixel_height(),
    };
    let th = theme();

    draw_sky(&v, &th);
    draw_clouds(&v, &th);
    draw_city(&v, &th);
    draw_birds(&v, &th);
    g.particles.draw(Layer::Back, |p| v.p(p), v.zoom);
    // Everything standing back on the roofs goes in before the facades on the
    // play plane, so a facade between it and the eye hides it: rooftop props,
    // then the player, enemies and pickups a quarter of the way back, then the
    // front walls between them and the eye.
    draw_buildings(&v, g, &th, || {
        draw_edges(&v, g, &th);
        draw_decor(&v, g, &th, false);
        let a = v.at_depth(ACTOR_Z);
        draw_checkpoints(&a, g, &th);
        draw_goal(&a, g);
        draw_coins(&a, g);
        draw_dash_orbs(&a, g);
        draw_bubbles(&a, g);
        draw_enemies(&a, g, alpha_t);
        draw_player(&a, g, alpha_t);
        g.particles.draw(Layer::Front, |p| a.p(p), a.zoom);
        draw_front_walls(&v, g, &th);
    });
    draw_platforms(&v, g, &th);
    draw_bounce_blocks(&v, g, &th, None);
    draw_crumble_blocks(&v, g, &th, None);
    draw_gates(&v, g, None);
    draw_acid(&v, g);
    draw_decor(&v, g, &th, true);
    if !g.preview {
        draw_foreground(&v, &th);
    }
    draw_speed_lines(&v, g);
    draw_sign_text(&v, g);
}


// ---------- Sky ----------

fn sky_at(th: &Theme, y01: f32) -> Color {
    if y01 < SKY_SPLIT {
        mix(th.sky_top, th.sky_bottom, (y01 / SKY_SPLIT).max(0.0))
    } else {
        mix(th.sky_bottom, th.haze, ((y01 - SKY_SPLIT) / (1.0 - SKY_SPLIT)).min(1.0))
    }
}

fn draw_sky(v: &View, th: &Theme) {
    let (sw, sh) = (screen_width(), screen_height());
    grad_rect(0.0, 0.0, sw, sh * SKY_SPLIT, th.sky_top, th.sky_bottom);
    grad_rect(0.0, sh * SKY_SPLIT, sw, sh * (1.0 - SKY_SPLIT) + 1.0, th.sky_bottom, th.haze);

    // Stars at night.
    let night = ((th.lights - 0.5) * 2.0).clamp(0.0, 1.0);
    if night > 0.0 {
        for i in 0..140 {
            let x = (hash(i, 1, 71) * sw - v.cam.x * v.zoom * 0.01).rem_euclid(sw);
            let y = hash(i, 2, 71) * sh * 0.65;
            let tw = 0.55 + 0.45 * (v.t * (1.0 + hash(i, 3, 71) * 2.0) + i as f32).sin();
            let a = night * tw * (1.0 - y / (sh * 0.65)) * 0.9;
            draw_circle(x, y, 0.6 + hash(i, 4, 71) * 1.1, alpha(WHITE, a));
        }
    }

    // Sun (or moon) with a broad glow.
    let k = sh / 360.0;
    let sun = vec2(th.sun_at.0 * sw, th.sun_at.1 * sh + v.rise() * 0.02);
    let day = 1.0 - night;
    glow(sun, sh * (0.25 + 0.35 * day), alpha(th.sun, 0.25 + 0.2 * day));
    glow(sun, sh * 0.1, alpha(th.sun, 0.55));
    draw_circle(sun.x, sun.y, 14.0 * k, th.sun);
    if day > 0.5 {
        // Lens flare: soft discs along the line through the screen centre.
        let axis = vec2(sw, sh) / 2.0 - sun;
        for (f, r, a) in [(0.55, 10.0, 0.10), (0.85, 22.0, 0.07), (1.25, 6.0, 0.14), (1.6, 34.0, 0.05), (1.9, 12.0, 0.08)] {
            let p = sun + axis * f;
            draw_circle(p.x, p.y, r * k, alpha(mix(th.sun, th.sky_bottom, 0.3), a * day));
        }
    } else {
        // Moon craters.
        draw_circle(sun.x - 4.0 * k, sun.y - 3.0 * k, 3.5 * k, alpha(th.haze, 0.18));
        draw_circle(sun.x + 5.0 * k, sun.y + 4.0 * k, 2.5 * k, alpha(th.haze, 0.15));
    }
}

fn draw_clouds(v: &View, th: &Theme) {
    let (sw, sh) = (screen_width(), screen_height());
    let rise = v.rise();
    for (li, (factor, size, drift)) in [(0.02, 0.9, 4.0), (0.05, 1.4, 9.0)].into_iter().enumerate() {
        let slot = sh * 1.1;
        let scroll = v.cam.x * v.zoom * factor + v.t * drift;
        let i0 = (scroll / slot).floor() as i32 - 1;
        let i1 = ((scroll + sw) / slot).ceil() as i32 + 1;
        for i in i0..=i1 {
            let h = |k: u32| hash(i, li as i32, 90 + k);
            if h(0) < 0.3 {
                continue;
            }
            let base = vec2(i as f32 * slot - scroll + h(1) * slot * 0.5, sh * (0.12 + h(2) * 0.38) + rise * factor * 2.0);
            if base.y > sh * 0.95 {
                continue;
            }
            let sky = sky_at(th, base.y / sh);
            let body = mix(sky, mix(WHITE, th.sun, 0.25), 0.5 - 0.3 * th.lights);
            let shade = mix(body, sky, 0.4);
            let scale = sh * 0.05 * size * (0.7 + h(3) * 0.6);
            let puffs = 4 + (h(4) * 3.0) as i32;
            // Shadowed underside first, then the lit body over it.
            for pass in 0..2 {
                let (col, off) = if pass == 0 { (shade, scale * 0.18) } else { (body, 0.0) };
                for p in 0..puffs {
                    let f = p as f32 / (puffs - 1) as f32;
                    let r = scale * (0.55 + 0.6 * (f * PI).sin()) * (0.8 + hash(i, p, 95 + li as u32) * 0.4);
                    let c = base + vec2(f * scale * puffs as f32 * 0.9, -r * 0.35 + off);
                    draw_circle(c.x, c.y, r, col);
                }
            }
        }
    }
}

// ---------- Perspective ----------

/// Distance from the eye to the play plane (world px): sets how strong the perspective is.
const EYE: f32 = 1000.0;
/// How far the level's buildings reach back from the play plane (world px).
const BUILDING_DEPTH: f32 = 280.0;
/// How far catwalk decks reach back.
const DECK_DEPTH: f32 = 90.0;
/// Catwalk deck thickness.
const DECK_THICK: f32 = 5.0;
/// Interior walls stand this far back: half the depth of a building.
const INTERIOR_DEPTH: f32 = BUILDING_DEPTH / 2.0;
/// The player, enemies and pickups stand a quarter of the way back, in front
/// of the interior walls but behind the front walls.
const ACTOR_Z: f32 = BUILDING_DEPTH / 4.0;
/// Rooftop props (crates, AC units, signs, screens) stand just behind them.
const PROP_Z: f32 = ACTOR_Z + 18.0;
/// Front walls: between the play plane and the player.
const FRONT_Z0: f32 = 6.0;
const FRONT_Z1: f32 = ACTOR_Z - 22.0;
/// The street, this far below the level's bottom edge (world px).
const STREET_DROP: f32 = 1400.0;
/// Storey height on distant towers (world px).
const STOREY: f32 = 96.0;
/// Storey height on the level's buildings, in tiles.
const FLOOR_TILES: i32 = 3;
/// Structures on a roof at most this many tiles tall are part of the building
/// (ramps, steps, rooftop boxes); taller ones are buildings of their own.
const LOW_STRUCTURE: i32 = 3;
/// Mirror's Edge-style accent colours for the odd coloured building.
const ACCENTS: [u32; 5] = [0xff6a1a, 0xe8232b, 0xffc21a, 0x27b36a, 0x1f9fe8];

/// Perspective scale at depth `z` behind the play plane.
fn depth_scale(z: f32) -> f32 {
    EYE / (EYE + z)
}

/// How much of the haze colour shows at depth `z`.
fn fog(z: f32) -> f32 {
    // Even the nearest towers sit back in the air a little, so the level stands out.
    0.2 + 0.8 * (1.0 - (-z / 7000.0).exp())
}

/// A wall colour turned toward or away from the sun.
fn side_shade(c: Color, th: &Theme, lit: bool) -> Color {
    if lit {
        lighten(c, 0.12)
    } else {
        mix(darken(c, 0.14), th.sky_top, 0.22)
    }
}

fn roof_shade(c: Color) -> Color {
    lighten(c, 0.2)
}

// ---------- Distant city ----------

struct CityLayer {
    /// Depth of the front faces (world px behind the play plane).
    z: f32,
    /// Tower spacing as it appears on screen (world px at the play plane).
    spacing: f32,
    /// Tower heights above the level's bottom edge (world px).
    height: (f32, f32),
    seed: u32,
}

const CITY: [CityLayer; 5] = [
    CityLayer { z: 14000.0, spacing: 120.0, height: (1200.0, 5500.0), seed: 1 },
    CityLayer { z: 7000.0, spacing: 140.0, height: (500.0, 3000.0), seed: 2 },
    CityLayer { z: 3500.0, spacing: 165.0, height: (200.0, 1700.0), seed: 3 },
    CityLayer { z: 1600.0, spacing: 200.0, height: (0.0, 1100.0), seed: 4 },
    CityLayer { z: 900.0, spacing: 250.0, height: (-500.0, 250.0), seed: 5 },
];

#[derive(Clone, Copy, PartialEq)]
enum TowerKind {
    Concrete,
    Glass,
    Accent(Color),
}

/// A box-shaped building in the distance.
struct Tower {
    x0: f32,
    x1: f32,
    top: f32,
    z0: f32,
    z1: f32,
    kind: TowerKind,
    /// Random numbers for its details.
    h: [f32; 4],
    id: i32,
}

/// A vertical face from `top` down to the street, between two (x, z) edges. It
/// keeps its colour down to the level's bottom edge, then fades into the haze.
#[allow(clippy::too_many_arguments)]
fn wall(v: &View, a: (f32, f32), b: (f32, f32), top: f32, street: f32, c_top: Color, c_low: Color, haze: Color) {
    let split = v.ground.max(top);
    let pt = |e: (f32, f32), y: f32| v.p3(vec2(e.0, y), e.1);
    if split > top {
        grad_quad([pt(a, top), pt(b, top), pt(b, split), pt(a, split)], [c_top, c_top, c_low, c_low]);
    }
    grad_quad([pt(a, split), pt(b, split), pt(b, street), pt(a, street)], [c_low, c_low, haze, haze]);
}

/// Horizontal face at height `y` between depths z0 and z1.
#[allow(clippy::too_many_arguments)]
fn slab(v: &View, x0: f32, x1: f32, y: f32, z0: f32, z1: f32, front: Color, back: Color) {
    grad_quad(
        [v.p3(vec2(x0, y), z0), v.p3(vec2(x1, y), z0), v.p3(vec2(x1, y), z1), v.p3(vec2(x0, y), z1)],
        [front, front, back, back],
    );
}

fn draw_city(v: &View, th: &Theme) {
    let (sw, sh) = (screen_width(), screen_height());
    let street = v.ground + STREET_DROP;
    // Below the far horizon the street is lost in mist.
    let horizon = v.p3(vec2(0.0, street), CITY[0].z).y;
    grad_rect(0.0, horizon - sh * 0.12, sw, (sh - horizon + sh * 0.12).max(0.0) + 1.0, alpha(th.haze, 0.0), th.haze);

    for (li, l) in CITY.iter().enumerate() {
        let k = depth_scale(l.z);
        let spacing = l.spacing / k;
        let half_w = v.half.x / (v.zoom * k);
        let i0 = ((v.cam.x - half_w) / spacing).floor() as i32 - 2;
        let i1 = ((v.cam.x + half_w) / spacing).ceil() as i32 + 2;
        let mut towers: Vec<Tower> = Vec::new();
        for i in i0..=i1 {
            let h = |n: u32| hash(i, li as i32, l.seed * 32 + n);
            if h(0) < 0.08 {
                continue;
            }
            let w = spacing * (0.5 + 0.42 * h(1));
            let cx = (i as f32 + 0.5) * spacing + (h(2) - 0.5) * (spacing - w);
            let mut height = l.height.0 + (l.height.1 - l.height.0) * h(3).powf(1.6);
            if h(4) > 0.95 {
                height = height * 1.4 + 300.0; // the odd landmark tower
            }
            let z0 = l.z + h(5) * l.z * 0.12;
            let kind = match h(6) {
                x if x < 0.48 => TowerKind::Concrete,
                x if x < 0.88 || li >= 3 => TowerKind::Glass,
                _ => TowerKind::Accent(Color::from_hex(ACCENTS[(h(7) * ACCENTS.len() as f32) as usize % ACCENTS.len()])),
            };
            towers.push(Tower {
                x0: cx - w / 2.0,
                x1: cx + w / 2.0,
                top: v.ground - height,
                z0,
                z1: z0 + w * (0.6 + 0.6 * h(8)),
                kind,
                h: [h(9), h(10), h(11), h(12)],
                id: i * 8 + li as i32,
            });
        }
        // Painter's order: furthest from the centre of view first.
        towers.sort_by(|a, b| {
            let da = ((a.x0 + a.x1) / 2.0 - v.cam.x).abs();
            let db = ((b.x0 + b.x1) / 2.0 - v.cam.x).abs();
            db.total_cmp(&da)
        });
        for t in &towers {
            draw_tower(v, th, t, street, li);
        }
    }
}

fn draw_tower(v: &View, th: &Theme, t: &Tower, street: f32, layer: usize) {
    let f = fog(t.z0);
    let haze = th.haze;
    let tint = |c: Color| mix(c, haze, f);
    let base = match t.kind {
        TowerKind::Concrete => mix(th.facade, th.city, 0.25),
        TowerKind::Glass => th.glass,
        TowerKind::Accent(c) => mix(c, th.facade, 0.15 + 0.4 * th.lights),
    };
    let sun_right = th.sun_at.0 > 0.5;

    // Side facing the centre of view, then the roof if we're above it.
    if t.x0 > v.cam.x {
        let c = tint(side_shade(base, th, !sun_right));
        wall(v, (t.x0, t.z1), (t.x0, t.z0), t.top, street, c, c, haze);
    } else if t.x1 < v.cam.x {
        let c = tint(side_shade(base, th, sun_right));
        wall(v, (t.x1, t.z0), (t.x1, t.z1), t.top, street, c, c, haze);
    }
    if t.top > v.cam.y {
        let c = tint(roof_shade(base));
        slab(v, t.x0, t.x1, t.top, t.z0, t.z1, c, mix(c, haze, 0.2));
    }

    // Rooftop structures.
    let w = t.x1 - t.x0;
    let d = t.z1 - t.z0;
    if t.h[0] < 0.35 {
        let bx = t.x0 + w * (0.15 + 0.3 * t.h[1]);
        let sub = Tower {
            x0: bx,
            x1: bx + w * 0.4,
            top: t.top - 40.0 - 80.0 * t.h[2],
            z0: t.z0 + d * 0.25,
            z1: t.z0 + d * 0.7,
            kind: TowerKind::Concrete,
            h: [1.0; 4],
            id: t.id,
        };
        draw_tower(v, th, &sub, t.top + 1.0, layer);
    }

    // Front face.
    let (top_col, low_col) = match t.kind {
        TowerKind::Glass => (tint(mix(th.glass, th.sky_bottom, 0.45)), tint(darken(th.glass, 0.08))),
        _ => (tint(base), tint(mix(base, th.shadow, 0.08))),
    };
    wall(v, (t.x0, t.z0), (t.x1, t.z0), t.top, street, top_col, low_col, haze);
    draw_tower_storeys(v, th, t, street, f);

    // Antenna with a warning light, or a construction crane.
    let k = depth_scale(t.z0);
    let mid_z = (t.z0 + t.z1) / 2.0;
    if t.h[3] > 0.82 && t.h[0] >= 0.35 {
        let a = v.p3(vec2((t.x0 + t.x1) / 2.0, t.top), mid_z);
        let b = v.p3(vec2((t.x0 + t.x1) / 2.0, t.top - 260.0 * (0.5 + t.h[1])), mid_z);
        draw_line(a.x, a.y, b.x, b.y, (12.0 * v.zoom * k).max(1.0), tint(mix(th.facade, th.shadow, 0.3)));
        if th.lights > 0.2 && (v.t * 0.8 + t.h[2] * 3.0).fract() < 0.5 {
            glow(b, (60.0 * v.zoom * k).max(3.0), alpha(th.hazard, 0.9));
        }
    } else if t.h[3] < 0.1 && (1..=3).contains(&layer) {
        draw_crane(v, th, t, f);
    }
}

/// Storey lines on a distant tower's front: window bands, glass mullions, lit windows.
fn draw_tower_storeys(v: &View, th: &Theme, t: &Tower, street: f32, f: f32) {
    let k = depth_scale(t.z0);
    let fh = STOREY * v.zoom * k;
    if fh < 3.0 {
        return;
    }
    let sh = screen_height();
    let a = v.p3(vec2(t.x0, t.top), t.z0);
    let b = v.p3(vec2(t.x1, t.top), t.z0);
    let w = b.x - a.x;
    if b.x < 0.0 || a.x > screen_width() {
        return;
    }
    let fade = |y: f32| f.max(((y - v.ground) / (street - v.ground)).clamp(0.0, 1.0));
    // Only the storeys on screen.
    let wy_max = v.cam.y + v.half.y / (v.zoom * k);
    let first = (((v.cam.y - v.half.y / (v.zoom * k)) - t.top) / STOREY).floor().max(0.0) as i32;
    let night = th.lights > 0.3;
    let mut n = first;
    loop {
        let wy = t.top + (n as f32 + 0.3) * STOREY;
        if wy > wy_max.min(street) {
            break;
        }
        let sy = v.p3(vec2(t.x0, wy), t.z0).y;
        let fd = fade(wy);
        match t.kind {
            TowerKind::Glass => {
                let line = mix(lighten(th.glass, 0.5), th.haze, fd);
                draw_rectangle(a.x, sy - fh * 0.3, w, (fh * 0.1).max(1.0), alpha(line, 0.5));
            }
            _ => {
                let band = mix(mix(th.glass, th.sky_bottom, 0.3), th.haze, fd);
                draw_rectangle(a.x + w * 0.07, sy, w * 0.86, fh * 0.42, band);
            }
        }
        if night {
            let segs = ((w / (fh * 1.2)) as i32).clamp(1, 12);
            let sw = w * 0.86 / segs as f32;
            for s in 0..segs {
                if hash(t.id * 16 + s, n, 41) < th.lights * 0.4 {
                    draw_rectangle(a.x + w * 0.07 + s as f32 * sw + sw * 0.1, sy, sw * 0.8, fh * 0.42, mix(th.window_lit, th.haze, fd * 0.8));
                }
            }
        }
        if sy > sh {
            break;
        }
        n += 1;
    }
    // Glass mullions.
    if t.kind == TowerKind::Glass {
        let ms = 40.0 * v.zoom * k;
        if ms >= 4.0 {
            let bottom = v.p3(vec2(t.x0, v.ground), t.z0).y.min(sh);
            let mut x = a.x + ms;
            while x < b.x - 1.0 {
                draw_line(x, a.y, x, bottom, 1.0, alpha(mix(lighten(th.glass, 0.5), th.haze, f), 0.35));
                x += ms;
            }
        }
    }
}

/// A tower crane on a rooftop: lattice mast, jib and counter-jib.
fn draw_crane(v: &View, th: &Theme, t: &Tower, f: f32) {
    let z = (t.z0 + t.z1) / 2.0;
    let k = depth_scale(z);
    let col = mix(Color::from_hex(ACCENTS[(t.h[1] * 3.0) as usize]), th.haze, f);
    let col = mix(col, th.shadow, 0.5 * th.lights);
    let mx = t.x0 + (t.x1 - t.x0) * (0.3 + 0.4 * t.h[2]);
    let height = 500.0 + 300.0 * t.h[0];
    let jib = 700.0 + 300.0 * t.h[1];
    let dir = if t.h[2] > 0.5 { 1.0 } else { -1.0 };
    let p = |x: f32, y: f32| v.p3(vec2(x, y), z);
    let thick = (14.0 * v.zoom * k).max(1.0);
    let (base, top) = (p(mx, t.top), p(mx, t.top - height));
    draw_line(base.x, base.y, top.x, top.y, thick * 1.6, col);
    let jy = t.top - height + 30.0;
    let (j0, j1) = (p(mx - dir * jib * 0.3, jy), p(mx + dir * jib, jy));
    draw_line(j0.x, j0.y, j1.x, j1.y, thick, col);
    // Tie lines from the peak.
    let peak = p(mx, t.top - height - 90.0);
    draw_line(peak.x, peak.y, j1.x, j1.y, thick * 0.4, col);
    draw_line(peak.x, peak.y, j0.x, j0.y, thick * 0.4, col);
    draw_line(peak.x, peak.y, top.x, top.y, thick, col);
    // Counterweight and a hanging hook.
    let cw = p(mx - dir * jib * 0.25, jy);
    draw_rectangle(cw.x - thick * 2.0, cw.y, thick * 4.0, thick * 3.0, col);
    let hx = mx + dir * jib * (0.4 + 0.4 * t.h[3]);
    let (h0, h1) = (p(hx, jy), p(hx, jy + 250.0 + 300.0 * t.h[0]));
    draw_line(h0.x, h0.y, h1.x, h1.y, (thick * 0.3).max(1.0), col);
}

// ---------- Level buildings ----------

/// Visible tile range (x0, x1, y0, y1).
fn visible_cells(v: &View, lvl: &Level) -> (i32, i32, i32, i32) {
    let (min, max) = v.world_bounds(TILE);
    (
        ((min.x / TILE).floor() as i32).max(0),
        ((max.x / TILE).ceil() as i32).min(lvl.width - 1),
        ((min.y / TILE).floor() as i32).max(0),
        ((max.y / TILE).ceil() as i32).min(lvl.height - 1),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Facade {
    /// Small rooftop structures: bare walls.
    Plain,
    /// White concrete with a ribbon window every storey.
    Concrete,
    /// Glass curtain wall.
    Glass,
    /// Concrete with a coloured band and window trim.
    Accent(Color),
}

/// One building: a run of columns sharing a roof line, in one row.
#[derive(Clone, Copy)]
struct Block {
    x0: i32,
    x1: i32,
    roof: i32,
    facade: Facade,
}

/// A side, roof or underside face, drawn back to front before the facades.
struct Face {
    key: f32,
    /// Screen position and depth of each corner.
    pts: [(Vec2, f32); 4],
    cols: [Color; 4],
}

/// The part of a face in front of (`back` false) or behind depth `m`, as a
/// polygon with a colour per corner. Edges crossing `m` run straight back in
/// depth, so the crossing point follows the perspective scale.
fn clip_face(f: &Face, m: f32, back: bool) -> Vec<(Vec2, Color)> {
    let inside = |z: f32| if back { z >= m } else { z <= m };
    let mut out = Vec::with_capacity(6);
    for i in 0..4 {
        let ((a, za), (b, zb)) = (f.pts[i], f.pts[(i + 1) % 4]);
        let (ca, cb) = (f.cols[i], f.cols[(i + 1) % 4]);
        if inside(za) {
            out.push((a, ca));
        }
        if inside(za) != inside(zb) && (za - m).abs() > 1e-3 && (zb - m).abs() > 1e-3 {
            let (ka, kb, km) = (depth_scale(za), depth_scale(zb), depth_scale(m));
            let t = (ka - km) / (ka - kb);
            out.push((a.lerp(b, t), mix(ca, cb, t)));
        }
    }
    out
}

/// A convex polygon with a colour per corner.
fn draw_poly_cols(pts: &[(Vec2, Color)]) {
    if pts.len() < 3 {
        return;
    }
    let n = pts.len() as u16;
    draw_mesh(&Mesh {
        vertices: pts.iter().map(|&(p, c)| vert(p, c)).collect(),
        indices: (1..n - 1).flat_map(|i| [0, i, i + 1]).collect(),
        texture: None,
    });
}

/// The level's buildings, seen as boxes reaching back from the play plane.
/// Solid tiles at the bottom row carry on down to the street.
struct Buildings<'a> {
    lvl: &'a Level,
    /// Tile rows covered (may go past the level's bottom edge).
    y0: i32,
    y1: i32,
    /// Top row of each solid stack, per column and row (-1 = empty).
    roofs: Vec<i32>,
    /// Block per visible row: index into `blocks`, per column.
    block_at: Vec<Option<u16>>,
    blocks: Vec<Block>,
}

impl<'a> Buildings<'a> {
    fn tile(&self, x: i32, y: i32) -> Tile {
        let l = self.lvl;
        if x < 0 || x >= l.width || y < 0 {
            return Tile::Empty;
        }
        if y >= l.height {
            return match l.tile(x, l.height - 1) {
                Tile::Solid | Tile::Slope(_) => Tile::Solid,
                _ => Tile::Empty,
            };
        }
        l.tile(x, y)
    }

    fn mass(&self, x: i32, y: i32) -> bool {
        matches!(self.tile(x, y), Tile::Solid | Tile::Slope(_))
    }

    /// Whether the neighbouring cell hides this side (`side`: -1 left, 1 right, 0 above).
    fn covered(&self, x: i32, y: i32, side: i32) -> bool {
        match self.tile(x, y) {
            Tile::Slope(s) => side == 0 || (side < 0 && s.right == 2) || (side > 0 && s.left == 2),
            t => t == Tile::Solid,
        }
    }

    fn new(lvl: &'a Level, y0: i32, y1: i32) -> Self {
        let w = lvl.width;
        let rows = (y1 - y0 + 1) as usize;
        let mut b = Buildings { lvl, y0, y1, roofs: vec![-1; w as usize * rows], block_at: vec![None; w as usize * rows], blocks: Vec::new() };
        // Roof row of every stack, scanning each column from the top.
        for x in 0..w {
            let mut roof = -1;
            let mut prev = false;
            for y in 0..=y1 {
                let m = b.mass(x, y);
                if m && !prev {
                    roof = y;
                }
                prev = m;
                if y >= y0 {
                    b.roofs[((y - y0) * w + x) as usize] = if m { roof } else { -1 };
                }
            }
        }
        // Group each row into buildings. Columns belong together while the roof
        // surface runs on without a vertical step (so ramps join the roof they rise
        // from); narrow bumps (vault steps, rooftop boxes) join their neighbour.
        // A building's roof is its lowest top, so ramps and bumps sit above it.
        let edge_y = |b: &Buildings, x: i32, r: i32, right: bool| match b.tile(x, r) {
            Tile::Slope(s) => (r + 1) as f32 * TILE - Slope::height_at(s, if right { 1.0 } else { 0.0 }),
            _ => r as f32 * TILE,
        };
        // Bottom up, so whatever stands on a building can join it.
        for y in (y0..=y1).rev() {
            let row = ((y - y0) * w) as usize;
            let mut runs: Vec<(i32, i32, i32)> = Vec::new();
            for x in 0..w {
                let r = b.roofs[row + x as usize];
                if r < 0 {
                    continue;
                }
                let joined = match runs.last_mut() {
                    Some(last) if last.1 + 1 == x => {
                        let pr = b.roofs[row + (x - 1) as usize];
                        if edge_y(&b, x - 1, pr, true) == edge_y(&b, x, r, false) {
                            last.1 = x;
                            last.2 = last.2.max(r);
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if !joined {
                    runs.push((x, x, r));
                }
            }
            let mut merged: Vec<(i32, i32, i32)> = Vec::new();
            for r in runs {
                if let Some(m) = merged.last_mut() {
                    if m.1 + 1 == r.0 && (r.1 - r.0 < 2 || m.1 - m.0 < 2) {
                        m.1 = r.1;
                        m.2 = m.2.max(r.2);
                        continue;
                    }
                }
                merged.push(r);
            }
            for (s, e, roof) in merged {
                // Part of the building below: its body, or a low structure on its
                // roof (ramp hill, step, rooftop box). Taller things are their own.
                let below = (y < y1).then(|| b.block_at[row + w as usize + s as usize]).flatten();
                let joins = below.filter(|&id| {
                    let under = b.blocks[id as usize];
                    let all_on = (s..=e).all(|x| b.block_at[row + w as usize + x as usize] == Some(id));
                    let top = (s..=e).map(|x| b.roofs[row + x as usize]).min().unwrap_or(roof);
                    all_on && (y >= under.roof || under.roof - top <= LOW_STRUCTURE)
                });
                if let Some(id) = joins {
                    for x in s..=e {
                        b.block_at[row + x as usize] = Some(id);
                    }
                    continue;
                }
                let width = e - s + 1;
                let h = hash(s, roof, 61);
                let facade = if width < 4 {
                    Facade::Plain
                } else if h < 0.55 {
                    Facade::Concrete
                } else if h < 0.78 {
                    Facade::Glass
                } else {
                    Facade::Accent(Color::from_hex(ACCENTS[(hash(s, roof, 62) * ACCENTS.len() as f32) as usize % ACCENTS.len()]))
                };
                let id = b.blocks.len() as u16;
                b.blocks.push(Block { x0: s, x1: e, roof, facade });
                for x in s..=e {
                    b.block_at[row + x as usize] = Some(id);
                }
            }
        }
        b
    }

    fn block(&self, x: i32, y: i32) -> Option<Block> {
        let y = y.min(self.y1); // below the map, buildings carry on as they were
        if x < 0 || x >= self.lvl.width || y < self.y0 {
            return None;
        }
        self.block_at[((y - self.y0) * self.lvl.width + x) as usize].map(|i| self.blocks[i as usize])
    }
}

fn facade_base(th: &Theme, f: Facade) -> Color {
    match f {
        Facade::Plain => mix(th.facade, th.shadow, 0.07),
        Facade::Glass => th.glass,
        Facade::Concrete | Facade::Accent(_) => th.facade,
    }
}

/// Whether row `fy` storeys below the roof holds a ribbon window.
fn ribbon_row(fy: i32) -> bool {
    fy >= 1 && fy % FLOOR_TILES == 1
}

/// `before_facades` draws whatever stands back on the roofs, after everything
/// behind the play plane and before the facades on it.
fn draw_buildings(v: &View, g: &Game, th: &Theme, before_facades: impl FnOnce()) {
    let lvl = &g.level;
    let kd = depth_scale(BUILDING_DEPTH);
    // Side and roof faces recede toward the centre, so look past the screen edges for them.
    let ext = v.half / (v.zoom * kd) + Vec2::splat(TILE);
    let fx0 = ((v.cam.x - ext.x) / TILE).floor() as i32;
    let fx1 = ((v.cam.x + ext.x) / TILE).ceil() as i32;
    let fy0 = (((v.cam.y - ext.y) / TILE).floor() as i32).max(0);
    let fy1 = (((v.cam.y + ext.y) / TILE).ceil() as i32).min(lvl.height + 40);
    if fy1 < fy0 {
        return;
    }
    // Grouped from the level's bottom row up, so facades don't change as the view moves.
    let bld = Buildings::new(lvl, fy0.min(lvl.height), lvl.height);
    let sun_right = th.sun_at.0 > 0.5;
    let roof_col = roof_shade(mix(th.trim, th.facade, 0.5));
    let roof_back = mix(roof_col, th.haze, 0.15);
    let d = BUILDING_DEPTH;
    let p3 = |x: f32, y: f32, z: f32| (v.p3(vec2(x, y), z), z);
    let key = |x: f32, y: f32| (vec2(x, y) - v.cam).length();

    let mut faces: Vec<Face> = Vec::new();
    let mut push = |key: f32, pts: [(Vec2, f32); 4], cols: [Color; 4]| faces.push(Face { key, pts, cols });
    for y in fy0..=fy1 {
        for x in fx0.max(0)..=fx1.min(lvl.width - 1) {
            let (wx, wy) = (x as f32 * TILE, y as f32 * TILE);
            match bld.tile(x, y) {
                Tile::Solid => {
                    let Some(b) = bld.block(x, y) else { continue };
                    let b = if y < b.roof { Block { facade: Facade::Plain, ..b } } else { b };
                    let slick = lvl.is_slick(x, y);
                    let base = if slick { slick_glass(th) } else { facade_base(th, b.facade) };
                    // Side walls facing the centre of view.
                    for (side, ex) in [(-1, wx), (1, wx + TILE)] {
                        let faces_eye = if side < 0 { ex > v.cam.x } else { ex < v.cam.x };
                        if !faces_eye || bld.covered(x + side, y, side) {
                            continue;
                        }
                        let lit = (side > 0) == sun_right;
                        let c = side_shade(base, th, lit);
                        let cb = mix(c, th.haze, 0.12);
                        let k0 = key(ex, wy + TILE / 2.0);
                        push(k0, [p3(ex, wy, 0.0), p3(ex, wy, d), p3(ex, wy + TILE, d), p3(ex, wy + TILE, 0.0)], [c, cb, cb, c]);
                        let fy = y - b.roof;
                        if slick {
                            // Glints streaking across the polished glass.
                            let gl = alpha(WHITE, 0.3);
                            for z in [d * 0.12, d * 0.5] {
                                push(k0 - 0.01, [p3(ex, wy, z + 26.0), p3(ex, wy, z + 40.0), p3(ex, wy + TILE, z + 14.0), p3(ex, wy + TILE, z)], [gl; 4]);
                            }
                        } else if b.facade != Facade::Plain && b.facade != Facade::Glass && ribbon_row(fy) {
                            let g = side_shade(mix(th.glass, th.sky_bottom, 0.2), th, lit);
                            let (a, z) = (wy + 7.0, wy + TILE - 5.0);
                            push(k0 - 0.01, [p3(ex, a, 8.0), p3(ex, a, d - 8.0), p3(ex, z, d - 8.0), p3(ex, z, 8.0)], [g, g, g, g]);
                        }
                    }
                    // Roof, seen from above.
                    if wy > v.cam.y && !bld.covered(x, y - 1, 0) {
                        push(key(wx + TILE / 2.0, wy), [p3(wx, wy + 1.0, 0.0), p3(wx + TILE, wy + 1.0, 0.0), p3(wx + TILE, wy, d), p3(wx, wy, d)], [roof_col, roof_col, roof_back, roof_back]);
                    }
                    // Underside, seen from below.
                    let by = wy + TILE;
                    if by < v.cam.y && !bld.mass(x, y + 1) {
                        let c = darken(base, 0.35);
                        push(key(wx + TILE / 2.0, by), [p3(wx, by, 0.0), p3(wx + TILE, by, 0.0), p3(wx + TILE, by, d), p3(wx, by, d)], [c, c, c, c]);
                    }
                }
                Tile::Slope(s) => {
                    let base_y = wy + TILE;
                    let surf = |fx: f32| base_y - Slope::height_at(s, fx);
                    let (ly, ry) = (surf(0.0), surf(1.0));
                    // A sloped surface faces the eye when the eye is above the line it lies
                    // on (not just above its midpoint: off to one side the line rises or falls).
                    let line_y = ly + (ry - ly) * (v.cam.x - wx) / TILE;
                    if v.cam.y < line_y {
                        push(
                            key(wx + TILE / 2.0, (ly + ry) / 2.0),
                            [p3(wx, ly + 1.0, 0.0), p3(wx + TILE, ry + 1.0, 0.0), p3(wx + TILE, ry, d), p3(wx, ly, d)],
                            [roof_col, roof_col, roof_back, roof_back],
                        );
                    }
                    let base = facade_base(th, Facade::Plain);
                    for (side, ex, ey) in [(-1, wx, ly), (1, wx + TILE, ry)] {
                        let faces_eye = if side < 0 { ex > v.cam.x } else { ex < v.cam.x };
                        if !faces_eye || ey >= base_y || bld.covered(x + side, y, side) {
                            continue;
                        }
                        let c = side_shade(base, th, (side > 0) == sun_right);
                        let cb = mix(c, th.haze, 0.12);
                        push(key(ex, (ey + base_y) / 2.0), [p3(ex, ey, 0.0), p3(ex, ey, d), p3(ex, base_y, d), p3(ex, base_y, 0.0)], [c, cb, cb, c]);
                    }
                }
                Tile::Half(hb) => {
                    // Half blocks are little concrete boxes, faced like the buildings.
                    let r = hb.rect(wx, wy);
                    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
                    let base = facade_base(th, Facade::Plain);
                    for (side, ex) in [(-1, x0), (1, x1)] {
                        let faces_eye = if side < 0 { ex > v.cam.x } else { ex < v.cam.x };
                        let flush = if side < 0 { x0 == wx } else { x1 == wx + TILE };
                        if !faces_eye || (flush && half_joined(&bld, hb, x, y, side, 0)) {
                            continue;
                        }
                        let c = side_shade(base, th, (side > 0) == sun_right);
                        let cb = mix(c, th.haze, 0.12);
                        push(key(ex, (y0 + y1) / 2.0), [p3(ex, y0, 0.0), p3(ex, y0, d), p3(ex, y1, d), p3(ex, y1, 0.0)], [c, cb, cb, c]);
                    }
                    if y0 > v.cam.y && !(y0 == wy && half_joined(&bld, hb, x, y, 0, -1)) {
                        push(key((x0 + x1) / 2.0, y0), [p3(x0, y0 + 1.0, 0.0), p3(x1, y0 + 1.0, 0.0), p3(x1, y0, d), p3(x0, y0, d)], [roof_col, roof_col, roof_back, roof_back]);
                    }
                    if y1 < v.cam.y && !(y1 == wy + TILE && (bld.mass(x, y + 1) || half_joined(&bld, hb, x, y, 0, 1))) {
                        let c = darken(base, 0.35);
                        push(key((x0 + x1) / 2.0, y1), [p3(x0, y1, 0.0), p3(x1, y1, 0.0), p3(x1, y1, d), p3(x0, y1, d)], [c, c, c, c]);
                    }
                }
                _ => {}
            }
        }
    }

    // Catwalks are slabs reaching back DECK_DEPTH: deck, underside and end caps.
    let deck_top = roof_shade(th.trim);
    let deck_under = darken(mix(th.trim, th.shadow, 0.4), 0.1);
    for s in g.surfaces.strips.iter().filter(|s| s.kind == SurfaceKind::Membrane) {
        let top = s.top();
        if s.x1 < v.cam.x - ext.x || s.x0 > v.cam.x + ext.x {
            continue;
        }
        let n = ((s.x1 - s.x0) / TILE).ceil().max(1.0) as usize;
        for i in 0..n {
            let (a, b) = (s.x0 + (s.x1 - s.x0) * i as f32 / n as f32, s.x0 + (s.x1 - s.x0) * (i + 1) as f32 / n as f32);
            let (ya, yb) = (top + s.offset_at(a), top + s.offset_at(b));
            for (dy, col, show) in [(0.0, deck_top, top > v.cam.y), (DECK_THICK, deck_under, top + DECK_THICK < v.cam.y)] {
                if show {
                    faces.push(Face {
                        key: key((a + b) / 2.0, top + dy),
                        pts: [p3(a, ya + dy, 0.0), p3(b, yb + dy, 0.0), p3(b, yb + dy, DECK_DEPTH), p3(a, ya + dy, DECK_DEPTH)],
                        cols: [col, col, mix(col, th.haze, 0.1), mix(col, th.haze, 0.1)],
                    });
                }
            }
        }
        for (ex, faces_eye, lit) in [(s.x0, s.x0 > v.cam.x, !sun_right), (s.x1, s.x1 < v.cam.x, sun_right)] {
            if faces_eye {
                let y = top + s.offset_at(ex);
                let c = side_shade(th.trim, th, lit);
                faces.push(Face {
                    key: key(ex, y),
                    pts: [p3(ex, y, 0.0), p3(ex, y, DECK_DEPTH), p3(ex, y + DECK_THICK, DECK_DEPTH), p3(ex, y + DECK_THICK, 0.0)],
                    cols: [c, c, c, c],
                });
            }
        }
    }

    // Back to front: faces further from the centre of view are behind nearer
    // ones. Interior walls fill the back half of the depth, so draw in two
    // layers: everything behind the interior wall plane (with the walls
    // themselves), then everything in front of it.
    spike_faces(v, g, th, &mut faces);
    grass_faces(v, g, th, &mut faces);
    draw_bounce_blocks(v, g, th, Some(&mut faces));
    draw_crumble_blocks(v, g, th, Some(&mut faces));
    draw_gates(v, g, Some(&mut faces));
    faces.sort_by(|a, b| b.key.total_cmp(&a.key));
    let mut back: Vec<Face> = wall_faces(v, g, th, &g.level.walls, INTERIOR_DEPTH, BUILDING_DEPTH, |_, _| 1.0);
    back.sort_by(|a, b| b.key.total_cmp(&a.key));
    let mut bi = back.iter().peekable();
    for f in &faces {
        while let Some(b) = bi.next_if(|b| b.key >= f.key) {
            draw_poly_cols(&clip_face(b, INTERIOR_DEPTH, true));
        }
        draw_poly_cols(&clip_face(f, INTERIOR_DEPTH, true));
    }
    for b in bi {
        draw_poly_cols(&clip_face(b, INTERIOR_DEPTH, true));
    }
    draw_platform_backs(v, g, th);
    for f in &faces {
        draw_poly_cols(&clip_face(f, INTERIOR_DEPTH, false));
    }

    // Spikes sit among the buildings: anything of theirs behind a facade is hidden by it.
    draw_spikes(v, g, th);
    before_facades();

    // Facades on the play plane, over everything behind them.
    let (min, max) = v.world_bounds(TILE);
    let (x0, x1) = (((min.x / TILE).floor() as i32).max(0), ((max.x / TILE).ceil() as i32).min(lvl.width - 1));
    let (y0, y1) = (((min.y / TILE).floor() as i32).max(fy0), ((max.y / TILE).ceil() as i32).min(fy1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            if bld.tile(x, y) == Tile::Solid {
                if let Some(b) = bld.block(x, y) {
                    draw_facade_tile(v, th, &bld, b, x, y);
                }
            }
        }
    }
    // Slick blocks: polished glass cladding wherever their edges are exposed.
    for y in y0..=y1.min(lvl.height - 1) {
        for x in x0..=x1 {
            if lvl.is_slick(x, y) {
                draw_slick_front(v, th, &bld, x, y);
            }
        }
    }
    // Ramps last: their ledges dip into the tile below and must stay on top.
    for y in y0..=y1 {
        for x in x0..=x1 {
            match bld.tile(x, y) {
                Tile::Slope(s) => draw_slope_front(v, th, s, x, y),
                Tile::Half(hb) => draw_half_front(v, th, &bld, hb, x, y),
                _ => {}
            }
        }
    }
}

/// Polished glass on slick blocks: nothing to grip.
fn slick_glass(th: &Theme) -> Color {
    mix(Color::from_hex(0x8fdcff), th.shadow, 0.35 * th.lights)
}

/// A slick block's front: glass cladding with glints down each exposed side
/// and a bright lip along an exposed top, so its edges read as ungrippable.
fn draw_slick_front(v: &View, th: &Theme, bld: &Buildings, x: i32, y: i32) {
    let (wx, wy) = (x as f32 * TILE, y as f32 * TILE);
    let glass = slick_glass(th);
    let w = 7.0;
    for side in [-1, 1] {
        if bld.covered(x + side, y, side) {
            continue;
        }
        let x0 = if side < 0 { wx } else { wx + TILE - w };
        let p = v.p(vec2(x0, wy));
        grad_rect(p.x, p.y, v.s(w), v.s(TILE), lighten(glass, 0.25), darken(glass, 0.1));
        let inner = if side < 0 { x0 + w } else { x0 };
        let (a, b) = (v.p(vec2(inner, wy)), v.p(vec2(inner, wy + TILE)));
        draw_line(a.x, a.y, b.x, b.y, v.s(1.0), alpha(darken(glass, 0.4), 0.8));
        for k in [0.2, 0.62] {
            let (a, b) = (v.p(vec2(x0, wy + TILE * k + 6.0)), v.p(vec2(x0 + w, wy + TILE * k)));
            draw_line(a.x, a.y, b.x, b.y, v.s(1.2), alpha(WHITE, 0.75));
        }
    }
    if !bld.covered(x, y - 1, 0) {
        let p = v.p(vec2(wx, wy));
        grad_rect(p.x, p.y, v.s(TILE), v.s(4.0), lighten(glass, 0.5), glass);
        draw_line(p.x, p.y + v.s(0.5), p.x + v.s(TILE), p.y + v.s(0.5), v.s(1.0), alpha(WHITE, 0.9));
    }
}

/// One tile of a building's front face.
fn draw_facade_tile(v: &View, th: &Theme, bld: &Buildings, b: Block, x: i32, y: i32) {
    let p = v.p(vec2(x as f32 * TILE, y as f32 * TILE));
    let s = v.s(TILE);
    let fy = y - b.roof;
    // Above the main roof (ramps, bumps) the walls are bare concrete.
    let b = if fy < 0 { Block { facade: Facade::Plain, ..b } } else { b };
    let night = th.lights > 0.3;
    let base = facade_base(th, b.facade);
    let shade = |f: f32| mix(base, th.shadow, (f.max(0.0) * 0.01).min(0.12));
    let at_top = !bld.covered(x, y - 1, 0);

    match b.facade {
        Facade::Glass => {
            // Curtain wall reflecting the sky, darker lower down; storey slabs and mullions.
            let t0 = (fy as f32 / 14.0).min(1.0);
            let t1 = ((fy + 1) as f32 / 14.0).min(1.0);
            let hi = mix(th.glass, th.sky_bottom, 0.5);
            grad_rect(p.x - 0.5, p.y - 0.5, s + 1.0, s + 1.0, mix(hi, th.glass, t0), mix(hi, th.glass, t1));
            let cell = (x - b.x0) / 2;
            if night && fy % FLOOR_TILES != 0 && hash(b.x0 * 64 + cell, fy / FLOOR_TILES, 17) < th.lights * 0.4 {
                grad_rect(p.x - 0.5, p.y - 0.5, s + 1.0, s + 1.0, alpha(th.window_lit, 0.45), alpha(th.window_lit, 0.65));
            }
            if fy % FLOOR_TILES == 0 && !at_top {
                draw_rectangle(p.x - 0.5, p.y, s + 1.0, v.s(4.0), alpha(mix(th.trim, th.glass, 0.4), 0.9));
            }
            if (x - b.x0) % 2 == 0 && x != b.x0 {
                draw_rectangle(p.x - v.s(0.6), p.y, v.s(1.2), s, alpha(th.trim, 0.45));
            }
            // Concrete corner columns.
            let col = v.s(6.0);
            if x == b.x0 {
                grad_rect(p.x - 0.5, p.y - 0.5, col, s + 1.0, th.facade, th.facade);
            }
            if x == b.x1 {
                grad_rect(p.x + s - col + 0.5, p.y - 0.5, col, s + 1.0, th.facade, th.facade);
            }
        }
        _ => {
            grad_rect(p.x - 0.5, p.y - 0.5, s + 1.0, s + 1.0, shade(fy as f32), shade(fy as f32 + 1.0));
            if b.facade != Facade::Plain && ribbon_row(fy) {
                // Ribbon window, stopping short of the building's corners.
                let l = if x == b.x0 { v.s(8.0) } else { 0.0 };
                let r = if x == b.x1 { v.s(8.0) } else { 0.0 };
                let (wy0, wy1) = (p.y + v.s(7.0), p.y + v.s(TILE - 5.0));
                let seg = (x - b.x0) / 3;
                let lit = night && hash(b.x0 * 64 + seg, fy, 23) < th.lights * 0.5;
                if lit {
                    grad_rect(p.x + l - 0.5, wy0, s - l - r + 1.0, wy1 - wy0, lighten(th.window_lit, 0.3), th.window_lit);
                } else {
                    let top = mix(th.glass, th.sky_bottom, 0.45);
                    grad_rect(p.x + l - 0.5, wy0, s - l - r + 1.0, wy1 - wy0, top, darken(th.glass, 0.05));
                }
                if let Facade::Accent(c) = b.facade {
                    draw_rectangle(p.x + l - 0.5, wy1, s - l - r + 1.0, v.s(3.0), c);
                }
            }
        }
    }

    // Corners: a crisp edge where the building ends or meets its neighbour.
    let line = v.s(1.5).max(1.0);
    let edge = mix(base, th.shadow, 0.45);
    if (x == b.x0 && bld.mass(x - 1, y)) || !bld.covered(x - 1, y, -1) {
        draw_rectangle(p.x - 0.5, p.y, line, s, edge);
    }
    if (x == b.x1 && bld.mass(x + 1, y)) || !bld.covered(x + 1, y, 1) {
        draw_rectangle(p.x + s - line + 0.5, p.y, line, s, edge);
    }
    if !bld.mass(x, y + 1) {
        draw_rectangle(p.x, p.y + s - v.s(3.0), s, v.s(3.0), darken(base, 0.3));
    }
    if at_top {
        // Parapet along the roof edge (level with ramp ledges, so they join cleanly);
        // accent buildings get a coloured band under it.
        let ty = p.y;
        if let Facade::Accent(c) = b.facade {
            draw_rectangle(p.x - 0.5, ty + v.s(TRIM), s + 1.0, v.s(5.0), c);
        }
        draw_rectangle(p.x - 0.5, ty + v.s(TRIM), s + 1.0, v.s(2.0), alpha(th.shadow, 0.2));
        grad_rect(p.x - 0.5, ty, s + 1.0, v.s(TRIM), th.trim, mix(th.trim, th.facade, 0.6));
        draw_rectangle(p.x - 0.5, ty, s + 1.0, v.s(1.0).max(1.0), alpha(WHITE, 0.8));
    }
}

/// Whether the neighbour at (dx, dy) hides the matching edge of half block `hb` at (x, y).
fn half_joined(bld: &Buildings, hb: Half, x: i32, y: i32, dx: i32, dy: i32) -> bool {
    let along = match hb {
        Half::Lower | Half::Upper => dy == 0,
        Half::Left | Half::Right => dx == 0,
    };
    let side = if dy == 0 { dx } else { 0 };
    bld.covered(x + dx, y + dy, side) || (along && bld.tile(x + dx, y + dy) == Tile::Half(hb))
}

/// A half block's front: bare concrete with a ledge on top and crisp edges.
fn draw_half_front(v: &View, th: &Theme, bld: &Buildings, hb: Half, x: i32, y: i32) {
    let (wx, wy) = (x as f32 * TILE, y as f32 * TILE);
    let r = hb.rect(wx, wy);
    let (a, b) = (v.p(vec2(r.x, r.y)), v.p(vec2(r.x + r.w, r.y + r.h)));
    let base = facade_base(th, Facade::Plain);
    grad_rect(a.x - 0.5, a.y - 0.5, b.x - a.x + 1.0, b.y - a.y + 1.0, base, mix(base, th.shadow, 0.04));
    let line = v.s(1.5).max(1.0);
    let edge = mix(base, th.shadow, 0.45);
    if !(r.x == wx && half_joined(bld, hb, x, y, -1, 0)) {
        draw_rectangle(a.x - 0.5, a.y, line, b.y - a.y, edge);
    }
    if !(r.x + r.w == wx + TILE && half_joined(bld, hb, x, y, 1, 0)) {
        draw_rectangle(b.x - line + 0.5, a.y, line, b.y - a.y, edge);
    }
    if !(r.y + r.h == wy + TILE && (bld.mass(x, y + 1) || half_joined(bld, hb, x, y, 0, 1))) {
        draw_rectangle(a.x, b.y - v.s(3.0), b.x - a.x, v.s(3.0), darken(base, 0.3));
    }
    if !(r.y == wy && half_joined(bld, hb, x, y, 0, -1)) {
        let t = v.s(TRIM).min(b.y - a.y);
        grad_rect(a.x - 0.5, a.y, b.x - a.x + 1.0, t, th.trim, mix(th.trim, th.facade, 0.6));
        draw_rectangle(a.x - 0.5, a.y, b.x - a.x + 1.0, v.s(1.0).max(1.0), alpha(WHITE, 0.8));
    }
}

/// A ramp's front: wall under a sloped ledge.
fn draw_slope_front(v: &View, th: &Theme, s: Slope, x: i32, y: i32) {
    let (wx, base) = (x as f32 * TILE, (y + 1) as f32 * TILE + 0.5);
    let surface = |fx: f32| base - Slope::height_at(s, fx);
    let (l, r) = (vec2(wx - 0.5, surface(0.0)), vec2(wx + TILE + 0.5, surface(1.0)));
    let wall = facade_base(th, Facade::Plain);
    grad_quad([v.p(l), v.p(r), v.p(vec2(r.x, base)), v.p(vec2(l.x, base))], [wall, wall, wall, wall]);
    let (lt, rt) = (l + vec2(0.0, TRIM), r + vec2(0.0, TRIM));
    quad(v.p(lt), v.p(rt), v.p(rt + vec2(0.0, 2.0)), v.p(lt + vec2(0.0, 2.0)), alpha(th.shadow, 0.2));
    let lower = mix(th.trim, th.facade, 0.6);
    grad_quad([v.p(l), v.p(r), v.p(rt), v.p(lt)], [th.trim, th.trim, lower, lower]);
    let (a, b) = (v.p(l), v.p(r));
    draw_line(a.x, a.y, b.x, b.y, v.s(1.0).max(1.0), alpha(WHITE, 0.8));
}

/// The far side of each catwalk: a second truss at the back of the deck, and
/// cross braces between the two. Drawn among the buildings so facades hide them.
fn draw_platform_backs(v: &View, g: &Game, th: &Theme) {
    let (min, max) = v.world_bounds(TILE * 4.0);
    let truss = mix(mix(th.trim, th.shadow, 0.5), th.haze, 0.15);
    for s in g.surfaces.strips.iter().filter(|s| s.kind == SurfaceKind::Membrane) {
        let top = s.top();
        if top < min.y || top > max.y || s.x1 < min.x || s.x0 > max.x {
            continue;
        }
        let n = ((s.x1 - s.x0) / 8.0).ceil().max(1.0) as usize;
        let xs: Vec<f32> = (0..=n).map(|i| s.x0 + (s.x1 - s.x0) * i as f32 / n as f32).collect();
        let at = |x: f32, dy: f32, z: f32| v.p3(vec2(x, top + s.offset_at(x) + dy), z);
        let k = depth_scale(DECK_DEPTH);
        for w in xs.windows(2) {
            for dy in [5.0, 11.0] {
                let (p, q) = (at(w[0], dy, DECK_DEPTH), at(w[1], dy, DECK_DEPTH));
                draw_line(p.x, p.y, q.x, q.y, v.s(1.4) * k, truss);
            }
        }
        for (i, w) in xs.windows(2).enumerate() {
            let (p, q) = if i % 2 == 0 { (at(w[0], 5.0, DECK_DEPTH), at(w[1], 11.0, DECK_DEPTH)) } else { (at(w[0], 11.0, DECK_DEPTH), at(w[1], 5.0, DECK_DEPTH)) };
            draw_line(p.x, p.y, q.x, q.y, v.s(1.1) * k, truss);
        }
        // Bottom chords joining front and back, every tile.
        let mut x = s.x0;
        while x <= s.x1 + 0.5 {
            let (p, q) = (at(x, 11.0, 0.0), at(x, 11.0, DECK_DEPTH));
            draw_line(p.x, p.y, q.x, q.y, v.s(1.1), truss);
            x += TILE;
        }
    }
}

/// Cells with a half-depth wall behind them: things there stop at the wall.
fn interior_cells(g: &Game) -> std::collections::HashSet<(i32, i32)> {
    g.level.walls.iter().map(|w| (w.x, w.y)).collect()
}

/// The solid part of a wall cell, in cell coordinates (glass has none).
fn wall_box(kind: WallKind) -> Option<Rect> {
    match kind {
        WallKind::Wall | WallKind::Shutter | WallKind::Accent => Some(Rect::new(0.0, 0.0, TILE, TILE)),
        WallKind::Low => Some(Rect::new(0.0, TILE / 2.0, TILE, TILE / 2.0)),
        WallKind::Pillar => Some(Rect::new(TILE * 0.3, 0.0, TILE * 0.4, TILE)),
        WallKind::Window | WallKind::Glass => None,
    }
}

/// Wall cells as faces, standing from depth z0 back to z1. Half-depth walls
/// stand at INTERIOR_DEPTH and reach back to the rear of the building, so the
/// space in front reads as a room (or a free-standing wall outdoors); front
/// walls stand between the player and the eye. `fade` scales each cell's opacity.
#[allow(clippy::too_many_arguments)]
fn wall_faces(v: &View, g: &Game, th: &Theme, walls: &[WallCell], z0: f32, z1: f32, fade: impl Fn(i32, i32) -> f32) -> Vec<Face> {
    let mut out = Vec::new();
    let (min, max) = v.world_bounds(TILE * 8.0);
    let cells: std::collections::HashMap<(i32, i32), WallKind> =
        walls.iter().filter(|w| !g.level.is_solid(w.x, w.y)).map(|w| ((w.x, w.y), w.kind)).collect();
    let kind_at = |x: i32, y: i32| cells.get(&(x, y)).copied();
    let box_at = |x: i32, y: i32| kind_at(x, y).and_then(wall_box);
    // Whether the neighbour on `side` (dx, dy) fills the edge our box shares with it.
    let hidden = |x: i32, y: i32, b: Rect, dx: i32, dy: i32| {
        let Some(n) = box_at(x + dx, y + dy) else { return false };
        match (dx, dy) {
            (-1, 0) => b.x == 0.0 && n.right() == TILE && n.y <= b.y && n.bottom() >= b.bottom(),
            (1, 0) => b.right() == TILE && n.x == 0.0 && n.y <= b.y && n.bottom() >= b.bottom(),
            (0, -1) => b.y == 0.0 && n.bottom() == TILE && n.x <= b.x && n.right() >= b.right(),
            _ => b.bottom() == TILE && n.y == 0.0 && n.x <= b.x && n.right() >= b.right(),
        }
    };
    let wall = mix(th.facade, th.window_lit, 0.06 + 0.1 * th.lights);
    let wall = mix(wall, th.shadow, 0.08 + 0.25 * th.lights);
    let night = th.lights > 0.3;
    let sun_right = th.sun_at.0 > 0.5;
    let key = |x: f32, y: f32| (vec2(x, y) - v.cam).length();
    let q = |x: f32, y: f32, z: f32| (v.p3(vec2(x, y), z), z);
    // A flat rectangle on the wall plane.
    let rect = |out: &mut Vec<Face>, k: f32, x0: f32, y0: f32, x1: f32, y1: f32, top: Color, bot: Color| {
        out.push(Face { key: k, pts: [q(x0, y0, z0), q(x1, y0, z0), q(x1, y1, z0), q(x0, y1, z0)], cols: [top, top, bot, bot] });
    };
    let frame = mix(th.trim, th.shadow, 0.35);
    for (&(x, y), &kind) in &cells {
        let (cx0, cy0) = (x as f32 * TILE, y as f32 * TILE);
        if cx0 > max.x || cx0 + TILE < min.x || cy0 > max.y || cy0 + TILE < min.y {
            continue;
        }
        let kc = key(cx0 + TILE / 2.0, cy0 + TILE / 2.0);
        let first = out.len();
        if let Some(b) = wall_box(kind) {
            let (x0, y0, x1, y1) = (cx0 + b.x, cy0 + b.y, cx0 + b.right(), cy0 + b.bottom());
            let col = match kind {
                WallKind::Shutter => mix(th.trim, th.shadow, 0.3),
                WallKind::Accent => {
                    let a = Color::from_hex(ACCENTS[(hash(x.div_euclid(8), 0, 71) * ACCENTS.len() as f32) as usize % ACCENTS.len()]);
                    mix(a, th.shadow, 0.35 * th.lights)
                }
                WallKind::Pillar => mix(wall, th.shadow, 0.05),
                _ => wall,
            };
            // Sides and top/underside wherever the wall ends, then the front.
            if x0 > v.cam.x && !hidden(x, y, b, -1, 0) {
                let c = side_shade(col, th, !sun_right);
                out.push(Face { key: key(x0, (y0 + y1) / 2.0), pts: [q(x0, y0, z0), q(x0, y0, z1), q(x0, y1, z1), q(x0, y1, z0)], cols: [c; 4] });
            } else if x1 < v.cam.x && !hidden(x, y, b, 1, 0) {
                let c = side_shade(col, th, sun_right);
                out.push(Face { key: key(x1, (y0 + y1) / 2.0), pts: [q(x1, y0, z0), q(x1, y0, z1), q(x1, y1, z1), q(x1, y1, z0)], cols: [c; 4] });
            }
            if y0 > v.cam.y && !hidden(x, y, b, 0, -1) {
                let c = roof_shade(col);
                out.push(Face { key: key((x0 + x1) / 2.0, y0), pts: [q(x0, y0, z0), q(x1, y0, z0), q(x1, y0, z1), q(x0, y0, z1)], cols: [c; 4] });
            } else if y1 < v.cam.y && !hidden(x, y, b, 0, 1) {
                let c = darken(col, 0.3);
                out.push(Face { key: key((x0 + x1) / 2.0, y1), pts: [q(x0, y1, z0), q(x1, y1, z0), q(x1, y1, z1), q(x0, y1, z1)], cols: [c; 4] });
            }
            rect(&mut out, kc, x0, y0, x1, y1, col, col);
            let same = |dx: i32, dy: i32| kind_at(x + dx, y + dy) == Some(kind);
            let detail = |out: &mut Vec<Face>, a: f32, b: f32, c: f32, d: f32, col: Color| rect(out, kc - 0.01, a, b, c, d, col, col);
            match kind {
                WallKind::Wall => {
                    // Panel seams every two cells, and a skirting board along the floor.
                    if x % 2 == 0 && same(-1, 0) {
                        detail(&mut out, x0 - 0.5, y0, x0 + 0.5, y1, alpha(th.shadow, 0.12));
                    }
                    if kind_at(x, y + 1).is_none() {
                        detail(&mut out, x0, y1 - 4.0, x1, y1, darken(col, 0.2));
                    }
                }
                WallKind::Shutter => {
                    let mut sy = y0 + 4.0;
                    while sy < y1 {
                        detail(&mut out, x0, sy - 0.5, x1, sy + 0.5, alpha(th.shadow, 0.25));
                        sy += 4.0;
                    }
                    if !same(0, -1) {
                        detail(&mut out, x0, y0, x1, y0 + 5.0, darken(col, 0.25));
                    }
                    if !same(0, 1) {
                        detail(&mut out, x0, y1 - 3.0, x1, y1, darken(col, 0.35));
                    }
                }
                WallKind::Accent => {
                    if !same(0, -1) {
                        detail(&mut out, x0, y0 + 3.0, x1, y0 + 6.0, alpha(WHITE, 0.85));
                    }
                }
                WallKind::Low => detail(&mut out, x0, y0, x1, y0 + 3.0, th.trim),
                _ => {}
            }
        } else {
            // Glass that joins up with neighbouring glass of its kind into one pane,
            // framed only around its outside.
            let joins = |dx: i32, dy: i32| kind_at(x + dx, y + dy) == Some(kind);
            let curtain = kind == WallKind::Glass;
            let fw = if curtain { 1.5 } else { 2.5 };
            let (x0, y0, x1, y1) = (cx0, cy0, cx0 + TILE, cy0 + TILE);
            let lit = night && hash(x.div_euclid(2), y.div_euclid(3), 7) < th.lights * 0.6;
            let (top, bot) = if lit {
                (alpha(lighten(th.window_lit, 0.3), 0.55), alpha(th.window_lit, 0.55))
            } else if curtain {
                (alpha(mix(th.sky_bottom, WHITE, 0.4), 0.2), alpha(mix(th.glass, th.sky_top, 0.3), 0.32))
            } else {
                (alpha(mix(th.sky_bottom, WHITE, 0.4), 0.28), alpha(mix(th.glass, th.sky_top, 0.3), 0.38))
            };
            rect(&mut out, kc, x0, y0, x1, y1, top, bot);
            let fr = |out: &mut Vec<Face>, a: f32, b: f32, c: f32, d: f32| rect(out, kc - 0.01, a, b, c, d, frame, frame);
            if !joins(-1, 0) {
                fr(&mut out, x0, y0, x0 + fw, y1);
            } else if x.rem_euclid(2) == 0 {
                fr(&mut out, x0 - fw * 0.3, y0, x0 + fw * 0.3, y1);
            }
            if !joins(1, 0) {
                fr(&mut out, x1 - fw, y0, x1, y1);
            }
            if !joins(0, -1) {
                fr(&mut out, x0, y0, x1, y0 + fw);
            } else if curtain && y.rem_euclid(3) == 0 {
                fr(&mut out, x0, y0 - fw * 0.3, x1, y0 + fw * 0.3);
            }
            if !joins(0, 1) {
                fr(&mut out, x0, y1 - fw * 1.6, x1, y1);
            }
        }
        let f = fade(x, y);
        if f < 1.0 {
            for face in &mut out[first..] {
                for c in &mut face.cols {
                    c.a *= f;
                }
            }
        }
    }
    out
}

/// Walls between the player and the eye. Near the player they turn
/// see-through, so you can always see where you are.
fn draw_front_walls(v: &View, g: &Game, th: &Theme) {
    if g.level.front_walls.is_empty() {
        return;
    }
    let who = (!g.preview && !g.player.dead).then(|| g.player.center());
    let fade = |x: i32, y: i32| match who {
        Some(c) => {
            let d = (vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE) - c).length() / TILE;
            let t = ((d - 0.8) / 1.5).clamp(0.0, 1.0);
            0.3 + 0.7 * t * t * (3.0 - 2.0 * t)
        }
        None => 1.0,
    };
    let mut faces = wall_faces(v, g, th, &g.level.front_walls, FRONT_Z0, FRONT_Z1, fade);
    faces.sort_by(|a, b| b.key.total_cmp(&a.key));
    for f in &faces {
        let pts: Vec<(Vec2, Color)> = (0..4).map(|i| (f.pts[i].0, f.cols[i])).collect();
        draw_poly_cols(&pts);
    }
}

/// One-way platforms: light catwalks with a truss underneath. They flex when landed on.
fn draw_platforms(v: &View, g: &Game, th: &Theme) {
    let (min, max) = v.world_bounds(TILE);
    let truss = mix(th.trim, th.shadow, 0.5);
    for s in g.surfaces.strips.iter().filter(|s| s.kind == SurfaceKind::Membrane) {
        let top = s.top();
        if top < min.y || top > max.y || s.x1 < min.x || s.x0 > max.x {
            continue;
        }
        let a = s.x0.max(min.x - TILE);
        let b = s.x1.min(max.x + TILE);
        let n = ((b - a) / 8.0).ceil().max(1.0) as usize;
        let xs: Vec<f32> = (0..=n).map(|i| a + (b - a) * i as f32 / n as f32).collect();
        let at = |x: f32, dy: f32| v.p(vec2(x, top + s.offset_at(x) + dy));
        // Truss: two chords with a zigzag between.
        for w in xs.windows(2) {
            for dy in [5.0, 11.0] {
                let (p, q) = (at(w[0], dy), at(w[1], dy));
                draw_line(p.x, p.y, q.x, q.y, v.s(1.4), truss);
            }
        }
        for (i, w) in xs.windows(2).enumerate() {
            let (p, q) = if i % 2 == 0 { (at(w[0], 5.0), at(w[1], 11.0)) } else { (at(w[0], 11.0), at(w[1], 5.0)) };
            draw_line(p.x, p.y, q.x, q.y, v.s(1.1), truss);
        }
        // Deck edge.
        for w in xs.windows(2) {
            let (p0, p1) = (at(w[0], 0.0), at(w[1], 0.0));
            let (q0, q1) = (at(w[0], 5.0), at(w[1], 5.0));
            grad_quad([p0, p1, q1, q0], [th.trim, th.trim, mix(th.trim, th.facade, 0.7), mix(th.trim, th.facade, 0.7)]);
            draw_line(p0.x, p0.y, p1.x, p1.y, v.s(1.0).max(1.0), alpha(WHITE, 0.85));
        }
    }
}

// ---------- Solids in depth ----------

/// Convex hull of a point set (monotone chain), counter-clockwise.
fn hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len() + 1);
    for pass in 0..2 {
        let start = out.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while out.len() >= start + 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// The sides of a convex outline (world space) pushed back from depth z0 to z1.
/// Draw the outline's front face over it afterwards.
fn extrude_sides(v: &View, outline: &[Vec2], z0: f32, z1: f32, color: Color) {
    let pts: Vec<Vec2> = outline.iter().flat_map(|&p| [v.p3(p, z0), v.p3(p, z1)]).collect();
    if pts.len() >= 3 {
        fill_poly(&hull(pts), color);
    }
}

/// Which faces of a box may show: neighbours can hide its sides and top.
#[derive(Clone, Copy)]
struct Open {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

const OPEN: Open = Open { left: true, right: true, top: true, bottom: true };

/// Faces of a tile-sized box that aren't buried against solid neighbours.
fn tile_open(lvl: &Level, x: i32, y: i32) -> Open {
    let solid = |x: i32, y: i32| matches!(lvl.tile(x, y), Tile::Solid | Tile::Slope(_) | Tile::Bounce | Tile::Crumble | Tile::Gate);
    Open { left: !solid(x - 1, y), right: !solid(x + 1, y), top: !solid(x, y - 1), bottom: !solid(x, y + 1) }
}

/// A box on the world grid: `r` on the play plane, reaching back from depth z0 to z1.
/// Draws the faces you'd see (side toward the centre of view, top or bottom), then the front.
fn box3(v: &View, th: &Theme, r: Rect, z0: f32, z1: f32, front: Color, open: Open) {
    let p = |x: f32, y: f32, z: f32| v.p3(vec2(x, y), z);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let sun_right = th.sun_at.0 > 0.5;
    if x0 > v.cam.x && open.left {
        quad(p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1), p(x0, y1, z0), side_shade(front, th, !sun_right));
    } else if x1 < v.cam.x && open.right {
        quad(p(x1, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0), side_shade(front, th, sun_right));
    }
    if y0 > v.cam.y && open.top {
        quad(p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1), roof_shade(front));
    } else if y1 < v.cam.y && open.bottom {
        quad(p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1), darken(front, 0.3));
    }
    quad(p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0), front);
}

/// A sphere `z` behind the play plane: glossy ball scaled by perspective.
fn ball3(v: &View, c: Vec2, z: f32, r: f32, color: Color, glow_r: f32, glow_a: f32) {
    let sc = v.p3(c, z);
    let k = depth_scale(z) * v.zoom;
    if glow_a > 0.0 {
        glow(sc, glow_r * k, alpha(color, glow_a));
    }
    gloss_ball(sc, r * k, color, 1.0);
}

/// The level's left and right edges: a faint barrier wall across the roof depth.
fn draw_edges(v: &View, g: &Game, th: &Theme) {
    let (min, max) = v.world_bounds(TILE);
    let (y0, y1) = (min.y - TILE * 4.0, max.y + TILE * 4.0);
    let field = th.player;
    for x in [0.0, g.level.pixel_width()] {
        if x < min.x - TILE * 8.0 || x > max.x + TILE * 8.0 {
            continue;
        }
        let p = |y: f32, z: f32| v.p3(vec2(x, y), z);
        grad_quad([p(y0, 0.0), p(y0, BUILDING_DEPTH), p(y1, BUILDING_DEPTH), p(y1, 0.0)], [alpha(field, 0.12), alpha(field, 0.03), alpha(field, 0.03), alpha(field, 0.12)]);
        // Scanlines drifting up the front edge.
        let mut y = y0 - (v.t * 40.0).rem_euclid(48.0) + 48.0;
        while y < y1 {
            let (a, b) = (p(y, 0.0), p(y, BUILDING_DEPTH));
            draw_line(a.x, a.y, b.x, b.y, v.s(1.0), alpha(field, 0.2));
            y += 48.0;
        }
        let (a, b) = (p(y0, 0.0), p(y1, 0.0));
        draw_line(a.x, a.y, b.x, b.y, v.s(1.5), alpha(field, 0.45));
    }
}

// ---------- Hazards & objects ----------

/// Which way a spike tile's spikes point: away from the surface it's mounted on.
fn spike_dir(lvl: &Level, x: i32, y: i32) -> Option<Vec2> {
    let mounted = |dx: i32, dy: i32| matches!(lvl.tile(x + dx, y + dy), Tile::Solid | Tile::Crumble | Tile::Bounce | Tile::Slope(_));
    if mounted(0, 1) {
        Some(vec2(0.0, -1.0))
    } else if mounted(0, -1) {
        Some(vec2(0.0, 1.0))
    } else if mounted(-1, 0) {
        Some(vec2(1.0, 0.0))
    } else if mounted(1, 0) {
        Some(vec2(-1.0, 0.0))
    } else {
        None
    }
}

/// The faces of a box (like `box3`) as sortable faces, all sharing one key.
#[allow(clippy::too_many_arguments)]
fn box_faces(v: &View, th: &Theme, r: Rect, z0: f32, z1: f32, front: Color, open: Open, key: f32, out: &mut Vec<Face>) {
    let q = |x: f32, y: f32, z: f32| (v.p3(vec2(x, y), z), z);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let sun_right = th.sun_at.0 > 0.5;
    let mut push = |pts, c: Color| out.push(Face { key, pts, cols: [c; 4] });
    if x0 > v.cam.x && open.left {
        push([q(x0, y0, z0), q(x0, y0, z1), q(x0, y1, z1), q(x0, y1, z0)], side_shade(front, th, !sun_right));
    } else if x1 < v.cam.x && open.right {
        push([q(x1, y0, z0), q(x1, y0, z1), q(x1, y1, z1), q(x1, y1, z0)], side_shade(front, th, sun_right));
    }
    if y0 > v.cam.y && open.top {
        push([q(x0, y0, z0), q(x1, y0, z0), q(x1, y0, z1), q(x0, y0, z1)], roof_shade(front));
    } else if y1 < v.cam.y && open.bottom {
        push([q(x0, y1, z0), q(x1, y1, z0), q(x1, y1, z1), q(x0, y1, z1)], darken(front, 0.3));
    }
    push([q(x0, y0, z0), q(x1, y0, z0), q(x1, y1, z0), q(x0, y1, z0)], front);
}

/// Spike beds (rails and rows of spikes across the roof's depth) as faces, so
/// they sort in with the buildings and anything nearer hides them.
fn spike_faces(v: &View, g: &Game, th: &Theme, out: &mut Vec<Face>) {
    let lvl = &g.level;
    let (x0, x1, y0, y1) = visible_cells(v, lvl);
    let metal = mix(th.shadow, th.facade, 0.35);
    let backs = interior_cells(g);
    // The cell above counts too: a floor block under a room stops at its wall.
    let depth_at = |x: i32, y: i32| if backs.contains(&(x, y)) || backs.contains(&(x, y - 1)) { INTERIOR_DEPTH } else { BUILDING_DEPTH };
    for y in y0..=y1 {
        for x in x0..=x1 {
            if lvl.tile(x, y) != Tile::Spike {
                continue;
            }
            let d = depth_at(x, y);
            let c = vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
            let ph = hash(x, y, 5) * TAU;
            let pulse = 0.75 + 0.25 * (v.t * 5.0 + ph).sin();
            if let Some(up) = spike_dir(lvl, x, y) {
                {
                    let side = vec2(-up.y, up.x);
                    let base = c - up * (TILE / 2.0);
                    // The spikes cover the mounting surface's whole depth, but rows
                    // further back only show when that surface faces us.
                    let faces_eye = (v.cam - base).dot(up) > 0.0;
                    let rows: &[f32] = if faces_eye { &[0.85, 0.6, 0.35, 0.1] } else { &[0.1] };
                    glow(v.p(base + up * 10.0), v.s(22.0), alpha(th.hazard, 0.3 * pulse));
                    // Mounting rail, the full depth of the roof.
                    let r0 = base - side * 16.0;
                    let r1 = base + side * 16.0 + up * 4.0;
                    let rail = Rect::new(r0.x.min(r1.x), r0.y.min(r1.y), (r1.x - r0.x).abs(), (r1.y - r0.y).abs());
                    // Neighbouring spikes share one continuous rail.
                    let same = |dx: i32, dy: i32| lvl.tile(x + dx, y + dy) == Tile::Spike && spike_dir(lvl, x + dx, y + dy) == Some(up);
                    let open = if up.x == 0.0 {
                        Open { left: !same(-1, 0), right: !same(1, 0), ..OPEN }
                    } else {
                        Open { top: !same(0, -1), bottom: !same(0, 1), ..OPEN }
                    };
                    // Rail first, then rows back to front (deeper rows sort earlier).
                    let k0 = (c - v.cam).length();
                    box_faces(v, th, rail, 0.0, d, metal, open, k0 + 1.0, out);
                    for &f in rows {
                        let z = d * f;
                        let q = |w: Vec2| (v.p3(w, z), z);
                        for k in 0..3 {
                            let m = base + side * ((k as f32 - 1.0) * 10.0) + up * 4.0;
                            let tip = m + up * 16.0;
                            let (bl, br) = (m - side * 5.0, m + side * 5.0);
                            let key = k0 + z / 1000.0;
                            let (lit, dark) = (lighten(th.hazard, 0.25), darken(th.hazard, 0.15));
                            out.push(Face { key, pts: [q(bl), q(m), q(tip), q(tip)], cols: [lit; 4] });
                            out.push(Face { key, pts: [q(m), q(br), q(tip), q(tip)], cols: [dark; 4] });
                        }
                    }
                }
            }
        }
    }
}

fn grass_color(th: &Theme) -> Color {
    mix(Color::from_hex(0x5cc23e), th.shadow, 0.55 * th.lights)
}

/// A row of `n` grass blades along the floor under decor cell (x, y), as world
/// triangles (base, base, tip) with their colours.
#[allow(clippy::too_many_arguments)]
fn grass_blades(v: &View, x: i32, y: i32, z: f32, n: i32, tall: f32, grass: Color, mut f: impl FnMut([Vec2; 3], Color)) {
    let (wx, fy) = (x as f32 * TILE, (y + 1) as f32 * TILE);
    let h = |k: u32| hash(x, y, k);
    for k in 0..n {
        let bx = wx + (k as f32 + 0.5) * TILE / n as f32 + (h(k as u32 + (z as u32)) - 0.5) * 3.0;
        let ht = tall * (0.6 + 0.6 * h(k as u32 + 40));
        let sway = (v.t * 2.0 + bx * 0.13).sin() * 1.2;
        let shade = if k % 2 == 0 { grass } else { lighten(grass, 0.15) };
        f([vec2(bx - 1.6, fy), vec2(bx + 1.6, fy), vec2(bx + sway, fy - ht)], shade);
    }
}

/// Grass turf across the roof's depth, with tufts further back, as faces so it
/// sorts in with the buildings and anything nearer hides it. The front row of
/// blades is drawn with the front decorations.
fn grass_faces(v: &View, g: &Game, th: &Theme, out: &mut Vec<Face>) {
    // Turf reaches back toward the centre of view, so look past the screen edges.
    let (min, max) = bounds_at(v, BUILDING_DEPTH, TILE);
    let backs = interior_cells(g);
    let grass = grass_color(th);
    let q = |p: Vec2, z: f32| (v.p3(p, z), z);
    for d in g.level.decor.iter().filter(|d| d.kind == DecorKind::Grass) {
        let w = vec2(d.x as f32 * TILE, d.y as f32 * TILE);
        let fy = w.y + TILE;
        // Only seen from above.
        if fy <= v.cam.y || w.x > max.x || w.x + TILE < min.x || w.y > max.y || fy < min.y {
            continue;
        }
        let depth = if backs.contains(&(d.x, d.y)) { INTERIOR_DEPTH } else { BUILDING_DEPTH };
        // Just after the roof it lies on (same key), then the tufts, deepest first.
        let k0 = (vec2(w.x + TILE / 2.0, fy) - v.cam).length() - 0.02;
        let (near, far) = (lighten(grass, 0.1), darken(grass, 0.1));
        out.push(Face {
            key: k0,
            pts: [q(vec2(w.x, fy), 0.0), q(vec2(w.x + TILE, fy), 0.0), q(vec2(w.x + TILE, fy), depth), q(vec2(w.x, fy), depth)],
            cols: [near, near, far, far],
        });
        for f in [0.22, 0.48, 0.74] {
            let z = depth * f;
            grass_blades(v, d.x, d.y, z, 3, 5.0, grass, |p, c| {
                out.push(Face { key: k0 - 0.01 + z / 1e5, pts: [q(p[0], z), q(p[1], z), q(p[2], z), q(p[2], z)], cols: [c; 4] });
            });
        }
    }
}

/// Free-floating spike mines (mounted spikes are drawn with the buildings).
fn draw_spikes(v: &View, g: &Game, th: &Theme) {
    let lvl = &g.level;
    let (x0, x1, y0, y1) = visible_cells(v, lvl);
    let metal = mix(th.shadow, th.facade, 0.35);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if lvl.tile(x, y) != Tile::Spike {
                continue;
            }
            let c = vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
            let ph = hash(x, y, 5) * TAU;
            let pulse = 0.75 + 0.25 * (v.t * 5.0 + ph).sin();
            match spike_dir(lvl, x, y) {
                Some(_) => {}
                None => {
                    // Free-floating mine.
                    let z = 20.0;
                    let sc = v.p3(c, z);
                    let k = depth_scale(z) * v.zoom;
                    glow(sc, 24.0 * k, alpha(th.hazard, 0.3 * pulse));
                    let spin = v.t * 1.5 + ph;
                    for i in 0..8 {
                        let dir = Vec2::from_angle(spin + i as f32 / 8.0 * TAU);
                        let n = vec2(-dir.y, dir.x);
                        draw_triangle(sc + (dir * 6.0 + n * 3.5) * k, sc + (dir * 6.0 - n * 3.5) * k, sc + dir * 15.0 * k, th.hazard);
                    }
                    gloss_ball(sc, 7.5 * k, metal, 1.0);
                    draw_circle(sc.x, sc.y, 2.5 * k, alpha(lighten(th.hazard, 0.3), pulse));
                }
            }
        }
    }
}

fn draw_coins(v: &View, g: &Game) {
    let (min, max) = v.world_bounds(TILE);
    for (i, n) in g.nutrients.iter().enumerate() {
        if n.taken {
            continue;
        }
        let bob = (v.t * 3.0 + i as f32 * 0.7).sin() * 3.0;
        let c = n.home + n.off + vec2(0.0, bob);
        if c.x < min.x || c.x > max.x || c.y < min.y || c.y > max.y {
            continue;
        }
        ball3(v, c, 16.0, 8.0, pickup(), 20.0, 0.35);
    }
}

/// Points of a ring around `c` (depth `z`) spanned by the 3D axes `u` and `w`
/// (x, y on the play plane, z into the screen), with each point's depth offset.
fn ring3_pts(v: &View, c: Vec2, z: f32, u: Vec3, w: Vec3) -> Vec<(Vec2, f32)> {
    (0..=32)
        .map(|i| {
            let a = i as f32 / 32.0 * TAU;
            let o = u * a.cos() + w * a.sin();
            (v.p3(c + vec2(o.x, o.y), z + o.z), o.z)
        })
        .collect()
}

/// Half of a ring: `front` picks the half nearer the eye, so something can be
/// drawn between the two halves.
#[allow(clippy::too_many_arguments)]
fn ring3(v: &View, c: Vec2, z: f32, u: Vec3, w: Vec3, front: bool, thickness: f32, color: Color) {
    for s in ring3_pts(v, c, z, u, w).windows(2) {
        let ((a, za), (b, zb)) = (s[0], s[1]);
        if ((za + zb) < 0.0) == front {
            draw_line(a.x, a.y, b.x, b.y, thickness, color);
        }
    }
}

/// A flat ring tilted up from horizontal by `tilt` (0..1) and spun by `spin`.
fn orbit_axes(r: f32, tilt: f32, spin: f32) -> (Vec3, Vec3) {
    let (s, c) = spin.sin_cos();
    (vec3(c, 0.0, s) * r, vec3(-s, tilt, c) * r)
}

fn draw_dash_orbs(v: &View, g: &Game) {
    let col = orb();
    let z = 16.0;
    for (i, o) in g.dash_orbs.iter().enumerate() {
        let c = o.pos + vec2(0.0, (v.t * 2.2 + i as f32).sin() * 2.5);
        let k = depth_scale(z) * v.zoom;
        let sc = v.p3(c, z);
        if o.regrow > 0.0 {
            // Regrowing: a faint ring that fills in as the orb comes back.
            let progress = 1.0 - o.regrow / ORB_REGROW_TIME;
            let dots = 20;
            for d in 0..dots {
                let a = d as f32 / dots as f32 * TAU - TAU / 4.0;
                let lit = (d as f32 / dots as f32) < progress;
                let p = sc + Vec2::from_angle(a) * 9.0 * k;
                draw_circle(p.x, p.y, k, alpha(col, if lit { 0.7 } else { 0.2 }));
            }
            continue;
        }
        let pulse = 1.0 + (v.t * 5.0).sin() * 0.06 + o.wobble * 0.25 * (v.t * 25.0).sin();
        let r = 9.0 * pulse;
        let ring_col = alpha(lighten(col, 0.3), 0.85);
        let (u, w) = orbit_axes(r * 1.7, 0.35, v.t * 1.4);
        ring3(v, c, z, u, w, false, 1.3 * k, ring_col);
        glow(sc, r * 2.6 * k, alpha(col, 0.4));
        gloss_ball(sc, r * k, col, 1.0);
        // Lightning mark: dashes recharge here.
        let zig = [vec2(1.5, -5.0), vec2(-2.0, 0.5), vec2(1.0, 0.5), vec2(-1.5, 5.0)].map(|p| sc + p * k * pulse);
        for w in zig.windows(2) {
            draw_line(w[0].x, w[0].y, w[1].x, w[1].y, 1.6 * k, alpha(darken(col, 0.45), 0.9));
        }
        ring3(v, c, z, u, w, true, 1.3 * k, ring_col);
    }
}

fn draw_checkpoints(v: &View, g: &Game, th: &Theme) {
    let pole = mix(th.trim, th.shadow, 0.45);
    let z = 24.0;
    for cp in &g.checkpoints {
        let k = depth_scale(z) * v.zoom;
        let sway = cp.wobble * (v.t * 20.0).sin() * 3.0;
        box3(v, th, Rect::new(cp.pos.x - 8.0, cp.pos.y - 4.0, 16.0, 4.0), z - 8.0, z + 8.0, pole, OPEN);
        let base = v.p3(cp.pos - vec2(0.0, 4.0), z);
        let top = v.p3(cp.pos + vec2(sway, -30.0), z);
        draw_line(base.x, base.y, top.x, top.y, 2.5 * k, pole);
        let lamp = cp.pos + vec2(sway, -36.0);
        let lc = v.p3(lamp, z);
        if cp.active {
            let pulse = (v.t * 3.0).sin() * 0.5 + 0.5;
            glow(lc, (30.0 + pulse * 6.0) * k, alpha(th.player, 0.5));
            let ring_t = (v.t * 0.8).fract();
            let (u, w) = orbit_axes(8.0 + ring_t * 22.0, 0.0, 0.0);
            let col = alpha(th.player, 1.0 - ring_t);
            ring3(v, lamp, z, u, w, false, 1.5 * k, col);
            gloss_ball(lc, 6.5 * k, th.player, 1.0);
            ring3(v, lamp, z, u, w, true, 1.5 * k, col);
        } else {
            gloss_ball(lc, 6.5 * k, mix(th.trim, th.shadow, 0.3), 1.0);
        }
    }
}

/// The goal: a portal ring spinning about the vertical.
fn draw_goal(v: &View, g: &Game) {
    let c = g.goal_center();
    let z = 30.0;
    let k = depth_scale(z) * v.zoom;
    let sc = v.p3(c, z);
    let r = 28.0 + (v.t * 2.0).sin() * 1.5 + g.goal_wobble * 8.0 * (v.t * 14.0).sin();
    let spin = (v.t * 0.7).sin() * 0.9;
    glow(sc, r * 2.4 * k, alpha(pickup(), 0.35));
    // An upright ring turned about the vertical: the portal's surface, then the
    // ring's back and front halves.
    let (u, w) = (vec3(spin.cos(), 0.0, spin.sin()) * r, vec3(0.0, r, 0.0));
    let disc: Vec<Vec2> = ring3_pts(v, c, z, u, w).into_iter().take(32).map(|(p, _)| p).collect();
    fan(&disc, |p| alpha(mix(WHITE, pickup(), ((p - sc).length() / (r * k)).min(1.0)), 0.5));
    for front in [false, true] {
        ring3(v, c, z, u, w, front, 7.0 * k, darken(pickup(), 0.2));
        ring3(v, c, z, u, w, front, 4.0 * k, pickup());
    }
    for i in 0..10 {
        let a = v.t * 1.2 + i as f32 / 10.0 * TAU;
        let (p, q) = (sc + Vec2::from_angle(a) * r * 1.3 * k, sc + Vec2::from_angle(a + 0.25) * r * 1.3 * k);
        draw_line(p.x, p.y, q.x, q.y, 2.5 * k, alpha(pickup(), 0.8));
    }
}

/// With `out`, adds the pads' boxes to the depth-sorted faces; without, draws
/// the details on their fronts (after the buildings).
fn draw_bounce_blocks(v: &View, g: &Game, th: &Theme, mut out: Option<&mut Vec<Face>>) {
    let (x0, x1, y0, y1) = visible_cells(v, &g.level);
    let base = mix(th.shadow, th.facade, 0.35);
    let backs = interior_cells(g);
    // The cell above counts too: a floor block under a room stops at its wall.
    let depth_at = |x: i32, y: i32| if backs.contains(&(x, y)) || backs.contains(&(x, y - 1)) { INTERIOR_DEPTH } else { BUILDING_DEPTH };
    for y in y0..=y1 {
        for x in x0..=x1 {
            if g.level.tile(x, y) != Tile::Bounce {
                continue;
            }
            let d = depth_at(x, y);
            let a = g.bounce_anim.get(&(x, y)).copied().unwrap_or(0.0);
            // Squish down, then spring back.
            let squish = (a * PI * 3.0).sin() * a * 7.0;
            let w = vec2(x as f32 * TILE, y as f32 * TILE);
            let open = tile_open(&g.level, x, y);
            // The pad: a slab across the block's depth that squashes on landing.
            let top = w.y + 1.0 + squish;
            let bulge = a * 2.0;
            let pad = Rect::new(w.x + 2.0 - bulge, top, TILE - 4.0 + bulge * 2.0, 8.0 + squish * 0.3 - squish);
            if let Some(out) = out.as_deref_mut() {
                let key = (w + Vec2::splat(TILE / 2.0) - v.cam).length();
                box_faces(v, th, Rect::new(w.x, w.y + 8.0, TILE, TILE - 8.0), 0.0, d, base, Open { top: true, ..open }, key + 0.5, out);
                box_faces(v, th, pad, 4.0, d - 4.0, pickup(), Open { top: true, ..open }, key, out);
                continue;
            }
            // Up chevrons, brighter just after a launch.
            for k in 0..2 {
                let cy = w.y + 19.0 + k as f32 * 6.0;
                let (l, m, r) = (v.p(vec2(w.x + 10.0, cy + 3.0)), v.p(vec2(w.x + 16.0, cy - 1.0)), v.p(vec2(w.x + 22.0, cy + 3.0)));
                let c = alpha(pickup(), 0.6 - k as f32 * 0.2 + a * 0.4);
                draw_line(l.x, l.y, m.x, m.y, v.s(2.0), c);
                draw_line(m.x, m.y, r.x, r.y, v.s(2.0), c);
            }
            if a > 0.01 {
                glow(v.p3(vec2(w.x + TILE / 2.0, top), d / 2.0), v.s(30.0), alpha(pickup(), 0.5 * a));
            }
            let (p, q) = (v.p3(vec2(pad.x + 3.0, top + 1.0), 4.0), v.p3(vec2(pad.x + pad.w - 3.0, top + 1.0), 4.0));
            draw_line(p.x, p.y, q.x, q.y, v.s(1.2), alpha(WHITE, 0.8));
        }
    }
}

/// Crumble blocks: glass sections of roof that crack and shatter.
/// With `out`, adds the glass boxes to the depth-sorted faces; without, draws
/// the cracks and frames on their fronts (after the buildings).
fn draw_crumble_blocks(v: &View, g: &Game, th: &Theme, mut out: Option<&mut Vec<Face>>) {
    let (min, max) = v.world_bounds(TILE);
    let frame = mix(th.trim, th.shadow, 0.25);
    let backs = interior_cells(g);
    // The cell above counts too: a floor block under a room stops at its wall.
    let depth_at = |x: i32, y: i32| if backs.contains(&(x, y)) || backs.contains(&(x, y - 1)) { INTERIOR_DEPTH } else { BUILDING_DEPTH };
    for c in &g.crumbles {
        let w = vec2(c.x as f32 * TILE, c.y as f32 * TILE);
        if w.x > max.x || w.x + TILE < min.x || w.y > max.y || w.y + TILE < min.y {
            continue;
        }
        let jitter = match c.state {
            CrumbleState::Solid => Vec2::ZERO,
            CrumbleState::Shaking(t) => {
                let k = 1.0 - t / CRUMBLE_DELAY;
                let j = 1.0 + k * 2.0;
                vec2((v.t * 90.0).sin() * j, (v.t * 70.0).cos() * j * 0.5)
            }
            CrumbleState::Gone(_) if out.is_some() => continue,
            CrumbleState::Gone(t) => {
                // Faint frame, filling back in as it regrows.
                let p = v.p(w);
                let s = v.s(TILE);
                let grow = 1.0 - t / CRUMBLE_REGROW;
                draw_rectangle_lines(p.x + v.s(2.0), p.y + v.s(2.0), s - v.s(4.0), s - v.s(4.0), 1.0, alpha(frame, 0.35));
                let inner = s * grow * 0.6;
                draw_rectangle(p.x + (s - inner) / 2.0, p.y + (s - inner) / 2.0, inner, inner, alpha(th.glass, 0.2 * grow));
                continue;
            }
        };
        let w = w + jitter;
        let d = depth_at(c.x, c.y);
        if let Some(out) = out.as_deref_mut() {
            let key = (w + Vec2::splat(TILE / 2.0) - v.cam).length();
            let glass = alpha(mix(th.glass, th.sky_bottom, 0.3), 0.7);
            box_faces(v, th, Rect::new(w.x + 1.0, w.y + 1.0, TILE - 2.0, TILE - 2.0), 0.0, d, glass, tile_open(&g.level, c.x, c.y), key, out);
            continue;
        }
        let p = v.p(w);
        let s = v.s(TILE);
        let i = v.s(1.5);
        draw_triangle(p + vec2(i, i), p + vec2(s * 0.55, i), p + vec2(i, s * 0.55), alpha(WHITE, 0.22));
        // Cracks (varied per block), more of them once it starts to go.
        let h = hash(c.x, c.y, 31);
        let mid = v.p(w + vec2(10.0 + h * 12.0, 12.0 + h * 8.0));
        let cracks: &[(f32, f32)] = if matches!(c.state, CrumbleState::Shaking(_)) {
            &[(-1.0, -1.0), (1.0, -0.6), (0.3, 1.0), (-1.0, 0.5), (1.0, 0.8)]
        } else {
            &[(-1.0, -1.0), (1.0, -0.6), (0.3, 1.0)]
        };
        for (dx, dy) in cracks {
            let end = mid + vec2(dx * v.s(10.0), dy * v.s(10.0));
            draw_line(mid.x, mid.y, end.x, end.y, v.s(1.1), alpha(WHITE, 0.75));
        }
        draw_rectangle_lines(p.x + i, p.y + i, s - 2.0 * i, s - 2.0 * i, v.s(2.0), frame);
    }
}

/// Deadly liquid: a glowing energy pool.
fn draw_acid(v: &View, g: &Game) {
    let (x0, x1, y0, y1) = visible_cells(v, &g.level);
    let deep_col = darken(hazard(), 0.55);
    // Deeper tiles are plain fills; the top row is drawn by its rippling strip.
    for y in y0..=y1 {
        for x in x0..=x1 {
            if g.level.tile(x, y) == Tile::Acid && g.level.tile(x, y - 1) == Tile::Acid {
                let p = v.p(vec2(x as f32 * TILE, y as f32 * TILE));
                draw_rectangle(p.x - 0.5, p.y - 0.5, v.s(TILE) + 1.0, v.s(TILE) + 1.0, deep_col);
            }
        }
    }
    let (min, max) = v.world_bounds(TILE);
    for s in g.surfaces.strips.iter().filter(|s| s.kind == SurfaceKind::Acid) {
        let top = s.top();
        if top < min.y || top > max.y || s.x1 < min.x || s.x0 > max.x {
            continue;
        }
        let a = s.x0.max(min.x);
        let b = s.x1.min(max.x);
        let n = ((b - a) / 4.0).ceil().max(1.0) as usize;
        let xs: Vec<f32> = (0..=n).map(|i| a + (b - a) * i as f32 / n as f32).collect();
        let surf = |x: f32| vec2(x, top + 6.0 + s.offset_at(x) + (x * 0.08 + v.t * 3.0).sin() * 1.2);
        for w in xs.windows(2) {
            let (p0, p1) = (v.p(surf(w[0])), v.p(surf(w[1])));
            let (q0, q1) = (v.p(vec2(w[0], top + TILE)), v.p(vec2(w[1], top + TILE)));
            grad_quad([p0, p1, q1, q0], [alpha(hazard(), 0.9), alpha(hazard(), 0.9), deep_col, deep_col]);
        }
        let mid = v.p(vec2((a + b) / 2.0, top + 6.0));
        glow(mid, v.s(((b - a) / 2.0).min(120.0) + 20.0), alpha(hazard(), 0.18));
        for w in xs.windows(2) {
            let (p, q) = (v.p(surf(w[0])), v.p(surf(w[1])));
            draw_line(p.x, p.y, q.x, q.y, v.s(2.0), lighten(hazard(), 0.5));
        }
    }
}

/// Jump bubbles: clear glass bubbles holding an extra jump.
fn draw_bubbles(v: &View, g: &Game) {
    for (i, b) in g.bubbles.iter().enumerate() {
        let c = b.pos + vec2((v.t * 1.3 + i as f32).sin() * 2.0, (v.t * 2.0 + i as f32).sin() * 3.0);
        let sc = v.p3(c, 16.0);
        if b.regrow > 0.0 {
            let progress = 1.0 - b.regrow / BUBBLE_REGROW_TIME;
            let dots = 16;
            for k in 0..dots {
                let a = k as f32 / dots as f32 * TAU - TAU / 4.0;
                let lit = (k as f32 / dots as f32) < progress;
                let p = sc + Vec2::from_angle(a) * v.s(9.0);
                draw_circle(p.x, p.y, v.s(0.9), alpha(WHITE, if lit { 0.7 } else { 0.2 }));
            }
            continue;
        }
        let r = v.s(10.0 * (1.0 + (v.t * 3.0).sin() * 0.04 + b.wobble * 0.2 * (v.t * 22.0).sin())) * depth_scale(16.0);
        fan(&ellipse_pts(sc, r, r, 20), |p| alpha(mix(WHITE, player(), 0.4), 0.08 + 0.25 * ((p - sc).length() / r)));
        draw_circle_lines(sc.x, sc.y, r, v.s(1.3), alpha(WHITE, 0.8));
        fill_poly(&ellipse_pts(sc + vec2(-r * 0.3, -r * 0.5), r * 0.32, r * 0.16, 10), alpha(WHITE, 0.85));
        // Up chevron: "extra jump".
        let (l, m, rr) = (sc + vec2(-r * 0.35, r * 0.25), sc + vec2(0.0, -r * 0.12), sc + vec2(r * 0.35, r * 0.25));
        draw_line(l.x, l.y, m.x, m.y, v.s(1.8), player());
        draw_line(m.x, m.y, rr.x, rr.y, v.s(1.8), player());
    }
}

fn draw_enemies(v: &View, g: &Game, alpha_t: f32) {
    let (min, max) = v.world_bounds(TILE * 2.0);
    let shell = mix(deep(), terrain(), 0.25);
    for e in &g.enemies {
        if !e.alive && e.death_t <= 0.0 {
            continue;
        }
        let pos = e.prev_pos.lerp(e.pos, alpha_t);
        if pos.x < min.x || pos.x > max.x || pos.y < min.y || pos.y > max.y {
            continue;
        }
        // Squashed flat and fading after a stomp.
        let k = if e.alive { 1.0 } else { e.death_t / DEATH_TIME };
        let (sx, sy, a) = (1.0 + (1.0 - k) * 0.7, 0.25 + 0.75 * k, k);
        let size = e.size();
        let eye = alpha(hazard(), a);
        match e.kind {
            EnemyKind::Crawler => {
                // Security bot: a dark dome on treads with a red scanner slit.
                let base = pos + vec2(0.0, size.y / 2.0);
                let rx = size.x / 2.0 * sx;
                let ry = size.y * sy;
                let dome_w: Vec<Vec2> =
                    (0..=16).map(|i| base + vec2(0.0, -3.0) + Vec2::from_angle(PI + i as f32 / 16.0 * PI) * vec2(rx, ry - 3.0)).collect();
                extrude_sides(v, &dome_w, 0.0, 16.0, alpha(darken(shell, 0.25), a));
                let dome: Vec<Vec2> = dome_w.iter().map(|&p| v.p(p)).collect();
                grad_poly(&dome, alpha(lighten(shell, 0.25), a), alpha(shell, a));
                round_rect(v.p(base).x - v.s(rx), v.p(base).y - v.s(5.0), v.s(rx * 2.0), v.s(5.0), v.s(2.5), alpha(darken(shell, 0.3), a), alpha(darken(shell, 0.5), a));
                for f in [-0.6, 0.0, 0.6] {
                    let roll = if e.alive { e.t * 8.0 } else { 0.0 };
                    let wp = v.p(base + vec2(f * rx, -2.5));
                    draw_circle(wp.x, wp.y, v.s(1.6), alpha(darken(shell, 0.1), a));
                    let spoke = wp + Vec2::from_angle(roll + f) * v.s(1.5);
                    draw_line(wp.x, wp.y, spoke.x, spoke.y, 1.0, alpha(BLACK, 0.4 * a));
                }
                let slit = v.p(base + vec2(e.dir * rx * 0.25, -ry * 0.55));
                glow(slit, v.s(10.0), alpha(hazard(), 0.5 * a));
                draw_rectangle(slit.x - v.s(5.0), slit.y - v.s(1.3), v.s(10.0), v.s(2.6), eye);
                fill_poly(&ellipse_pts(v.p(base + vec2(-rx * 0.35, -ry * 0.8)), v.s(rx * 0.3), v.s(1.5), 10), alpha(WHITE, 0.35 * a));
            }
            EnemyKind::Floater => {
                // Drone: round body, two rotor arms.
                let cw = pos - vec2(0.0, 2.0);
                let c = v.p(cw);
                let rx = v.s(size.x / 2.0 * sx);
                let ry = v.s(size.y / 2.0 * sy);
                // Four rotor arms: the back pair first, the front pair after the body.
                let rotors = |front: bool| {
                    for side in [-1.0, 1.0] {
                        let z = if front { -6.0 } else { 18.0 };
                        let hub = v.p3(cw, z);
                        let tip = v.p3(cw + vec2(side * size.x / 2.0 * sx * 1.35, -size.y / 2.0 * sy * 0.55), z);
                        draw_line(hub.x, hub.y - ry * 0.2, tip.x, tip.y, v.s(2.0), alpha(shell, a));
                        let blur = (e.t * 40.0).sin().abs();
                        fill_poly(&ellipse_pts(tip - vec2(0.0, v.s(1.5)), v.s(8.0) * (0.6 + 0.4 * blur), v.s(1.4), 12), alpha(WHITE, 0.45 * a));
                    }
                };
                rotors(false);
                fan(&ellipse_pts(c, rx * 0.85, ry * 0.85, 20), |p| {
                    alpha(mix(lighten(shell, 0.3), shell, ((p.y - c.y + ry) / (2.0 * ry)).clamp(0.0, 1.0)), a)
                });
                let dir = (g.player.center().x - pos.x).signum();
                let ec = c + vec2(dir * rx * 0.3, 0.0);
                glow(ec, v.s(12.0), alpha(hazard(), 0.5 * a));
                draw_circle(ec.x, ec.y, v.s(3.2), eye);
                draw_circle(ec.x - v.s(1.0), ec.y - v.s(1.0), v.s(1.0), alpha(WHITE, 0.8 * a));
                fill_poly(&ellipse_pts(c + vec2(-rx * 0.3, -ry * 0.5), rx * 0.3, ry * 0.15, 10), alpha(WHITE, 0.4 * a));
                rotors(true);
            }
        }
    }
}

// ---------- Player ----------

fn draw_player(v: &View, g: &Game, alpha_t: f32) {
    for gh in &g.ghosts {
        let a = gh.life / GHOST_LIFE * 0.45;
        draw_robot(v, &gh.pose, false, 0, Some(alpha(player(), a)));
    }
    let p = &g.player;
    if p.dead || g.preview {
        return;
    }
    draw_robot(v, &g.pose(alpha_t), p.has_dash, p.air_jumps, None);
}

/// Maps the robot's local space (origin at the feet, x toward its facing,
/// y down) onto the world: rotated about a pivot and squashed.
struct RobotFrame {
    anchor: Vec2,
    pivot: Vec2,
    ang: f32,
    scale: Vec2,
    f: f32,
}

impl RobotFrame {
    fn w(&self, l: Vec2) -> Vec2 {
        self.anchor + rot((vec2(l.x * self.f, l.y) - self.pivot) * self.scale, self.ang)
    }
}

/// Height of the robot's feet.
const FOOT_H: f32 = 5.0;

/// Where one foot is (sole centre, robot space) and how it's tilted (+ = toe down).
/// Foot 0 is the far one.
fn foot_pose(pose: &Pose, i: usize, t: f32, stride: f32, lift: f32, running: bool) -> (Vec2, f32) {
    let side = if i == 0 { -1.0 } else { 1.0 };
    let lerp = |a: (Vec2, f32), b: (Vec2, f32), k: f32| (a.0.lerp(b.0, k), a.1 + (b.1 - a.1) * k);
    match pose.state {
        Move::Run | Move::WallRun if running => {
            // Each foot swings forward in the air (toe up), then plants and pushes
            // back (heel lifting as it leaves the ground).
            let (s, c) = (pose.stride + i as f32 * PI).sin_cos();
            let up = c.max(0.0);
            let ang = if c > 0.0 { -0.5 * up } else { 0.45 * (-s).max(0.0) * (-c) };
            (vec2(s * stride, -up * lift), ang)
        }
        Move::Slide => {
            if i == 1 {
                (vec2(8.5, 0.0), -0.25)
            } else {
                (vec2(-2.0, -1.5), 0.35)
            }
        }
        Move::Crouch => (vec2(side * 4.5, 0.0), 0.0),
        Move::Dash => (vec2(-6.0 + side * 2.0, -4.0 - i as f32 * 1.5), 0.7),
        Move::Air => {
            // Tucked while rising, dangling and kicking while falling.
            let rising = if i == 1 { (vec2(4.0, -5.0), -0.5) } else { (vec2(-4.5, -1.5), 0.5) };
            let kick = (t * 16.0 + i as f32 * PI).sin();
            let falling = if i == 1 { (vec2(3.0 + kick * 1.5, 2.5), 0.3 + kick * 0.2) } else { (vec2(-3.5 - kick * 1.5, 1.5), -0.25 - kick * 0.2) };
            lerp(rising, falling, ((pose.vel.y / 450.0).clamp(-1.0, 1.0) + 1.0) / 2.0)
        }
        // Both soles flat against the wall behind (robot faces away from it).
        Move::WallSlide => (vec2(-9.0, if i == 1 { -11.0 } else { -3.5 }), PI / 2.0),
        _ => (vec2(side * 2.5, 0.0), 0.0),
    }
}

/// The player: a little box robot on big running feet. `ghost` draws a flat-coloured afterimage instead.
fn draw_robot(v: &View, pose: &Pose, has_dash: bool, air_jumps: u8, ghost: Option<Color>) {
    let th = theme();
    let t = tuning::get();
    let speed = pose.vel.length();
    let fast = ((speed - t.run_speed) / (t.top_speed - t.run_speed).max(1.0)).clamp(0.0, 1.0);
    let amp = (speed / t.run_speed).min(1.0);
    let running = matches!(pose.state, Move::Run | Move::WallRun) && speed > 30.0;

    // Body box (half width, height) and leg length.
    let (bw, bh, leg) = match pose.state {
        _ if pose.crouching && pose.state != Move::Slide => (10.5, 8.0, 0.5),
        Move::Slide => (10.0, 8.5, 0.5),
        _ => (9.0, 14.0, 4.0),
    };
    let stride = 4.0 + 5.0 * amp + 2.0 * fast;
    let lift = 2.5 + 3.5 * amp;
    // The body dips each time a foot plants; idle, it breathes.
    let bob = if running {
        (2.0 * pose.stride).cos() * 1.3 * amp
    } else if pose.state == Move::Run {
        (v.t * 2.5).sin() * 0.5
    } else {
        0.0
    };
    let bottom = -(FOOT_H + leg) + bob;
    let cy = bottom - bh / 2.0;

    // Facing and tilt for each move.
    let mut f = if pose.facing < 0.0 { -1.0 } else { 1.0 };
    let slope = pose.gradient.atan() * 0.6;
    let mut scale = pose.scale;
    let fall = (pose.vel.y / 450.0).clamp(-1.0, 1.0);
    let (ang, about_feet) = match pose.state {
        Move::Run => (slope + f * (0.05 + 0.15 * fast) * amp, true),
        Move::Slide => (slope - f * 0.3, true),
        Move::Crouch => (slope, true),
        Move::WallRun => {
            // Turned onto the wall, running up it.
            f = pose.wall_dir as f32;
            (-(pose.wall_dir as f32) * PI / 2.0, false)
        }
        Move::WallSlide => {
            f = -(pose.wall_dir as f32);
            ((pose.wall_dir as f32) * 0.12, false)
        }
        Move::Dash => {
            scale *= vec2(1.2, 0.85);
            (f * (pose.vel.y.atan2(pose.vel.x.abs()) * 0.6), false)
        }
        Move::Air => (f * (0.06 * (pose.vel.x.abs() / t.top_speed).min(1.0) - 0.12 * fall), false),
    };
    let pivot = if about_feet { Vec2::ZERO } else { vec2(0.0, cy) };
    let fr = RobotFrame { anchor: pose.feet + pivot, pivot, ang, scale, f };
    let sp = |l: Vec2| v.p(fr.w(l));
    let sp3 = |l: Vec2, z: f32| v.p3(fr.w(l), z);
    let k = v.zoom * (scale.x + scale.y) / 2.0;

    // Colours: a white shell lit by the sky, dark boots with glowing soles.
    let (shell_top, shell_bot, boot, joint) = match ghost {
        Some(c) => (c, c, c, c),
        None => {
            let bot = mix(Color::from_hex(0xb4c4d6), th.sky_bottom, 0.2);
            (mix(WHITE, th.sky_bottom, 0.06), bot, mix(th.deep, bot, 0.3), mix(bot, th.deep, 0.5))
        }
    };

    let draw_foot = |i: usize| {
        let (pos, fang) = foot_pose(pose, i, v.t, stride, lift, running);
        let (z0, z1) = if i == 0 { (ROBOT_DEPTH - 7.0, ROBOT_DEPTH - 1.0) } else { (1.0, 7.0) };
        let zc = (z0 + z1) / 2.0;
        let at = |p: Vec2| pos + rot(p, fang);
        // Leg from the hip to the top of the foot.
        let hip = vec2(if i == 0 { -2.0 } else { 2.0 }, bottom + 1.0);
        let (a, b) = (sp3(hip, zc), sp3(at(vec2(0.0, -FOOT_H + 1.0)), zc));
        draw_line(a.x, a.y, b.x, b.y, 3.0 * k, if i == 0 { darken(joint, 0.15) } else { joint });
        let outline: Vec<Vec2> = round_rect_pts(-4.5, -FOOT_H, 12.0, FOOT_H, 2.2).into_iter().map(at).collect();
        if ghost.is_none() {
            let world: Vec<Vec2> = outline.iter().map(|&p| fr.w(p)).collect();
            extrude_sides(v, &world, z0, z1, darken(boot, 0.3));
        }
        let front: Vec<Vec2> = outline.iter().map(|&p| sp3(p, z0)).collect();
        let shade = if i == 0 { 0.15 } else { 0.0 };
        grad_poly(&front, darken(lighten(boot, 0.2), shade), darken(boot, shade));
        if ghost.is_none() {
            let (a, b) = (sp3(at(vec2(-3.5, -0.8)), z0), sp3(at(vec2(6.5, -0.8)), z0));
            draw_line(a.x, a.y, b.x, b.y, 1.4 * k, alpha(th.player, if i == 0 { 0.6 } else { 0.95 }));
        }
    };

    // Stored air jump: a little bubble orbiting the body, passing behind and in front.
    let orbit = -v.t * 3.0;
    let bubble_z = ROBOT_DEPTH / 2.0 + orbit.sin() * 20.0;
    let draw_bubble = || {
        if air_jumps > 0 && ghost.is_none() {
            let o = v.p3(fr.w(vec2(0.0, cy)) + vec2(orbit.cos() * 20.0, 0.0), bubble_z);
            let r = 4.0 * k * depth_scale(bubble_z);
            draw_circle(o.x, o.y, r, alpha(WHITE, 0.15));
            draw_circle_lines(o.x, o.y, r, 1.2 * k, alpha(WHITE, 0.8));
        }
    };
    if bubble_z > ROBOT_DEPTH / 2.0 {
        draw_bubble();
    }

    // Back to front: far foot, the body's sides, thruster and antenna, near foot, then the body's face.
    draw_foot(0);
    let body_l = round_rect_pts(-bw, cy - bh / 2.0, bw * 2.0, bh, 3.5);
    if ghost.is_none() {
        let world: Vec<Vec2> = body_l.iter().map(|&l| fr.w(l)).collect();
        extrude_sides(v, &world, 0.0, ROBOT_DEPTH, mix(darken(shell_bot, 0.1), th.sky_top, 0.15));
    }
    let mid = ROBOT_DEPTH / 2.0;
    if pose.state == Move::Dash && ghost.is_none() {
        let flick = 1.0 + (v.t * 60.0).sin() * 0.2;
        let (a, b, tip) = (sp3(vec2(-bw, cy - 4.0), mid), sp3(vec2(-bw, cy + 4.0), mid), sp3(vec2(-bw - 14.0 * flick, cy), mid));
        glow((a + b) / 2.0, 14.0 * k, alpha(th.player, 0.6));
        draw_triangle(a, b, tip, alpha(th.player, 0.85));
        draw_triangle(a.lerp(b, 0.3), a.lerp(b, 0.7), a.lerp(tip, 0.55).lerp(b.lerp(tip, 0.55), 0.5), alpha(WHITE, 0.9));
    }
    // Antenna, with a tip that lags behind your motion. Its light shows a stored dash.
    let lag = (-pose.vel * 0.006).clamp(Vec2::splat(-4.0), Vec2::splat(4.0));
    let top = cy - bh / 2.0;
    let a_base = sp3(vec2(-4.0, top + 1.0), mid);
    let a_tip = sp3(vec2(-5.5, top - 6.0), mid) + lag * v.zoom;
    draw_line(a_base.x, a_base.y, a_tip.x, a_tip.y, 1.4 * k, joint);
    if ghost.is_none() {
        if has_dash {
            glow(a_tip, 9.0 * k, alpha(th.orb, 0.7));
            draw_circle(a_tip.x, a_tip.y, 2.4 * k, lighten(th.orb, 0.3));
        } else {
            draw_circle(a_tip.x, a_tip.y, 2.0 * k, shell_bot);
        }
    }
    draw_foot(1);

    // Body: a rounded box.
    let body: Vec<Vec2> = body_l.iter().map(|&l| sp(l)).collect();
    let (by0, by1) = (sp(vec2(0.0, top)), sp(vec2(0.0, top + bh)));
    let span = by1 - by0;
    fan(&body, |p| mix(shell_top, shell_bot, ((p - by0).dot(span) / span.length_squared()).clamp(0.0, 1.0)));
    if ghost.is_some() {
        return;
    }
    outline_poly(&body, 1.1 * k, alpha(th.deep, 0.45));
    // Panel seam and a status light on the back.
    let (s0, s1) = (sp(vec2(-bw + 1.5, cy + bh * 0.3)), sp(vec2(bw - 1.5, cy + bh * 0.3)));
    draw_line(s0.x, s0.y, s1.x, s1.y, 0.8 * k, alpha(th.deep, 0.25));
    let led = sp(vec2(-bw + 3.0, cy + bh * 0.3 + 2.2));
    draw_circle(led.x, led.y, 0.9 * k, th.player);

    // Screen face with glowing eyes on the front of the box.
    let visor: Vec<Vec2> = round_rect_pts(-1.5, cy - 4.5, bw + 0.5, 7.5, 2.5).into_iter().map(sp).collect();
    fill_poly(&visor, mix(th.deep, th.player, 0.12));
    let blink = (v.t % 3.7) < 0.1;
    let squint = match pose.state {
        Move::Dash | Move::Slide => 0.5,
        _ if pose.crouching => 0.5,
        Move::Air if fall > 0.5 => 1.25, // wide-eyed falling
        _ => 1.0 - 0.4 * fast,
    };
    let look = fall * 1.2;
    for ex in [2.2, 6.0] {
        let c = sp(vec2(ex, cy - 0.8 + look));
        glow(c, 7.0 * k, alpha(th.player, 0.45));
        let h = if blink { 0.4 } else { 2.0 * squint };
        let pts: Vec<Vec2> = ellipse_pts(Vec2::ZERO, 1.2, h, 12).into_iter().map(|p| c + rot(p * k, ang)).collect();
        fill_poly(&pts, lighten(th.player, 0.35));
    }
    // Gloss along the top edge.
    let hl: Vec<Vec2> = round_rect_pts(-bw + 2.0, top + 1.2, bw * 1.1, 1.8, 0.9).into_iter().map(sp).collect();
    fill_poly(&hl, alpha(WHITE, 0.85));

    if bubble_z <= ROBOT_DEPTH / 2.0 {
        draw_bubble();
    }
}

// ---------- Foreground ----------

/// Depths in front of the play plane (negative z) for the foreground layers.
const FG_CLOUD_Z: f32 = -260.0;
const FG_ROOF_Z: f32 = -330.0;
const FG_GIRDER_Z: f32 = -460.0;

/// World rect visible at depth `z` (min, max), with a margin.
fn bounds_at(v: &View, z: f32, margin: f32) -> (Vec2, Vec2) {
    let half = v.half / (v.zoom * depth_scale(z)) + Vec2::splat(margin);
    (v.cam - half, v.cam + half)
}

/// Close to the camera, in the same perspective as everything else: soft cloud
/// wisps, dark rooftops along the street, and the odd steel girder. Because
/// they're nearer than the level they slide past faster, following the camera.
fn draw_foreground(v: &View, th: &Theme) {
    draw_fg_clouds(v, th);
    draw_fg_roofs(v, th);
    draw_fg_girders(v, th);
}

/// Wisps of cloud drifting just in front of the camera, spread through the sky
/// in both directions so climbing feels as fast as running.
fn draw_fg_clouds(v: &View, th: &Theme) {
    let z = FG_CLOUD_Z;
    let (cw, chh) = (900.0, 640.0);
    let (min, max) = bounds_at(v, z, 300.0);
    // Clouds thin out below the rooftops.
    let floor = v.ground - 200.0;
    let col = mix(mix(WHITE, th.sun, 0.2), th.haze, 0.3 * th.lights);
    let k = depth_scale(z) * v.zoom;
    for j in (min.y / chh).floor() as i32..=(max.y / chh).ceil() as i32 {
        for i in (min.x / cw).floor() as i32 - 1..=(max.x / cw).ceil() as i32 {
            let h = |n: u32| hash(i, j, 400 + n);
            if h(0) > 0.22 {
                continue;
            }
            let drift = (v.t * (8.0 + 10.0 * h(1))) % cw;
            let c = vec2((i as f32 + h(2)) * cw + drift, (j as f32 + h(3)) * chh);
            if c.y > floor {
                continue;
            }
            let a = (0.16 + 0.1 * h(4)) * (1.0 - 0.5 * th.lights);
            for p in 0..5 {
                let off = vec2((p as f32 - 2.0) * 70.0 * (0.8 + 0.4 * hash(i, j * 7 + p, 410)), (hash(i, j * 7 + p, 411) - 0.5) * 40.0);
                let r = 90.0 + 70.0 * hash(i, j * 7 + p, 412);
                glow(v.p3(c + off, z), r * k, alpha(col, a));
            }
        }
    }
}

/// Dark rooftops between the camera and the level, standing on the street:
/// boxes with railings. They only peek in along the bottom of a level; climbing
/// leaves them behind.
fn draw_fg_roofs(v: &View, th: &Theme) {
    let z0 = FG_ROOF_Z;
    let z1 = z0 + 120.0;
    let col = alpha(th.shadow, 0.94);
    let rail = alpha(lighten(th.shadow, 0.08), 0.94);
    let slot = 420.0;
    let (min, max) = bounds_at(v, z0, slot);
    let street = v.ground + STREET_DROP;
    let mut boxes: Vec<(Rect, i32)> = Vec::new();
    for i in (min.x / slot).floor() as i32..=(max.x / slot).ceil() as i32 {
        let h = |n: u32| hash(i, 9, 500 + n);
        if h(0) < 0.3 {
            continue;
        }
        let w = slot * (0.55 + 0.4 * h(1));
        let x = i as f32 * slot + (slot - w) * h(2);
        let top = v.ground - 60.0 - 20.0 * h(3);
        boxes.push((Rect::new(x, top, w, street - top), i));
    }
    // Furthest from the centre of view first.
    boxes.sort_by(|a, b| (b.0.center().x - v.cam.x).abs().total_cmp(&(a.0.center().x - v.cam.x).abs()));
    let k = depth_scale(z0) * v.zoom;
    for (r, _) in boxes {
        if r.y > max.y {
            continue;
        }
        let p = |x: f32, y: f32, z: f32| v.p3(vec2(x, y), z);
        // Side toward the centre, roof if we're above it, then the front.
        let (x0, x1, y0, y1) = (r.x, r.x + r.w, r.y, r.y + r.h);
        if x0 > v.cam.x {
            quad(p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1), p(x0, y1, z0), col);
        } else if x1 < v.cam.x {
            quad(p(x1, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0), col);
        }
        if y0 > v.cam.y {
            quad(p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1), rail);
        }
        quad(p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0), col);
        // Railing along the front edge.
        let ry = y0 - 16.0;
        let (a, b) = (p(x0, ry, z0), p(x1, ry, z0));
        draw_line(a.x, a.y, b.x, b.y, 3.0 * k, rail);
        let mut x = x0 + 4.0;
        while x < x1 {
            let (a, b) = (p(x, ry, z0), p(x, y0, z0));
            draw_line(a.x, a.y, b.x, b.y, 2.2 * k, rail);
            x += 40.0;
        }
    }
}

/// Steel girders right by the camera, floor to sky: boxes with cross bracing.
fn draw_fg_girders(v: &View, th: &Theme) {
    let z0 = FG_GIRDER_Z;
    let z1 = z0 + 50.0;
    let col = alpha(th.shadow, 0.85);
    let brace = alpha(lighten(th.shadow, 0.1), 0.85);
    let slot = 1300.0;
    let (min, max) = bounds_at(v, z0, slot);
    let k = depth_scale(z0) * v.zoom;
    let p = |x: f32, y: f32, z: f32| v.p3(vec2(x, y), z);
    for i in (min.x / slot).floor() as i32..=(max.x / slot).ceil() as i32 {
        let h = |n: u32| hash(i, 3, 600 + n);
        if h(0) > 0.2 {
            continue;
        }
        let w = 24.0 + 16.0 * h(1);
        let x0 = i as f32 * slot + (slot - w) * h(2);
        let x1 = x0 + w;
        let (y0, y1) = (min.y - 100.0, max.y + 100.0);
        // Side toward the centre of view.
        if x0 > v.cam.x {
            quad(p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1), p(x0, y1, z0), col);
        } else if x1 < v.cam.x {
            quad(p(x1, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0), col);
        }
        // Two posts joined by diagonal bracing, anchored to the world so it scrolls.
        let post = w * 0.22;
        for px in [x0, x1 - post] {
            quad(p(px, y0, z0), p(px + post, y0, z0), p(px + post, y1, z0), p(px, y1, z0), col);
        }
        let step = w * 1.2;
        let mut y = (y0 / step).floor() * step;
        while y < y1 {
            for (a, b) in [((x0, y), (x1, y + step)), ((x1, y), (x0, y + step))] {
                let (a, b) = (p(a.0, a.1, z0), p(b.0, b.1, z0));
                draw_line(a.x, a.y, b.x, b.y, post * 0.5 * k, brace);
            }
            y += step;
        }
    }
}

// ---------- Birds ----------

/// Flocks of birds gliding through the sky at a few depths, scattered over the
/// whole height of the level so there's always something to pass on the way up.
fn draw_birds(v: &View, th: &Theme) {
    for (layer, z) in [(0, 2200.0), (1, 900.0)] {
        let k = depth_scale(z);
        let (cw, chh) = (1400.0 / k, 900.0 / k);
        let (min, max) = bounds_at(v, z, cw);
        let col = mix(th.shadow, th.haze, fog(z) * 0.8 + 0.1);
        for j in (min.y / chh).floor() as i32..=(max.y / chh).ceil() as i32 {
            for i in (min.x / cw).floor() as i32..=(max.x / cw).ceil() as i32 {
                let h = |n: u32| hash(i, j * 4 + layer, 700 + n);
                if h(0) > 0.3 {
                    continue;
                }
                // Each flock crosses its cell and fades at the ends before looping.
                let dir = if h(1) < 0.5 { -1.0 } else { 1.0 };
                let f = (h(2) + v.t * (25.0 + 25.0 * h(3)) / cw).fract();
                let fade = (f * PI).sin().min(0.6) / 0.6;
                let c = vec2(i as f32 * cw + if dir > 0.0 { f } else { 1.0 - f } * cw, (j as f32 + h(4)) * chh);
                if c.y > v.ground - 100.0 {
                    continue;
                }
                let n = 3 + (h(5) * 4.0) as i32;
                for b in 0..n {
                    // V formation, each bird flapping a little out of step.
                    let row = (b + 1) / 2;
                    let side = if b % 2 == 0 { 1.0 } else { -1.0 };
                    let off = vec2(-dir * row as f32 * 26.0, side * row as f32 * 14.0) / k * 0.35;
                    let flap = (v.t * (7.0 + h(6) * 2.0) + b as f32 * 0.9).sin();
                    let s = 8.0 * v.zoom * (0.4 + k);
                    let body = v.p3(c + off, z);
                    let wing = vec2(s, s * 0.6 * flap);
                    draw_line(body.x - wing.x, body.y - wing.y, body.x, body.y, 1.4 * v.zoom, alpha(col, fade));
                    draw_line(body.x, body.y, body.x + wing.x, body.y - wing.y, 1.4 * v.zoom, alpha(col, fade));
                }
            }
        }
    }
}

/// Streaks across the screen at high speed.
fn draw_speed_lines(v: &View, g: &Game) {
    let fast = g.player.speed01();
    if fast <= 0.05 || g.player.dead || g.preview {
        return;
    }
    let (sw, sh) = (screen_width(), screen_height());
    let dir = g.player.vel.normalize_or_zero();
    for i in 0..24 {
        // Each streak loops across the screen at its own speed and height.
        let speed = 0.6 + hash(i, 1, 7) * 0.8;
        let phase = (v.t * speed * 2.5 + hash(i, 2, 7)).fract();
        let across = hash(i, 3, 7) * sh;
        let along = (1.0 - phase) * (sw + 400.0) - 200.0;
        let len = (60.0 + hash(i, 4, 7) * 140.0) * fast;
        let p = vec2(sw / 2.0, sh / 2.0) + dir * (along - sw / 2.0) + vec2(-dir.y, dir.x) * (across - sh / 2.0);
        let a = fast * 0.3 * (phase * PI).sin();
        draw_line(p.x, p.y, p.x + dir.x * len, p.y + dir.y * len, v.s(1.5), alpha(WHITE, a));
    }
}

// ---------- Gates ----------

/// Gates: holographic barriers that switch off once you have enough coins.
/// With `out`, adds the gates' field boxes to the depth-sorted faces; without,
/// draws their fronts (after the buildings).
fn draw_gates(v: &View, g: &Game, mut out: Option<&mut Vec<Face>>) {
    let backs = interior_cells(g);
    let (min, max) = v.world_bounds(TILE);
    let have = g.coins_collected();
    let is_gate = |x: i32, y: i32| g.level.tile(x, y) == Tile::Gate;
    let field = pickup();
    let frame = mix(terrain(), deep(), 0.4);
    for gate in &g.gates {
        let w = vec2(gate.x as f32 * TILE, gate.y as f32 * TILE);
        if w.x > max.x || w.x + TILE < min.x || w.y > max.y || w.y + TILE < min.y || gate.open >= 1.0 {
            continue;
        }
        let p = v.p(w);
        let s = v.s(TILE);
        let shut = 1.0 - gate.open;
        let flicker = 0.85 + 0.15 * (v.t * 23.0 + gate.y as f32).sin();
        let field_col = alpha(field, 0.12 * shut * flicker);
        let th = theme();
        let d = if backs.contains(&(gate.x, gate.y)) || backs.contains(&(gate.x, gate.y - 1)) { INTERIOR_DEPTH } else { BUILDING_DEPTH };
        if let Some(out) = out.as_deref_mut() {
            let key = (w + Vec2::splat(TILE / 2.0) - v.cam).length();
            box_faces(v, &th, Rect::new(w.x + TILE * 0.2, w.y, TILE * 0.6, TILE), 0.0, d, field_col, tile_open(&g.level, gate.x, gate.y), key, out);
            continue;
        }
        grad_rect(p.x + s * 0.2, p.y, s * 0.6, s, alpha(field, 0.35 * shut * flicker), alpha(field, 0.2 * shut * flicker));
        // Scanlines drifting up.
        for k in 0..4 {
            let f = (k as f32 / 4.0 - v.t * 0.6).rem_euclid(1.0);
            let y = p.y + s * f;
            draw_line(p.x + s * 0.2, y, p.x + s * 0.8, y, v.s(1.2), alpha(lighten(field, 0.5), 0.6 * shut));
        }
        for x in [p.x + s * 0.2, p.x + s * 0.8] {
            draw_line(x, p.y, x, p.y + s, v.s(1.4), alpha(lighten(field, 0.4), 0.8 * shut));
        }
        let top_of_run = !is_gate(gate.x, gate.y - 1);
        if top_of_run {
            round_rect(p.x, p.y - v.s(3.0), s, v.s(6.0), v.s(2.0), lighten(frame, 0.2), frame);
        }
        if !is_gate(gate.x, gate.y + 1) {
            round_rect(p.x, p.y + s - v.s(3.0), s, v.s(6.0), v.s(2.0), lighten(frame, 0.2), frame);
        }
        // Coins still needed, on the top cell of each column.
        if top_of_run && !gate.opening {
            let left = gate.need.saturating_sub(have);
            let c = p + vec2(s / 2.0, s / 2.0);
            draw_circle(c.x, c.y, v.s(9.0), alpha(deep(), 0.85));
            draw_circle_lines(c.x, c.y, v.s(9.0), v.s(1.5), field);
            let text = left.to_string();
            let size = v.s(13.0);
            let dims = measure_text(&text, None, size as u16, 1.0);
            draw_text(&text, c.x - dims.width / 2.0, c.y + dims.offset_y / 2.0, size, field);
        }
    }
}

// ---------- Decorations ----------

/// Made-up adverts for the screens: headline, tagline, background gradient, accent.
const ADS: [(&str, &str, u32, u32, u32); 6] = [
    ("SKY FIZZ", "taste the altitude", 0x1fa8ff, 0x7cf0ff, 0xffffff),
    ("VOLT RUN", "faster every day", 0xff3ca8, 0x6a2cff, 0xffe14d),
    ("AERO+", "breathe the future", 0x27d07a, 0xbfffd0, 0x0b3a2a),
    ("ORBIT MILK", "now in zero-g", 0xffffff, 0xcfe6ff, 0x1f6fe8),
    ("NEO TRANSIT", "the city in minutes", 0xff8a1a, 0xffd36a, 0x2a1400),
    ("GLASS CITY", "live above it all", 0x0d1b3a, 0x2e5bd6, 0x7cf0ff),
];

/// Neon tube colours.
const NEONS: [u32; 6] = [0xff3ca8, 0x3cf0ff, 0xff9a2e, 0xb6ff3c, 0xb07cff, 0xff4040];

/// Text shrunk to fit `max_w`, from a starting size.
fn fit_text(text: &str, size: f32, max_w: f32) -> (f32, f32) {
    let w = measure_text(text, None, size.max(1.0) as u16, 1.0).width;
    if w > max_w && w > 0.0 {
        let s = (size * max_w / w).max(1.0);
        (s, measure_text(text, None, s as u16, 1.0).width)
    } else {
        (size, w)
    }
}

/// One advert filling a screen-space rect.
fn draw_ad(r: Rect, i: usize, t: f32) {
    let (head, tag, c1, c2, acc) = ADS[i % ADS.len()];
    let (c1, c2, acc) = (Color::from_hex(c1), Color::from_hex(c2), Color::from_hex(acc));
    grad_rect(r.x, r.y, r.w, r.h, c1, c2);
    // A slowly drifting accent circle and stripes.
    let cx = r.x + r.w * (0.75 + 0.08 * (t * 0.8).sin());
    draw_circle(cx, r.y + r.h * 0.5, r.h * 0.42, alpha(acc, 0.25));
    for k in 0..3 {
        let x = r.x + r.w * (0.08 + k as f32 * 0.05);
        draw_rectangle(x, r.y, r.w * 0.02, r.h, alpha(acc, 0.15));
    }
    if r.h < 10.0 {
        return;
    }
    let lum = 0.3 * c1.r + 0.6 * c1.g + 0.1 * c1.b;
    let ink = if lum > 0.6 { Color::from_hex(0x0b1a2b) } else { WHITE };
    let (hs, _) = fit_text(head, r.h * 0.38, r.w * 0.8);
    draw_text(head, r.x + r.w * 0.08, r.y + r.h * 0.5, hs, ink);
    let (ts, _) = fit_text(tag, hs * 0.45, r.w * 0.8);
    draw_text(tag, r.x + r.w * 0.08, r.y + r.h * 0.5 + ts * 1.4, ts, alpha(ink, 0.8));
}

/// Advert screens: neighbouring screen cells join into one big screen, which
/// cycles through adverts.
fn draw_screens(v: &View, g: &Game, th: &Theme) {
    let cells: std::collections::HashSet<(i32, i32)> =
        g.level.decor.iter().filter(|d| d.kind == DecorKind::Screen).map(|d| (d.x, d.y)).collect();
    let mut seen = std::collections::HashSet::new();
    let (min, max) = v.world_bounds(TILE * 2.0);
    for &start in &cells {
        if !seen.insert(start) {
            continue;
        }
        // Flood fill the group and take its bounding box.
        let (mut lo, mut hi) = (start, start);
        let mut stack = vec![start];
        while let Some((x, y)) = stack.pop() {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
            for n in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if cells.contains(&n) && seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        let (x0, y0) = (lo.0 as f32 * TILE + 2.0, lo.1 as f32 * TILE + 2.0);
        let (x1, y1) = ((hi.0 + 1) as f32 * TILE - 2.0, (hi.1 + 1) as f32 * TILE - 2.0);
        if x0 > max.x || x1 < min.x || y0 > max.y || y1 < min.y {
            continue;
        }
        let z = 6.0;
        box3(v, th, Rect::new(x0, y0, x1 - x0, y1 - y0), z, z + 12.0, mix(th.shadow, th.trim, 0.15), OPEN);
        let (a, b) = (v.p3(vec2(x0 + 2.0, y0 + 2.0), z), v.p3(vec2(x1 - 2.0, y1 - 2.0), z));
        let r = Rect::new(a.x, a.y, b.x - a.x, b.y - a.y);
        let cycle = v.t / 5.0 + hash(lo.0, lo.1, 33) * 10.0;
        draw_ad(r, cycle.floor() as usize + (hash(lo.0, lo.1, 34) * ADS.len() as f32) as usize, v.t);
        // Brief fade through black between adverts.
        let f = cycle.fract();
        if f < 0.06 {
            draw_rectangle(r.x, r.y, r.w, r.h, alpha(BLACK, 1.0 - f / 0.06));
        }
        // Scanlines and a glare across the glass.
        let mut sy = r.y;
        while sy < r.y + r.h {
            draw_rectangle(r.x, sy, r.w, 1.0, alpha(BLACK, 0.06));
            sy += 3.0;
        }
        draw_triangle(vec2(r.x, r.y), vec2(r.x + r.w * 0.45, r.y), vec2(r.x, r.y + r.h * 0.7), alpha(WHITE, 0.1));
        glow(r.center(), r.w.max(r.h) * 0.8, alpha(Color::from_hex(ADS[(cycle.floor() as usize) % ADS.len()].2), 0.12 * (0.4 + th.lights)));
    }
}

/// Neon signs: the typed text in glowing tubes, flickering now and then.
fn draw_neons(v: &View, g: &Game) {
    let (min, max) = v.world_bounds(TILE * 8.0);
    let z = 6.0;
    let k = depth_scale(z) * v.zoom;
    for n in &g.level.neons {
        let w = vec2(n.x as f32 * TILE, n.y as f32 * TILE);
        if w.x > max.x || w.x + TILE < min.x || w.y > max.y || w.y + TILE < min.y {
            continue;
        }
        let text = if n.text.trim().is_empty() { "OPEN" } else { n.text.as_str() };
        let col = Color::from_hex(NEONS[(hash(n.x, n.y, 9) * NEONS.len() as f32) as usize % NEONS.len()]);
        let f = (v.t * 0.7 + hash(n.x, n.y, 10) * 10.0).fract();
        let a = if f < 0.03 || (0.05..0.06).contains(&f) { 0.25 } else { 1.0 };
        let size = 22.0 * k;
        for (i, line) in text.lines().enumerate() {
            let origin = v.p3(w + vec2(2.0, 22.0 + i as f32 * 24.0), z);
            let width = measure_text(line, None, size as u16, 1.0).width;
            let mut gx = 0.0;
            while gx <= width {
                glow(origin + vec2(gx, -size * 0.3), size * 1.1, alpha(col, 0.16 * a));
                gx += size * 0.8;
            }
            let o = (1.2 * k).max(1.0);
            for d in [vec2(o, 0.0), vec2(-o, 0.0), vec2(0.0, o), vec2(0.0, -o)] {
                draw_text(line, origin.x + d.x, origin.y + d.y, size, alpha(col, 0.85 * a));
            }
            draw_text(line, origin.x, origin.y, size, alpha(lighten(col, 0.65), a));
        }
    }
}


/// Decorations on the play plane, which the player passes behind (drawn after
/// the facades). Grass's turf is drawn with the buildings; only its front edge is here.
fn in_front(kind: DecorKind) -> bool {
    matches!(kind, DecorKind::Tendril | DecorKind::Railing | DecorKind::Ladder | DecorKind::Eye | DecorKind::Grass)
}

/// `front` picks the pass: props standing back on the roofs behind the player
/// (pushed PROP_Z back), or decorations on the play plane in front of it.
fn draw_decor(v: &View, g: &Game, th: &Theme, front: bool) {
    let back_view = v.at_depth(PROP_Z);
    let v = if front { v } else { &back_view };
    let (min, max) = v.world_bounds(TILE * 2.0);
    let metal = mix(th.trim, th.shadow, 0.4);
    let dark = mix(th.trim, th.shadow, 0.6);
    let leaf = mix(Color::from_hex(0x46c84a), th.shadow, 0.6 * th.lights);
    let player_c = g.player.center();
    if !front {
        draw_screens(v, g, th);
        draw_neons(v, g);
    }
    for d in &g.level.decor {
        let w = vec2(d.x as f32 * TILE, d.y as f32 * TILE);
        if w.x > max.x || w.x + TILE < min.x || w.y > max.y || w.y + TILE < min.y {
            continue;
        }
        if in_front(d.kind) != front {
            continue;
        }
        let h = |k: u32| hash(d.x, d.y, k);
        match d.kind {
            DecorKind::Screen | DecorKind::Neon => {} // drawn whole, below
            DecorKind::Grass => {
                // The front edge of the turf (the rest sorts in with the buildings).
                let fy = w.y + TILE;
                let grass = grass_color(th);
                let (a, b) = (v.p(vec2(w.x, fy - 2.0)), v.p(vec2(w.x + TILE, fy + 1.0)));
                draw_rectangle(a.x, a.y, b.x - a.x, b.y - a.y, darken(grass, 0.15));
                grass_blades(v, d.x, d.y, 0.0, 8, 8.0, grass, |p, c| draw_triangle(v.p(p[0]), v.p(p[1]), v.p(p[2]), c));
            }
            DecorKind::Railing => {
                // Handrail along the floor below; neighbouring railings join up.
                let fy = w.y + TILE;
                let col = lighten(metal, 0.25);
                let p = |x: f32, y: f32| v.p3(vec2(x, y), 8.0);
                for x in [w.x + 4.0, w.x + 20.0] {
                    let (a, b) = (p(x, fy), p(x, fy - 18.0));
                    draw_line(a.x, a.y, b.x, b.y, v.s(1.6), col);
                }
                for (y, t) in [(fy - 18.0, 2.2), (fy - 9.0, 1.2)] {
                    let (a, b) = (p(w.x, y), p(w.x + TILE, y));
                    draw_line(a.x, a.y, b.x, b.y, v.s(t), col);
                }
            }
            DecorKind::Ladder => {
                // Red maintenance ladder; stacked cells make a tall ladder.
                let red = mix(Color::from_hex(0xe8232b), th.shadow, 0.3 * th.lights);
                let top = !g.level.decor.iter().any(|o| o.kind == DecorKind::Ladder && o.x == d.x && o.y == d.y - 1);
                let y0 = if top { w.y - 10.0 } else { w.y };
                let p = |x: f32, y: f32| v.p3(vec2(x, y), 3.0);
                for x in [w.x + 8.0, w.x + 24.0] {
                    let (a, b) = (p(x, y0), p(x, w.y + TILE));
                    draw_line(a.x, a.y, b.x, b.y, v.s(2.4), red);
                }
                for k in 0..4 {
                    let y = w.y + 4.0 + k as f32 * 8.0;
                    let (a, b) = (p(w.x + 8.0, y), p(w.x + 24.0, y));
                    draw_line(a.x, a.y, b.x, b.y, v.s(1.6), darken(red, 0.1));
                }
            }
            DecorKind::Floodlight => {
                // Lamp on a pole, throwing a beam up into the air.
                let (fy, cx, z) = (w.y + TILE, w.x + 10.0, 40.0);
                let p = |x: f32, y: f32| v.p3(vec2(x, y), z);
                let light = mix(th.window_lit, WHITE, 0.4);
                let head = p(cx, fy - 30.0);
                draw_triangle(head, p(cx + 70.0, fy - 110.0), p(cx + 100.0, fy - 60.0), alpha(light, 0.1 + 0.25 * th.lights));
                let (a, b) = (p(cx, fy), p(cx, fy - 26.0));
                draw_line(a.x, a.y, b.x, b.y, v.s(2.2), dark);
                box3(v, th, Rect::new(cx - 6.0, fy - 34.0, 12.0, 8.0), z - 4.0, z + 4.0, metal, OPEN);
                glow(p(cx + 6.0, fy - 30.0), v.s(12.0), alpha(light, 0.6 + 0.3 * th.lights));
            }
            DecorKind::Skylight => {
                // A glass ridge on the roof, reaching back across it.
                let (fy, x0, x1) = (w.y + TILE, w.x + 2.0, w.x + TILE - 2.0);
                let (za, zb) = (6.0, 50.0);
                box3(v, th, Rect::new(x0, fy - 6.0, x1 - x0, 6.0), za, zb, mix(th.trim, th.facade, 0.4), OPEN);
                let glass = if th.lights > 0.3 { alpha(th.window_lit, 0.5) } else { alpha(mix(th.glass, th.sky_bottom, 0.4), 0.5) };
                let p = |x: f32, y: f32, z: f32| v.p3(vec2(x, y), z);
                let (ridge, mid) = (fy - 20.0, (x0 + x1) / 2.0);
                for z in [zb, za] {
                    draw_triangle(p(x0, fy - 6.0, z), p(x1, fy - 6.0, z), p(mid, ridge, z), glass);
                    if z == zb {
                        quad(p(x0, fy - 6.0, za), p(mid, ridge, za), p(mid, ridge, zb), p(x0, fy - 6.0, zb), glass);
                        quad(p(x1, fy - 6.0, za), p(mid, ridge, za), p(mid, ridge, zb), p(x1, fy - 6.0, zb), alpha(glass, 0.8));
                    }
                }
                let (a, b) = (p(mid, ridge, za), p(mid, ridge, zb));
                draw_line(a.x, a.y, b.x, b.y, v.s(1.5), th.trim);
            }
            DecorKind::Crates => {
                let fy = w.y + TILE;
                let orange = mix(Color::from_hex(ACCENTS[0]), th.shadow, 0.35 * th.lights);
                let blue = mix(Color::from_hex(ACCENTS[4]), th.shadow, 0.35 * th.lights);
                box3(v, th, Rect::new(w.x + 18.0, fy - 11.0, 12.0, 11.0), 30.0, 52.0, blue, OPEN);
                box3(v, th, Rect::new(w.x + 2.0, fy - 18.0, 18.0, 18.0), 10.0, 40.0, orange, OPEN);
                for k in 1..3 {
                    let y = fy - 18.0 + k as f32 * 6.0;
                    let (a, b) = (v.p3(vec2(w.x + 2.0, y), 10.0), v.p3(vec2(w.x + 20.0, y), 10.0));
                    draw_line(a.x, a.y, b.x, b.y, v.s(1.0), darken(orange, 0.25));
                }
            }
            DecorKind::Cilia => {
                // Potted plant.
                let pot_top = w.y + TILE - 9.0;
                box3(v, th, Rect::new(w.x + 8.0, pot_top, 16.0, 9.0), 4.0, 20.0, mix(th.trim, th.facade, 0.4), OPEN);
                for k in 0..6 {
                    let ang = -PI / 2.0 + (k as f32 / 5.0 - 0.5) * 1.6 + (v.t * 1.8 + h(k) * 6.0).sin() * 0.08;
                    let len = 12.0 + h(k + 10) * 10.0;
                    let root = vec2(w.x + 16.0 + (k as f32 - 2.5) * 1.5, pot_top);
                    let tip = root + Vec2::from_angle(ang) * len;
                    let n = Vec2::from_angle(ang + PI / 2.0) * 2.6;
                    let mid = root.lerp(tip, 0.5);
                    let shade = if k % 2 == 0 { leaf } else { lighten(leaf, 0.2) };
                    let p = |w: Vec2| v.p3(w, 6.0 + (k % 3) as f32 * 5.0);
                    draw_triangle(p(root), p(mid + n), p(tip), shade);
                    draw_triangle(p(root), p(mid - n), p(tip), darken(shade, 0.12));
                }
            }
            DecorKind::Tendril => {
                // Vines hanging from above.
                for k in 0..2 {
                    let x = w.x + 9.0 + k as f32 * 13.0 + h(k) * 4.0;
                    let len = 18.0 + h(k + 10) * 18.0;
                    let ph = h(k + 20) * TAU;
                    let mut prev = v.p(vec2(x, w.y));
                    for seg in 1..=6 {
                        let f = seg as f32 / 6.0;
                        let sway = (v.t * 1.4 + ph + f * 2.0).sin() * 4.0 * f;
                        let p = v.p(vec2(x + sway, w.y + len * f));
                        draw_line(prev.x, prev.y, p.x, p.y, v.s(1.4), darken(leaf, 0.2));
                        let side = if seg % 2 == 0 { 1.0 } else { -1.0 };
                        let lc = p + vec2(side * v.s(2.5), 0.0);
                        draw_ellipse(lc.x, lc.y, v.s(2.6), v.s(1.5), side * 20.0, leaf);
                        prev = p;
                    }
                }
            }
            DecorKind::Eye => {
                // Security camera on a mount (ceiling, wall or floor by rotation),
                // tracking the player.
                let c = w + Vec2::splat(TILE / 2.0);
                let up = rot(vec2(0.0, -1.0), d.rot as f32 * PI / 2.0); // toward the mounting surface
                let side = vec2(-up.y, up.x);
                let plate = c + up * 14.0;
                let mount = c + up * 10.0;
                let target = if g.preview { mount - up * 2.0 + side } else { player_c };
                let dir = (target - mount).normalize_or_zero();
                let ang = dir.y.atan2(dir.x);
                let (a, b) = (v.p(plate - side * 5.0), v.p(plate + side * 5.0));
                draw_line(a.x, a.y, b.x, b.y, v.s(3.0), dark);
                let m = v.p(mount);
                let mp = v.p(plate);
                draw_line(mp.x, mp.y, m.x, m.y, v.s(2.0), dark);
                let body: Vec<Vec2> = [vec2(-4.0, -3.5), vec2(9.0, -3.0), vec2(9.0, 3.0), vec2(-4.0, 3.5)]
                    .into_iter()
                    .map(|p| m + rot(p * v.zoom, ang))
                    .collect();
                grad_poly(&body, th.trim, metal);
                let lens = m + rot(vec2(9.5, 0.0) * v.zoom, ang);
                draw_circle(lens.x, lens.y, v.s(2.6), th.shadow);
                if (v.t + h(1) * 2.0) % 1.2 < 0.6 {
                    let led = m + rot(vec2(0.0, -1.5) * v.zoom, ang);
                    glow(led, v.s(6.0), alpha(th.hazard, 0.7));
                }
            }
            DecorKind::Cysts => {
                // Rooftop AC unit with a spinning fan.
                box3(v, th, Rect::new(w.x + 3.0, w.y + 9.0, 26.0, TILE - 9.0), 0.0, 48.0, mix(th.trim, metal, 0.4), OPEN);
                let a = v.p(w + vec2(3.0, 9.0));
                let bh = v.s(TILE - 9.0);
                for k in 0..3 {
                    let y = a.y + v.s(5.0 + k as f32 * 4.0);
                    draw_line(a.x + v.s(3.0), y, a.x + v.s(10.0), y, v.s(1.0), dark);
                }
                let fc = a + vec2(v.s(18.5), bh / 2.0);
                draw_circle(fc.x, fc.y, v.s(6.5), dark);
                let spin = v.t * 9.0 + h(1) * TAU;
                for b in 0..3 {
                    let d = Vec2::from_angle(spin + b as f32 / 3.0 * TAU);
                    let tip = fc + d * v.s(5.5);
                    draw_line(fc.x, fc.y, tip.x, tip.y, v.s(2.2), metal);
                }
            }
            DecorKind::Scar => {
                // A pipe running across the tile (or up it, when rotated), with a flange.
                let off = 14.0 + (h(1) - 0.5) * 8.0;
                let along = 4.0 + h(2) * 20.0;
                let pipe = mix(th.trim, metal, 0.5);
                if d.rot % 2 == 0 {
                    let y = w.y + off;
                    box3(v, th, Rect::new(w.x - 0.5, y - 3.5, TILE + 1.0, 7.0), 10.0, 17.0, pipe, Open { left: false, right: false, ..OPEN });
                    box3(v, th, Rect::new(w.x + along, y - 5.0, 4.0, 10.0), 8.0, 19.0, lighten(metal, 0.1), OPEN);
                } else {
                    let x = w.x + off;
                    box3(v, th, Rect::new(x - 3.5, w.y - 0.5, 7.0, TILE + 1.0), 10.0, 17.0, pipe, Open { top: false, bottom: false, ..OPEN });
                    box3(v, th, Rect::new(x - 5.0, w.y + along, 10.0, 4.0), 8.0, 19.0, lighten(metal, 0.1), OPEN);
                }
            }
            DecorKind::Veins => {
                // Antenna mast with a dish and a blinking light, standing back on the roof.
                let base = v.p3(w + vec2(TILE / 2.0, TILE), 50.0);
                let top = v.p3(w + vec2(TILE / 2.0, -14.0), 50.0);
                draw_line(base.x, base.y, top.x, top.y, v.s(2.0), dark);
                for k in 0..3 {
                    let y = base.y + (top.y - base.y) * (0.3 + k as f32 * 0.22);
                    let half = v.s(7.0 - k as f32 * 2.0);
                    draw_line(base.x - half, y, base.x + half, y, v.s(1.4), dark);
                }
                let dish = base + (top - base) * 0.45 + vec2(v.s(4.0), 0.0);
                let pts: Vec<Vec2> = (0..=8).map(|k| dish + Vec2::from_angle(-PI / 2.0 + k as f32 / 8.0 * PI) * vec2(v.s(4.0), v.s(7.0))).collect();
                fill_poly(&pts, th.trim);
                if (v.t * 0.9 + h(1)).fract() < 0.45 {
                    glow(top, v.s(8.0), alpha(th.hazard, 0.9));
                }
                draw_circle(top.x, top.y, v.s(1.8), th.hazard);
            }
            DecorKind::Sign => {
                // Holo sign on a post, standing on the first floor below it.
                let ground = (1..=8)
                    .map(|dy| d.y + dy)
                    .find(|&y| matches!(g.level.tile(d.x, y), Tile::Solid | Tile::Slope(_) | Tile::Bounce | Tile::Crumble))
                    .map_or(w.y + TILE, |y| y as f32 * TILE);
                let post_top = v.p3(w + vec2(TILE / 2.0, 12.0), 3.0);
                let post_bot = v.p3(vec2(w.x + TILE / 2.0, ground), 3.0);
                box3(v, th, Rect::new(w.x + TILE / 2.0 - 5.0, ground - 3.0, 10.0, 3.0), 0.0, 6.0, dark, OPEN);
                draw_line(post_top.x, post_top.y, post_bot.x, post_bot.y, v.s(2.5), dark);
                box3(v, th, Rect::new(w.x + 3.0, w.y + 2.0, TILE - 6.0, 13.0), 0.0, 6.0, dark, OPEN);
                let a = v.p(w + vec2(3.0, 2.0));
                let (bw, bh) = (v.s(TILE - 6.0), v.s(13.0));
                glow(a + vec2(bw, bh) / 2.0, v.s(20.0), alpha(th.player, 0.25));
                round_rect(a.x, a.y, bw, bh, v.s(2.0), alpha(th.deep, 0.8), alpha(mix(th.deep, th.player, 0.25), 0.8));
                for k in 0..2 {
                    let y = a.y + v.s(4.5 + k as f32 * 4.0);
                    let len = bw * (0.7 - k as f32 * 0.25);
                    draw_line(a.x + v.s(3.0), y, a.x + v.s(3.0) + len, y, v.s(1.2), alpha(th.player, 0.8));
                }
            }
        }
    }
}

/// Wrap text to at most `width` characters per line (explicit newlines kept).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        out.push(line);
    }
    out
}

/// Text panels above signs: faded in by proximity in play, always shown in the editor.
fn draw_sign_text(v: &View, g: &Game) {
    for (i, sign) in g.level.signs.iter().enumerate() {
        let a = if g.preview { 0.85 } else { g.sign_alpha.get(i).copied().unwrap_or(0.0) };
        if a <= 0.01 || sign.text.trim().is_empty() {
            continue;
        }
        let lines = wrap(&sign.text, 30);
        let size = v.s(11.0).max(12.0);
        let line_h = size * 1.15;
        let pad = size * 0.6;
        let tw = lines.iter().map(|l| measure_text(l, None, size as u16, 1.0).width).fold(0.0, f32::max);
        let (bw, bh) = (tw + pad * 2.0, line_h * lines.len() as f32 + pad * 1.4);
        let anchor = v.p3(vec2((sign.x as f32 + 0.5) * TILE, sign.y as f32 * TILE - 4.0), PROP_Z); // over the board, back with the props
        let x = (anchor.x - bw / 2.0).clamp(4.0, (screen_width() - bw - 4.0).max(4.0));
        let y = anchor.y - bh - v.s(6.0);
        glass_panel(x, y, bw, bh, a);
        draw_triangle(
            vec2(anchor.x - v.s(5.0), y + bh),
            vec2(anchor.x + v.s(5.0), y + bh),
            vec2(anchor.x, y + bh + v.s(6.0)),
            alpha(deep(), 0.75 * a),
        );
        for (k, l) in lines.iter().enumerate() {
            draw_text(l, x + pad, y + pad * 0.7 + line_h * (k as f32 + 0.8), size, alpha(WHITE, a));
        }
    }
}

// ---------- HUD ----------

fn shadow_text(text: &str, x: f32, y: f32, size: f32, color: Color) {
    let o = (size / 14.0).max(1.0);
    draw_text(text, x + o, y + o, size, alpha(BLACK, 0.45 * color.a));
    draw_text(text, x, y, size, color);
}

/// Horizontally centered text, shrunk if needed to fit the window.
fn centered_text(text: &str, y: f32, size: f32, color: Color) {
    let max_w = screen_width() - 40.0;
    let mut size = size;
    let mut dims = measure_text(text, None, size as u16, 1.0);
    if dims.width > max_w {
        size = (size * max_w / dims.width).floor().max(8.0);
        dims = measure_text(text, None, size as u16, 1.0);
    }
    shadow_text(text, (screen_width() - dims.width) / 2.0, y, size, color);
}

/// Bottom-left speedometer: current speed, move, stored dash/jump, and the speed graph.
fn draw_speedometer(g: &Game) {
    let k = ui();
    let r = speedometer::panel_rect();
    glass_panel(r.x, r.y, r.w, r.h, 1.0);
    let p = &g.player;
    let tx = r.x + 16.0 * k;
    draw_text("SPEED", tx, r.y + 24.0 * k, 15.0 * k, alpha(WHITE, 0.6));
    shadow_text(&format!("{:.0}", p.speed()), tx, r.y + 62.0 * k, 44.0 * k, WHITE);
    draw_text(p.state.name(), tx, r.y + 84.0 * k, 18.0 * k, move_color(p.state));
    // Stored dash and air jump pips.
    let py = r.y + r.h - 20.0 * k;
    let dash_c = vec2(tx + 8.0 * k, py);
    if p.has_dash {
        glow(dash_c, 14.0 * k, alpha(orb(), 0.5));
        gloss_ball(dash_c, 6.5 * k, orb(), 1.0);
    } else {
        draw_circle_lines(dash_c.x, dash_c.y, 6.5 * k, 1.2 * k, alpha(orb(), 0.4));
    }
    let jump_c = dash_c + vec2(20.0 * k, 0.0);
    if p.air_jumps > 0 {
        draw_circle(jump_c.x, jump_c.y, 6.5 * k, alpha(WHITE, 0.2));
        draw_circle_lines(jump_c.x, jump_c.y, 6.5 * k, 1.2 * k, WHITE);
    }
    let gx = r.x + 112.0 * k;
    speedometer::draw_graph(Rect::new(gx, r.y + 14.0 * k, r.x + r.w - gx - 12.0 * k, r.h - 28.0 * k), k);
}

pub fn draw_hud(g: &Game, input: &Input, paused: bool, finish_action: &str) {
    let (sw, sh) = (screen_width(), screen_height());
    let k = ui();
    let collected = g.nutrients.iter().filter(|n| n.taken).count();
    let mins = (g.time / 60.0) as u32;
    let secs = g.time % 60.0;

    // Top left: coins, time, deaths.
    let (px, py, pw, ph) = (16.0 * k, 16.0 * k, 250.0 * k, 72.0 * k);
    glass_panel(px, py, pw, ph, 1.0);
    let icon = vec2(px + 26.0 * k, py + 26.0 * k);
    glow(icon, 18.0 * k, alpha(pickup(), 0.4));
    gloss_ball(icon, 10.0 * k, pickup(), 1.0);
    shadow_text(&format!("{collected}/{}", g.nutrients.len()), px + 44.0 * k, py + 36.0 * k, 32.0 * k, WHITE);
    let time = format!("{mins:02}:{secs:05.2}");
    let tw = measure_text(&time, None, (28.0 * k) as u16, 1.0).width;
    shadow_text(&time, px + pw - tw - 16.0 * k, py + 34.0 * k, 28.0 * k, WHITE);
    shadow_text(&format!("deaths {}", g.deaths), px + 16.0 * k, py + 60.0 * k, 18.0 * k, lighten(hazard(), 0.45));

    draw_speedometer(g);

    // Controller status: shown for the first few seconds only (and not over a notice).
    let status_a = (8.0 - g.anim_t).clamp(0.0, 1.0);
    if status_a > 0.0 && input.notice().is_none() {
        let (status, color) = if !input.gamepad_available() {
            ("Gamepad support unavailable".to_string(), hazard())
        } else if let Some(name) = input.controller_name() {
            let power = input.controller_power().map(|p| format!(" ({p})")).unwrap_or_default();
            let extra = match input.connected_count() {
                n if n > 1 => format!("  [{n} connected]"),
                _ => String::new(),
            };
            (format!("Controller: {name}{power}{extra}"), player())
        } else {
            ("No controller - using keyboard".to_string(), WHITE)
        };
        let size = 20.0 * k;
        let dims = measure_text(&status, None, size as u16, 1.0);
        shadow_text(&status, sw - dims.width - 18.0 * k, 32.0 * k, size, alpha(color, status_a));
    }

    if let Some((msg, a)) = input.notice() {
        let size = 26.0 * k;
        let dims = measure_text(msg, None, size as u16, 1.0);
        let (w, h) = (dims.width + 40.0 * k, 46.0 * k);
        let x = (sw - w) / 2.0;
        glass_panel(x, py, w, h, a);
        draw_text(msg, x + 20.0 * k, py + 30.0 * k, size, alpha(WHITE, a));
    }

    let pad = input.last_device == Device::Gamepad;
    if g.anim_t < 10.0 && !paused && !g.finished {
        let a = (10.0 - g.anim_t).min(1.0);
        let hint = if pad {
            "Stick move   A jump   X/RB/RT dash (in air)   Down slide   Start pause   F1 debug"
        } else {
            "Arrows/WASD move   Space jump   Shift dash (in air)   Down slide   Esc pause   F1 debug"
        };
        centered_text(hint, py + ph + 36.0 * k, 20.0 * k, alpha(WHITE, a));
    }

    if paused {
        draw_rectangle(0.0, 0.0, sw, sh, alpha(deep(), 0.45));
        let lines: &[&str] = if pad {
            &[
                "Left stick / D-pad  -  move: keep running to build speed",
                "A  -  jump (hold for full height; faster runs jump higher)",
                "Jump right as you land for a speed boost",
                "X / RB / RT  -  dash in mid-air (refills on landing and from orbs)",
                "Down while moving  -  slide (fast downhill)   Slide + jump  -  long jump",
                "Hit a wall fast to run up it   A on a wall  -  kick off",
                "Low steps are vaulted, ledges grabbed   Down on a catwalk  -  drop",
                "",
                "Start - resume      A - resume      Back/Select - restart",
            ]
        } else {
            &[
                "Arrows / WASD  -  move: keep running to build speed",
                "Space / Z / K  -  jump (hold for full height; faster runs jump higher)",
                "Jump right as you land for a speed boost",
                "Shift / X / J  -  dash in mid-air (refills on landing and from orbs)",
                "Down while moving  -  slide (fast downhill)   Slide + jump  -  long jump",
                "Hit a wall fast to run up it   Jump on a wall  -  kick off",
                "Low steps are vaulted, ledges grabbed   Down on a catwalk  -  drop",
                "",
                "Esc - resume      Enter - resume      R - restart",
                "M - music on/off      N - sound effects on/off",
            ]
        };
        let line_h = 30.0 * k;
        let (w, h) = ((sw - 60.0 * k).min(1000.0 * k), 130.0 * k + lines.len() as f32 * line_h);
        let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0);
        glass_panel(x, y, w, h, 1.0);
        centered_text("PAUSED", y + 70.0 * k, 60.0 * k, player());
        for (i, l) in lines.iter().enumerate() {
            centered_text(l, y + 120.0 * k + i as f32 * line_h, 22.0 * k, WHITE);
        }
    }

    if g.finished {
        draw_rectangle(0.0, 0.0, sw, sh, alpha(deep(), 0.35));
        let (w, h) = ((sw - 60.0 * k).min(900.0 * k), 260.0 * k);
        let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0);
        glass_panel(x, y, w, h, 1.0);
        centered_text("LEVEL CLEARED", y + 80.0 * k, 64.0 * k, pickup());
        centered_text(
            &format!("Time {mins:02}:{secs:05.2}    Coins {collected}/{}    Deaths {}", g.nutrients.len(), g.deaths),
            y + 145.0 * k,
            30.0 * k,
            WHITE,
        );
        let again = format!("Press {} to {finish_action}", if pad { "A" } else { "Enter" });
        centered_text(&again, y + 210.0 * k, 26.0 * k, lighten(player(), 0.3));
    }
}
