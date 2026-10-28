//! Sound playback: effects, per-theme music with crossfades, mute toggles.
//!
//! Everything is synthesized by `synth.rs`. To replace a sound with a real
//! recording, drop a file into `assets/sounds/<name>.wav|.ogg` (names from
//! `Sfx::name`, e.g. `jump.ogg`) or `assets/music/<theme>.wav|.ogg` (e.g. `necrosis.ogg`).
//!
//! All functions are no-ops until `init` has run, so game logic stays testable.

use crate::palette::THEMES;
use crate::synth::{self, Sfx};
use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

const SFX_VOLUME: f32 = 0.7;
const MUSIC_VOLUME: f32 = 0.45;
/// Music volume change per second (fades in/out and ducking).
const FADE_SPEED: f32 = 0.8;
/// The same effect won't retrigger faster than this (stops pile-ups).
const MIN_GAP: f64 = 0.035;
/// Nutrients collected within this many seconds climb in pitch.
const COMBO_WINDOW: f64 = 0.9;
/// Generated music tracks kept in memory.
const MUSIC_CACHE: usize = 3;
const NOTICE_TIME: f32 = 1.5;

struct Track {
    theme: usize,
    sound: Sound,
    vol: f32,
}

struct Audio {
    sfx: HashMap<Sfx, Vec<Sound>>,
    last_played: HashMap<Sfx, f64>,
    combo: (usize, f64),
    sfx_on: bool,
    music_on: bool,
    /// Theme and loudness (0..1) the game currently wants.
    want: Option<(usize, f32)>,
    playing: Option<Track>,
    fading: Vec<Track>,
    /// Loaded music, most recently used last.
    cache: Vec<(usize, Sound)>,
    pending: Vec<(usize, Receiver<Vec<u8>>)>,
    notice: Option<(String, f32)>,
}

thread_local! {
    static AUDIO: RefCell<Option<Audio>> = const { RefCell::new(None) };
}

fn with(f: impl FnOnce(&mut Audio)) {
    AUDIO.with(|a| {
        if let Some(a) = a.borrow_mut().as_mut() {
            f(a)
        }
    });
}

fn assets_dir(sub: &str) -> PathBuf {
    let local = PathBuf::from("assets").join(sub);
    if local.is_dir() {
        return local;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join(sub)
}

/// A user-supplied replacement file, if there is one.
fn override_file(sub: &str, name: &str) -> Option<Vec<u8>> {
    let dir = assets_dir(sub);
    ["ogg", "wav"].iter().find_map(|ext| std::fs::read(dir.join(format!("{name}.{ext}"))).ok())
}

/// Sound loading only waits on wasm; on desktop the future is ready immediately.
fn load(bytes: &[u8]) -> Option<Sound> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let mut fut = std::pin::pin!(load_sound_from_bytes(bytes));
    match fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Ok(s)) => Some(s),
        _ => None,
    }
}

/// Synthesize (or load) all sound effects. Music is generated lazily in the background.
pub fn init() {
    let mut sfx = HashMap::new();
    for s in Sfx::ALL {
        let sounds: Vec<Sound> = match override_file("sounds", &s.name()) {
            Some(bytes) => load(&bytes).into_iter().collect(),
            None => s.render().iter().filter_map(|v| load(&synth::wav(1, v))).collect(),
        };
        if !sounds.is_empty() {
            sfx.insert(s, sounds);
        }
    }
    let audio = Audio {
        sfx,
        last_played: HashMap::new(),
        combo: (0, 0.0),
        sfx_on: true,
        music_on: true,
        want: None,
        playing: None,
        fading: Vec::new(),
        cache: Vec::new(),
        pending: Vec::new(),
        notice: None,
    };
    AUDIO.with(|a| *a.borrow_mut() = Some(audio));
}

pub fn play(s: Sfx) {
    play_vol(s, 1.0);
}

/// Play an effect at a relative volume (0..1).
pub fn play_vol(s: Sfx, volume: f32) {
    with(|a| {
        if !a.sfx_on {
            return;
        }
        let now = get_time();
        if a.last_played.get(&s).is_some_and(|t| now - t < MIN_GAP) {
            return;
        }
        let Some(variants) = a.sfx.get(&s) else { return };
        let i = if s == Sfx::Nutrient {
            let step = if now - a.combo.1 < COMBO_WINDOW { a.combo.0 + 1 } else { 0 };
            a.combo = (step, now);
            step.min(variants.len() - 1)
        } else {
            gen_range(0, variants.len())
        };
        a.last_played.insert(s, now);
        play_sound(&variants[i], PlaySoundParams { looped: false, volume: volume.clamp(0.0, 1.0) * SFX_VOLUME });
    });
}

/// Which theme's music should play, and how loud (0..1, for ducking). Call every frame.
pub fn music(theme: usize, level: f32) {
    with(|a| a.want = Some((theme, level)));
}

pub fn toggle_music() {
    with(|a| {
        a.music_on = !a.music_on;
        let msg = if a.music_on { "Music on" } else { "Music off" };
        a.notice = Some((msg.into(), NOTICE_TIME));
    });
}

pub fn toggle_sfx() {
    with(|a| {
        a.sfx_on = !a.sfx_on;
        let msg = if a.sfx_on { "Sound effects on" } else { "Sound effects off" };
        a.notice = Some((msg.into(), NOTICE_TIME));
    });
}

/// Run once per frame: music generation, crossfades, notices.
pub fn update(dt: f32) {
    with(|a| a.update(dt));
}

impl Audio {
    fn update(&mut self, dt: f32) {
        if let Some((_, t)) = &mut self.notice {
            *t -= dt;
            if *t <= 0.0 {
                self.notice = None;
            }
        }

        // Collect finished background renders.
        let mut i = 0;
        while i < self.pending.len() {
            match self.pending[i].1.try_recv() {
                Ok(bytes) => {
                    let theme = self.pending.remove(i).0;
                    if let Some(sound) = load(&bytes) {
                        self.cache.push((theme, sound));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                Err(_) => {
                    self.pending.remove(i);
                }
            }
        }

        let target = self.want.filter(|_| self.music_on);
        let target_theme = target.map(|(t, _)| t);

        // Switching themes (or muting): fade the current track out.
        if self.playing.as_ref().is_some_and(|p| Some(p.theme) != target_theme) {
            self.fading.push(self.playing.take().unwrap());
        }
        if let (None, Some(theme)) = (&self.playing, target_theme) {
            if let Some(i) = self.fading.iter().position(|f| f.theme == theme) {
                // Coming back to a track that's still fading: just fade it back in.
                self.playing = Some(self.fading.remove(i));
            } else if let Some(i) = self.cache.iter().position(|c| c.0 == theme) {
                let entry = self.cache.remove(i);
                play_sound(&entry.1, PlaySoundParams { looped: true, volume: 0.0 });
                self.playing = Some(Track { theme, sound: entry.1.clone(), vol: 0.0 });
                self.cache.push(entry);
            } else if !self.pending.iter().any(|p| p.0 == theme) {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let name = THEMES[theme % THEMES.len()].name.to_lowercase();
                    let bytes = override_file("music", &name).unwrap_or_else(|| synth::wav(2, &synth::music(theme)));
                    let _ = tx.send(bytes);
                });
                self.pending.push((theme, rx));
            }
        }

        let step = FADE_SPEED * dt;
        if let (Some(p), Some((_, level))) = (&mut self.playing, target) {
            let goal = level * MUSIC_VOLUME;
            p.vol += (goal - p.vol).clamp(-step, step);
            set_sound_volume(&p.sound, p.vol);
        }
        for f in &mut self.fading {
            f.vol = (f.vol - step).max(0.0);
            if f.vol > 0.0 {
                set_sound_volume(&f.sound, f.vol);
            } else {
                stop_sound(&f.sound);
            }
        }
        self.fading.retain(|f| f.vol > 0.0);

        // Drop the least recently used tracks that aren't audible.
        while self.cache.len() > MUSIC_CACHE {
            let busy = |t: usize| {
                self.playing.as_ref().is_some_and(|p| p.theme == t) || self.fading.iter().any(|f| f.theme == t)
            };
            match self.cache.iter().position(|c| !busy(c.0)) {
                Some(i) => {
                    self.cache.remove(i);
                }
                None => break,
            }
        }
    }
}

/// Small toast for the mute toggles.
pub fn draw_notice() {
    AUDIO.with(|a| {
        let a = a.borrow();
        let Some((msg, t)) = a.as_ref().and_then(|a| a.notice.as_ref()) else { return };
        let k = (t / 0.3).min(1.0);
        let s = (screen_width() / 1280.0).min(screen_height() / 720.0).clamp(0.45, 2.0);
        let size = 22.0 * s;
        let w = measure_text(msg, None, size as u16, 1.0).width;
        let (x, y) = (screen_width() - w - 24.0 * s, screen_height() - 52.0 * s);
        draw_rectangle(x - 10.0 * s, y - 24.0 * s, w + 20.0 * s, 34.0 * s, Color::new(0.0, 0.0, 0.0, 0.55 * k));
        draw_text(msg, x, y, size, Color::new(1.0, 1.0, 1.0, k));
    });
}
