//! Key string parser.
//!
//! TS: keybindings/ parser module — maps strings like `"ctrl+shift+a"`,
//! `"cmd+k"`, `"enter"` into a structured `KeyCombo`.
//!
//! Accepted syntax:
//! - Single key: `"a"`, `"1"`, `"enter"`, `"f1"`, `"space"`
//! - Modifiers joined by `+`: `"ctrl+a"`, `"ctrl+shift+p"`, `"cmd+k"`
//! - Case-insensitive: `"Ctrl+A"` parses the same as `"ctrl+a"`
//! - Alias: `"cmd"` == `"meta"` == `"super"` (platform-mapped by resolver)
//! - Chord: `"ctrl+k, ctrl+s"` — two combos separated by `,`

use serde::Deserialize;
use serde::Serialize;

/// One key combination (modifiers + base key).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Meta / cmd / super — platform-resolved at lookup time.
    pub meta: bool,
    /// The base key: either a single char (normalized lowercase) or a
    /// named key like `"enter"`, `"esc"`, `"f1"`.
    pub key: String,
}

/// A full chord: one or more combos that must be pressed in sequence.
/// Single-key bindings are one-element chords.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyChord(pub Vec<KeyCombo>);

impl KeyChord {
    /// Whether this is a single-combo chord (common case).
    pub fn is_single(&self) -> bool {
        self.0.len() == 1
    }
}

/// Parse errors are stringly because callers want a user-facing message,
/// not a matchable variant — they're surfaced via validator output.
pub type ParseError = String;

/// Parse a key chord string.
///
/// Empty input fails; whitespace around `+` and `,` is ignored.
pub fn parse_chord(input: &str) -> Result<KeyChord, ParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty key chord".into());
    }
    let combos: Vec<KeyCombo> = trimmed
        .split(',')
        .map(parse_combo)
        .collect::<Result<_, _>>()?;
    if combos.is_empty() {
        return Err("empty key chord after splitting on ','".into());
    }
    Ok(KeyChord(combos))
}

/// Parse a single combo (no commas).
pub fn parse_combo(input: &str) -> Result<KeyCombo, ParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty key combo".into());
    }
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut meta = false;
    let mut key: Option<String> = None;

    for piece in trimmed.split('+') {
        let token = piece.trim().to_ascii_lowercase();
        if token.is_empty() {
            return Err(format!("empty token in `{trimmed}`"));
        }
        match token.as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" | "opt" => alt = true,
            "meta" | "cmd" | "command" | "super" => meta = true,
            _ => {
                if key.is_some() {
                    return Err(format!(
                        "combo `{trimmed}` has more than one non-modifier key"
                    ));
                }
                key = Some(normalize_key(&token));
            }
        }
    }

    let key = key.ok_or_else(|| format!("combo `{trimmed}` has no base key"))?;
    Ok(KeyCombo {
        ctrl,
        shift,
        alt,
        meta,
        key,
    })
}

/// Normalize named keys to a canonical form. Single characters stay as
/// themselves; common aliases get collapsed (e.g. `return` → `enter`).
fn normalize_key(raw: &str) -> String {
    match raw {
        "return" => "enter".into(),
        "escape" => "esc".into(),
        "del" | "delete" => "del".into(),
        "bs" | "backspace" => "backspace".into(),
        "tab" => "tab".into(),
        "space" | " " => "space".into(),
        "up" | "down" | "left" | "right" => raw.into(),
        "home" | "end" | "pageup" | "pagedown" | "pgup" | "pgdn" => match raw {
            "pgup" => "pageup".into(),
            "pgdn" => "pagedown".into(),
            _ => raw.into(),
        },
        _ => raw.into(),
    }
}

#[cfg(test)]
#[path = "parser.test.rs"]
mod tests;
