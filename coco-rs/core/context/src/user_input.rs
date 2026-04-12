//! User input processing.
//!
//! TS: utils/processUserInput/ (1.8K LOC)
//! Pre-processes user input before sending to the model.

/// Process raw user input text before sending to the model.
///
/// Steps:
/// 1. Trim whitespace
/// 2. Detect /commands (slash prefix)
/// 3. Expand paste references
/// 4. Detect @mentions (file paths, URLs)
/// 5. Detect image pastes
pub fn process_user_input(raw: &str) -> ProcessedInput {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return ProcessedInput {
            text: String::new(),
            is_command: false,
            command_name: None,
            command_args: None,
            mentions: Vec::new(),
            has_images: false,
        };
    }

    // Check for slash commands
    if let Some(stripped) = trimmed.strip_prefix('/') {
        let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
        let cmd_name = parts[0].to_string();
        let cmd_args = parts.get(1).map(|s| s.to_string());
        return ProcessedInput {
            text: trimmed.to_string(),
            is_command: true,
            command_name: Some(cmd_name),
            command_args: cmd_args,
            mentions: Vec::new(),
            has_images: false,
        };
    }

    // Extract @mentions (file paths)
    let mentions = extract_mentions(trimmed);

    ProcessedInput {
        text: trimmed.to_string(),
        is_command: false,
        command_name: None,
        command_args: None,
        mentions,
        has_images: false,
    }
}

/// Processed user input with metadata.
#[derive(Debug, Clone)]
pub struct ProcessedInput {
    pub text: String,
    pub is_command: bool,
    pub command_name: Option<String>,
    pub command_args: Option<String>,
    pub mentions: Vec<Mention>,
    pub has_images: bool,
}

/// A mention in user input (file path, URL, etc).
#[derive(Debug, Clone)]
pub struct Mention {
    pub text: String,
    pub mention_type: MentionType,
    pub start: usize,
    pub end: usize,
    /// Start line for line-range mentions (`@file#L10` → Some(10)).
    pub line_start: Option<i32>,
    /// End line for line-range mentions (`@file#L10-20` → Some(20)).
    pub line_end: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionType {
    FilePath,
    Url,
    Symbol,
    Agent,
}

/// Extract @mentions from text.
///
/// Supports:
/// - `@file.rs` — simple file path
/// - `@"path with spaces.rs"` — quoted path
/// - `@file.rs#L10` or `@file.rs#L10-20` — line ranges
/// - `@agent-type` — agent mentions
/// - `@https://url` — URL mentions
fn extract_mentions(text: &str) -> Vec<Mention> {
    let mut mentions = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();

    while i < bytes.len() {
        // @ must be at start or preceded by whitespace
        if bytes[i] == b'@' && i + 1 < bytes.len() && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            let start = i;
            i += 1; // skip @

            let mention_text = if bytes[i] == b'"' {
                // Quoted path: @"path with spaces"
                i += 1; // skip opening "
                let quote_start = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                if i >= bytes.len() {
                    // Unclosed quote — skip this malformed mention
                    continue;
                }
                let raw = &text[quote_start..i];
                i += 1; // skip closing "
                raw
            } else {
                // Unquoted: collect until whitespace
                let token_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                &text[token_start..i]
            };

            if mention_text.is_empty() {
                continue;
            }

            // Parse line range from fragment: file#L10 or file#L10-20
            let (path_text, line_start, line_end) = parse_line_range(mention_text);

            // Classify mention type
            let is_agent = path_text.starts_with("agent-") || mention_text.ends_with(" (agent)");
            let mention_type = if is_agent {
                MentionType::Agent
            } else if path_text.contains("://") {
                MentionType::Url
            } else if path_text.contains('/') || path_text.contains('.') {
                MentionType::FilePath
            } else {
                MentionType::Symbol
            };

            mentions.push(Mention {
                text: path_text.to_string(),
                mention_type,
                start,
                end: i,
                line_start,
                line_end,
            });
        } else {
            i += 1;
        }
    }
    mentions
}

/// Parse `#L10` or `#L10-20` suffix from a mention token.
/// Returns `(path_without_fragment, line_start, line_end)`.
fn parse_line_range(mention: &str) -> (&str, Option<i32>, Option<i32>) {
    // Find #L pattern (case sensitive)
    if let Some(hash_pos) = mention.find("#L") {
        let path = &mention[..hash_pos];
        let fragment = &mention[hash_pos + 2..]; // after "#L"
        if let Some(dash) = fragment.find('-') {
            let start = fragment[..dash].parse::<i32>().ok();
            let end = fragment[dash + 1..].parse::<i32>().ok();
            if start.is_some() {
                return (path, start, end);
            }
        } else if let Ok(line) = fragment.parse::<i32>() {
            return (path, Some(line), None);
        }
    }
    (mention, None, None)
}

#[cfg(test)]
#[path = "user_input.test.rs"]
mod tests;
