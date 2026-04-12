//! Vim text objects — select regions of text.
//!
//! Each text object returns (start, end) character positions.

use super::TextObjScope;

/// Text object result: (start, end) character positions (inclusive).
pub type TextObjResult = Option<(i32, i32)>;

/// Word text object (iw / aw).
pub fn word(text: &str, pos: i32, scope: TextObjScope) -> TextObjResult {
    let chars: Vec<char> = text.chars().collect();
    if pos < 0 || pos >= chars.len() as i32 {
        return None;
    }

    let p = pos as usize;
    let is_word_char = |c: char| !c.is_whitespace();

    if is_word_char(chars[p]) {
        // Inside a word
        let mut start = p;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = p;
        while end + 1 < chars.len() && is_word_char(chars[end + 1]) {
            end += 1;
        }

        if scope == TextObjScope::Around {
            // Include trailing whitespace
            while end + 1 < chars.len() && chars[end + 1].is_whitespace() {
                end += 1;
            }
        }

        Some((start as i32, end as i32))
    } else {
        // Inside whitespace
        let mut start = p;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let mut end = p;
        while end + 1 < chars.len() && chars[end + 1].is_whitespace() {
            end += 1;
        }
        Some((start as i32, end as i32))
    }
}

/// Quoted text object (i" / a" / i' / a').
pub fn quoted(text: &str, pos: i32, quote: char, scope: TextObjScope) -> TextObjResult {
    let chars: Vec<char> = text.chars().collect();
    let p = pos as usize;

    // Find opening quote (search backward then forward)
    let mut open = None;
    for i in (0..=p).rev() {
        if chars[i] == quote {
            open = Some(i);
            break;
        }
    }
    let open = open?;

    // Find closing quote
    let mut close = None;
    for i in (open + 1)..chars.len() {
        if chars[i] == quote {
            close = Some(i);
            break;
        }
    }
    let close = close?;

    match scope {
        TextObjScope::Inner => Some(((open + 1) as i32, (close - 1) as i32)),
        TextObjScope::Around => Some((open as i32, close as i32)),
    }
}

/// Bracket/brace text object (i( / a( / i{ / a{ / i[ / a[).
pub fn bracket(
    text: &str,
    pos: i32,
    open_ch: char,
    close_ch: char,
    scope: TextObjScope,
) -> TextObjResult {
    let chars: Vec<char> = text.chars().collect();
    let p = pos as usize;

    // Find matching opening bracket (search backward with nesting)
    let mut depth = 0i32;
    let mut open = None;
    for i in (0..=p).rev() {
        if chars[i] == close_ch {
            depth += 1;
        }
        if chars[i] == open_ch {
            if depth == 0 {
                open = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let open = open?;

    // Find matching closing bracket
    depth = 0;
    let mut close = None;
    for i in (open + 1)..chars.len() {
        if chars[i] == open_ch {
            depth += 1;
        }
        if chars[i] == close_ch {
            if depth == 0 {
                close = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let close = close?;

    match scope {
        TextObjScope::Inner => Some(((open + 1) as i32, (close - 1) as i32)),
        TextObjScope::Around => Some((open as i32, close as i32)),
    }
}
