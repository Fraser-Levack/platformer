//! Procedural audio: every sound effect and a music loop per theme are
//! synthesized in code (dark, minor-key, weighty),  so the game needs no audio files. Pure DSP only;
//! playback lives in `audio.rs`.

use std::f32::consts::TAU;

pub const SAMPLE_RATE: u32 = 44100;
const SR: f32 = SAMPLE_RATE as f32;
/// Fast fade applied when a tone with a fixed length is cut off.
const RELEASE: f32 = 0.25;

// ---------- Basics ----------

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed.wrapping_mul(2654435761).max(1))
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// 0..1
    fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.next() as usize % items.len()]
    }
}

fn midi_hz(note: i32) -> f32 {
    440.0 * 2f32.powf((note - 69) as f32 / 12.0)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Sine,
    Tri,
    /// Soft-clipped sine: buzzy but not harsh.
    Square,
    /// White noise through a lowpass whose cutoff follows the pitch.
    Noise,
}

/// One synthesized partial: pitch glides from `from` to `to`, amplitude has a
/// linear attack and exponential decay, optionally cut off after `len` seconds.
#[derive(Clone, Copy, Debug)]
pub struct Tone {
    wave: Wave,
    from: f32,
    to: f32,
    /// Time constant of the pitch glide, seconds.
    glide: f32,
    attack: f32,
    /// Time constant of the amplitude decay, seconds.
    decay: f32,
    /// Hard length (0 = let it ring out).
    len: f32,
    delay: f32,
    gain: f32,
    /// (rate Hz, depth as a fraction of the pitch)
    vibrato: (f32, f32),
}

fn tone(wave: Wave, from: f32, to: f32, glide: f32, decay: f32, gain: f32) -> Tone {
    Tone { wave, from, to, glide, attack: 0.003, decay, len: 0.0, delay: 0.0, gain, vibrato: (0.0, 0.0) }
}

impl Tone {
    fn at(self, delay: f32) -> Self {
        Tone { delay, ..self }
    }
    fn attack(self, attack: f32) -> Self {
        Tone { attack, ..self }
    }
    fn len(self, len: f32) -> Self {
        Tone { len, ..self }
    }
    fn vib(self, rate: f32, depth: f32) -> Self {
        Tone { vibrato: (rate, depth), ..self }
    }

    fn duration(&self) -> f32 {
        let natural = self.attack + self.decay * 7.0;
        if self.len > 0.0 {
            natural.min(self.len + RELEASE)
        } else {
            natural
        }
    }

    /// Render into `out` starting at sample `start`, wrapping around the end
    /// (so music notes that overhang the loop point continue at the start).
    fn render(&self, out: &mut [f32], start: usize, pitch: f32, rng: &mut Rng) {
        let n = (self.duration() * SR) as usize;
        let len = out.len();
        let mut phase = 0.0f32;
        let mut lp = 0.0f32;
        for i in 0..n {
            let t = i as f32 / SR;
            let mut f = (self.to + (self.from - self.to) * (-t / self.glide.max(1e-4)).exp()) * pitch;
            if self.vibrato.1 > 0.0 {
                f *= 1.0 + self.vibrato.1 * (TAU * self.vibrato.0 * t).sin();
            }
            phase = (phase + f / SR).fract();
            let s = match self.wave {
                Wave::Sine => (phase * TAU).sin(),
                Wave::Tri => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Square => ((phase * TAU).sin() * 3.0).tanh() * 0.7,
                Wave::Noise => {
                    let a = 1.0 - (-TAU * f.min(SR * 0.45) / SR).exp();
                    lp += (rng.range(-1.0, 1.0) - lp) * a;
                    // Keep loudness roughly constant whatever the cutoff.
                    lp * (2.0 / a - 1.0).sqrt().min(30.0)
                }
            };
            let mut env = (t / self.attack.max(1e-4)).min(1.0) * (-t / self.decay).exp();
            if self.len > 0.0 && t > self.len {
                env *= (1.0 - (t - self.len) / RELEASE).max(0.0);
            }
            out[(start + i) % len] += s * env * self.gain;
        }
    }
}

/// 16-bit PCM WAV from samples in -1..1 (interleaved when stereo).
pub fn wav(channels: u16, samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

// ---------- Sound effects ----------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sfx {
    Step,
    Jump,
    JumpBig,
    JumpLong,
    WallJump,
    Land,
    Bounce,
    AirJump,
    Dash,
    Slide,
    Perfect,
    WallRun,
    DropThrough,
    Death,
    Respawn,
    Nutrient,
    Orb,
    Bubble,
    Checkpoint,
    Goal,
    Stomp,
    Splash,
    CrumbleShake,
    Crumble,
    GateOpen,
    Pause,
    Click,
    Paint,
    Erase,
    Undo,
}

impl Sfx {
    pub const ALL: [Sfx; 30] = [
        Sfx::Step,
        Sfx::Jump,
        Sfx::JumpBig,
        Sfx::JumpLong,
        Sfx::WallJump,
        Sfx::Land,
        Sfx::Bounce,
        Sfx::AirJump,
        Sfx::Dash,
        Sfx::Slide,
        Sfx::Perfect,
        Sfx::WallRun,
        Sfx::DropThrough,
        Sfx::Death,
        Sfx::Respawn,
        Sfx::Nutrient,
        Sfx::Orb,
        Sfx::Bubble,
        Sfx::Checkpoint,
        Sfx::Goal,
        Sfx::Stomp,
        Sfx::Splash,
        Sfx::CrumbleShake,
        Sfx::Crumble,
        Sfx::GateOpen,
        Sfx::Pause,
        Sfx::Click,
        Sfx::Paint,
        Sfx::Erase,
        Sfx::Undo,
    ];

    /// File name (without extension) used to override the synthesized sound.
    pub fn name(self) -> String {
        format!("{self:?}").to_lowercase()
    }

    /// Pitch multipliers for the pre-rendered variants. Most sounds get a few
    /// random-pick variants so repeats don't sound robotic; nutrients climb a
    /// minor pentatonic scale as you collect them in quick succession.
    fn variants(self) -> Vec<f32> {
        let semis: &[i32] = match self {
            Sfx::Nutrient => &[0, 3, 5, 7, 10, 12, 15, 17], // minor pentatonic
            Sfx::Step => &[-2, -1, 0, 1, 2],
            Sfx::Jump | Sfx::Land | Sfx::Stomp | Sfx::Paint | Sfx::Erase | Sfx::AirJump | Sfx::WallJump => {
                &[-1, 0, 1]
            }
            _ => &[0],
        };
        semis.iter().map(|s| 2f32.powf(*s as f32 / 12.0)).collect()
    }

    fn recipe(self) -> Vec<Tone> {
        use Wave::*;
        // A bell-like pair: the 2.76x partial is inharmonic, which reads as cold metal.
        let bell = |f: f32, decay: f32, gain: f32| [tone(Sine, f, f, 0.1, decay, gain), tone(Sine, f * 2.76, f * 2.76, 0.1, decay * 0.3, gain * 0.35)];
        match self {
            // Soft wet footstep.
            Sfx::Step => vec![tone(Sine, 170.0, 95.0, 0.012, 0.025, 0.35), tone(Noise, 800.0, 450.0, 0.02, 0.012, 0.12)],
            // Short push of air with a low body, not a cartoon bloop.
            Sfx::Jump => vec![tone(Sine, 150.0, 300.0, 0.03, 0.06, 0.45), tone(Noise, 1800.0, 600.0, 0.03, 0.03, 0.14)],
            Sfx::JumpBig => vec![
                tone(Sine, 80.0, 340.0, 0.1, 0.18, 0.55),
                tone(Noise, 300.0, 2400.0, 0.12, 0.12, 0.18).attack(0.03),
            ],
            Sfx::JumpLong => vec![tone(Sine, 140.0, 300.0, 0.05, 0.08, 0.4), tone(Noise, 2400.0, 800.0, 0.08, 0.08, 0.16)],
            Sfx::WallJump => vec![
                tone(Noise, 2600.0, 1000.0, 0.02, 0.025, 0.28),
                tone(Sine, 220.0, 360.0, 0.03, 0.06, 0.4).at(0.01),
            ],
            // Thud.
            Sfx::Land => vec![tone(Sine, 150.0, 50.0, 0.035, 0.07, 0.65), tone(Noise, 1000.0, 300.0, 0.03, 0.035, 0.22)],
            // Taut membrane snapping back.
            Sfx::Bounce => vec![
                tone(Sine, 70.0, 45.0, 0.05, 0.12, 0.5),
                tone(Tri, 90.0, 320.0, 0.06, 0.18, 0.3),
                tone(Noise, 500.0, 2000.0, 0.06, 0.06, 0.12),
            ],
            // A gasp of air.
            Sfx::AirJump => vec![tone(Noise, 600.0, 2600.0, 0.05, 0.08, 0.25), tone(Sine, 260.0, 440.0, 0.04, 0.07, 0.3)],
            Sfx::Dash => vec![
                tone(Noise, 400.0, 5000.0, 0.06, 0.13, 0.38),
                tone(Square, 220.0, 90.0, 0.08, 0.1, 0.2),
            ],
            Sfx::Slide => vec![tone(Noise, 1200.0, 500.0, 0.2, 0.16, 0.25).attack(0.02)],
            // Bright rising fifth: a perfectly timed landing.
            Sfx::Perfect => vec![
                tone(Sine, 880.0, 880.0, 0.08, 0.12, 0.22),
                tone(Sine, 1318.5, 1318.5, 0.08, 0.16, 0.2).at(0.04),
                tone(Noise, 6000.0, 9000.0, 0.04, 0.03, 0.05),
            ],
            // Rising whoosh as you run up a wall.
            Sfx::WallRun => vec![tone(Noise, 500.0, 3200.0, 0.2, 0.12, 0.25).attack(0.03), tone(Sine, 180.0, 420.0, 0.15, 0.1, 0.25)],
            Sfx::DropThrough => vec![tone(Sine, 300.0, 110.0, 0.05, 0.09, 0.45), tone(Noise, 1200.0, 400.0, 0.05, 0.06, 0.15)],
            // Wet burst over a low boom.
            Sfx::Death => vec![
                tone(Noise, 3500.0, 250.0, 0.06, 0.22, 0.5),
                tone(Sine, 300.0, 40.0, 0.09, 0.3, 0.6),
                tone(Sine, 70.0, 30.0, 0.2, 0.5, 0.4),
                tone(Square, 160.0, 35.0, 0.12, 0.15, 0.2),
            ],
            // Low minor arpeggio swelling back.
            Sfx::Respawn => {
                let mut v: Vec<Tone> = [110.0, 130.81, 164.81]
                    .iter()
                    .enumerate()
                    .map(|(i, f)| tone(Tri, *f, *f, 0.1, 0.25, 0.25).attack(0.03).at(i as f32 * 0.06))
                    .collect();
                v.push(tone(Noise, 300.0, 1500.0, 0.15, 0.06, 0.12).attack(0.12));
                v
            }
            // Small cold bell.
            Sfx::Nutrient => bell(659.25, 0.18, 0.35).to_vec(),
            Sfx::Orb => {
                let mut v = vec![tone(Sine, 330.0, 330.0, 0.1, 0.35, 0.3), tone(Sine, 392.0, 392.0, 0.1, 0.3, 0.22).at(0.04)];
                v.push(tone(Noise, 3000.0, 3000.0, 0.1, 0.12, 0.06).attack(0.03));
                v
            }
            Sfx::Bubble => vec![tone(Sine, 300.0, 520.0, 0.04, 0.12, 0.35), tone(Noise, 900.0, 2200.0, 0.05, 0.06, 0.1)],
            // Minor third tolled on a bell.
            Sfx::Checkpoint => {
                let mut v = bell(523.25, 0.5, 0.3).to_vec();
                v.extend(bell(622.25, 0.6, 0.28).map(|t| t.at(0.12)));
                v
            }
            // Not a fanfare: a dark chord settling, with a bell and a deep boom.
            Sfx::Goal => {
                let mut v: Vec<Tone> = [110.0, 164.81, 220.0, 261.63]
                    .iter()
                    .enumerate()
                    .map(|(i, f)| tone(Tri, *f, *f, 0.1, 1.0, 0.16).attack(0.04).at(i as f32 * 0.05))
                    .collect();
                v.extend(bell(880.0, 0.9, 0.18).map(|t| t.at(0.2)));
                v.push(tone(Sine, 80.0, 50.0, 0.3, 0.8, 0.45));
                v
            }
            // Crunch.
            Sfx::Stomp => vec![tone(Sine, 260.0, 60.0, 0.04, 0.1, 0.55), tone(Noise, 1600.0, 500.0, 0.03, 0.06, 0.3)],
            // Something dissolving in acid.
            Sfx::Splash => vec![
                tone(Noise, 2500.0, 400.0, 0.1, 0.2, 0.3),
                tone(Noise, 5000.0, 5000.0, 0.1, 0.25, 0.06).attack(0.05),
            ],
            Sfx::CrumbleShake => vec![
                tone(Noise, 450.0, 450.0, 0.1, 0.15, 0.2).vib(25.0, 0.5),
                tone(Sine, 70.0, 70.0, 0.1, 0.1, 0.2).vib(25.0, 0.2),
            ],
            Sfx::Crumble => vec![tone(Noise, 1000.0, 180.0, 0.12, 0.25, 0.35), tone(Sine, 120.0, 40.0, 0.1, 0.15, 0.3)],
            // Heavy sinew grinding apart.
            Sfx::GateOpen => vec![
                tone(Noise, 320.0, 90.0, 0.25, 0.4, 0.4).attack(0.04).vib(18.0, 0.4),
                tone(Sine, 75.0, 38.0, 0.3, 0.6, 0.5),
                tone(Square, 58.0, 52.0, 0.3, 0.3, 0.14).attack(0.05),
                tone(Sine, 220.0, 207.65, 0.3, 0.5, 0.1).at(0.1),
            ],
            Sfx::Pause => vec![tone(Sine, 330.0, 330.0, 0.1, 0.06, 0.25), tone(Sine, 246.9, 246.9, 0.1, 0.1, 0.25).at(0.06)],
            Sfx::Click => vec![tone(Sine, 1100.0, 900.0, 0.01, 0.018, 0.22)],
            Sfx::Paint => vec![tone(Sine, 420.0, 560.0, 0.01, 0.03, 0.22)],
            Sfx::Erase => vec![tone(Sine, 330.0, 200.0, 0.015, 0.03, 0.22), tone(Noise, 1500.0, 1500.0, 0.1, 0.015, 0.06)],
            Sfx::Undo => vec![tone(Tri, 520.0, 350.0, 0.03, 0.05, 0.2)],
        }
    }

    /// Mono samples for every variant.
    pub fn render(self) -> Vec<Vec<f32>> {
        let tones = self.recipe();
        let len = tones.iter().map(|t| t.delay + t.duration()).fold(0.0, f32::max);
        let n = (len * SR) as usize + 1;
        self.variants()
            .into_iter()
            .enumerate()
            .map(|(i, pitch)| {
                let mut rng = Rng::new(self as u32 * 31 + i as u32 + 1);
                let mut out = vec![0.0; n];
                for t in &tones {
                    // Delays don't scale with pitch, so `out` never wraps here.
                    t.render(&mut out, (t.delay * SR) as usize, pitch, &mut rng);
                }
                for s in &mut out {
                    *s = s.tanh();
                }
                out
            })
            .collect()
    }
}

// ---------- Music ----------

const MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 10];
const HARMONIC_MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 11];
const PHRYGIAN: &[i32] = &[0, 1, 3, 5, 7, 8, 10];

/// Musical character for one colour theme (same order as `palette::THEMES`).
struct Style {
    bpm: f32,
    /// MIDI note of the key's root (low; pads sit an octave up, bass an octave down).
    root: i32,
    scale: &'static [i32],
    /// Chord roots as scale degrees, two bars each.
    prog: [i32; 4],
    /// 0 = no drums, 1 = heartbeat, 2 = driving.
    drums: u8,
    lead: Wave,
    /// Chance of a melody note on each eighth.
    density: f32,
    /// Reverb mix.
    space: f32,
    seed: u32,
}

const STYLES: [Style; 6] = [
    // Necrosis: slow, heavy, a heartbeat under everything.
    Style { bpm: 76.0, root: 45, scale: MINOR, prog: [0, 5, 3, 4], drums: 1, lead: Wave::Tri, density: 0.35, space: 0.35, seed: 11 },
    // Ichor: the flat second of Phrygian keeps it uneasy.
    Style { bpm: 92.0, root: 43, scale: PHRYGIAN, prog: [0, 1, 0, 6], drums: 2, lead: Wave::Tri, density: 0.4, space: 0.25, seed: 23 },
    // Rot: crawling and sparse.
    Style { bpm: 70.0, root: 41, scale: MINOR, prog: [0, 6, 5, 4], drums: 1, lead: Wave::Sine, density: 0.3, space: 0.4, seed: 37 },
    // Fever: harmonic minor's raised seventh pulls hard toward the root.
    Style { bpm: 100.0, root: 44, scale: HARMONIC_MINOR, prog: [0, 3, 5, 4], drums: 2, lead: Wave::Square, density: 0.45, space: 0.25, seed: 41 },
    // Frost: almost still, all space.
    Style { bpm: 62.0, root: 42, scale: MINOR, prog: [0, 3, 0, 5], drums: 0, lead: Wave::Sine, density: 0.25, space: 0.5, seed: 53 },
    // Cinder: fast and relentless.
    Style { bpm: 116.0, root: 40, scale: PHRYGIAN, prog: [0, 1, 0, 6], drums: 2, lead: Wave::Square, density: 0.5, space: 0.2, seed: 67 },
];

const BARS: usize = 16;

impl Style {
    fn note(&self, degree: i32) -> i32 {
        let n = self.scale.len() as i32;
        self.root + 12 * degree.div_euclid(n) + self.scale[degree.rem_euclid(n) as usize]
    }
}

/// Stereo buffers being mixed.
struct Mix {
    l: Vec<f32>,
    r: Vec<f32>,
    scratch: Vec<f32>,
    rng: Rng,
}

impl Mix {
    /// Add a tone at `time` seconds, panned -1 (left) .. 1 (right).
    fn add(&mut self, t: Tone, time: f32, pan: f32) {
        let start = (time * SR) as usize;
        let n = self.l.len();
        let dur = ((t.duration() * SR) as usize).min(n);
        self.scratch.clear();
        self.scratch.resize(dur.max(1), 0.0);
        t.render(&mut self.scratch, 0, 1.0, &mut self.rng);
        let (gl, gr) = ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0));
        for (i, s) in self.scratch.iter().enumerate() {
            let k = (start + i) % n;
            self.l[k] += s * gl;
            self.r[k] += s * gr;
        }
    }
}

/// Interleaved stereo samples of a seamless music loop for a theme.
pub fn music(theme: usize) -> Vec<f32> {
    let st = &STYLES[theme % STYLES.len()];
    let beat = 60.0 / st.bpm;
    let bar = beat * 4.0;
    let n = (BARS as f32 * bar * SR) as usize;
    let mut drums = Mix { l: vec![0.0; n], r: vec![0.0; n], scratch: Vec::new(), rng: Rng::new(st.seed) };
    let mut dry = Mix { l: vec![0.0; n], r: vec![0.0; n], scratch: Vec::new(), rng: Rng::new(st.seed + 1) };
    // Melody and bells go through a delay as well.
    let mut echo = Mix { l: vec![0.0; n], r: vec![0.0; n], scratch: Vec::new(), rng: Rng::new(st.seed + 2) };

    // Drone on the root: two slightly detuned sines that slowly beat against
    // each other. Frequencies are rounded to whole cycles per loop so it's seamless.
    let loop_secs = n as f32 / SR;
    let whole = |f: f32| (f * loop_secs).round() / loop_secs;
    let (d1, d2) = (whole(midi_hz(st.root - 12)), whole(midi_hz(st.root - 12) * 1.004));
    let d5 = whole(midi_hz(st.root - 5)); // fifth above, quieter
    let swell = whole(1.0 / (bar * 4.0));
    for i in 0..n {
        let t = i as f32 / SR;
        let amp = 0.75 + 0.25 * (TAU * swell * t).sin();
        let s = ((TAU * d1 * t).sin() + (TAU * d2 * t).sin()) * 0.06 + (TAU * d5 * t).sin() * 0.02;
        dry.l[i] += s * amp;
        dry.r[i] += s * amp;
    }

    for b in 0..BARS {
        let t0 = b as f32 * bar;
        let chord = st.prog[(b / 2) % 4];
        let first_of_chord = b % 2 == 0;

        // Pad: a dark triad held for the chord's two bars.
        if first_of_chord {
            for (k, d) in [0, 2, 4].iter().enumerate() {
                let f = midi_hz(st.note(chord + d) + 12);
                for detune in [0.997, 1.003] {
                    let pan = (k as f32 - 1.0) * 0.5 + (detune - 1.0) * 100.0;
                    let t = tone(Wave::Tri, f * detune, f * detune, 0.1, 6.0, 0.03).attack(1.0).len(bar * 2.0 - 0.1);
                    dry.add(t, t0, pan);
                }
            }
        }

        // Bass.
        let pattern: &[f32] = match st.drums {
            0 => &[0.0],
            1 => &[0.0, 2.5],
            _ => &[0.0, 0.5, 1.5, 2.0, 3.0],
        };
        for (i, p) in pattern.iter().enumerate() {
            let deg = if i == pattern.len() - 1 && b % 2 == 1 { chord + 4 } else { chord };
            let f = midi_hz(st.note(deg) - 12);
            let decay = if st.drums == 0 { 2.0 } else { 0.4 };
            let len = if st.drums == 0 { bar * 0.95 } else { beat * 0.9 };
            dry.add(tone(Wave::Sine, f, f, 0.02, decay, 0.32).attack(0.01).len(len), t0 + p * beat, 0.0);
            dry.add(tone(Wave::Square, f, f, 0.1, decay * 0.4, 0.05).len(len), t0 + p * beat, 0.0);
        }

        // Drums: a heartbeat (lub-dub) for the slow themes, a driving pulse for the fast ones.
        if st.drums > 0 {
            let kick = |g: f32| tone(Wave::Sine, 120.0, 38.0, 0.035, 0.22, g);
            let hits: &[(f32, f32)] = if st.drums == 1 {
                &[(0.0, 0.5), (0.3, 0.32)]
            } else {
                &[(0.0, 0.5), (0.3, 0.3), (2.0, 0.5), (2.75, 0.3)]
            };
            for (k, g) in hits {
                drums.add(kick(*g), t0 + k * beat, 0.0);
            }
            if st.drums == 2 {
                for s in [1.0, 3.0] {
                    drums.add(tone(Wave::Noise, 900.0, 700.0, 0.05, 0.12, 0.14), t0 + s * beat, 0.05);
                }
                for e in 0..8 {
                    if e % 2 == 1 {
                        drums.add(tone(Wave::Noise, 6000.0, 6000.0, 0.1, 0.015, 0.035), t0 + e as f32 * beat / 2.0, 0.25);
                    }
                }
            }
        }

        // Melody: 4-bar phrases in an A A' B A shape. The same phrase id
        // re-seeds the generator, so the motif repeats; pitches still follow the chord.
        let phrase = [0, 1, 2, 3, 0, 1, 2, 4, 5, 6, 7, 8, 0, 1, 2, 3][b];
        let mut rng = Rng::new(st.seed * 1000 + phrase);
        let mut deg = chord + 7 + rng.pick(&[0, 2, 4]);
        for e in 0..8 {
            let strong = e % 4 == 0;
            let p = if strong { st.density + 0.25 } else { st.density };
            let step = rng.pick(&[-2, -1, -1, 1, 1, 2, 0, 3, -3]);
            let play = rng.chance(p);
            if !play {
                continue;
            }
            deg = if strong {
                // Land on the nearest chord tone.
                let rel = (deg - chord).rem_euclid(7);
                deg - rel + [0, 0, 2, 2, 4, 4, 7][rel as usize]
            } else {
                (deg + step).clamp(chord + 3, chord + 13)
            };
            let f = midi_hz(st.note(deg));
            let dur = if rng.chance(0.4) { beat } else { beat * 0.5 };
            let pan = if e % 2 == 0 { -0.2 } else { 0.2 };
            let t = tone(st.lead, f * 0.995, f, 0.02, beat * 0.9, 0.1).attack(0.01).len(dur * 0.9);
            echo.add(t, t0 + e as f32 * beat / 2.0, pan);
        }

        // Distant bells on chord tones, rarely.
        let mut rng = Rng::new(st.seed * 7 + b as u32);
        for s in 0..16 {
            if rng.chance(0.035) {
                let f = midi_hz(st.note(chord + rng.pick(&[0, 2, 4])) + 24);
                let pan = rng.range(-0.7, 0.7);
                echo.add(tone(Wave::Sine, f, f, 0.1, 1.2, 0.04), t0 + s as f32 * beat / 4.0, pan);
                echo.add(tone(Wave::Sine, f * 2.76, f * 2.76, 0.1, 0.3, 0.015), t0 + s as f32 * beat / 4.0, pan);
            }
        }
    }

    // Ping-pong delay on the melody bus (dotted eighth).
    ping_pong(&mut echo.l, &mut echo.r, (beat * 0.75 * SR) as usize, 0.4, 0.35);

    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    for i in 0..n {
        l[i] = dry.l[i] + echo.l[i];
        r[i] = dry.r[i] + echo.r[i];
    }
    reverb(&mut l, &mut r, st.space);
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        out.push(((l[i] + drums.l[i]) * 1.1).tanh() * 0.85);
        out.push(((r[i] + drums.r[i]) * 1.1).tanh() * 0.85);
    }
    out
}

/// Feedback delay bouncing between channels. Runs over the loop twice so
/// the echoes wrap seamlessly from the end back to the start.
fn ping_pong(l: &mut [f32], r: &mut [f32], delay: usize, feedback: f32, wet: f32) {
    let n = l.len();
    let (dry_l, dry_r) = (l.to_vec(), r.to_vec());
    let (mut bl, mut br) = (vec![0.0f32; delay], vec![0.0f32; delay]);
    let mut k = 0;
    for pass in 0..2 {
        for i in 0..n {
            let (el, er) = (bl[k], br[k]);
            bl[k] = dry_l[i] * 0.5 + dry_r[i] * 0.5 + er * feedback;
            br[k] = el * feedback;
            k = (k + 1) % delay;
            if pass == 1 {
                l[i] = dry_l[i] + el * wet;
                r[i] = dry_r[i] + er * wet;
            }
        }
    }
}

/// Small Schroeder reverb (parallel combs into series allpasses), run twice
/// around the loop like the delay.
fn reverb(l: &mut [f32], r: &mut [f32], wet: f32) {
    fn channel(x: &mut [f32], combs: &[usize], wet: f32) {
        let n = x.len();
        let dry = x.to_vec();
        let mut cbuf: Vec<Vec<f32>> = combs.iter().map(|&d| vec![0.0; d]).collect();
        let mut cidx = vec![0usize; combs.len()];
        let mut damp = vec![0.0f32; combs.len()];
        let mut abuf = [vec![0.0f32; 556], vec![0.0f32; 441]];
        let mut aidx = [0usize; 2];
        for pass in 0..2 {
            for i in 0..n {
                let input = dry[i] * 0.3;
                let mut acc = 0.0;
                for c in 0..combs.len() {
                    let y = cbuf[c][cidx[c]];
                    damp[c] = y * 0.7 + damp[c] * 0.3;
                    cbuf[c][cidx[c]] = input + damp[c] * 0.82;
                    cidx[c] = (cidx[c] + 1) % cbuf[c].len();
                    acc += y;
                }
                for a in 0..2 {
                    let y = abuf[a][aidx[a]];
                    abuf[a][aidx[a]] = acc + y * 0.5;
                    acc = y - acc * 0.5;
                    aidx[a] = (aidx[a] + 1) % abuf[a].len();
                }
                if pass == 1 {
                    x[i] = dry[i] + acc * wet * 0.5;
                }
            }
        }
    }
    channel(l, &[1557, 1617, 1491, 1422], wet);
    channel(r, &[1580, 1640, 1514, 1445], wet);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sfx_renders_audible_and_bounded() {
        for s in Sfx::ALL {
            for v in s.render() {
                assert!(!v.is_empty(), "{s:?} is empty");
                let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
                assert!(peak > 0.05 && peak <= 1.0, "{s:?} peak {peak}");
                assert!(v.iter().all(|x| x.is_finite()));
            }
        }
    }

    #[test]
    fn wav_header_matches_data() {
        let w = wav(2, &[0.0, 0.5, -0.5, 1.0]);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
    }

    #[test]
    fn music_loops_are_stereo_and_bounded() {
        let m = music(4); // the sparsest style keeps the test quick
        assert_eq!(m.len() % 2, 0);
        let secs = m.len() as f32 / 2.0 / SR;
        assert!(secs > 20.0, "loop is {secs}s");
        let peak = m.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(peak > 0.1 && peak < 1.0, "peak {peak}");
    }
}
