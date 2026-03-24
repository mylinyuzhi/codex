//! Keybinding error types.

use snafu::Snafu;

/// Errors that can occur when parsing keybindings.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum KeybindingError {
    /// Failed to parse a keystroke string.
    #[snafu(display("failed to parse keystroke '{input}': {reason}"))]
    ParseKeystroke { input: String, reason: String },
}

pub type Result<T, E = KeybindingError> = std::result::Result<T, E>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
