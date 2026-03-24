//! Key representation and parsing.
//!
//! Parses human-readable key strings like `"ctrl+shift+t"` or chord
//! sequences like `"ctrl+k ctrl+c"` into structured types that can be
//! matched against crossterm key events.

use std::fmt;
use std::str::FromStr;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::error::KeybindingError;
use crate::error::ParseKeystrokeSnafu;

/// A single key combination (modifiers + key code).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub modifiers: KeyModifiers,
    pub code: KeyCode,
}

impl KeyCombo {
    pub fn new(modifiers: KeyModifiers, code: KeyCode) -> Self {
        Self { modifiers, code }
    }

    /// Check if this combo matches a crossterm key event.
    pub fn matches(&self, event: &KeyEvent) -> bool {
        let Some(combo) = key_event_to_combo(event) else {
            return false;
        };
        combos_match(self, &combo)
    }
}

/// Normalize modifiers: treat ALT and META as equivalent.
fn normalize_modifiers(m: KeyModifiers) -> KeyModifiers {
    let mut result = m;
    if m.contains(KeyModifiers::ALT) || m.contains(KeyModifiers::META) {
        result |= KeyModifiers::ALT | KeyModifiers::META;
    }
    result
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut need_sep = false;
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("Ctrl")?;
            need_sep = true;
        }
        if self.modifiers.contains(KeyModifiers::ALT) || self.modifiers.contains(KeyModifiers::META)
        {
            if need_sep {
                f.write_str("+")?;
            }
            f.write_str("Alt")?;
            need_sep = true;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            if need_sep {
                f.write_str("+")?;
            }
            f.write_str("Shift")?;
            need_sep = true;
        }
        if need_sep {
            f.write_str("+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) => {
                let upper = c.to_ascii_uppercase();
                write!(f, "{upper}")
            }
            other => f.write_str(key_code_display_name(other)),
        }
    }
}

fn key_code_display_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::Enter => "Enter",
        KeyCode::Esc => "Esc",
        KeyCode::Tab => "Tab",
        KeyCode::BackTab => "Shift+Tab",
        KeyCode::Backspace => "Backspace",
        KeyCode::Delete => "Delete",
        KeyCode::Up => "Up",
        KeyCode::Down => "Down",
        KeyCode::Left => "Left",
        KeyCode::Right => "Right",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "PageUp",
        KeyCode::PageDown => "PageDown",
        KeyCode::F(n) => match n {
            1 => "F1",
            2 => "F2",
            3 => "F3",
            4 => "F4",
            5 => "F5",
            6 => "F6",
            7 => "F7",
            8 => "F8",
            9 => "F9",
            10 => "F10",
            11 => "F11",
            12 => "F12",
            _ => "F?",
        },
        _ => "?",
    }
}

/// A sequence of key combinations forming a chord (or a single key press).
///
/// A single-key binding has length 1. A chord like `ctrl+k ctrl+c` has
/// length 2.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence {
    pub(crate) keys: Vec<KeyCombo>,
}

impl KeySequence {
    pub fn single(combo: KeyCombo) -> Self {
        Self { keys: vec![combo] }
    }

    /// Access the key combos in this sequence.
    pub fn keys(&self) -> &[KeyCombo] {
        &self.keys
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn is_chord(&self) -> bool {
        self.keys.len() > 1
    }

    /// Check if `pending` is a prefix of this sequence.
    pub fn is_prefix_of(&self, pending: &[KeyCombo]) -> bool {
        if pending.len() >= self.keys.len() {
            return false;
        }
        pending
            .iter()
            .zip(&self.keys)
            .all(|(a, b)| combos_match(a, b))
    }

    /// Check if `pending` exactly matches this sequence.
    pub fn matches_exactly(&self, pending: &[KeyCombo]) -> bool {
        if pending.len() != self.keys.len() {
            return false;
        }
        pending
            .iter()
            .zip(&self.keys)
            .all(|(a, b)| combos_match(a, b))
    }
}

fn combos_match(a: &KeyCombo, b: &KeyCombo) -> bool {
    let mods_match = normalize_modifiers(a.modifiers) == normalize_modifiers(b.modifiers);
    if !mods_match {
        return false;
    }
    match (a.code, b.code) {
        (KeyCode::Char(x), KeyCode::Char(y)) => x.eq_ignore_ascii_case(&y),
        (x, y) => x == y,
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, combo) in self.keys.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{combo}")?;
        }
        Ok(())
    }
}

/// Parse a key sequence string like `"ctrl+k ctrl+c"`.
///
/// Space separates chord steps. `+` separates modifiers from the key.
impl FromStr for KeySequence {
    type Err = KeybindingError;

    fn from_str(s: &str) -> crate::error::Result<Self> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.is_empty() {
            return Err(KeybindingError::ParseKeystroke {
                input: s.to_string(),
                reason: "empty key sequence".to_string(),
            });
        }

        let mut keys = Vec::with_capacity(parts.len());
        for part in parts {
            keys.push(parse_single_keystroke(part)?);
        }
        Ok(Self { keys })
    }
}

/// Parse a single keystroke like `"ctrl+shift+t"` or `"enter"`.
fn parse_single_keystroke(s: &str) -> crate::error::Result<KeyCombo> {
    let segments: Vec<&str> = s.split('+').collect();
    if segments.is_empty() || segments.iter().any(|seg| seg.is_empty()) {
        return ParseKeystrokeSnafu {
            input: s.to_string(),
            reason: "empty segment".to_string(),
        }
        .fail();
    }

    let mut modifiers = KeyModifiers::empty();
    let mut key_part = None;

    for (i, segment) in segments.iter().enumerate() {
        let lower = segment.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "opt" | "option" => modifiers |= KeyModifiers::ALT,
            "meta" | "cmd" | "command" | "super" | "win" => modifiers |= KeyModifiers::META,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            _ => {
                // Must be the key (last or only segment).
                if i != segments.len() - 1 {
                    return ParseKeystrokeSnafu {
                        input: s.to_string(),
                        reason: format!("unexpected modifier '{segment}'"),
                    }
                    .fail();
                }
                key_part = Some(lower);
            }
        }
    }

    let key_str = match key_part {
        Some(k) => k,
        None => {
            return ParseKeystrokeSnafu {
                input: s.to_string(),
                reason: "no key specified, only modifiers".to_string(),
            }
            .fail();
        }
    };
    let code = parse_key_code(&key_str).ok_or_else(|| KeybindingError::ParseKeystroke {
        input: s.to_string(),
        reason: format!("unknown key '{key_str}'"),
    })?;

    Ok(KeyCombo::new(modifiers, code))
}

/// Map a key name to a `KeyCode`.
fn parse_key_code(s: &str) -> Option<KeyCode> {
    match s {
        "enter" | "return" => Some(KeyCode::Enter),
        "esc" | "escape" => Some(KeyCode::Esc),
        "tab" => Some(KeyCode::Tab),
        "backtab" => Some(KeyCode::BackTab),
        "space" => Some(KeyCode::Char(' ')),
        "backspace" | "bs" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        "up" | "\u{2191}" => Some(KeyCode::Up),
        "down" | "\u{2193}" => Some(KeyCode::Down),
        "left" | "\u{2190}" => Some(KeyCode::Left),
        "right" | "\u{2192}" => Some(KeyCode::Right),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pageup" | "pgup" => Some(KeyCode::PageUp),
        "pagedown" | "pgdn" | "pgdown" => Some(KeyCode::PageDown),
        "f1" => Some(KeyCode::F(1)),
        "f2" => Some(KeyCode::F(2)),
        "f3" => Some(KeyCode::F(3)),
        "f4" => Some(KeyCode::F(4)),
        "f5" => Some(KeyCode::F(5)),
        "f6" => Some(KeyCode::F(6)),
        "f7" => Some(KeyCode::F(7)),
        "f8" => Some(KeyCode::F(8)),
        "f9" => Some(KeyCode::F(9)),
        "f10" => Some(KeyCode::F(10)),
        "f11" => Some(KeyCode::F(11)),
        "f12" => Some(KeyCode::F(12)),
        "_" => Some(KeyCode::Char('_')),
        "-" => Some(KeyCode::Char('-')),
        "?" => Some(KeyCode::Char('?')),
        s if s.len() == 1 => s
            .chars()
            .next()
            .map(|c| KeyCode::Char(c.to_ascii_lowercase())),
        _ => None,
    }
}

/// Convert a crossterm `KeyEvent` to a `KeyCombo` for chord tracking.
pub fn key_event_to_combo(event: &KeyEvent) -> Option<KeyCombo> {
    let code = match event.code {
        KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    };
    Some(KeyCombo::new(event.modifiers, code))
}

#[cfg(test)]
#[path = "key.test.rs"]
mod tests;
