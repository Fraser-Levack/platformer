mod audio;
mod debug;
mod editor;
mod enemies;
mod game;
mod input;
mod level;
mod levels;
mod palette;
mod particles;
mod player;
mod render;
mod session;
mod speedometer;
mod surface;
mod synth;
mod trace;
mod tuning;

use editor::Editor;
use input::Input;
use macroquad::prelude::*;
use session::Session;

fn window_conf() -> Conf {
    Conf {
        window_title: "Cytoplasm".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        high_dpi: true,
        ..Default::default()
    }
}

const USAGE: &str = "\
Usage: platformer [--editor] [--level N] [--debug] [--screenshot FILE]

  --editor, -e       open the level editor
  --level N          start playing at level N (1-based)
  --debug            start with the movement debug overlay (F1) open
  --screenshot FILE  save a PNG after two seconds and quit
";

/// Frames to wait before taking a `--screenshot`.
const SCREENSHOT_FRAMES: u32 = 120;

struct Args {
    editor: bool,
    start_level: usize,
    debug: bool,
    screenshot: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args { editor: false, start_level: 0, debug: false, screenshot: None };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--editor" | "-e" => args.editor = true,
            "--debug" => args.debug = true,
            "--screenshot" => args.screenshot = it.next(),
            "--level" | "-l" => {
                args.start_level = it.next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(1).saturating_sub(1)
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => eprintln!("Ignoring unknown argument: {other}\n{USAGE}"),
        }
    }
    args
}

/// Normal play: every level in `assets/levels/`, in order.
struct Campaign {
    files: Vec<std::path::PathBuf>,
    index: usize,
    session: Session,
}

impl Campaign {
    fn new(start: usize) -> Self {
        let files = levels::list();
        let index = start.min(files.len() - 1);
        let session = Session::new(levels::load(&files[index]), None, true);
        Campaign { files, index, session }
    }

    fn frame(&mut self, input: &mut Input, dt: f32) {
        if self.session.update(input, dt) {
            self.index = (self.index + 1) % self.files.len();
            self.session = Session::new(levels::load(&self.files[self.index]), None, true);
        }
        let next = if self.files.len() == 1 {
            "play again"
        } else if self.index + 1 == self.files.len() {
            "start over from level 1"
        } else {
            "continue to the next level"
        };
        self.session.draw(input, dt, next);
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = parse_args();
    let mut input = Input::new();
    audio::init();
    if let Some(msg) = tuning::init() {
        debug::notify(msg);
    }
    if args.debug {
        debug::toggle();
    }
    let mut frames = 0u32;

    enum Mode {
        Play(Campaign),
        Edit(Box<Editor>),
    }
    let mut mode = if args.editor {
        Mode::Edit(Box::new(Editor::new()))
    } else {
        Mode::Play(Campaign::new(args.start_level))
    };

    loop {
        // Clamp so a hitch (window drag, breakpoint) doesn't cause a huge catch-up.
        let frame_dt = get_frame_time().min(0.1);
        input.poll(frame_dt);
        let typing = matches!(&mode, Mode::Edit(e) if e.typing());
        if !typing && is_key_pressed(KeyCode::M) {
            audio::toggle_music();
        }
        if !typing && is_key_pressed(KeyCode::N) {
            audio::toggle_sfx();
        }
        if is_key_pressed(KeyCode::F1) {
            debug::toggle();
        }
        if let Some(msg) = tuning::poll(frame_dt) {
            debug::notify(msg);
        }
        match &mut mode {
            Mode::Play(c) => c.frame(&mut input, frame_dt),
            Mode::Edit(e) => e.frame(&mut input, frame_dt),
        }
        audio::update(frame_dt);
        audio::draw_notice();
        debug::draw_notice(frame_dt);
        frames += 1;
        if let Some(path) = args.screenshot.as_deref().filter(|_| frames == SCREENSHOT_FRAMES) {
            get_screen_data().export_png(path);
            break;
        }
        next_frame().await;
    }
}
