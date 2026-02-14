//! Skill hook registration.
//!
//! Converts skill frontmatter hooks to [`HookDefinition`]s and handles
//! registration and cleanup with the hook registry.
//!
//! ## Lifecycle
//!
//! 1. When a skill starts, call [`register_skill_hooks`] to add hooks
//! 2. The hooks are registered with [`HookSource::Skill`] scope
//! 3. When the skill ends, call [`cleanup_skill_hooks`] to remove them

use cocode_hooks::HookDefinition;
use cocode_hooks::HookEventType;
use cocode_hooks::HookHandler;
use cocode_hooks::HookMatcher;
use cocode_hooks::HookRegistry;
use cocode_hooks::HookSource;
use tracing::debug;
use tracing::warn;

use crate::interface::SkillHookConfig;
use crate::interface::SkillInterface;

/// Converts a [`SkillInterface`] hook configuration into [`HookDefinition`]s.
///
/// Returns a vector of hook definitions that can be registered with a registry.
pub fn convert_skill_hooks(interface: &SkillInterface) -> Vec<HookDefinition> {
    let Some(ref hooks_map) = interface.hooks else {
        return Vec::new();
    };

    let mut definitions = Vec::new();

    for (event_type_str, configs) in hooks_map {
        // Parse the event type
        let event_type = match parse_event_type(event_type_str) {
            Some(et) => et,
            None => {
                warn!(
                    skill = %interface.name,
                    event_type = %event_type_str,
                    "Unknown hook event type, skipping"
                );
                continue;
            }
        };

        for (idx, config) in configs.iter().enumerate() {
            if let Some(def) = convert_single_hook(&interface.name, event_type.clone(), config, idx)
            {
                definitions.push(def);
            }
        }
    }

    debug!(
        skill = %interface.name,
        hook_count = definitions.len(),
        "Converted skill hooks"
    );

    definitions
}

/// Register hooks from a skill interface with the registry.
///
/// Returns the number of hooks successfully registered.
pub fn register_skill_hooks(registry: &HookRegistry, interface: &SkillInterface) -> i32 {
    let definitions = convert_skill_hooks(interface);
    let count = definitions.len() as i32;

    for def in definitions {
        registry.register(def);
    }

    debug!(
        skill = %interface.name,
        count,
        "Registered skill hooks"
    );

    count
}

/// Remove all hooks registered by a specific skill.
pub fn cleanup_skill_hooks(registry: &HookRegistry, skill_name: &str) {
    registry.remove_hooks_by_source_name(skill_name);

    debug!(skill = skill_name, "Cleaned up skill hooks");
}

/// Parse an event type string into a [`HookEventType`].
///
/// Supports both PascalCase (from YAML keys) and snake_case.
fn parse_event_type(s: &str) -> Option<HookEventType> {
    match s {
        "PreToolUse" | "pre_tool_use" => Some(HookEventType::PreToolUse),
        "PostToolUse" | "post_tool_use" => Some(HookEventType::PostToolUse),
        "PostToolUseFailure" | "post_tool_use_failure" => Some(HookEventType::PostToolUseFailure),
        "UserPromptSubmit" | "user_prompt_submit" => Some(HookEventType::UserPromptSubmit),
        "SessionStart" | "session_start" => Some(HookEventType::SessionStart),
        "SessionEnd" | "session_end" => Some(HookEventType::SessionEnd),
        "Stop" | "stop" => Some(HookEventType::Stop),
        "SubagentStart" | "subagent_start" => Some(HookEventType::SubagentStart),
        "SubagentStop" | "subagent_stop" => Some(HookEventType::SubagentStop),
        "PreCompact" | "pre_compact" => Some(HookEventType::PreCompact),
        "Notification" | "notification" => Some(HookEventType::Notification),
        "PermissionRequest" | "permission_request" => Some(HookEventType::PermissionRequest),
        _ => None,
    }
}

/// Convert a single skill hook config to a hook definition.
fn convert_single_hook(
    skill_name: &str,
    event_type: HookEventType,
    config: &SkillHookConfig,
    index: usize,
) -> Option<HookDefinition> {
    // Determine the handler type
    let handler = if let Some(ref command) = config.command {
        HookHandler::Command {
            command: command.clone(),
            args: config.args.clone().unwrap_or_default(),
        }
    } else {
        warn!(
            skill = %skill_name,
            index,
            "Skill hook has no command, skipping"
        );
        return None;
    };

    // Convert the string-based matcher
    let matcher = config.matcher.as_ref().map(|s| convert_string_matcher(s));

    let hook_name = format!("{}:hook:{}", skill_name, index);

    Some(HookDefinition {
        name: hook_name,
        event_type,
        matcher,
        handler,
        source: HookSource::Skill {
            name: skill_name.to_string(),
        },
        enabled: true,
        timeout_secs: config.timeout_secs,
        once: config.once,
    })
}

/// Convert a string-based matcher pattern to a [`HookMatcher`].
///
/// Supports three formats:
/// - Pipe-separated: `"Write|Edit"` → `Or([Exact("Write"), Exact("Edit")])`
/// - Wildcard: `"Bash*"` → `Wildcard { pattern: "Bash*" }`
/// - Plain string: `"Write"` → `Exact { value: "Write" }`
fn convert_string_matcher(pattern: &str) -> HookMatcher {
    if pattern.contains('|') {
        HookMatcher::Or {
            matchers: pattern
                .split('|')
                .map(|s| HookMatcher::Exact {
                    value: s.trim().to_string(),
                })
                .collect(),
        }
    } else if pattern.contains('*') || pattern.contains('?') {
        HookMatcher::Wildcard {
            pattern: pattern.to_string(),
        }
    } else {
        HookMatcher::Exact {
            value: pattern.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "hooks.test.rs"]
mod tests;
