//! The engine side of `pg.keyboard`, `pg.mouse` and `pg.touch`: reads macroquad's input
//! events, turns them into Love2D-style events, and tracks the state the getters report. It has
//! no Lua dependency.

use macroquad::{
    input::{
        set_cursor_grab, show_mouse, simulate_mouse_with_touch,
        utils::{register_input_subscriber, repeat_all_miniquad_input},
    },
    math::Vec2,
    miniquad::{
        CursorIcon, EventHandler, KeyCode, KeyMods, MouseButton, TouchPhase,
        window::{set_mouse_cursor, show_keyboard},
    },
    window::screen_dpi_scale,
};

/// Presses of the same button this close together, in seconds and pixels, count as one
/// multi-click (SDL's defaults).
const MULTI_CLICK_TIME: f64 = 0.5;
const MULTI_CLICK_DISTANCE: f32 = 32.0;

/// Converts miniquad's wheel deltas to Love2D's: about 1 per notch, positive x to the right and
/// positive y away from the player. miniquad passes each platform's own units, and its x points
/// left everywhere except Windows.
const WHEEL_SCALE: Vec2 = if cfg!(windows) {
    Vec2::new(1.0 / 120.0, 1.0 / 120.0)
} else if cfg!(target_os = "macos") {
    Vec2::new(-0.1, 0.1)
} else if cfg!(target_arch = "wasm32") {
    Vec2::new(-0.01, 0.01)
} else {
    Vec2::new(-1.0, 1.0)
};

/// The engine's subscription to macroquad's input events. macroquad can't unsubscribe, so the
/// engine makes one and drains it every frame, whichever screen is showing.
pub struct InputQueue(usize);

impl InputQueue {
    pub fn new() -> Self {
        // The engine turns the primary touch into mouse events itself, so it can flag them.
        simulate_mouse_with_touch(false);
        InputQueue(register_input_subscriber())
    }

    /// The events since the last call, in the order they happened.
    pub fn drain(&self) -> Vec<RawEvent> {
        let mut collector = Collector {
            events: Vec::new(),
            dpi: screen_dpi_scale(),
        };
        repeat_all_miniquad_input(&mut collector, self.0);
        collector.events
    }
}

/// An input event from macroquad, with positions in DPI-scaled pixels.
pub enum RawEvent {
    MouseMoved(Vec2),
    Wheel(Vec2),
    MouseButton {
        button: MouseButton,
        pos: Vec2,
        pressed: bool,
    },
    Key {
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    },
    Char(char),
    Touch {
        phase: TouchPhase,
        id: u64,
        pos: Vec2,
    },
    /// miniquad reports focus changes, and minimizing, as the window being minimized or
    /// restored.
    Focus(bool),
}

struct Collector {
    events: Vec<RawEvent>,
    dpi: f32,
}

impl Collector {
    fn pos(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y) / self.dpi
    }

    fn button(&mut self, button: MouseButton, x: f32, y: f32, pressed: bool) {
        let pos = self.pos(x, y);
        self.events.push(RawEvent::MouseButton {
            button,
            pos,
            pressed,
        });
    }
}

impl EventHandler for Collector {
    fn update(&mut self) {}

    fn draw(&mut self) {}

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        let pos = self.pos(x, y);
        self.events.push(RawEvent::MouseMoved(pos));
    }

    fn mouse_wheel_event(&mut self, x: f32, y: f32) {
        self.events
            .push(RawEvent::Wheel(Vec2::new(x, y) * WHEEL_SCALE));
    }

    fn mouse_button_down_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.button(button, x, y, true);
    }

    fn mouse_button_up_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.button(button, x, y, false);
    }

    fn char_event(&mut self, character: char, _mods: KeyMods, _repeat: bool) {
        self.events.push(RawEvent::Char(character));
    }

    fn key_down_event(&mut self, key: KeyCode, _mods: KeyMods, repeat: bool) {
        self.events.push(RawEvent::Key {
            key,
            pressed: true,
            repeat,
        });
    }

    fn key_up_event(&mut self, key: KeyCode, _mods: KeyMods) {
        self.events.push(RawEvent::Key {
            key,
            pressed: false,
            repeat: false,
        });
    }

    fn touch_event(&mut self, phase: TouchPhase, id: u64, x: f32, y: f32) {
        let pos = self.pos(x, y);
        self.events.push(RawEvent::Touch { phase, id, pos });
    }

    fn window_minimized_event(&mut self) {
        self.events.push(RawEvent::Focus(false));
    }

    fn window_restored_event(&mut self) {
        self.events.push(RawEvent::Focus(true));
    }
}

/// An input event for a `pg.*` callback.
#[derive(Debug, PartialEq)]
pub enum Event {
    KeyPressed { key: &'static str, repeat: bool },
    KeyReleased { key: &'static str },
    TextInput(char),
    MousePressed(Click),
    MouseReleased(Click),
    MouseMoved { pos: Vec2, delta: Vec2, touch: bool },
    WheelMoved(Vec2),
    TouchPressed(TouchPoint),
    TouchMoved(TouchPoint),
    TouchReleased(TouchPoint),
    Visible(bool),
}

impl Event {
    /// The name of the `pg.*` callback this event goes to.
    pub fn callback(&self) -> &'static str {
        match self {
            Event::KeyPressed { .. } => "keypressed",
            Event::KeyReleased { .. } => "keyreleased",
            Event::TextInput(_) => "textinput",
            Event::MousePressed(_) => "mousepressed",
            Event::MouseReleased(_) => "mousereleased",
            Event::MouseMoved { .. } => "mousemoved",
            Event::WheelMoved(_) => "wheelmoved",
            Event::TouchPressed(_) => "touchpressed",
            Event::TouchMoved(_) => "touchmoved",
            Event::TouchReleased(_) => "touchreleased",
            Event::Visible(_) => "visible",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Click {
    pub pos: Vec2,
    /// Love2D numbering: 1 left, 2 right, 3 middle.
    pub button: u8,
    pub touch: bool,
    /// 1 for a single click, 2 for a double click, and so on.
    pub presses: u32,
}

#[derive(Debug, PartialEq)]
pub struct TouchPoint {
    pub id: u64,
    pub pos: Vec2,
    pub delta: Vec2,
}

/// The last button press, for counting multi-clicks.
struct LastPress {
    button: u8,
    time: f64,
    pos: Vec2,
    presses: u32,
}

/// Input state as the game sees it. It's updated from events rather than read from macroquad,
/// so callbacks and getters always agree.
pub struct Input {
    /// Whether held keys fire repeat `keypressed` events.
    pub key_repeat: bool,
    text_input: bool,
    keys_down: Vec<KeyCode>,
    /// Indexed by Love2D button number minus one.
    buttons_down: [bool; 3],
    presses: [u32; 3],
    last_press: Option<LastPress>,
    /// The cursor position the game sees; frozen in relative mode.
    position: Vec2,
    /// The position in macroquad's last motion event. In relative mode macroquad accumulates
    /// raw motion into it, so motion deltas come from here.
    raw_position: Option<Vec2>,
    relative: bool,
    cursor_visible: bool,
    cursor_changed: bool,
    /// Active touches in the order they started.
    touches: Vec<(u64, Vec2)>,
    /// The touch that also drives the mouse.
    primary_touch: Option<u64>,
    focused: bool,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            key_repeat: false,
            text_input: true,
            keys_down: Vec::new(),
            buttons_down: [false; 3],
            presses: [0; 3],
            last_press: None,
            position: Vec2::ZERO,
            raw_position: None,
            relative: false,
            cursor_visible: true,
            cursor_changed: false,
            touches: Vec::new(),
            primary_touch: None,
            focused: true,
        }
    }
}

impl Input {
    pub fn new(position: Vec2) -> Self {
        Input {
            position,
            ..Default::default()
        }
    }

    pub fn is_key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    /// Whether a mouse button (Love2D numbering) is held. Unknown buttons are never down.
    pub fn is_button_down(&self, button: i64) -> bool {
        (1..=3).contains(&button) && self.buttons_down[button as usize - 1]
    }

    pub fn position(&self) -> Vec2 {
        self.position
    }

    pub fn touch_ids(&self) -> Vec<u64> {
        self.touches.iter().map(|(id, _)| *id).collect()
    }

    pub fn touch_position(&self, id: u64) -> Option<Vec2> {
        self.touches
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, pos)| *pos)
    }

    pub fn text_input(&self) -> bool {
        self.text_input
    }

    /// Turns `textinput` events on or off, along with the on-screen keyboard where there is one.
    pub fn set_text_input(&mut self, enable: bool) {
        self.text_input = enable;
        show_keyboard(enable);
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
        self.apply_cursor_visibility();
    }

    pub fn relative_mode(&self) -> bool {
        self.relative
    }

    /// In relative mode the cursor is hidden and captured, its position stays put, and mouse
    /// motion only reports deltas.
    pub fn set_relative_mode(&mut self, enable: bool) {
        if enable != self.relative {
            self.relative = enable;
            set_cursor_grab(enable);
            self.apply_cursor_visibility();
        }
    }

    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor_changed = cursor != CursorIcon::Default;
        set_mouse_cursor(cursor);
    }

    /// Undoes the game's cursor changes, for when it stops running.
    pub fn restore_cursor(&mut self) {
        self.set_relative_mode(false);
        self.set_cursor_visible(true);
        if self.cursor_changed {
            self.set_cursor(CursorIcon::Default);
        }
    }

    fn apply_cursor_visibility(&self) {
        show_mouse(self.cursor_visible && !self.relative);
    }

    /// Applies a frame's events to the state and returns the events for `pg.*` callbacks.
    /// `time` is the current time in seconds, for counting multi-clicks.
    pub fn process(&mut self, events: Vec<RawEvent>, time: f64) -> Vec<Event> {
        let mut out = Vec::new();
        for event in events {
            match event {
                RawEvent::Key {
                    key,
                    pressed: true,
                    repeat,
                } => self.key_pressed(&mut out, key, repeat),
                RawEvent::Key {
                    key,
                    pressed: false,
                    ..
                } => self.key_released(&mut out, key),
                RawEvent::Char(c) => {
                    if self.text_input && !c.is_control() {
                        out.push(Event::TextInput(c));
                    }
                }
                RawEvent::MouseMoved(pos) => self.mouse_moved(&mut out, pos, false),
                RawEvent::Wheel(delta) => {
                    if delta != Vec2::ZERO {
                        out.push(Event::WheelMoved(delta));
                    }
                }
                RawEvent::MouseButton {
                    button,
                    pos,
                    pressed,
                } => {
                    let button = match button {
                        MouseButton::Left => 1,
                        MouseButton::Right => 2,
                        MouseButton::Middle => 3,
                        MouseButton::Unknown => continue,
                    };
                    let pos = if self.relative { self.position } else { pos };
                    self.mouse_button(&mut out, button, pos, pressed, false, time);
                }
                RawEvent::Touch { phase, id, pos } => self.touch(&mut out, phase, id, pos, time),
                RawEvent::Focus(focused) => self.focus(&mut out, focused, time),
            }
        }
        out
    }

    fn key_pressed(&mut self, out: &mut Vec<Event>, key: KeyCode, repeat: bool) {
        // A repeat for a key that isn't down (held since before the game started, or across a
        // focus change) counts as a fresh press.
        let repeat = repeat && self.is_key_down(key);
        if !self.is_key_down(key) {
            self.keys_down.push(key);
        }
        if !repeat || self.key_repeat {
            out.push(Event::KeyPressed {
                key: key_name(key),
                repeat,
            });
        }
    }

    fn key_released(&mut self, out: &mut Vec<Event>, key: KeyCode) {
        let Some(index) = self.keys_down.iter().position(|k| *k == key) else {
            return;
        };
        self.keys_down.remove(index);
        out.push(Event::KeyReleased { key: key_name(key) });
    }

    fn mouse_moved(&mut self, out: &mut Vec<Event>, pos: Vec2, touch: bool) {
        let (pos, delta) = if self.relative && !touch {
            let delta = pos - self.raw_position.unwrap_or(pos);
            self.raw_position = Some(pos);
            (self.position, delta)
        } else {
            let delta = pos - self.position;
            self.move_to(pos);
            (pos, delta)
        };
        if delta != Vec2::ZERO {
            out.push(Event::MouseMoved { pos, delta, touch });
        }
    }

    fn move_to(&mut self, pos: Vec2) {
        self.position = pos;
        self.raw_position = Some(pos);
    }

    fn mouse_button(
        &mut self,
        out: &mut Vec<Event>,
        button: u8,
        pos: Vec2,
        pressed: bool,
        touch: bool,
        time: f64,
    ) {
        let index = usize::from(button - 1);
        if pressed == self.buttons_down[index] {
            return;
        }
        self.buttons_down[index] = pressed;
        if !self.relative {
            self.move_to(pos);
        }

        if !pressed {
            let presses = self.presses[index];
            out.push(Event::MouseReleased(Click {
                pos,
                button,
                touch,
                presses,
            }));
            return;
        }
        let presses = match &self.last_press {
            Some(last)
                if last.button == button
                    && time - last.time <= MULTI_CLICK_TIME
                    && last.pos.distance(pos) <= MULTI_CLICK_DISTANCE =>
            {
                last.presses + 1
            }
            _ => 1,
        };
        self.last_press = Some(LastPress {
            button,
            time,
            pos,
            presses,
        });
        self.presses[index] = presses;
        out.push(Event::MousePressed(Click {
            pos,
            button,
            touch,
            presses,
        }));
    }

    /// Touches also drive the mouse like SDL does: the first finger down while no finger is
    /// driving it moves the cursor and holds button 1, with mouse events sent before the touch
    /// event.
    fn touch(&mut self, out: &mut Vec<Event>, phase: TouchPhase, id: u64, pos: Vec2, time: f64) {
        let index = self.touches.iter().position(|(t, _)| *t == id);
        match phase {
            TouchPhase::Started => {
                if self.primary_touch.is_none() {
                    self.primary_touch = Some(id);
                    self.mouse_moved(out, pos, true);
                    self.mouse_button(out, 1, pos, true, true, time);
                }
                match index {
                    Some(i) => self.touches[i].1 = pos,
                    None => self.touches.push((id, pos)),
                }
                out.push(Event::TouchPressed(TouchPoint {
                    id,
                    pos,
                    delta: Vec2::ZERO,
                }));
            }
            TouchPhase::Moved => {
                let Some(i) = index else { return };
                let delta = pos - self.touches[i].1;
                if delta == Vec2::ZERO {
                    return;
                }
                self.touches[i].1 = pos;
                if self.primary_touch == Some(id) {
                    self.mouse_moved(out, pos, true);
                }
                out.push(Event::TouchMoved(TouchPoint { id, pos, delta }));
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(i) = index else { return };
                let (_, last) = self.touches.remove(i);
                if self.primary_touch == Some(id) {
                    self.primary_touch = None;
                    self.mouse_moved(out, pos, true);
                    self.mouse_button(out, 1, pos, false, true, time);
                }
                out.push(Event::TouchReleased(TouchPoint {
                    id,
                    pos,
                    delta: pos - last,
                }));
            }
        }
    }

    fn focus(&mut self, out: &mut Vec<Event>, focused: bool, time: f64) {
        if focused == self.focused {
            return;
        }
        self.focused = focused;
        if !focused {
            // Release everything held, like SDL, since the releases would go to another
            // window and leave keys stuck down.
            for key in self.keys_down.clone() {
                self.key_released(out, key);
            }
            for (id, pos) in self.touches.clone() {
                self.touch(out, TouchPhase::Cancelled, id, pos, time);
            }
            for button in 1..=3 {
                self.mouse_button(out, button, self.position, false, false, time);
            }
        }
        out.push(Event::Visible(focused));
    }
}

/// miniquad key codes and their Love2D names. miniquad codes are physical keys on Windows,
/// macOS and the web, so these are named after a US layout, like Love2D's scancodes.
const KEYS: &[(KeyCode, &str)] = &[
    (KeyCode::A, "a"),
    (KeyCode::B, "b"),
    (KeyCode::C, "c"),
    (KeyCode::D, "d"),
    (KeyCode::E, "e"),
    (KeyCode::F, "f"),
    (KeyCode::G, "g"),
    (KeyCode::H, "h"),
    (KeyCode::I, "i"),
    (KeyCode::J, "j"),
    (KeyCode::K, "k"),
    (KeyCode::L, "l"),
    (KeyCode::M, "m"),
    (KeyCode::N, "n"),
    (KeyCode::O, "o"),
    (KeyCode::P, "p"),
    (KeyCode::Q, "q"),
    (KeyCode::R, "r"),
    (KeyCode::S, "s"),
    (KeyCode::T, "t"),
    (KeyCode::U, "u"),
    (KeyCode::V, "v"),
    (KeyCode::W, "w"),
    (KeyCode::X, "x"),
    (KeyCode::Y, "y"),
    (KeyCode::Z, "z"),
    (KeyCode::Key0, "0"),
    (KeyCode::Key1, "1"),
    (KeyCode::Key2, "2"),
    (KeyCode::Key3, "3"),
    (KeyCode::Key4, "4"),
    (KeyCode::Key5, "5"),
    (KeyCode::Key6, "6"),
    (KeyCode::Key7, "7"),
    (KeyCode::Key8, "8"),
    (KeyCode::Key9, "9"),
    (KeyCode::Space, "space"),
    (KeyCode::Apostrophe, "'"),
    (KeyCode::Comma, ","),
    (KeyCode::Minus, "-"),
    (KeyCode::Period, "."),
    (KeyCode::Slash, "/"),
    (KeyCode::Semicolon, ";"),
    (KeyCode::Equal, "="),
    (KeyCode::LeftBracket, "["),
    (KeyCode::Backslash, "\\"),
    (KeyCode::RightBracket, "]"),
    (KeyCode::GraveAccent, "`"),
    (KeyCode::Escape, "escape"),
    (KeyCode::Enter, "return"),
    (KeyCode::Tab, "tab"),
    (KeyCode::Backspace, "backspace"),
    (KeyCode::Insert, "insert"),
    (KeyCode::Delete, "delete"),
    (KeyCode::Right, "right"),
    (KeyCode::Left, "left"),
    (KeyCode::Down, "down"),
    (KeyCode::Up, "up"),
    (KeyCode::PageUp, "pageup"),
    (KeyCode::PageDown, "pagedown"),
    (KeyCode::Home, "home"),
    (KeyCode::End, "end"),
    (KeyCode::CapsLock, "capslock"),
    (KeyCode::ScrollLock, "scrolllock"),
    (KeyCode::NumLock, "numlock"),
    (KeyCode::PrintScreen, "printscreen"),
    (KeyCode::Pause, "pause"),
    (KeyCode::F1, "f1"),
    (KeyCode::F2, "f2"),
    (KeyCode::F3, "f3"),
    (KeyCode::F4, "f4"),
    (KeyCode::F5, "f5"),
    (KeyCode::F6, "f6"),
    (KeyCode::F7, "f7"),
    (KeyCode::F8, "f8"),
    (KeyCode::F9, "f9"),
    (KeyCode::F10, "f10"),
    (KeyCode::F11, "f11"),
    (KeyCode::F12, "f12"),
    (KeyCode::F13, "f13"),
    (KeyCode::F14, "f14"),
    (KeyCode::F15, "f15"),
    (KeyCode::F16, "f16"),
    (KeyCode::F17, "f17"),
    (KeyCode::F18, "f18"),
    (KeyCode::F19, "f19"),
    (KeyCode::F20, "f20"),
    (KeyCode::F21, "f21"),
    (KeyCode::F22, "f22"),
    (KeyCode::F23, "f23"),
    (KeyCode::F24, "f24"),
    (KeyCode::Kp0, "kp0"),
    (KeyCode::Kp1, "kp1"),
    (KeyCode::Kp2, "kp2"),
    (KeyCode::Kp3, "kp3"),
    (KeyCode::Kp4, "kp4"),
    (KeyCode::Kp5, "kp5"),
    (KeyCode::Kp6, "kp6"),
    (KeyCode::Kp7, "kp7"),
    (KeyCode::Kp8, "kp8"),
    (KeyCode::Kp9, "kp9"),
    (KeyCode::KpDecimal, "kp."),
    (KeyCode::KpDivide, "kp/"),
    (KeyCode::KpMultiply, "kp*"),
    (KeyCode::KpSubtract, "kp-"),
    (KeyCode::KpAdd, "kp+"),
    (KeyCode::KpEnter, "kpenter"),
    (KeyCode::KpEqual, "kp="),
    (KeyCode::LeftShift, "lshift"),
    (KeyCode::LeftControl, "lctrl"),
    (KeyCode::LeftAlt, "lalt"),
    (KeyCode::LeftSuper, "lgui"),
    (KeyCode::RightShift, "rshift"),
    (KeyCode::RightControl, "rctrl"),
    (KeyCode::RightAlt, "ralt"),
    (KeyCode::RightSuper, "rgui"),
    // The context-menu key, which SDL calls "Application".
    (KeyCode::Menu, "application"),
    // Android's back button, which SDL reports as AC Back.
    (KeyCode::Back, "appback"),
];

/// Love2D key constants that miniquad never reports. They're valid in `isDown` but never down.
const OTHER_KEY_CONSTANTS: &[&str] = &[
    "unknown",
    "!",
    "\"",
    "#",
    "%",
    "$",
    "&",
    "(",
    ")",
    "*",
    "+",
    ":",
    "<",
    ">",
    "?",
    "@",
    "^",
    "_",
    "kp,",
    "power",
    "execute",
    "help",
    "menu",
    "select",
    "stop",
    "again",
    "undo",
    "cut",
    "copy",
    "paste",
    "find",
    "mute",
    "volumeup",
    "volumedown",
    "alterase",
    "sysreq",
    "cancel",
    "clear",
    "prior",
    "return2",
    "separator",
    "out",
    "oper",
    "clearagain",
    "thousandsseparator",
    "decimalseparator",
    "currencyunit",
    "currencysubunit",
    "mode",
    "audionext",
    "audioprev",
    "audiostop",
    "audioplay",
    "audiomute",
    "mediaselect",
    "www",
    "mail",
    "calculator",
    "computer",
    "appsearch",
    "apphome",
    "appforward",
    "appstop",
    "apprefresh",
    "appbookmarks",
    "brightnessdown",
    "brightnessup",
    "displayswitch",
    "kbdillumtoggle",
    "kbdillumdown",
    "kbdillumup",
    "eject",
    "sleep",
];

/// The Love2D name of a key, or `"unknown"`.
pub fn key_name(key: KeyCode) -> &'static str {
    KEYS.iter()
        .find(|(code, _)| *code == key)
        .map_or("unknown", |(_, name)| name)
}

/// Looks up a Love2D key constant. `Err` means it isn't one; `Ok(None)` means it is, but
/// miniquad never reports that key.
pub fn key_code(name: &str) -> Result<Option<KeyCode>, ()> {
    if let Some((code, _)) = KEYS.iter().find(|(_, n)| *n == name) {
        Ok(Some(*code))
    } else if OTHER_KEY_CONSTANTS.contains(&name) {
        Ok(None)
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: KeyCode, pressed: bool, repeat: bool) -> RawEvent {
        RawEvent::Key {
            key,
            pressed,
            repeat,
        }
    }

    fn button(pressed: bool, x: f32, y: f32) -> RawEvent {
        RawEvent::MouseButton {
            button: MouseButton::Left,
            pos: Vec2::new(x, y),
            pressed,
        }
    }

    fn touch(phase: TouchPhase, id: u64, x: f32, y: f32) -> RawEvent {
        RawEvent::Touch {
            phase,
            id,
            pos: Vec2::new(x, y),
        }
    }

    fn names(events: &[Event]) -> Vec<&'static str> {
        events.iter().map(Event::callback).collect()
    }

    #[test]
    fn key_names_round_trip() {
        for (code, name) in KEYS {
            assert_eq!(key_name(*code), *name);
            assert_eq!(key_code(name), Ok(Some(*code)));
            assert!(!OTHER_KEY_CONSTANTS.contains(name), "{name} listed twice");
        }
        assert_eq!(key_name(KeyCode::World1), "unknown");
        assert_eq!(key_code("unknown"), Ok(None));
        assert_eq!(key_code("Space"), Err(()));
    }

    #[test]
    fn key_repeat_is_off_by_default() {
        let mut input = Input::default();
        let events = input.process(
            vec![
                key(KeyCode::A, true, false),
                key(KeyCode::A, true, true),
                key(KeyCode::A, false, false),
            ],
            0.0,
        );
        assert_eq!(names(&events), ["keypressed", "keyreleased"]);

        input.key_repeat = true;
        let events = input.process(
            vec![key(KeyCode::A, true, false), key(KeyCode::A, true, true)],
            0.0,
        );
        assert_eq!(
            events[1],
            Event::KeyPressed {
                key: "a",
                repeat: true
            }
        );
    }

    #[test]
    fn stray_repeats_and_releases() {
        let mut input = Input::default();
        let events = input.process(
            vec![key(KeyCode::B, false, false), key(KeyCode::A, true, true)],
            0.0,
        );
        assert_eq!(
            events,
            [Event::KeyPressed {
                key: "a",
                repeat: false
            }]
        );
    }

    #[test]
    fn text_input_skips_control_characters() {
        let mut input = Input::default();
        let events = input.process(
            vec![
                RawEvent::Char('\u{8}'),
                RawEvent::Char('é'),
                RawEvent::Char('\r'),
            ],
            0.0,
        );
        assert_eq!(events, [Event::TextInput('é')]);
    }

    #[test]
    fn counts_multi_clicks() {
        let mut input = Input::default();
        let presses = |events: &[Event]| -> Vec<u32> {
            events
                .iter()
                .filter_map(|e| match e {
                    Event::MousePressed(click) => Some(click.presses),
                    _ => None,
                })
                .collect()
        };
        let clicks = vec![
            button(true, 10.0, 10.0),
            button(false, 10.0, 10.0),
            button(true, 12.0, 10.0),
            button(false, 12.0, 10.0),
        ];
        assert_eq!(presses(&input.process(clicks, 1.0)), [1, 2]);
        // Too late for a triple click.
        let late = vec![button(true, 12.0, 10.0), button(false, 12.0, 10.0)];
        assert_eq!(presses(&input.process(late, 2.0)), [1]);
        // Too far away.
        let far = vec![button(true, 200.0, 10.0)];
        assert_eq!(presses(&input.process(far, 2.1)), [1]);
    }

    #[test]
    fn mouse_motion_reports_deltas() {
        let mut input = Input::new(Vec2::new(5.0, 5.0));
        let events = input.process(
            vec![
                RawEvent::MouseMoved(Vec2::new(8.0, 9.0)),
                RawEvent::MouseMoved(Vec2::new(8.0, 9.0)),
            ],
            0.0,
        );
        assert_eq!(
            events,
            [Event::MouseMoved {
                pos: Vec2::new(8.0, 9.0),
                delta: Vec2::new(3.0, 4.0),
                touch: false
            }]
        );
    }

    #[test]
    fn primary_touch_drives_the_mouse() {
        let mut input = Input::default();
        let events = input.process(
            vec![
                touch(TouchPhase::Started, 7, 10.0, 20.0),
                touch(TouchPhase::Started, 8, 50.0, 50.0),
                touch(TouchPhase::Moved, 7, 11.0, 20.0),
                touch(TouchPhase::Moved, 8, 50.0, 50.0),
                touch(TouchPhase::Ended, 7, 11.0, 20.0),
            ],
            0.0,
        );
        assert_eq!(
            names(&events),
            [
                "mousemoved",
                "mousepressed",
                "touchpressed",
                "touchpressed",
                "mousemoved",
                "touchmoved",
                "mousereleased",
                "touchreleased",
            ]
        );
        assert!(matches!(&events[1], Event::MousePressed(c) if c.touch && c.button == 1));
        assert_eq!(input.touch_ids(), [8]);
        assert!(!input.is_button_down(1));
    }

    #[test]
    fn focus_loss_releases_everything_once() {
        let mut input = Input::default();
        input.process(
            vec![
                key(KeyCode::W, true, false),
                button(true, 1.0, 1.0),
                touch(TouchPhase::Started, 3, 4.0, 4.0),
            ],
            0.0,
        );
        let events = input.process(
            vec![
                RawEvent::Focus(false),
                RawEvent::Focus(false),
                key(KeyCode::W, false, false),
            ],
            0.0,
        );
        assert_eq!(
            names(&events),
            ["keyreleased", "mousereleased", "touchreleased", "visible"]
        );
        assert!(!input.is_key_down(KeyCode::W));
        assert!(input.touch_ids().is_empty());
    }
}
