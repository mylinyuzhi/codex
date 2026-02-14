//! Configuration loading for hooks.
//!
//! Loads hook definitions from JSON files.

use std::path::Path;

use serde::Deserialize;
use tracing::debug;

use crate::definition::HookDefinition;
use crate::definition::HookHandler;
use crate::event::HookEventType;
use crate::matcher::HookMatcher;

/// Top-level JSON structure for hook configuration.
#[derive(Debug, Deserialize)]
struct HooksJson {
    #[serde(default)]
    hooks: Vec<HookJsonEntry>,
}

/// A single hook entry in JSON format.
#[derive(Debug, Deserialize)]
struct HookJsonEntry {
    name: String,
    event: HookEventType,
    #[serde(default)]
    matcher: Option<HookMatcherJson>,
    handler: HookHandlerJson,
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

/// JSON representation of a hook matcher.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HookMatcherJson {
    Exact { value: String },
    Wildcard { pattern: String },
    Regex { pattern: String },
    All,
    Or { matchers: Vec<HookMatcherJson> },
}

/// JSON representation of a hook handler.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HookHandlerJson {
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

impl From<HookMatcherJson> for HookMatcher {
    fn from(json: HookMatcherJson) -> Self {
        match json {
            HookMatcherJson::Exact { value } => HookMatcher::Exact { value },
            HookMatcherJson::Wildcard { pattern } => HookMatcher::Wildcard { pattern },
            HookMatcherJson::Regex { pattern } => HookMatcher::Regex { pattern },
            HookMatcherJson::All => HookMatcher::All,
            HookMatcherJson::Or { matchers } => HookMatcher::Or {
                matchers: matchers.into_iter().map(Into::into).collect(),
            },
        }
    }
}

impl From<HookHandlerJson> for HookHandler {
    fn from(json: HookHandlerJson) -> Self {
        match json {
            HookHandlerJson::Command { command, args } => HookHandler::Command { command, args },
            HookHandlerJson::Prompt { template, model } => HookHandler::Prompt { template, model },
            HookHandlerJson::Agent {
                max_turns,
                prompt,
                timeout,
            } => HookHandler::Agent {
                max_turns,
                prompt,
                timeout,
            },
            HookHandlerJson::Webhook { url } => HookHandler::Webhook { url },
        }
    }
}

impl From<HookJsonEntry> for HookDefinition {
    fn from(entry: HookJsonEntry) -> Self {
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

/// Loads hook definitions from a JSON file.
///
/// The file should have the following structure:
///
/// ```json
/// {
///   "hooks": [
///     {
///       "name": "lint-check",
///       "event": "pre_tool_use",
///       "timeout_secs": 10,
///       "matcher": {
///         "type": "exact",
///         "value": "bash"
///       },
///       "handler": {
///         "type": "command",
///         "command": "lint",
///         "args": ["--check"]
///       }
///     }
///   ]
/// }
/// ```
pub fn load_hooks_from_json(path: &Path) -> Result<Vec<HookDefinition>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read hooks file '{}': {e}", path.display()))?;

    let hooks_json: HooksJson = serde_json::from_str(&content)
        .map_err(|e| format!("failed to parse hooks JSON '{}': {e}", path.display()))?;

    let definitions: Vec<HookDefinition> = hooks_json.hooks.into_iter().map(Into::into).collect();

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
        "Loaded hooks from JSON"
    );

    Ok(definitions)
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
