//! Permission rule persistence to settings files.
//!
//! Supports persisting allow/deny/ask rules to user, project, or local settings
//! files with file-level locking for concurrent session safety.

use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use tracing::debug;
use tracing::warn;

use crate::rule::RuleAction;

/// Destination for persisted permission rules.
///
/// Aligned with Claude Code's three settings file locations:
/// - User: `{cocode_home}/settings.json` (applies to all projects)
/// - Project: `{project_root}/.cocode/settings.json` (committed to repo)
/// - Local: `{cocode_home}/settings.local.json` (machine-local, not committed)
///
/// For `User` and `Local`, the base path is the cocode home directory
/// (`~/.cocode/`). For `Project`, the base path is the **project root**
/// (the directory containing `.cocode/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDestination {
    /// User-level settings (`{cocode_home}/settings.json`).
    User,
    /// Project-level settings (`{project_root}/.cocode/settings.json`).
    ///
    /// **Note:** When using this variant, pass the project root as
    /// `base_path` (not the cocode home directory).
    Project,
    /// Machine-local settings (`{cocode_home}/settings.local.json`).
    Local,
}

impl RuleDestination {
    /// Resolve the settings file path relative to the given base directory.
    ///
    /// For `User`/`Local`, `base` should be the cocode home (`~/.cocode/`).
    /// For `Project`, `base` should be the project root directory.
    pub fn resolve_path(self, base: &Path) -> PathBuf {
        match self {
            RuleDestination::User => base.join("settings.json"),
            RuleDestination::Project => base.join(".cocode").join("settings.json"),
            RuleDestination::Local => base.join("settings.local.json"),
        }
    }
}

/// Persist a permission rule to the default location (`settings.local.json`).
///
/// This is the backward-compatible entry point that always uses `Allow` action
/// and `Local` destination.
pub async fn persist_rule(
    cocode_home: &Path,
    tool_name: &str,
    pattern: &str,
) -> std::io::Result<()> {
    persist_rule_with_options(
        cocode_home,
        tool_name,
        pattern,
        RuleAction::Allow,
        RuleDestination::Local,
    )
    .await
}

/// Persist a permission rule with explicit action and destination.
///
/// Appends the rule to the appropriate `permissions.{allow|deny|ask}` array
/// in the target settings file, creating the file if necessary.
///
/// Uses file-level locking (via `std::fs::File::lock()`) for safe concurrent
/// access from multiple sessions. Deduplicates automatically.
///
/// `base_path` is the cocode home for `User`/`Local`, or the project root
/// for `Project`.
pub async fn persist_rule_with_options(
    base_path: &Path,
    tool_name: &str,
    pattern: &str,
    action: RuleAction,
    destination: RuleDestination,
) -> std::io::Result<()> {
    let settings_path = destination.resolve_path(base_path);
    let rule = format_rule(tool_name, pattern);
    let action_key = action_json_key(action);

    debug!(
        rule = %rule,
        action = %action_key,
        path = %settings_path.display(),
        "Persisting permission rule"
    );

    // Use spawn_blocking for file locking (blocks the thread)
    tokio::task::spawn_blocking(move || persist_rule_locked(&settings_path, &rule, action_key))
        .await
        .map_err(std::io::Error::other)?
}

/// Remove a permission rule from the default location.
pub async fn remove_rule(
    cocode_home: &Path,
    tool_name: &str,
    pattern: &str,
) -> std::io::Result<()> {
    remove_rule_with_options(
        cocode_home,
        tool_name,
        pattern,
        RuleAction::Allow,
        RuleDestination::Local,
    )
    .await
}

/// Remove a permission rule with explicit action and destination.
pub async fn remove_rule_with_options(
    base_path: &Path,
    tool_name: &str,
    pattern: &str,
    action: RuleAction,
    destination: RuleDestination,
) -> std::io::Result<()> {
    let settings_path = destination.resolve_path(base_path);
    let rule = format_rule(tool_name, pattern);
    let action_key = action_json_key(action);

    tokio::task::spawn_blocking(move || remove_rule_locked(&settings_path, &rule, action_key))
        .await
        .map_err(std::io::Error::other)?
}

/// File-locked persist operation (runs on blocking thread).
fn persist_rule_locked(settings_path: &Path, rule: &str, action_key: &str) -> std::io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Open file with read+write+create (don't truncate)
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(settings_path)?;

    // Acquire exclusive lock (blocks until available)
    file.lock()?;

    // Read current content
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let mut config: serde_json::Value = if content.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    "Failed to parse {}: {e}, starting fresh",
                    settings_path.display()
                );
                serde_json::Value::Object(serde_json::Map::new())
            }
        }
    };

    // Insert rule (idempotent)
    if !insert_rule(&mut config, rule, action_key) {
        return Ok(());
    }

    // Write back (truncate then write)
    let output = serde_json::to_string_pretty(&config).map_err(std::io::Error::other)?;

    file.set_len(0)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    file.write_all(output.as_bytes())?;
    file.flush()?;

    // Lock released on drop
    Ok(())
}

/// File-locked remove operation (runs on blocking thread).
fn remove_rule_locked(settings_path: &Path, rule: &str, action_key: &str) -> std::io::Result<()> {
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(settings_path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    file.lock()?;

    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let mut config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse {}: {e}", settings_path.display());
            return Ok(());
        }
    };

    let pointer = format!("/permissions/{action_key}");
    let Some(arr) = config.pointer_mut(&pointer).and_then(|v| v.as_array_mut()) else {
        return Ok(());
    };

    let rule_val = serde_json::Value::String(rule.to_string());
    arr.retain(|v| v != &rule_val);

    let output = serde_json::to_string_pretty(&config).map_err(std::io::Error::other)?;

    file.set_len(0)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    file.write_all(output.as_bytes())?;
    file.flush()?;

    Ok(())
}

/// Format a rule string: `"Bash(git *)"` or just `"Read"`.
fn format_rule(tool_name: &str, pattern: &str) -> String {
    if pattern.is_empty() {
        tool_name.to_string()
    } else {
        format!("{tool_name}({pattern})")
    }
}

/// Map RuleAction to JSON key in the permissions object.
fn action_json_key(action: RuleAction) -> &'static str {
    match action {
        RuleAction::Allow => "allow",
        RuleAction::Deny => "deny",
        RuleAction::Ask => "ask",
    }
}

/// Insert a rule into the `permissions.{action}` array. Returns `true` if the
/// rule was added (not a duplicate).
fn insert_rule(config: &mut serde_json::Value, rule: &str, action_key: &str) -> bool {
    let Some(root) = config.as_object_mut() else {
        warn!("Settings root is not an object");
        return false;
    };
    let permissions = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));
    let Some(perms_obj) = permissions.as_object_mut() else {
        warn!("Settings permissions field is not an object");
        return false;
    };
    let target = perms_obj
        .entry(action_key)
        .or_insert_with(|| serde_json::json!([]));
    let Some(arr) = target.as_array_mut() else {
        warn!("Settings permissions.{action_key} is not an array");
        return false;
    };

    let rule_val = serde_json::Value::String(rule.to_string());
    if arr.contains(&rule_val) {
        return false;
    }
    arr.push(rule_val);
    true
}

/// Resolve the settings file path for a given base path and destination.
///
/// For `User`/`Local`, pass the cocode home directory.
/// For `Project`, pass the project root directory.
pub fn settings_path(base: &Path, destination: RuleDestination) -> PathBuf {
    destination.resolve_path(base)
}

#[cfg(test)]
#[path = "persist.test.rs"]
mod tests;
