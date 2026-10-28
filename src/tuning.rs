//! Movement tuning, hot-reloaded from `assets/tuning.txt`.
//!
//! Every value the player controller uses lives here with its default. The
//! file is plain `name = value` lines (`#` starts a comment); save it while the
//! game is running and the new values apply within half a second. If the file
//! is missing it is written out with every default and its description.
//!
//! Units are pixels and seconds; a tile is 32 px.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::SystemTime;

/// How often the file's modification time is checked.
const POLL_INTERVAL: f32 = 0.5;

macro_rules! tuning {
    ($( $section:literal { $( $(#[doc = $doc:literal])* $name:ident : $default:expr, )* } )*) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Tuning {
            $( $( $(#[doc = $doc])* pub $name: f32, )* )*
        }

        impl Default for Tuning {
            fn default() -> Self {
                Tuning { $( $( $name: $default, )* )* }
            }
        }

        impl Tuning {
            fn set(&mut self, key: &str, value: f32) -> bool {
                match key {
                    $( $( stringify!($name) => self.$name = value, )* )*
                    _ => return false,
                }
                true
            }

            /// The whole tuning file: every value with its description.
            pub fn to_text(self) -> String {
                let mut out = String::from(HEADER);
                $(
                    out.push_str(&format!("\n# ---------- {} ----------\n", $section));
                    $(
                        $( out.push_str(&format!("#{}\n", $doc)); )*
                        out.push_str(&format!("{} = {}\n", stringify!($name), self.$name));
                    )*
                )*
                out
            }
        }
    };
}

const HEADER: &str = "\
# Movement tuning. Save while the game is running to apply changes.
# Units: pixels and seconds (a tile is 32 px). Delete a line to use its default;
# delete the file to regenerate it with every default.
";

tuning! {
    "Running" {
        /// Speed you reach almost immediately when you hold a direction.
        run_speed: 340.0,
        /// Speed you build up to by running without stopping.
        top_speed: 600.0,
        /// Acceleration from standing up to run_speed.
        run_accel: 2600.0,
        /// Acceleration from run_speed up to top_speed (slow on purpose: momentum builds).
        build_accel: 140.0,
        /// Braking when you let go on the ground.
        ground_friction: 1700.0,
        /// Braking when you push against your motion (skid).
        turn_accel: 3800.0,
        /// Above top_speed (from slopes, boosts, landings) speed bleeds off this slowly.
        overspeed_drag: 260.0,
        /// Nothing ever moves faster than this.
        hard_cap: 1400.0,
    }
    "Slopes" {
        /// How much slopes speed you up / slow you down while running (1 = full gravity).
        slope_run_factor: 0.35,
        /// Same, while sliding: slides really pick up speed downhill.
        slope_slide_factor: 1.0,
        /// Jumping on an uphill keeps this much of the upward slope velocity.
        slope_jump_keep: 0.5,
    }
    "Jumping" {
        /// Full jump height in tiles (holding jump).
        jump_height: 3.4,
        /// Seconds to reach the top of a full jump.
        jump_time_to_apex: 0.36,
        /// Extra jump height in tiles at top speed (scales from run_speed).
        speed_jump_bonus: 0.7,
        /// Gravity multiplier while falling.
        fall_gravity_mult: 1.55,
        /// Gravity multiplier near the top of a held jump (hang time).
        apex_gravity_mult: 0.55,
        /// Vertical speed counted as near the top of a jump.
        apex_threshold: 80.0,
        /// Releasing jump early keeps this much upward speed.
        jump_cut: 0.45,
        max_fall: 900.0,
        /// Fall speed while holding down in the air.
        fast_fall: 1250.0,
        coyote_time: 0.1,
        jump_buffer: 0.12,
    }
    "Air" {
        /// Steering toward run_speed in the air.
        air_accel: 2000.0,
        /// Braking in the air with no input (low: you keep your momentum).
        air_drag: 220.0,
        /// Above run_speed in the air, speed bleeds off this slowly.
        air_overspeed_drag: 20.0,
    }
    "Perfect landing" {
        /// Jump within this many seconds of landing (or buffer it) for a speed boost.
        perfect_window: 0.09,
        /// Speed added by a perfect landing jump.
        perfect_boost: 45.0,
        /// Perfect boosts can't push you past top_speed times this.
        perfect_cap_mult: 1.2,
        /// Minimum speed for a landing jump to count.
        perfect_min_speed: 200.0,
    }
    "Slide" {
        /// Holding down above this ground speed slides; below it you crouch.
        slide_min_speed: 110.0,
        slide_friction: 160.0,
        /// Braking when you hold against a slide.
        slide_brake: 900.0,
        /// Speed added when a slide starts (once per slide_boost_cooldown).
        slide_boost: 50.0,
        slide_boost_cooldown: 0.6,
        /// A slide this slow becomes a crouch.
        slide_stop_speed: 50.0,
        /// Crawling speed while crouched.
        crawl_speed: 110.0,
        /// Long jump (jump out of a slide): height in tiles.
        long_jump_height: 2.3,
        /// Long jump horizontal speed: slide speed times this...
        long_jump_mult: 1.1,
        /// ...but at least this.
        long_jump_min: 480.0,
    }
    "Walls" {
        /// Hit a wall at least this fast to run up it.
        wallrun_min_speed: 260.0,
        /// Upward speed from a wall-run = horizontal speed times this...
        wallrun_conversion: 1.0,
        /// ...clamped between these.
        wallrun_min_up: 420.0,
        wallrun_max_up: 760.0,
        /// Gravity multiplier while running up a wall.
        wallrun_gravity_mult: 0.6,
        /// 1 = running into a wall on the ground also starts a wall-run.
        ground_wallrun: 1.0,
        /// Max fall speed while sliding down a wall (holding toward it).
        wall_slide_speed: 160.0,
        /// Wall jump away speed...
        wall_kick_x: 400.0,
        /// ...plus this fraction of the speed you hit the wall with.
        wall_kick_keep: 0.45,
        wall_kick_y: 560.0,
        /// Reduced air control right after a wall jump.
        wall_kick_lock: 0.12,
        wall_coyote: 0.1,
    }
    "Vault & mantle" {
        /// Running into a step at most this many tiles high hops over it without losing speed.
        vault_max_height: 1.05,
        /// Minimum speed to vault (slower, you just bump into it).
        vault_min_speed: 150.0,
        /// In the air, grab ledges whose top is at most this far above your feet (px).
        mantle_reach: 20.0,
        /// Speed you keep moving at after a mantle (at least).
        mantle_speed: 220.0,
    }
    "Dash" {
        dash_speed: 760.0,
        dash_time: 0.14,
        dash_cooldown: 0.2,
        /// Horizontal speed kept when a dash ends (fraction).
        dash_end_keep: 0.85,
        /// 1 = touching the ground refills your dash (orbs refill it in mid-air).
        dash_ground_refill: 1.0,
        /// Landing out of a downward dash: slide speed = dash speed times this.
        wavedash_mult: 1.1,
    }
    "Bounce & pickups" {
        bounce_height: 5.0,
        bounce_high_height: 6.5,
        air_jump_height: 3.0,
        stomp_height: 1.8,
        stomp_high_height: 3.8,
    }
    "Camera" {
        /// How much the view zooms out at top speed (0.3 = 30% more visible).
        zoom_out: 0.3,
        /// How far ahead the camera looks, in seconds of travel.
        look_ahead_time: 0.35,
        look_ahead_max: 260.0,
    }
}

impl Tuning {
    /// Parse a tuning file on top of the defaults. Returns the unknown or bad lines.
    pub fn parse(src: &str) -> (Tuning, Vec<String>) {
        let mut t = Tuning::default();
        let mut bad = Vec::new();
        for line in src.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let ok = line
                .split_once('=')
                .and_then(|(k, v)| Some((k.trim(), v.trim().parse::<f32>().ok()?)))
                .is_some_and(|(k, v)| t.set(k, v));
            if !ok {
                bad.push(line.to_string());
            }
        }
        (t, bad)
    }

    /// Gravity that gives a `jump_height` jump peaking after `jump_time_to_apex`.
    pub fn gravity(&self) -> f32 {
        2.0 * self.jump_height * crate::level::TILE / (self.jump_time_to_apex * self.jump_time_to_apex)
    }

    /// Launch speed that reaches `tiles` high under normal gravity.
    pub fn jump_speed(&self, tiles: f32) -> f32 {
        (2.0 * self.gravity() * tiles.max(0.0) * crate::level::TILE).sqrt()
    }
}

thread_local! {
    static CURRENT: Cell<Tuning> = Cell::new(Tuning::default());
    static WATCH: RefCell<Watch> = RefCell::new(Watch { modified: None, timer: 0.0 });
}

struct Watch {
    modified: Option<SystemTime>,
    timer: f32,
}

/// The current tuning (defaults until `init` loads the file).
pub fn get() -> Tuning {
    CURRENT.with(|c| c.get())
}

pub fn path() -> PathBuf {
    crate::levels::levels_dir().parent().map(|p| p.join("tuning.txt")).unwrap_or_else(|| "tuning.txt".into())
}

/// Load the tuning file, writing the defaults out first if it doesn't exist.
pub fn init() -> Option<String> {
    let path = path();
    if !path.exists() {
        let _ = std::fs::write(&path, Tuning::default().to_text());
    }
    reload()
}

fn reload() -> Option<String> {
    let path = path();
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    WATCH.with(|w| w.borrow_mut().modified = modified);
    let src = std::fs::read_to_string(&path).ok()?;
    let (t, bad) = Tuning::parse(&src);
    let changed = t != get();
    CURRENT.with(|c| c.set(t));
    if let Some(line) = bad.first() {
        Some(format!("tuning.txt: couldn't read \"{line}\""))
    } else if changed {
        Some("Tuning reloaded".into())
    } else {
        None
    }
}

/// Call every frame: reloads the file when it changes. Returns a message to show.
pub fn poll(dt: f32) -> Option<String> {
    let due = WATCH.with(|w| {
        let mut w = w.borrow_mut();
        w.timer -= dt;
        if w.timer > 0.0 {
            return false;
        }
        w.timer = POLL_INTERVAL;
        let modified = std::fs::metadata(path()).and_then(|m| m.modified()).ok();
        modified.is_some() && modified != w.modified
    });
    if due {
        reload()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_round_trips() {
        let d = Tuning::default();
        let (t, bad) = Tuning::parse(&d.to_text());
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(t, d);
    }

    #[test]
    fn parse_overrides_and_reports_bad_lines() {
        let (t, bad) = Tuning::parse("run_speed = 123  # comment\nnope = 1\ntop_speed = fast\n");
        assert_eq!(t.run_speed, 123.0);
        assert_eq!(t.top_speed, Tuning::default().top_speed);
        assert_eq!(bad.len(), 2);
    }
}
