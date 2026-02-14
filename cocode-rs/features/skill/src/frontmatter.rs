//! YAML frontmatter parser for SKILL.md files.
//!
//! Splits a markdown string on `---` delimiters to extract YAML frontmatter
//! and the markdown body (which serves as the prompt content).

/// Parse YAML frontmatter from a markdown string.
///
/// Splits on `---` delimiters at line starts. Returns `(yaml_str, body_str)`.
///
/// # Errors
///
/// Returns an error if the frontmatter delimiters are not found.
pub fn parse_frontmatter(content: &str) -> Result<(&str, &str), String> {
    // Find the opening `---` delimiter (must be at the very start or on its own line)
    let content = content.trim_start_matches('\u{feff}'); // strip BOM if present
    let rest = if content.starts_with("---") {
        &content[3..]
    } else {
        return Err("missing opening `---` frontmatter delimiter".to_string());
    };

    // Skip to end of the opening delimiter line
    let rest = match rest.find('\n') {
        Some(pos) => &rest[pos + 1..],
        None => return Err("frontmatter is empty (no closing `---`)".to_string()),
    };

    // Find the closing `---` delimiter on its own line
    let closing_pos = find_closing_delimiter(rest)?;

    let yaml_str = &rest[..closing_pos];
    let after_closing = &rest[closing_pos + 3..];

    // Skip the rest of the closing delimiter line
    let body = match after_closing.find('\n') {
        Some(pos) => &after_closing[pos + 1..],
        None => "",
    };

    Ok((yaml_str, body))
}

/// Find the position of the closing `---` delimiter in the remaining content.
///
/// The delimiter must appear at the start of a line.
fn find_closing_delimiter(content: &str) -> Result<usize, String> {
    let mut pos = 0;
    for line in content.lines() {
        if line.trim() == "---" {
            // Return position of this line's start
            return Ok(pos);
        }
        // Move past this line and its newline
        pos += line.len() + 1; // +1 for '\n'
    }
    Err("missing closing `---` frontmatter delimiter".to_string())
}

#[cfg(test)]
#[path = "frontmatter.test.rs"]
mod tests;
