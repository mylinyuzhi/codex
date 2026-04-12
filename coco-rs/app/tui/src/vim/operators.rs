//! Vim operators — d/c/y applied to motions or text objects.

use super::Operator;
use super::PersistentState;
use crate::state::ui::InputState;

/// Apply an operator to a text range.
///
/// Returns the text that was operated on (for yank/register).
pub fn apply_operator(
    input: &mut InputState,
    op: Operator,
    start: i32,
    end: i32,
    persistent: &mut PersistentState,
) -> String {
    let chars: Vec<char> = input.text.chars().collect();
    let s = (start.min(end)).max(0) as usize;
    let e = ((start.max(end)) + 1).min(chars.len() as i32) as usize;

    let operated: String = chars[s..e].iter().collect();

    match op {
        Operator::Delete => {
            // Remove range, cursor at start
            let byte_start = char_to_byte(&input.text, s as i32);
            let byte_end = char_to_byte(&input.text, e as i32);
            input.text.replace_range(byte_start..byte_end, "");
            input.cursor = s as i32;

            // Save to register
            persistent.register = operated.clone();
            persistent.register_is_linewise = false;
        }
        Operator::Change => {
            // Delete range and enter insert mode (caller handles mode switch)
            let byte_start = char_to_byte(&input.text, s as i32);
            let byte_end = char_to_byte(&input.text, e as i32);
            input.text.replace_range(byte_start..byte_end, "");
            input.cursor = s as i32;

            persistent.register = operated.clone();
            persistent.register_is_linewise = false;
        }
        Operator::Yank => {
            // Copy range to register, don't modify text
            persistent.register = operated.clone();
            persistent.register_is_linewise = false;
        }
    }

    operated
}

/// Put (paste) from register after cursor.
pub fn put_after(input: &mut InputState, persistent: &PersistentState) {
    if persistent.register.is_empty() {
        return;
    }
    let pos = (input.cursor + 1).min(input.text.chars().count() as i32);
    let byte_pos = char_to_byte(&input.text, pos);
    input.text.insert_str(byte_pos, &persistent.register);
    input.cursor = pos + persistent.register.chars().count() as i32 - 1;
}

/// Put (paste) from register before cursor.
pub fn put_before(input: &mut InputState, persistent: &PersistentState) {
    if persistent.register.is_empty() {
        return;
    }
    let byte_pos = char_to_byte(&input.text, input.cursor);
    input.text.insert_str(byte_pos, &persistent.register);
    input.cursor += persistent.register.chars().count() as i32 - 1;
}

/// Replace character under cursor.
pub fn replace_char(input: &mut InputState, ch: char) {
    let len = input.text.chars().count() as i32;
    if input.cursor >= 0 && input.cursor < len {
        let byte_start = char_to_byte(&input.text, input.cursor);
        let byte_end = char_to_byte(&input.text, input.cursor + 1);
        input
            .text
            .replace_range(byte_start..byte_end, &ch.to_string());
    }
}

/// Convert character index to byte offset.
fn char_to_byte(text: &str, char_idx: i32) -> usize {
    text.char_indices()
        .nth(char_idx as usize)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}
