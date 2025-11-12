//! Configuration loading for hooks

use crate::action::{bash::BashAction, native::NativeAction, registry, HookAction};
use crate::executor::HookExecutor;
use crate::manager::HookManager;
use crate::types::{HookPhase, HookPriority, PRIORITY_NORMAL};
use codex_protocol::hooks::{HookActionConfig, HookDefinition, HooksConfig};
use std::path::Path;
use std::sync::Arc;

/// Load hooks configuration from a TOML file
pub fn load_config_from_file(path: impl AsRef<Path>) -> Result<HooksConfig, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    let config = toml::from_str(&content)?;
    Ok(config)
}

/// Build a HookManager from configuration
pub fn build_manager_from_config(config: HooksConfig) -> HookManager {
    let mut manager = HookManager::new();

    for (event_name, definitions) in config.hooks {
        let phase: HookPhase = event_name.into();

        for (index, definition) in definitions.into_iter().enumerate() {
            let actions = build_actions(&definition);

            if actions.is_empty() {
                tracing::warn!(
                    "No valid actions found for hook definition (event={:?}, matcher={})",
                    event_name,
                    definition.matcher
                );
                continue;
            }

            let executor = Arc::new(HookExecutor::new(actions, definition.sequential));

            // Use index as priority offset
            let priority = PRIORITY_NORMAL + (index as HookPriority);

            let num_actions = executor.actions.len();
            manager.register(phase, priority, executor);

            tracing::info!(
                "Registered hook: event={:?}, priority={}, actions={}, sequential={}",
                event_name,
                priority,
                num_actions,
                definition.sequential
            );
        }
    }

    // Enable the manager after loading config
    manager.set_enabled(true);

    manager
}

/// Build action list from hook definition
fn build_actions(definition: &HookDefinition) -> Vec<Arc<dyn HookAction>> {
    definition
        .hooks
        .iter()
        .filter_map(|action_config| match action_config {
            HookActionConfig::Command { command, timeout } => {
                Some(Arc::new(BashAction::new(command.clone(), *timeout)) as Arc<dyn HookAction>)
            }
            HookActionConfig::Native { function } => {
                if let Some(func) = registry::get_native_hook(function) {
                    Some(Arc::new(NativeAction::new(function.clone(), func)) as Arc<dyn HookAction>)
                } else {
                    tracing::warn!("Native hook function not found: {}", function);
                    None
                }
            }
        })
        .collect()
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_load_config_from_toml() {
        let toml_content = r#"
[[PreToolUse]]
matcher = "local_shell"
sequential = true

[[PreToolUse.hooks]]
type = "command"
command = "./test.sh"
timeout = 5000
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = load_config_from_file(temp_file.path()).unwrap();

        assert!(config.hooks.contains_key(&codex_protocol::hooks::HookEventName::PreToolUse));
    }

    #[test]
    fn test_build_manager_from_config() {
        let config = HooksConfig {
            hooks: std::collections::HashMap::new(),
        };

        let manager = build_manager_from_config(config);
        assert!(manager.is_enabled());
    }
}
