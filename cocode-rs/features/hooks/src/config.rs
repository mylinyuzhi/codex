//! Configuration loading for hooks.
//!
//! Loads hook definitions from TOML files.

use std::path::Path;

use serde::Deserialize;
use tracing::debug;

use crate::definition::HookDefinition;
use crate::definition::HookHandler;
use crate::event::HookEventType;
use crate::matcher::HookMatcher;

/// Top-level TOML structure for hook configuration.
#[derive(Debug, Deserialize)]
struct HooksToml {
    #[serde(default)]
    hooks: Vec<HookTomlEntry>,
}

/// A single hook entry in TOML format.
#[derive(Debug, Deserialize)]
struct HookTomlEntry {
    name: String,
    event: HookEventType,
    #[serde(default)]
    matcher: Option<HookMatcherToml>,
    handler: HookHandlerToml,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default = "default_timeout")]
    timeout_secs: i32,
    #[serde(default)]
    once: bool,
}

fn default_enabled() -> bool {
    true
}

fn default_timeout() -> i32 {
    30
}

/// TOML representation of a hook matcher.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HookMatcherToml {
    Exact { value: String },
    Wildcard { pattern: String },
    Regex { pattern: String },
    All,
    Or { matchers: Vec<HookMatcherToml> },
}

/// TOML representation of a hook handler.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HookHandlerToml {
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Prompt {
        template: String,
        #[serde(default)]
        model: Option<String>,
    },
    Agent {
        #[serde(default = "default_max_turns")]
        max_turns: i32,
        #[serde(default)]
        prompt: Option<String>,
        #[serde(default = "default_agent_timeout")]
        timeout: i32,
    },
    Webhook {
        url: String,
    },
}

fn default_max_turns() -> i32 {
    50
}

fn default_agent_timeout() -> i32 {
    60
}

impl From<HookMatcherToml> for HookMatcher {
    fn from(toml: HookMatcherToml) -> Self {
        match toml {
            HookMatcherToml::Exact { value } => HookMatcher::Exact { value },
            HookMatcherToml::Wildcard { pattern } => HookMatcher::Wildcard { pattern },
            HookMatcherToml::Regex { pattern } => HookMatcher::Regex { pattern },
            HookMatcherToml::All => HookMatcher::All,
            HookMatcherToml::Or { matchers } => HookMatcher::Or {
                matchers: matchers.into_iter().map(Into::into).collect(),
            },
        }
    }
}

impl From<HookHandlerToml> for HookHandler {
    fn from(toml: HookHandlerToml) -> Self {
        match toml {
            HookHandlerToml::Command { command, args } => HookHandler::Command { command, args },
            HookHandlerToml::Prompt { template, model } => HookHandler::Prompt { template, model },
            HookHandlerToml::Agent {
                max_turns,
                prompt,
                timeout,
            } => HookHandler::Agent {
                max_turns,
                prompt,
                timeout,
            },
            HookHandlerToml::Webhook { url } => HookHandler::Webhook { url },
        }
    }
}

impl From<HookTomlEntry> for HookDefinition {
    fn from(entry: HookTomlEntry) -> Self {
        HookDefinition {
            name: entry.name,
            event_type: entry.event,
            matcher: entry.matcher.map(Into::into),
            handler: entry.handler.into(),
            source: Default::default(), // Defaults to Session; source is set by aggregator
            enabled: entry.enabled,
            timeout_secs: entry.timeout_secs,
            once: entry.once,
        }
    }
}

/// Loads hook definitions from a TOML file.
///
/// The file should have the following structure:
///
/// ```toml
/// [[hooks]]
/// name = "lint-check"
/// event = "pre_tool_use"
/// timeout_secs = 10
///
/// [hooks.matcher]
/// type = "exact"
/// value = "bash"
///
/// [hooks.handler]
/// type = "command"
/// command = "lint"
/// args = ["--check"]
/// ```
pub fn load_hooks_from_toml(path: &Path) -> Result<Vec<HookDefinition>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read hooks file '{}': {e}", path.display()))?;

    let hooks_toml: HooksToml = toml::from_str(&content)
        .map_err(|e| format!("failed to parse hooks TOML '{}': {e}", path.display()))?;

    let definitions: Vec<HookDefinition> = hooks_toml.hooks.into_iter().map(Into::into).collect();

    // Validate all matchers
    for def in &definitions {
        if let Some(matcher) = &def.matcher {
            matcher
                .validate()
                .map_err(|e| format!("invalid matcher in hook '{}': {e}", def.name))?;
        }
    }

    debug!(
        path = %path.display(),
        count = definitions.len(),
        "Loaded hooks from TOML"
    );

    Ok(definitions)
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
