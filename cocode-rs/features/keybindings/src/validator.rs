//! Keybinding validation pipeline.
//!
//! Provides validators matching Claude Code's validation system:
//! parse, context, action, duplicate, reserved, chord length,
//! namespace, platform-specific reserved keys, and JSON structure.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::config::KeybindingsFile;
use crate::context::KeybindingContext;

/// Maximum chord sequence length (keys in a single binding).
const MAX_CHORD_LENGTH: usize = 4;

/// Known action namespaces.
const KNOWN_NAMESPACES: &[&str] = &[
    "app",
    "chat",
    "history",
    "task",
    "confirm",
    "permission",
    "autocomplete",
    "select",
    "tabs",
    "attachments",
    "footer",
    "messageSelector",
    "diff",
    "modelPicker",
    "transcript",
    "historySearch",
    "theme",
    "help",
    "settings",
    "plugin",
    "voice",
    "ext",
    "command",
];

/// A non-fatal validation warning.
#[derive(Debug, Clone)]
pub enum ValidationWarning {
    /// Failed to read the keybindings file.
    ReadError { path: PathBuf, message: String },
    /// JSON parse error.
    ParseError { path: PathBuf, message: String },
    /// Unknown context name.
    InvalidContext { name: String },
    /// Invalid action string.
    InvalidAction { action: String },
    /// Invalid keystroke syntax.
    InvalidKeystroke { key: String, message: String },
    /// Duplicate key in the same context.
    DuplicateBinding { key: String, context: String },
    /// Reserved key that may not work correctly.
    ReservedKey { key: String, reason: String },
    /// Unknown action namespace (the part before `:`).
    UnknownNamespace { namespace: String, action: String },
    /// Chord sequence exceeds maximum length.
    ChordLengthExceeded { key: String, length: usize },
    /// Platform-specific reserved key (macOS Cmd shortcuts, Unix signals).
    PlatformReservedKey {
        key: String,
        platform: String,
        reason: String,
    },
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadError { path, message } => {
                write!(f, "failed to read {}: {message}", path.display())
            }
            Self::ParseError { path, message } => {
                write!(f, "failed to parse {}: {message}", path.display())
            }
            Self::InvalidContext { name } => {
                write!(f, "unknown context '{name}'")
            }
            Self::InvalidAction { action } => {
                write!(f, "unknown action '{action}'")
            }
            Self::InvalidKeystroke { key, message } => {
                write!(f, "invalid keystroke '{key}': {message}")
            }
            Self::DuplicateBinding { key, context } => {
                write!(f, "duplicate binding for '{key}' in {context}")
            }
            Self::ReservedKey { key, reason } => {
                write!(f, "'{key}' is reserved: {reason}")
            }
            Self::UnknownNamespace { namespace, action } => {
                write!(f, "unknown namespace '{namespace}' in action '{action}'")
            }
            Self::ChordLengthExceeded { key, length } => {
                write!(
                    f,
                    "chord '{key}' has {length} keys (max {MAX_CHORD_LENGTH})"
                )
            }
            Self::PlatformReservedKey {
                key,
                platform,
                reason,
            } => {
                write!(f, "'{key}' is reserved on {platform}: {reason}")
            }
        }
    }
}

/// Validate a keybindings file structure and detect issues.
///
/// Returns warnings for all problems found. An empty vec means the file
/// is valid.
pub fn validate_file(file: &KeybindingsFile) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    for block in &file.bindings {
        validate_context(&block.context, &mut warnings);
        validate_bindings_in_block(&block.context, &block.bindings, &mut warnings);
    }

    warnings
}

/// Check that the context name is valid.
fn validate_context(name: &str, warnings: &mut Vec<ValidationWarning>) {
    if name.parse::<KeybindingContext>().is_err() {
        warnings.push(ValidationWarning::InvalidContext {
            name: name.to_string(),
        });
    }
}

/// Check bindings within a context block for duplicates and reserved keys.
fn validate_bindings_in_block(
    context: &str,
    bindings: &std::collections::BTreeMap<String, Option<String>>,
    warnings: &mut Vec<ValidationWarning>,
) {
    let mut seen_keys = HashSet::new();

    for (key_str, action_str) in bindings {
        let canonical = key_str.to_ascii_lowercase();
        if !seen_keys.insert(canonical.clone()) {
            warnings.push(ValidationWarning::DuplicateBinding {
                key: key_str.clone(),
                context: context.to_string(),
            });
        }

        if let Some(reason) = check_reserved_key(key_str) {
            warnings.push(ValidationWarning::ReservedKey {
                key: key_str.clone(),
                reason,
            });
        }

        check_platform_reserved_key(key_str, warnings);

        if let Some(action) = action_str {
            if action.parse::<crate::action::Action>().is_err() {
                warnings.push(ValidationWarning::InvalidAction {
                    action: action.clone(),
                });
            }
            check_namespace(action, warnings);
        }

        match key_str.parse::<crate::key::KeySequence>() {
            Ok(seq) if seq.keys.len() > MAX_CHORD_LENGTH => {
                warnings.push(ValidationWarning::ChordLengthExceeded {
                    key: key_str.clone(),
                    length: seq.keys.len(),
                });
            }
            Err(_) => {
                warnings.push(ValidationWarning::InvalidKeystroke {
                    key: key_str.clone(),
                    message: "could not parse keystroke".to_string(),
                });
            }
            _ => {}
        }
    }
}

/// Check if a key is reserved and may not work correctly.
fn check_reserved_key(key_str: &str) -> Option<String> {
    // Only check the first key of a chord sequence for reserved status.
    let first_key = key_str.split_whitespace().next().unwrap_or(key_str);
    let lower = first_key.to_ascii_lowercase();
    match lower.as_str() {
        "ctrl+c" => Some("terminal interrupt (SIGINT)".to_string()),
        "ctrl+z" => Some("Unix process suspend (SIGTSTP)".to_string()),
        "ctrl+d" => Some("terminal EOF".to_string()),
        "ctrl+m" => Some("identical to Enter in most terminals".to_string()),
        "ctrl+[" => Some("identical to Escape in most terminals".to_string()),
        "ctrl+\\" => Some("terminal quit signal (SIGQUIT)".to_string()),
        _ => None,
    }
}

/// Check for platform-specific reserved keys.
fn check_platform_reserved_key(key_str: &str, warnings: &mut Vec<ValidationWarning>) {
    let first_key = key_str.split_whitespace().next().unwrap_or(key_str);
    let lower = first_key.to_ascii_lowercase();

    // macOS Cmd-key reservations (meta/cmd/command/super)
    let macos_reserved: &[(&str, &str)] = &[
        ("meta+c", "system copy"),
        ("meta+v", "system paste"),
        ("meta+x", "system cut"),
        ("meta+q", "quit application"),
        ("meta+w", "close window/tab"),
        ("meta+tab", "app switcher"),
        ("meta+space", "Spotlight search"),
        ("cmd+c", "system copy"),
        ("cmd+v", "system paste"),
        ("cmd+x", "system cut"),
        ("cmd+q", "quit application"),
        ("cmd+w", "close window/tab"),
        ("cmd+tab", "app switcher"),
        ("cmd+space", "Spotlight search"),
    ];

    for (key, reason) in macos_reserved {
        if lower == *key {
            warnings.push(ValidationWarning::PlatformReservedKey {
                key: key_str.to_string(),
                platform: "macOS".to_string(),
                reason: reason.to_string(),
            });
            return;
        }
    }
}

/// Check that the action namespace is recognized.
fn check_namespace(action: &str, warnings: &mut Vec<ValidationWarning>) {
    if let Some(ns) = action.split(':').next()
        && !KNOWN_NAMESPACES.contains(&ns)
    {
        warnings.push(ValidationWarning::UnknownNamespace {
            namespace: ns.to_string(),
            action: action.to_string(),
        });
    }
}

#[cfg(test)]
#[path = "validator.test.rs"]
mod tests;
