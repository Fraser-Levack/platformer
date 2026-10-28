//! Colour themes: times of day over a clean, glossy future city. Each level
//! picks a theme; everything is drawn by *role* (sky, buildings, hazard,
//! pickup, player...) so swapping the theme recolours the whole game,
//! particles included.

use macroquad::prelude::*;
use std::cell::Cell;

const fn hex(v: u32) -> Color {
    Color::new(
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
        1.0,
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub name: &'static str,
    /// Sky gradient, top of the screen to the horizon.
    pub sky_top: Color,
    pub sky_bottom: Color,
    /// Sun (or moon) and its glow.
    pub sun: Color,
    /// Sun position as a fraction of the screen.
    pub sun_at: (f32, f32),
    /// Distant air: far buildings fade into this.
    pub haze: Color,
    /// Far skyline colour, before haze.
    pub city: Color,
    /// How many windows are lit (0 = day, 1 = deep night).
    pub lights: f32,
    pub window_lit: Color,
    /// Building walls you run on.
    pub facade: Color,
    /// Their window glass.
    pub glass: Color,
    /// Roof ledges and platform tops.
    pub trim: Color,
    /// Dark foreground silhouettes.
    pub shadow: Color,
    /// Danger.
    pub hazard: Color,
    /// Collectibles, jump pads, the goal: things to go for.
    pub pickup: Color,
    /// The robot's glow (eyes, trails).
    pub player: Color,
    /// Dash orbs and a stored dash.
    pub orb: Color,
    /// Dark UI backing.
    pub deep: Color,
}

pub const THEMES: [Theme; 6] = [
    // Bright midday: aqua sky, white towers.
    Theme {
        name: "Skyline",
        sky_top: hex(0x1f7fd6),
        sky_bottom: hex(0xbfeeff),
        sun: hex(0xfffbe6),
        sun_at: (0.78, 0.16),
        haze: hex(0xd4f2ff),
        city: hex(0x6fa8d6),
        lights: 0.0,
        window_lit: hex(0xfff3c4),
        facade: hex(0xe9f0f6),
        glass: hex(0x4d9fdc),
        trim: hex(0xffffff),
        shadow: hex(0x0a1a2c),
        hazard: hex(0xff3b3b),
        pickup: hex(0x5fd83a),
        player: hex(0x2fe6ff),
        orb: hex(0xffb020),
        deep: hex(0x0b1a2b),
    },
    // Early morning: peach horizon, soft blue.
    Theme {
        name: "Daybreak",
        sky_top: hex(0x5b95dc),
        sky_bottom: hex(0xffd9bd),
        sun: hex(0xfff0d6),
        sun_at: (0.22, 0.42),
        haze: hex(0xf5ded4),
        city: hex(0x8d9fc9),
        lights: 0.12,
        window_lit: hex(0xffe2a8),
        facade: hex(0xf2eeec),
        glass: hex(0x7aa4d8),
        trim: hex(0xffffff),
        shadow: hex(0x1a1830),
        hazard: hex(0xff3d4f),
        pickup: hex(0x4fd47a),
        player: hex(0x35d8ff),
        orb: hex(0xff9a2e),
        deep: hex(0x161a30),
    },
    // Sunset: gold and coral.
    Theme {
        name: "Golden",
        sky_top: hex(0x3457a8),
        sky_bottom: hex(0xffac63),
        sun: hex(0xffe6a0),
        sun_at: (0.7, 0.5),
        haze: hex(0xf6bd8a),
        city: hex(0xa47c9e),
        lights: 0.35,
        window_lit: hex(0xffd27a),
        facade: hex(0xf1e3d8),
        glass: hex(0xd98c62),
        trim: hex(0xfff6ec),
        shadow: hex(0x22101a),
        hazard: hex(0xe8203c),
        pickup: hex(0x7ee04a),
        player: hex(0x3ce8ff),
        orb: hex(0xffe24a),
        deep: hex(0x1e1328),
    },
    // Dusk: violet and pink.
    Theme {
        name: "Dusk",
        sky_top: hex(0x1a2766),
        sky_bottom: hex(0xd47aa8),
        sun: hex(0xffd2e2),
        sun_at: (0.3, 0.55),
        haze: hex(0xb47aae),
        city: hex(0x4f4888),
        lights: 0.7,
        window_lit: hex(0xffd9a0),
        facade: hex(0xd9d6ea),
        glass: hex(0x6a6cc8),
        trim: hex(0xf6f2ff),
        shadow: hex(0x0d0a1e),
        hazard: hex(0xff3a5c),
        pickup: hex(0x8af05a),
        player: hex(0x4ff0ff),
        orb: hex(0xffc23a),
        deep: hex(0x120f26),
    },
    // Night: navy, a full moon, every window lit.
    Theme {
        name: "Nightfall",
        sky_top: hex(0x040920),
        sky_bottom: hex(0x1c3b72),
        sun: hex(0xe2ebff),
        sun_at: (0.75, 0.18),
        haze: hex(0x284a80),
        city: hex(0x15244a),
        lights: 1.0,
        window_lit: hex(0xffe6a6),
        facade: hex(0x8494b0),
        glass: hex(0x1a2a4c),
        trim: hex(0xd2e2ff),
        shadow: hex(0x02040c),
        hazard: hex(0xff3355),
        pickup: hex(0x7dff5a),
        player: hex(0x46f2ff),
        orb: hex(0xffb82e),
        deep: hex(0x060c1e),
    },
    // Polar night: teal sky.
    Theme {
        name: "Aurora",
        sky_top: hex(0x03121c),
        sky_bottom: hex(0x0f6260),
        sun: hex(0xc8fff0),
        sun_at: (0.25, 0.2),
        haze: hex(0x1f716c),
        city: hex(0x10343c),
        lights: 0.85,
        window_lit: hex(0xbffff0),
        facade: hex(0x7d9ea4),
        glass: hex(0x10353e),
        trim: hex(0xd8fff6),
        shadow: hex(0x010a0c),
        hazard: hex(0xff4060),
        pickup: hex(0xb8ff4a),
        player: hex(0x5cf6ff),
        orb: hex(0xffd04a),
        deep: hex(0x031016),
    },
];

/// Names from earlier versions of the game, so old level files still load.
const OLD_NAMES: [[&str; 6]; 2] = [
    ["Necrosis", "Ichor", "Rot", "Fever", "Frost", "Cinder"],
    ["Jelly", "Sunset", "Forest", "Candy", "Ice", "Lava"],
];

pub fn theme_index(name: &str) -> Option<usize> {
    let name = name.trim();
    THEMES.iter().position(|t| t.name.eq_ignore_ascii_case(name)).or_else(|| {
        OLD_NAMES.iter().find_map(|names| names.iter().position(|n| n.eq_ignore_ascii_case(name)))
    })
}

thread_local! {
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

pub fn set_theme(index: usize) {
    CURRENT.with(|c| c.set(index % THEMES.len()));
}

pub fn theme() -> Theme {
    THEMES[CURRENT.with(|c| c.get())]
}

pub fn hazard() -> Color {
    theme().hazard
}
pub fn pickup() -> Color {
    theme().pickup
}
pub fn player() -> Color {
    theme().player
}
/// Buildings (walls you run on).
pub fn terrain() -> Color {
    theme().facade
}
pub fn deep() -> Color {
    theme().deep
}
pub fn orb() -> Color {
    theme().orb
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

pub fn lighten(c: Color, t: f32) -> Color {
    let mut out = mix(c, WHITE, t);
    out.a = c.a;
    out
}

pub fn darken(c: Color, t: f32) -> Color {
    let mut out = mix(c, BLACK, t);
    out.a = c.a;
    out
}
