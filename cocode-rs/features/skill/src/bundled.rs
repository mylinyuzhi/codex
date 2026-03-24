//! Bundled skills with fingerprinting.
//!
//! Bundled skills are compiled into the binary and serve as defaults.
//! Each bundled skill includes a SHA-256 fingerprint of its prompt content
//! so that changes can be detected when comparing against user-overridden
//! versions.

use sha2::Digest;
use sha2::Sha256;

use crate::command::CommandType;

// Bundled skill prompt templates (embedded at compile time)
const PLUGIN_PROMPT: &str = include_str!("bundled/plugin_prompt.md");
const LOOP_PROMPT: &str = include_str!("bundled/loop_prompt.md");
const KEYBINDINGS_HELP_PROMPT: &str = include_str!("bundled/keybindings_help_prompt.md");

/// A skill bundled with the binary.
///
/// Contains the full prompt text and a SHA-256 fingerprint for change
/// detection.
#[derive(Debug, Clone)]
pub struct BundledSkill {
    /// Skill name.
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// Prompt text.
    pub prompt: String,

    /// SHA-256 hex fingerprint of the prompt content.
    pub fingerprint: String,

    /// Command type classification.
    pub command_type: CommandType,
}

/// Computes a SHA-256 hex fingerprint of the given content.
///
/// This is used to detect changes between bundled and user-overridden
/// skill prompts.
///
/// # Example
///
/// ```
/// # use cocode_skill::compute_fingerprint;
/// let fp = compute_fingerprint(b"hello world");
/// assert_eq!(fp.len(), 64); // SHA-256 hex is 64 chars
/// ```
pub fn compute_fingerprint(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    cocode_utils_string::bytes_to_hex(&result)
}

/// Returns the list of bundled skills.
///
/// Bundled skills are compiled into the binary and provide essential
/// system commands like output-style management.
pub fn bundled_skills() -> Vec<BundledSkill> {
    vec![
        BundledSkill {
            name: "loop".to_string(),
            description: "Run a prompt or slash command on a recurring interval (e.g. /loop 5m /foo, defaults to 10m)".to_string(),
            prompt: LOOP_PROMPT.to_string(),
            fingerprint: compute_fingerprint(LOOP_PROMPT.as_bytes()),
            command_type: CommandType::Prompt,
        },
        BundledSkill {
            name: "plugin".to_string(),
            description: "Manage plugins: install, uninstall, enable, disable, marketplace".to_string(),
            prompt: PLUGIN_PROMPT.to_string(),
            fingerprint: compute_fingerprint(PLUGIN_PROMPT.as_bytes()),
            command_type: CommandType::LocalJsx,
        },
        BundledSkill {
            name: "keybindings-help".to_string(),
            description: "View and customize keyboard shortcuts, rebind keys, add chord bindings".to_string(),
            prompt: KEYBINDINGS_HELP_PROMPT.to_string(),
            fingerprint: compute_fingerprint(KEYBINDINGS_HELP_PROMPT.as_bytes()),
            command_type: CommandType::Prompt,
        },
    ]
}

#[cfg(test)]
#[path = "bundled.test.rs"]
mod tests;
