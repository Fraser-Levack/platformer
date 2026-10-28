//! Unified keyboard + gamepad input.
//!
//! Gamepads are handled through gilrs: controllers can be plugged in or
//! unplugged at any time, and whichever controller was used most recently
//! becomes the active one.

use gilrs::{Axis, Button, EventType, GamepadId, Gilrs, PowerInfo};
use macroquad::prelude::*;

const STICK_DEADZONE: f32 = 0.2;
const NOTICE_TIME: f32 = 3.0;

#[derive(Default, Clone, Copy, Debug)]
pub struct InputState {
    /// -1..1, analog when using a stick.
    pub move_x: f32,
    /// -1..1, positive is down.
    pub move_y: f32,
    pub jump_held: bool,
    /// Latched until consumed by a physics step, so quick taps are never lost.
    pub jump_pressed: bool,
    /// Dash (in mid-air, when you have one stored).
    pub dash_held: bool,
    pub dash_pressed: bool,
    // One-frame flags (reset every poll).
    pub pause_pressed: bool,
    pub restart_pressed: bool,
    /// Controller Back/Select (restart in play, exit in editor playtest).
    pub select_pressed: bool,
    pub confirm_pressed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Device {
    Keyboard,
    Gamepad,
}

pub struct Input {
    gilrs: Option<Gilrs>,
    active_pad: Option<GamepadId>,
    pub state: InputState,
    pub last_device: Device,
    notice: Option<(String, f32)>,
}

impl Input {
    pub fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("Gamepad support unavailable: {e}");
                None
            }
        };
        let mut input = Input {
            gilrs,
            active_pad: None,
            state: InputState::default(),
            last_device: Device::Keyboard,
            notice: None,
        };
        let first = input
            .gilrs
            .as_ref()
            .and_then(|g| g.gamepads().next().map(|(id, pad)| (id, pad.name().to_string())));
        if let Some((id, name)) = first {
            input.active_pad = Some(id);
            input.last_device = Device::Gamepad;
            input.notify(format!("Controller connected: {name}"));
        }
        input
    }

    fn notify(&mut self, msg: String) {
        self.notice = Some((msg, NOTICE_TIME));
    }

    /// Call once per rendered frame.
    pub fn poll(&mut self, frame_dt: f32) {
        self.state.pause_pressed = false;
        self.state.restart_pressed = false;
        self.state.select_pressed = false;
        self.state.confirm_pressed = false;

        if let Some((_, t)) = &mut self.notice {
            *t -= frame_dt;
            if *t <= 0.0 {
                self.notice = None;
            }
        }

        // ---- Gamepad ----
        let mut pad_move = Vec2::ZERO;
        let mut pad_jump_held = false;
        let mut pad_dash_held = false;
        let mut messages = Vec::new();

        if let Some(gilrs) = &mut self.gilrs {
            while let Some(ev) = gilrs.next_event() {
                match ev.event {
                    EventType::Connected => {
                        let name = gilrs.gamepad(ev.id).name().to_string();
                        if self.active_pad.is_none() {
                            self.active_pad = Some(ev.id);
                            self.last_device = Device::Gamepad;
                        }
                        messages.push(format!("Controller connected: {name}"));
                    }
                    EventType::Disconnected => {
                        if self.active_pad == Some(ev.id) {
                            // Fall back to any other connected controller.
                            self.active_pad =
                                gilrs.gamepads().map(|(id, _)| id).find(|id| *id != ev.id);
                            if self.active_pad.is_none() {
                                self.last_device = Device::Keyboard;
                            }
                        }
                        messages.push("Controller disconnected".to_string());
                    }
                    EventType::ButtonPressed(button, _) => {
                        self.active_pad = Some(ev.id);
                        self.last_device = Device::Gamepad;
                        match button {
                            Button::South => {
                                self.state.jump_pressed = true;
                                self.state.confirm_pressed = true;
                            }
                            Button::West
                            | Button::RightTrigger
                            | Button::RightTrigger2
                            | Button::LeftTrigger2 => self.state.dash_pressed = true,
                            Button::Start => self.state.pause_pressed = true,
                            Button::Select => self.state.select_pressed = true,
                            _ => {}
                        }
                    }
                    EventType::AxisChanged(_, value, _) if value.abs() > 0.5 => {
                        self.active_pad = Some(ev.id);
                        self.last_device = Device::Gamepad;
                    }
                    _ => {}
                }
            }

            if let Some(pad) = self.active_pad.and_then(|id| gilrs.connected_gamepad(id)) {
                // Radial deadzone, rescaled so movement starts smoothly from 0.
                let mut v = vec2(pad.value(Axis::LeftStickX), -pad.value(Axis::LeftStickY));
                let mag = v.length();
                v = if mag < STICK_DEADZONE {
                    Vec2::ZERO
                } else {
                    v / mag * ((mag - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).min(1.0)
                };
                // Make full speed easy to hold.
                if v.x.abs() > 0.85 {
                    v.x = v.x.signum();
                }
                if pad.is_pressed(Button::DPadLeft) {
                    v.x = -1.0;
                }
                if pad.is_pressed(Button::DPadRight) {
                    v.x = 1.0;
                }
                if pad.is_pressed(Button::DPadUp) {
                    v.y = -1.0;
                }
                if pad.is_pressed(Button::DPadDown) {
                    v.y = 1.0;
                }
                pad_move = v;
                pad_jump_held = pad.is_pressed(Button::South);
                pad_dash_held = [Button::West, Button::RightTrigger, Button::RightTrigger2, Button::LeftTrigger2]
                    .iter()
                    .any(|b| pad.is_pressed(*b));
            }
        }
        for m in messages {
            self.notify(m);
        }

        // ---- Keyboard ----
        let down = |keys: &[KeyCode]| keys.iter().any(|k| is_key_down(*k));
        let pressed = |keys: &[KeyCode]| keys.iter().any(|k| is_key_pressed(*k));

        const LEFT: &[KeyCode] = &[KeyCode::A, KeyCode::Left];
        const RIGHT: &[KeyCode] = &[KeyCode::D, KeyCode::Right];
        const UP: &[KeyCode] = &[KeyCode::W, KeyCode::Up];
        const DOWN: &[KeyCode] = &[KeyCode::S, KeyCode::Down];
        const JUMP: &[KeyCode] = &[KeyCode::Space, KeyCode::Z, KeyCode::K, KeyCode::C];
        const DASH: &[KeyCode] = &[KeyCode::X, KeyCode::J, KeyCode::LeftShift];

        let kb_x = down(RIGHT) as i32 as f32 - down(LEFT) as i32 as f32;
        let kb_y = down(DOWN) as i32 as f32 - down(UP) as i32 as f32;
        let kb_jump_held = down(JUMP);

        if pressed(JUMP) {
            self.state.jump_pressed = true;
        }
        if pressed(DASH) {
            self.state.dash_pressed = true;
        }
        if pressed(&[KeyCode::Escape, KeyCode::P]) {
            self.state.pause_pressed = true;
        }
        if pressed(&[KeyCode::R]) {
            self.state.restart_pressed = true;
        }
        if pressed(&[KeyCode::Enter, KeyCode::Space]) {
            self.state.confirm_pressed = true;
        }
        if get_last_key_pressed().is_some() {
            self.last_device = Device::Keyboard;
        }

        // ---- Combine: keyboard wins per-axis when held ----
        self.state.move_x = if kb_x != 0.0 { kb_x } else { pad_move.x };
        self.state.move_y = if kb_y != 0.0 { kb_y } else { pad_move.y };
        self.state.jump_held = kb_jump_held || pad_jump_held;
        self.state.dash_held = down(DASH) || pad_dash_held;
    }

    /// Clear latched presses once a physics step has seen them.
    pub fn consume_presses(&mut self) {
        self.state.jump_pressed = false;
        self.state.dash_pressed = false;
    }

    pub fn gamepad_available(&self) -> bool {
        self.gilrs.is_some()
    }

    pub fn controller_name(&self) -> Option<String> {
        let gilrs = self.gilrs.as_ref()?;
        let pad = gilrs.connected_gamepad(self.active_pad?)?;
        Some(pad.name().to_string())
    }

    pub fn controller_power(&self) -> Option<String> {
        let gilrs = self.gilrs.as_ref()?;
        let pad = gilrs.connected_gamepad(self.active_pad?)?;
        match pad.power_info() {
            PowerInfo::Wired => Some("wired".into()),
            PowerInfo::Discharging(p) => Some(format!("battery {p}%")),
            PowerInfo::Charging(p) => Some(format!("charging {p}%")),
            PowerInfo::Charged => Some("charged".into()),
            _ => None,
        }
    }

    pub fn connected_count(&self) -> usize {
        self.gilrs.as_ref().map_or(0, |g| g.gamepads().count())
    }

    /// Current toast message and its opacity.
    pub fn notice(&self) -> Option<(&str, f32)> {
        self.notice
            .as_ref()
            .map(|(m, t)| (m.as_str(), (t / 0.5).min(1.0)))
    }
}
