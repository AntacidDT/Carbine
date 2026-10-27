//! What the person at the keyboard is doing right now.
//!
//! The window layer turns device traffic into a [`FrameInput`] and hands it over;
//! the scene folds that into [`Input`], which is the state game logic and the
//! player controller read. Scripts never see a windowing type, and a headless
//! test can build the same frame by hand.

use std::collections::HashSet;
use std::fmt;

use glam::Vec2;

use crate::node::Scene;

/// The keys Carbine has names for. Movement uses letters; the rest is what a
/// first person controller needs plus the keys that release the mouse.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Shift,
    Control,
    Alt,
}

/// Every key Carbine answers for, so the error message can list them instead of
/// guessing.
pub const KEYS: &[Key] = &[
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
    Key::Space,
    Key::Enter,
    Key::Escape,
    Key::Tab,
    Key::Backspace,
    Key::Delete,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Shift,
    Key::Control,
    Key::Alt,
];

impl Key {
    /// Reads a key the way a script names it: `"w"`, `"space"`, `"escape"`.
    /// Letters match by position, not by the character they print, so an AZERTY
    /// keyboard's `w` key is still `Key::W`.
    pub fn from_name(name: &str) -> Option<Key> {
        let written = name.trim().to_lowercase();
        let mut letters = written.chars();
        if let (Some(only), None) = (letters.next(), letters.next())
            && let Some(key) = letter_key(only)
        {
            return Some(key);
        }

        Some(match written.as_str() {
            "space" | "spacebar" => Key::Space,
            "enter" | "return" => Key::Enter,
            "escape" | "esc" => Key::Escape,
            "tab" => Key::Tab,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "up" | "arrowup" => Key::Up,
            "down" | "arrowdown" => Key::Down,
            "left" | "arrowleft" => Key::Left,
            "right" | "arrowright" => Key::Right,
            "shift" | "shiftleft" | "shiftright" => Key::Shift,
            "ctrl" | "control" | "controlleft" | "controlright" => Key::Control,
            "alt" | "altleft" | "altright" => Key::Alt,
            _ => return None,
        })
    }

    /// The name a script would have typed to get this key.
    pub fn name(self) -> &'static str {
        match self {
            Key::A => "a",
            Key::B => "b",
            Key::C => "c",
            Key::D => "d",
            Key::E => "e",
            Key::F => "f",
            Key::G => "g",
            Key::H => "h",
            Key::I => "i",
            Key::J => "j",
            Key::K => "k",
            Key::L => "l",
            Key::M => "m",
            Key::N => "n",
            Key::O => "o",
            Key::P => "p",
            Key::Q => "q",
            Key::R => "r",
            Key::S => "s",
            Key::T => "t",
            Key::U => "u",
            Key::V => "v",
            Key::W => "w",
            Key::X => "x",
            Key::Y => "y",
            Key::Z => "z",
            Key::Num0 => "0",
            Key::Num1 => "1",
            Key::Num2 => "2",
            Key::Num3 => "3",
            Key::Num4 => "4",
            Key::Num5 => "5",
            Key::Num6 => "6",
            Key::Num7 => "7",
            Key::Num8 => "8",
            Key::Num9 => "9",
            Key::Space => "space",
            Key::Enter => "enter",
            Key::Escape => "escape",
            Key::Tab => "tab",
            Key::Backspace => "backspace",
            Key::Delete => "delete",
            Key::Up => "up",
            Key::Down => "down",
            Key::Left => "left",
            Key::Right => "right",
            Key::Shift => "shift",
            Key::Control => "ctrl",
            Key::Alt => "alt",
        }
    }
}

fn letter_key(letter: char) -> Option<Key> {
    Some(match letter {
        'a' => Key::A,
        'b' => Key::B,
        'c' => Key::C,
        'd' => Key::D,
        'e' => Key::E,
        'f' => Key::F,
        'g' => Key::G,
        'h' => Key::H,
        'i' => Key::I,
        'j' => Key::J,
        'k' => Key::K,
        'l' => Key::L,
        'm' => Key::M,
        'n' => Key::N,
        'o' => Key::O,
        'p' => Key::P,
        'q' => Key::Q,
        'r' => Key::R,
        's' => Key::S,
        't' => Key::T,
        'u' => Key::U,
        'v' => Key::V,
        'w' => Key::W,
        'x' => Key::X,
        'y' => Key::Y,
        'z' => Key::Z,
        '0' => Key::Num0,
        '1' => Key::Num1,
        '2' => Key::Num2,
        '3' => Key::Num3,
        '4' => Key::Num4,
        '5' => Key::Num5,
        '6' => Key::Num6,
        '7' => Key::Num7,
        '8' => Key::Num8,
        '9' => Key::Num9,
        _ => return None,
    })
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn from_name(name: &str) -> Option<MouseButton> {
        Some(match name.trim().to_lowercase().as_str() {
            "left" => MouseButton::Left,
            "right" => MouseButton::Right,
            "middle" => MouseButton::Middle,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
        }
    }
}

impl fmt::Display for MouseButton {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// One frame of device traffic, in the order it arrived.
#[derive(Clone, Debug, Default)]
pub struct FrameInput {
    pub keys: Vec<(Key, bool)>,
    /// Only the three buttons a player actually presses are reported.
    pub buttons: Vec<(MouseButton, bool)>,
    /// How far the mouse moved since the last frame, in pixels.
    pub mouse_delta: Vec2,
    /// Where the pointer is inside the window, top left being (0, 0).
    pub cursor: Option<Vec2>,
    /// Wheel notches since the last frame, positive being away from the user.
    pub wheel: f32,
    /// The window lost focus, so every key still held would be a lie.
    pub focus_lost: bool,
}

/// The folded answer to "what is being pressed right now".
#[derive(Clone, Debug, Default)]
pub struct Input {
    held: HashSet<Key>,
    pressed: HashSet<Key>,
    released: HashSet<Key>,
    buttons: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,
    mouse_delta: Vec2,
    cursor: Vec2,
    wheel: f32,
}

impl Input {
    /// A key down right now, across frames.
    pub fn key(&self, key: Key) -> bool {
        self.held.contains(&key)
    }

    /// A key that came down during this frame only.
    pub fn pressed(&self, key: Key) -> bool {
        self.pressed.contains(&key)
    }

    pub fn released(&self, key: Key) -> bool {
        self.released.contains(&key)
    }

    pub fn button(&self, button: MouseButton) -> bool {
        self.buttons.contains(&button)
    }

    pub fn button_pressed(&self, button: MouseButton) -> bool {
        self.buttons_pressed.contains(&button)
    }

    pub fn button_released(&self, button: MouseButton) -> bool {
        self.buttons_released.contains(&button)
    }

    /// Forgets a press, so a second step inside the same frame cannot act on the
    /// same tap twice.
    pub(crate) fn consume_press(&mut self, key: Key) {
        self.pressed.remove(&key);
    }

    pub fn mouse_delta(&self) -> Vec2 {
        self.mouse_delta
    }

    pub fn cursor(&self) -> Vec2 {
        self.cursor
    }

    pub fn wheel(&self) -> f32 {
        self.wheel
    }

    /// Folds one frame in: the held set carries across frames, while the pressed
    /// and released sets and the device deltas describe this frame alone.
    fn apply(&mut self, frame: &FrameInput) {
        let mut pressed = HashSet::new();
        let mut released = HashSet::new();
        for (key, down) in &frame.keys {
            if *down {
                if self.held.insert(*key) {
                    pressed.insert(*key);
                }
            } else if self.held.remove(key) {
                released.insert(*key);
            }
        }
        self.pressed = pressed;
        self.released = released;

        let mut pressed = HashSet::new();
        let mut released = HashSet::new();
        for (button, down) in &frame.buttons {
            if *down {
                if self.buttons.insert(*button) {
                    pressed.insert(*button);
                }
            } else if self.buttons.remove(button) {
                released.insert(*button);
            }
        }
        self.buttons_pressed = pressed;
        self.buttons_released = released;

        self.mouse_delta = frame.mouse_delta;
        if let Some(cursor) = frame.cursor {
            self.cursor = cursor;
        }
        self.wheel = frame.wheel;

        if frame.focus_lost {
            self.held.clear();
            self.buttons.clear();
        }
    }
}

impl Scene {
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// Folds one frame of device traffic into the scene, then applies the two
    /// rules the engine owns: the mouse looks when the player does, and Escape
    /// hands the pointer back while a click asks for it again.
    pub fn update_input(&mut self, frame: &FrameInput) {
        self.input.apply(frame);

        if self.input.pressed(Key::Escape) {
            self.player.grab_mouse = false;
        }
        if self.player.enabled
            && !self.player.grab_mouse
            && self.input.button_pressed(MouseButton::Left)
        {
            self.player.grab_mouse = true;
        }
        if self.player.enabled {
            self.player.look_at_mouse(self.input.mouse_delta());
        }
    }

    /// A key press from somewhere other than a window: a test, or a script that
    /// replays a recording.
    pub fn inject_key(&mut self, key: Key, down: bool) {
        self.update_input(&FrameInput {
            keys: vec![(key, down)],
            ..Default::default()
        });
    }

    pub fn inject_button(&mut self, button: MouseButton, down: bool) {
        self.update_input(&FrameInput {
            buttons: vec![(button, down)],
            ..Default::default()
        });
    }

    pub fn inject_mouse_delta(&mut self, delta: Vec2) {
        self.update_input(&FrameInput {
            mouse_delta: delta,
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_the_way_a_script_types_them() {
        assert_eq!(Key::from_name("W"), Some(Key::W));
        assert_eq!(Key::from_name(" space "), Some(Key::Space));
        assert_eq!(Key::from_name("esc"), Some(Key::Escape));
        assert_eq!(Key::from_name("arrowup"), Some(Key::Up));
        assert_eq!(Key::from_name("ctrl"), Some(Key::Control));
        assert_eq!(Key::from_name("f1"), None);
        assert_eq!(Key::W.to_string(), "w");
        assert_eq!(Key::Num7.name(), "7");
    }

    #[test]
    fn a_letter_key_is_the_key_in_its_position() {
        // "z" is a letter, so it is the AZERTY user's W, and nothing in the name
        // tells the two apart.
        assert_eq!(Key::from_name("z"), Some(Key::Z));
        assert_eq!(Key::from_name("w"), Some(Key::W));
    }

    #[test]
    fn a_frame_folds_into_held_and_edges() {
        let mut input = Input::default();

        input.apply(&FrameInput {
            keys: vec![(Key::W, true)],
            ..Default::default()
        });
        assert!(input.key(Key::W));
        assert!(input.pressed(Key::W));
        assert!(!input.released(Key::W));

        // The same key reported down twice is still one press.
        input.apply(&FrameInput {
            keys: vec![(Key::W, true)],
            ..Default::default()
        });
        assert!(!input.pressed(Key::W));
        assert!(input.key(Key::W));

        input.apply(&FrameInput {
            keys: vec![(Key::W, false)],
            ..Default::default()
        });
        assert!(!input.key(Key::W));
        assert!(input.released(Key::W));
    }

    #[test]
    fn a_released_window_lets_go_of_every_key() {
        let mut input = Input::default();
        input.apply(&FrameInput {
            keys: vec![(Key::W, true), (Key::Space, true)],
            ..Default::default()
        });

        input.apply(&FrameInput {
            focus_lost: true,
            ..Default::default()
        });
        assert!(!input.key(Key::W));
        assert!(!input.key(Key::Space));
    }

    #[test]
    fn mouse_traffic_describes_the_frame_that_just_happened() {
        let mut input = Input::default();
        input.apply(&FrameInput {
            buttons: vec![(MouseButton::Left, true)],
            mouse_delta: Vec2::new(12.0, -4.0),
            cursor: Some(Vec2::new(300.0, 180.0)),
            wheel: 2.0,
            ..Default::default()
        });
        assert!(input.button(MouseButton::Left));
        assert!(input.button_pressed(MouseButton::Left));
        assert_eq!(input.mouse_delta(), Vec2::new(12.0, -4.0));
        assert_eq!(input.cursor(), Vec2::new(300.0, 180.0));
        assert_eq!(input.wheel(), 2.0);

        input.apply(&FrameInput::default());
        assert_eq!(input.mouse_delta(), Vec2::ZERO);
        assert!(input.button(MouseButton::Left));
        // The pointer has nowhere new to report, so it keeps the place it had.
        assert_eq!(input.cursor(), Vec2::new(300.0, 180.0));
    }
}
