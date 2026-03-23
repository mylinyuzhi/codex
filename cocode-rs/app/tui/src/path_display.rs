//! Shared path-display utilities.
//!
//! Used by both the header bar and status bar to shorten filesystem
//! paths for display (HOME replacement + middle-segment truncation).

/// Shorten a filesystem path for display.
///
/// Replaces the `$HOME` prefix with `~`, then if the result exceeds
/// `max_len` characters truncates the middle segments with `...`.
pub fn shorten_path(path: &str, max_len: i32) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let shortened = if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    };

    if shortened.len() > max_len as usize {
        let parts: Vec<&str> = shortened.split('/').collect();
        if parts.len() > 3 {
            format!("{}/.../{}", parts[..2].join("/"), parts[parts.len() - 1])
        } else {
            shortened
        }
    } else {
        shortened
    }
}

#[cfg(test)]
#[path = "path_display.test.rs"]
mod tests;
