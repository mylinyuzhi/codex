//! Bundled skills with fingerprinting.
//!
//! Bundled skills are compiled into the binary and serve as defaults.
//! Each bundled skill includes a SHA-256 fingerprint of its prompt content
//! so that changes can be detected when comparing against user-overridden
//! versions.

use sha2::Digest;
use sha2::Sha256;

// Bundled skill prompt templates (embedded at compile time)
const OUTPUT_STYLE_PROMPT: &str = include_str!("bundled/output_style_prompt.md");
const PLUGIN_PROMPT: &str = include_str!("bundled/plugin_prompt.md");

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
    hex_encode(&result)
}

/// Encodes bytes as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Returns the list of bundled skills.
///
/// Bundled skills are compiled into the binary and provide essential
/// system commands like output-style management.
pub fn bundled_skills() -> Vec<BundledSkill> {
    vec![
        BundledSkill {
            name: "output-style".to_string(),
            description: "Manage response output styles (explanatory, learning, etc.)".to_string(),
            prompt: OUTPUT_STYLE_PROMPT.to_string(),
            fingerprint: compute_fingerprint(OUTPUT_STYLE_PROMPT.as_bytes()),
        },
        BundledSkill {
            name: "plugin".to_string(),
            description: "Manage plugins: install, uninstall, enable, disable, marketplace"
                .to_string(),
            prompt: PLUGIN_PROMPT.to_string(),
            fingerprint: compute_fingerprint(PLUGIN_PROMPT.as_bytes()),
        },
    ]
}

#[cfg(test)]
#[path = "bundled.test.rs"]
mod tests;
