//! Vim motions — cursor movement commands.
//!
//! Each motion takes cursor position + text, returns new position.

/// Motion result: new cursor position.
pub type MotionResult = i32;

/// Move to next word start.
pub fn word_forward(text: &str, pos: i32) -> MotionResult {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len() as i32;
    let mut p = pos;

    // Skip current word
    while p < len && !chars[p as usize].is_whitespace() {
        p += 1;
    }
    // Skip whitespace
    while p < len && chars[p as usize].is_whitespace() {
        p += 1;
    }
    p.min(len)
}

/// Move to previous word start.
pub fn word_backward(text: &str, pos: i32) -> MotionResult {
    let chars: Vec<char> = text.chars().collect();
    let mut p = pos;

    // Skip whitespace before cursor
    while p > 0 && chars[(p - 1) as usize].is_whitespace() {
        p -= 1;
    }
    // Skip to start of word
    while p > 0 && !chars[(p - 1) as usize].is_whitespace() {
        p -= 1;
    }
    p.max(0)
}

/// Move to end of current word.
pub fn word_end(text: &str, pos: i32) -> MotionResult {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len() as i32;
    let mut p = pos + 1;

    // Skip whitespace
    while p < len && chars[p as usize].is_whitespace() {
        p += 1;
    }
    // Skip to end of word
    while p < len && !chars[p as usize].is_whitespace() {
        p += 1;
    }
    (p - 1).max(pos).min(len - 1)
}

/// Move to start of line (0).
pub fn line_start(_text: &str, _pos: i32) -> MotionResult {
    0
}

/// Move to first non-whitespace character (^).
pub fn first_non_blank(text: &str, _pos: i32) -> MotionResult {
    text.chars().position(|c| !c.is_whitespace()).unwrap_or(0) as i32
}

/// Move to end of line ($).
pub fn line_end(text: &str, _pos: i32) -> MotionResult {
    (text.chars().count() as i32 - 1).max(0)
}

/// Find character forward (f).
pub fn find_char_forward(text: &str, pos: i32, ch: char) -> Option<MotionResult> {
    let chars: Vec<char> = text.chars().collect();
    for i in (pos as usize + 1)..chars.len() {
        if chars[i] == ch {
            return Some(i as i32);
        }
    }
    None
}

/// Find character backward (F).
pub fn find_char_backward(text: &str, pos: i32, ch: char) -> Option<MotionResult> {
    let chars: Vec<char> = text.chars().collect();
    for i in (0..pos as usize).rev() {
        if chars[i] == ch {
            return Some(i as i32);
        }
    }
    None
}

/// Find till character forward (t) — one before the match.
pub fn till_char_forward(text: &str, pos: i32, ch: char) -> Option<MotionResult> {
    find_char_forward(text, pos, ch).map(|p| (p - 1).max(pos + 1))
}

/// Find till character backward (T) — one after the match.
pub fn till_char_backward(text: &str, pos: i32, ch: char) -> Option<MotionResult> {
    find_char_backward(text, pos, ch).map(|p| (p + 1).min(pos - 1))
}

/// Go to top (gg).
pub fn go_to_top(_text: &str, _pos: i32) -> MotionResult {
    0
}

/// Go to bottom (G).
pub fn go_to_bottom(text: &str, _pos: i32) -> MotionResult {
    (text.chars().count() as i32 - 1).max(0)
}
