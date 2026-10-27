//! Turning winit's device traffic into the frame of input the scene expects.
//!
//! Nothing here decides what a key means: it only names keys and adds up the
//! deltas that arrived between two pumps. A headless scene never sees any of it.

use carbine_core::{FrameInput, Key, MouseButton, Vec2};
use winit::event::{
    DeviceEvent, ElementState, MouseButton as WMouseButton, MouseScrollDelta, WindowEvent,
};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Roughly one wheel notch, so a pixel-scrolled mouse reports the same scale as
/// a notched one.
const PIXELS_PER_NOTCH: f32 = 40.0;

#[derive(Default)]
pub(crate) struct InputCollector {
    frame: FrameInput,
}

impl InputCollector {
    /// Everything that arrived since the last pump, and a clean slate after.
    pub(crate) fn take(&mut self) -> FrameInput {
        std::mem::take(&mut self.frame)
    }

    /// Keeps traffic from two pumps in a row in one frame, in arrival order. The
    /// scene folds edges as it reads the list, so order is what matters.
    pub(crate) fn absorb(&mut self, later: FrameInput) {
        self.frame.keys.extend(later.keys);
        self.frame.buttons.extend(later.buttons);
        self.frame.mouse_delta += later.mouse_delta;
        if later.cursor.is_some() {
            self.frame.cursor = later.cursor;
        }
        self.frame.wheel += later.wheel;
        self.frame.focus_lost |= later.focus_lost;
    }

    /// A keystroke by physical position. Keys Carbine has no name for are
    /// dropped rather than guessed at, so a script never sees a half-truth.
    pub(crate) fn key(&mut self, physical: PhysicalKey, state: ElementState) {
        if let Some(key) = physical_key(physical) {
            self.frame.keys.push((key, is_down(state)));
        }
    }

    pub(crate) fn window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                self.key(event.physical_key, event.state);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = mouse_button(button) {
                    self.frame.buttons.push((button, is_down(state)));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.frame.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.y as f32) / PIXELS_PER_NOTCH
                    }
                };
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.frame.cursor = Some(Vec2::new(position.x as f32, position.y as f32));
            }
            WindowEvent::Focused(false) => self.frame.focus_lost = true,
            _ => {}
        }
    }

    /// Relative motion only ever arrives here, and only while the pointer is
    /// grabbed on Wayland. `CursorMoved` is absolute and useless to a camera.
    pub(crate) fn device_event(&mut self, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.frame.mouse_delta += Vec2::new(delta.0 as f32, delta.1 as f32);
        }
    }
}

fn is_down(state: ElementState) -> bool {
    matches!(state, ElementState::Pressed)
}

fn mouse_button(button: WMouseButton) -> Option<MouseButton> {
    Some(match button {
        WMouseButton::Left => MouseButton::Left,
        WMouseButton::Right => MouseButton::Right,
        WMouseButton::Middle => MouseButton::Middle,
        _ => return None,
    })
}

/// Keys are matched by the position of the key on the keyboard, so WASD means
/// WASD on a French layout too.
fn physical_key(key: PhysicalKey) -> Option<Key> {
    let code = match key {
        PhysicalKey::Code(code) => code,
        PhysicalKey::Unidentified(_) => return None,
    };

    Some(match code {
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        KeyCode::Digit0 | KeyCode::Numpad0 => Key::Num0,
        KeyCode::Digit1 | KeyCode::Numpad1 => Key::Num1,
        KeyCode::Digit2 | KeyCode::Numpad2 => Key::Num2,
        KeyCode::Digit3 | KeyCode::Numpad3 => Key::Num3,
        KeyCode::Digit4 | KeyCode::Numpad4 => Key::Num4,
        KeyCode::Digit5 | KeyCode::Numpad5 => Key::Num5,
        KeyCode::Digit6 | KeyCode::Numpad6 => Key::Num6,
        KeyCode::Digit7 | KeyCode::Numpad7 => Key::Num7,
        KeyCode::Digit8 | KeyCode::Numpad8 => Key::Num8,
        KeyCode::Digit9 | KeyCode::Numpad9 => Key::Num9,
        KeyCode::Space => Key::Space,
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Control,
        KeyCode::AltLeft | KeyCode::AltRight => Key::Alt,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, TouchPhase};

    #[test]
    fn keys_are_read_from_where_they_sit_on_the_keyboard() {
        assert_eq!(physical_key(PhysicalKey::Code(KeyCode::KeyW)), Some(Key::W));
        assert_eq!(
            physical_key(PhysicalKey::Code(KeyCode::ShiftLeft)),
            Some(Key::Shift)
        );
        assert_eq!(
            physical_key(PhysicalKey::Code(KeyCode::Digit5)),
            Some(Key::Num5)
        );
        assert_eq!(
            physical_key(PhysicalKey::Code(KeyCode::F1)),
            None,
            "a function key has no Carbine name"
        );
    }

    #[test]
    fn a_press_and_a_release_are_reported_in_order() {
        let mut collector = InputCollector::default();

        collector.key(PhysicalKey::Code(KeyCode::KeyW), ElementState::Pressed);
        collector.key(PhysicalKey::Code(KeyCode::F9), ElementState::Pressed);
        collector.key(PhysicalKey::Code(KeyCode::KeyW), ElementState::Released);

        assert_eq!(collector.take().keys, vec![(Key::W, true), (Key::W, false)]);
    }

    #[test]
    fn motion_adds_up_and_a_second_pump_keeps_its_order() {
        let mut collector = InputCollector::default();

        collector.device_event(DeviceEvent::MouseMotion { delta: (4.0, -2.0) });
        collector.device_event(DeviceEvent::MouseMotion { delta: (1.0, 1.0) });
        collector.window_event(WindowEvent::Focused(false));
        let first = collector.take();
        assert_eq!(first.mouse_delta, Vec2::new(5.0, -1.0));
        assert!(first.focus_lost);

        let mut held = InputCollector::default();
        held.absorb(first);
        held.absorb(FrameInput {
            keys: vec![(Key::Space, true)],
            ..Default::default()
        });
        let combined = held.take();
        assert_eq!(combined.mouse_delta, Vec2::new(5.0, -1.0));
        assert!(combined.focus_lost);
        assert_eq!(combined.keys, vec![(Key::Space, true)]);
    }

    #[test]
    fn the_wheel_and_the_pointer_are_captured() {
        let mut collector = InputCollector::default();

        collector.window_event(WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 2.0),
            phase: TouchPhase::Moved,
        });
        collector.window_event(WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 80.0)),
            phase: TouchPhase::Moved,
        });
        collector.window_event(WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(120.5, 60.0),
        });

        let frame = collector.take();
        assert_eq!(frame.wheel, 4.0);
        assert_eq!(frame.cursor, Some(Vec2::new(120.5, 60.0)));
    }

    #[test]
    fn only_the_three_buttons_a_player_presses_are_named() {
        let mut collector = InputCollector::default();

        collector.window_event(WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: WMouseButton::Right,
        });
        collector.window_event(WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: WMouseButton::Other(9),
        });

        assert_eq!(collector.take().buttons, vec![(MouseButton::Right, true)]);
    }
}
