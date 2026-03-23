//! Permission rule persistence to settings files.

use std::path::Path;

use tracing::warn;

/// Persist a permission rule to `settings.local.json`.
///
/// Appends the rule to the `permissions.allow` array in the settings file,
/// creating the file and intermediate directories if necessary.
///
/// The rule string is formatted as `"ToolName(pattern)"` or just `"ToolName"`
/// if the pattern is empty.
pub async fn persist_rule(
    cocode_home: &Path,
    tool_name: &str,
    pattern: &str,
) -> std::io::Result<()> {
    let settings_path = cocode_home.join("settings.local.json");

    let mut config: serde_json::Value = if settings_path.exists() {
        let content = tokio::fs::read_to_string(&settings_path).await?;
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
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    let rule = format_rule(tool_name, pattern);

    if !insert_allow_rule(&mut config, &rule) {
        return Ok(());
    }

    write_settings(&settings_path, &config).await
}

/// Remove a permission rule from `settings.local.json`.
///
/// Removes the rule from the `permissions.allow` array if it exists.
/// Returns `Ok(())` even if the rule was not found.
pub async fn remove_rule(
    cocode_home: &Path,
    tool_name: &str,
    pattern: &str,
) -> std::io::Result<()> {
    let settings_path = cocode_home.join("settings.local.json");

    if !settings_path.exists() {
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&settings_path).await?;
    let mut config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            warn!(
                "Failed to parse {}: {e}, starting fresh",
                settings_path.display()
            );
            serde_json::Value::Object(serde_json::Map::new())
        }
    };

    let rule = format_rule(tool_name, pattern);

    let Some(arr) = config
        .pointer_mut("/permissions/allow")
        .and_then(|v| v.as_array_mut())
    else {
        return Ok(());
    };

    let rule_val = serde_json::Value::String(rule);
    arr.retain(|v| v != &rule_val);

    write_settings(&settings_path, &config).await
}

/// Format a rule string: `"Bash(git *)"` or just `"Read"`.
fn format_rule(tool_name: &str, pattern: &str) -> String {
    if pattern.is_empty() {
        tool_name.to_string()
    } else {
        format!("{tool_name}({pattern})")
    }
}

/// Insert a rule into the `permissions.allow` array. Returns `true` if the
/// rule was added (not a duplicate).
fn insert_allow_rule(config: &mut serde_json::Value, rule: &str) -> bool {
    let Some(root) = config.as_object_mut() else {
        warn!("settings.local.json root is not an object");
        return false;
    };
    let permissions = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));
    let Some(perms_obj) = permissions.as_object_mut() else {
        warn!("settings.local.json permissions field is not an object");
        return false;
    };
    let allow = perms_obj
        .entry("allow")
        .or_insert_with(|| serde_json::json!([]));
    let Some(arr) = allow.as_array_mut() else {
        warn!("settings.local.json permissions.allow is not an array");
        return false;
    };

    let rule_val = serde_json::Value::String(rule.to_string());
    if arr.contains(&rule_val) {
        return false;
    }
    arr.push(rule_val);
    true
}

/// Write settings JSON to file, creating parent directories if needed.
async fn write_settings(settings_path: &Path, config: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = settings_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    tokio::fs::write(settings_path, content).await
}

#[cfg(test)]
#[path = "persist.test.rs"]
mod tests;
