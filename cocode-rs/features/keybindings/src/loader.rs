//! Keybinding configuration file loader.
//!
//! Loads and parses `~/.cocode/keybindings.json`, converting user-defined
//! bindings into the internal `Binding` type.

use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use tracing::debug;
use tracing::warn;

use crate::action::Action;
use crate::config::KeybindingsFile;
use crate::context::KeybindingContext;
use crate::key::KeySequence;
use crate::resolver::Binding;
use crate::validator::ValidationWarning;

/// The keybindings config filename.
const KEYBINDINGS_FILENAME: &str = "keybindings.json";

/// Load user keybindings from the config directory.
///
/// Returns an empty vec if the file doesn't exist (all defaults apply).
/// Returns warnings for any bindings that fail validation.
pub fn load_user_bindings(config_dir: &Path) -> (Vec<Binding>, Vec<ValidationWarning>) {
    let path = config_dir.join(KEYBINDINGS_FILENAME);
    load_from_path(&path)
}

/// Load bindings from a specific file path.
pub fn load_from_path(path: &Path) -> (Vec<Binding>, Vec<ValidationWarning>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            debug!("no keybindings file at {}, using defaults", path.display());
            return (Vec::new(), Vec::new());
        }
        Err(err) => {
            warn!("failed to read {}: {err}", path.display());
            return (
                Vec::new(),
                vec![ValidationWarning::ReadError {
                    path: path.to_path_buf(),
                    message: err.to_string(),
                }],
            );
        }
    };

    parse_keybindings_content(&content, path)
}

/// Parse keybindings from JSON content.
fn parse_keybindings_content(
    content: &str,
    source_path: &Path,
) -> (Vec<Binding>, Vec<ValidationWarning>) {
    let file: KeybindingsFile = match serde_json::from_str(content) {
        Ok(f) => f,
        Err(err) => {
            warn!("failed to parse {}: {err}", source_path.display());
            return (
                Vec::new(),
                vec![ValidationWarning::ParseError {
                    path: source_path.to_path_buf(),
                    message: err.to_string(),
                }],
            );
        }
    };

    let mut bindings = Vec::new();
    let mut warnings = Vec::new();

    for block in &file.bindings {
        let context = match KeybindingContext::from_str(&block.context) {
            Ok(ctx) => ctx,
            Err(_) => {
                warnings.push(ValidationWarning::InvalidContext {
                    name: block.context.clone(),
                });
                continue;
            }
        };

        for (key_str, action_str) in &block.bindings {
            let Some(action_str) = action_str else {
                continue;
            };

            let sequence = match KeySequence::from_str(key_str) {
                Ok(seq) => seq,
                Err(err) => {
                    warnings.push(ValidationWarning::InvalidKeystroke {
                        key: key_str.clone(),
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            let action = match Action::from_str(action_str) {
                Ok(a) => a,
                Err(_) => {
                    warnings.push(ValidationWarning::InvalidAction {
                        action: action_str.clone(),
                    });
                    continue;
                }
            };

            bindings.push(Binding {
                context,
                sequence,
                action,
            });
        }
    }

    let count = bindings.len();
    if count > 0 {
        debug!(
            "loaded {count} user bindings from {}",
            source_path.display()
        );
    }

    (bindings, warnings)
}

/// Get the keybindings file path for a config directory.
pub fn keybindings_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join(KEYBINDINGS_FILENAME)
}

#[cfg(test)]
#[path = "loader.test.rs"]
mod tests;
