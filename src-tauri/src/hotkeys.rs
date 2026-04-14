use crate::ClickerState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::engine::worker::now_epoch_ms;
use crate::engine::worker::start_clicker_inner;
use crate::engine::worker::stop_clicker_inner;
use crate::engine::worker::toggle_clicker_inner;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub main_code: Code,
    pub key_token: String,
}

impl HotkeyBinding {
    pub fn shortcut(&self) -> Shortcut {
        let mut modifiers = Modifiers::empty();

        if self.ctrl {
            modifiers |= Modifiers::CONTROL;
        }
        if self.alt {
            modifiers |= Modifiers::ALT;
        }
        if self.shift {
            modifiers |= Modifiers::SHIFT;
        }
        if self.super_key {
            modifiers |= Modifiers::SUPER;
        }

        Shortcut::new(Some(modifiers), self.main_code)
    }
}

pub fn register_hotkey_inner(app: &AppHandle, hotkey: String) -> Result<String, String> {
    if hotkey.trim().is_empty() {
        let previous = {
            let state = app.state::<ClickerState>();
            let previous = state.registered_hotkey.lock().unwrap().take();
            previous
        };

        if let Some(previous) = previous {
            let _ = app.global_shortcut().unregister(previous.shortcut());
        }

        let state = app.state::<ClickerState>();
        state.suppress_hotkey_until_ms.store(0, Ordering::SeqCst);
        state
            .suppress_hotkey_until_release
            .store(false, Ordering::SeqCst);
        return Ok(String::new());
    }

    let binding = parse_hotkey_binding(&hotkey)?;
    let previous = {
        let state = app.state::<ClickerState>();
        let previous = state.registered_hotkey.lock().unwrap().clone();
        previous
    };

    if previous.as_ref() != Some(&binding) {
        if let Some(previous) = previous.as_ref() {
            let _ = app.global_shortcut().unregister(previous.shortcut());
        }

        if let Err(error) = bind_shortcut(app, &binding) {
            if let Some(previous) = previous.as_ref() {
                let _ = bind_shortcut(app, previous);
            }
            return Err(error);
        }
    }

    let state = app.state::<ClickerState>();
    state
        .suppress_hotkey_until_ms
        .store(now_epoch_ms().saturating_add(250), Ordering::SeqCst);
    state
        .suppress_hotkey_until_release
        .store(true, Ordering::SeqCst);
    *state.registered_hotkey.lock().unwrap() = Some(binding.clone());

    Ok(format_hotkey_binding(&binding))
}

fn bind_shortcut(app: &AppHandle, binding: &HotkeyBinding) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(binding.shortcut(), move |app_handle, _shortcut, event| {
            handle_shortcut_event(app_handle, event.state);
        })
        .map_err(|e| e.to_string())
}

fn handle_shortcut_event(app: &AppHandle, event_state: ShortcutState) {
    let state = app.state::<ClickerState>();

    if matches!(event_state, ShortcutState::Released)
        && state.suppress_hotkey_until_release.load(Ordering::SeqCst)
    {
        state
            .suppress_hotkey_until_release
            .store(false, Ordering::SeqCst);
        return;
    }

    if state.hotkey_capture_active.load(Ordering::SeqCst) {
        return;
    }

    match event_state {
        ShortcutState::Pressed => {
            if state.suppress_hotkey_until_release.load(Ordering::SeqCst) {
                return;
            }

            let suppress_until = state.suppress_hotkey_until_ms.load(Ordering::SeqCst);
            if now_epoch_ms() < suppress_until {
                return;
            }

            handle_hotkey_pressed(app);
        }
        ShortcutState::Released => {
            handle_hotkey_released(app);
        }
    }
}

pub fn normalize_hotkey(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace("control", "ctrl")
        .replace("command", "super")
        .replace("meta", "super")
        .replace("win", "super")
}

pub fn parse_hotkey_binding(hotkey: &str) -> Result<HotkeyBinding, String> {
    let normalized = normalize_hotkey(hotkey);
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut super_key = false;
    let mut main_key: Option<(Code, String)> = None;

    for token in normalized.split('+').map(str::trim) {
        if token.is_empty() {
            return Err(format!("Invalid hotkey '{hotkey}': found empty key token"));
        }

        match token {
            "alt" | "option" => alt = true,
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "super" | "command" | "cmd" | "meta" | "win" => super_key = true,
            _ => {
                if main_key
                    .replace(parse_hotkey_main_key(token, hotkey)?)
                    .is_some()
                {
                    return Err(format!(
                        "Invalid hotkey '{hotkey}': use modifiers first and only one main key"
                    ));
                }
            }
        }
    }

    let (main_code, key_token) =
        main_key.ok_or_else(|| format!("Invalid hotkey '{hotkey}': missing main key"))?;

    Ok(HotkeyBinding {
        ctrl,
        alt,
        shift,
        super_key,
        main_code,
        key_token,
    })
}

pub fn parse_hotkey_main_key(token: &str, original_hotkey: &str) -> Result<(Code, String), String> {
    let lower = token.trim().to_lowercase();

    let mapped = match lower.as_str() {
        "<" | ">" | "intlbackslash" | "oem102" | "nonusbackslash" => {
            Some((Code::IntlBackslash, String::from("IntlBackslash")))
        }
        "space" | "spacebar" => Some((Code::Space, String::from("space"))),
        "tab" => Some((Code::Tab, String::from("tab"))),
        "enter" => Some((Code::Enter, String::from("enter"))),
        "backspace" => Some((Code::Backspace, String::from("backspace"))),
        "delete" => Some((Code::Delete, String::from("delete"))),
        "insert" => Some((Code::Insert, String::from("insert"))),
        "home" => Some((Code::Home, String::from("home"))),
        "end" => Some((Code::End, String::from("end"))),
        "pageup" => Some((Code::PageUp, String::from("pageup"))),
        "pagedown" => Some((Code::PageDown, String::from("pagedown"))),
        "up" => Some((Code::ArrowUp, String::from("up"))),
        "down" => Some((Code::ArrowDown, String::from("down"))),
        "left" => Some((Code::ArrowLeft, String::from("left"))),
        "right" => Some((Code::ArrowRight, String::from("right"))),
        "esc" | "escape" => Some((Code::Escape, String::from("escape"))),
        "/" | "slash" => Some((Code::Slash, String::from("/"))),
        "\\" | "backslash" => Some((Code::Backslash, String::from("\\"))),
        ";" | "semicolon" => Some((Code::Semicolon, String::from(";"))),
        "'" | "quote" => Some((Code::Quote, String::from("'"))),
        "[" | "bracketleft" => Some((Code::BracketLeft, String::from("["))),
        "]" | "bracketright" => Some((Code::BracketRight, String::from("]"))),
        "-" | "minus" => Some((Code::Minus, String::from("-"))),
        "=" | "equal" => Some((Code::Equal, String::from("="))),
        "`" | "backquote" => Some((Code::Backquote, String::from("`"))),
        "," | "comma" => Some((Code::Comma, String::from(","))),
        "." | "period" => Some((Code::Period, String::from("."))),
        _ => None,
    };

    if let Some(binding) = mapped {
        return Ok(binding);
    }

    if lower.starts_with('f') && lower.len() <= 3 {
        if let Ok(number) = lower[1..].parse::<u8>() {
            let code = match number {
                1 => Some(Code::F1),
                2 => Some(Code::F2),
                3 => Some(Code::F3),
                4 => Some(Code::F4),
                5 => Some(Code::F5),
                6 => Some(Code::F6),
                7 => Some(Code::F7),
                8 => Some(Code::F8),
                9 => Some(Code::F9),
                10 => Some(Code::F10),
                11 => Some(Code::F11),
                12 => Some(Code::F12),
                13 => Some(Code::F13),
                14 => Some(Code::F14),
                15 => Some(Code::F15),
                16 => Some(Code::F16),
                17 => Some(Code::F17),
                18 => Some(Code::F18),
                19 => Some(Code::F19),
                20 => Some(Code::F20),
                21 => Some(Code::F21),
                22 => Some(Code::F22),
                23 => Some(Code::F23),
                24 => Some(Code::F24),
                _ => None,
            };

            if let Some(code) = code {
                return Ok((code, lower));
            }
        }
    }

    if let Some(letter) = lower.strip_prefix("key") {
        if letter.len() == 1 {
            return parse_hotkey_main_key(letter, original_hotkey);
        }
    }

    if let Some(digit) = lower.strip_prefix("digit") {
        if digit.len() == 1 {
            return parse_hotkey_main_key(digit, original_hotkey);
        }
    }

    if lower.len() == 1 {
        let ch = lower.as_bytes()[0];
        if ch.is_ascii_lowercase() {
            let code = match ch {
                b'a' => Code::KeyA,
                b'b' => Code::KeyB,
                b'c' => Code::KeyC,
                b'd' => Code::KeyD,
                b'e' => Code::KeyE,
                b'f' => Code::KeyF,
                b'g' => Code::KeyG,
                b'h' => Code::KeyH,
                b'i' => Code::KeyI,
                b'j' => Code::KeyJ,
                b'k' => Code::KeyK,
                b'l' => Code::KeyL,
                b'm' => Code::KeyM,
                b'n' => Code::KeyN,
                b'o' => Code::KeyO,
                b'p' => Code::KeyP,
                b'q' => Code::KeyQ,
                b'r' => Code::KeyR,
                b's' => Code::KeyS,
                b't' => Code::KeyT,
                b'u' => Code::KeyU,
                b'v' => Code::KeyV,
                b'w' => Code::KeyW,
                b'x' => Code::KeyX,
                b'y' => Code::KeyY,
                b'z' => Code::KeyZ,
                _ => unreachable!(),
            };
            return Ok((code, lower));
        }

        if ch.is_ascii_digit() {
            let code = match ch {
                b'0' => Code::Digit0,
                b'1' => Code::Digit1,
                b'2' => Code::Digit2,
                b'3' => Code::Digit3,
                b'4' => Code::Digit4,
                b'5' => Code::Digit5,
                b'6' => Code::Digit6,
                b'7' => Code::Digit7,
                b'8' => Code::Digit8,
                b'9' => Code::Digit9,
                _ => unreachable!(),
            };
            return Ok((code, lower));
        }
    }

    Err(format!(
        "Couldn't recognize '{token}' as a valid key in '{original_hotkey}'"
    ))
}

pub fn format_hotkey_binding(binding: &HotkeyBinding) -> String {
    let mut parts: Vec<String> = Vec::new();

    if binding.ctrl {
        parts.push(String::from("ctrl"));
    }
    if binding.alt {
        parts.push(String::from("alt"));
    }
    if binding.shift {
        parts.push(String::from("shift"));
    }
    if binding.super_key {
        parts.push(String::from("super"));
    }

    parts.push(binding.key_token.clone());
    parts.join("+")
}

pub fn start_hotkey_listener(_app: AppHandle) {}

pub fn handle_hotkey_pressed(app: &AppHandle) {
    let mode = {
        let state = app.state::<ClickerState>();
        let mode = state.settings.lock().unwrap().mode.clone();
        mode
    };

    if mode == "Toggle" {
        let _ = toggle_clicker_inner(app);
    } else if mode == "Hold" {
        let _ = start_clicker_inner(app);
    }
}

pub fn handle_hotkey_released(app: &AppHandle) {
    let mode = {
        let state = app.state::<ClickerState>();
        let mode = state.settings.lock().unwrap().mode.clone();
        mode
    };

    if mode == "Hold" {
        let _ = stop_clicker_inner(app, Some(String::from("Stopped from hold hotkey")));
    }
}
