//! Enhanced skill tool features ported from TS SkillTool/.
//!
//! TS: tools/SkillTool/SkillTool.ts, prompt.ts, constants.ts
//!
//! Provides skill invocation with argument substitution, prompt expansion,
//! context fork vs inline execution, and skill validation.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ── Skill execution mode ──

/// How a skill is executed: inline (in the current agent context) or
/// forked (in an isolated sub-agent with its own token budget).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillExecutionMode {
    /// Inline: the skill prompt is injected into the current conversation.
    /// The model processes it as part of the main agent's turn.
    #[default]
    Inline,
    /// Forked: the skill runs in an isolated sub-agent with its own
    /// message history, token budget, and tool access.
    Forked,
}

// ── Skill metadata ──

/// Source of a skill definition (simplified categorization for the tool layer).
///
/// The canonical enum with full path data is `coco_skills::SkillSource`.
/// This version omits paths since the tool layer only needs the category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Bundled with the application (first-party).
    Bundled,
    /// From a project-local .claude/ directory.
    Project,
    /// From user's global ~/.claude/ directory.
    User,
    /// From an installed plugin.
    Plugin,
    /// Enterprise/policy-managed skills.
    Managed,
    /// From an MCP server.
    Mcp,
}

/// Resolved skill ready for invocation.
#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    /// Canonical skill name (without leading slash).
    pub name: String,
    /// The prompt template to expand.
    pub prompt: String,
    /// Source where this skill was loaded from.
    pub source: SkillSource,
    /// Execution mode preference.
    pub execution_mode: SkillExecutionMode,
    /// Optional model override (e.g., "fast" for cheap skills).
    pub model_override: Option<String>,
    /// Tools this skill is allowed to use (empty = all tools).
    pub allowed_tools: Vec<String>,
    /// Tools this skill must not use.
    pub disallowed_tools: Vec<String>,
    /// Whether the model is allowed to invoke this skill (vs. user-only).
    pub allow_model_invocation: bool,
    /// Optional effort level override.
    pub effort: Option<String>,
}

// ── Skill output ──

/// Output from a skill invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutput {
    pub success: bool,
    pub command_name: String,
    #[serde(default)]
    pub status: SkillExecutionMode,
    /// Result text (only populated for forked executions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Sub-agent ID (only populated for forked executions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Allowed tools (only populated for inline executions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Model override (only populated for inline executions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

// ── Argument substitution ──

/// Options for expanding a skill prompt template.
pub struct ExpandOptions<'a> {
    /// Arguments provided by the user (e.g., `/skill arg1 arg2`).
    pub args: &'a str,
    /// Named argument names from the skill definition (e.g., `["env", "region"]`).
    ///
    /// TS: `argumentNames` — enables `$env`, `$region` placeholders.
    pub argument_names: &'a [String],
    /// The skill's source directory path (for `${CLAUDE_SKILL_DIR}`).
    pub skill_dir: Option<&'a str>,
    /// The current session ID (for `${CLAUDE_SESSION_ID}`).
    pub session_id: Option<&'a str>,
    /// Base directory to prepend (for `"Base directory for this skill: ..."` line).
    ///
    /// TS: `prependBaseDir()` in `loadSkillsDir.ts`.
    pub base_dir: Option<&'a str>,
    /// Plugin root directory (for `${CLAUDE_PLUGIN_ROOT}`).
    ///
    /// TS: `substitutePluginVariables()` in `loadPluginCommands.ts`.
    pub plugin_root: Option<&'a str>,
    /// Plugin persistent data directory (for `${CLAUDE_PLUGIN_DATA}`).
    ///
    /// TS: `substitutePluginVariables()` — separate from plugin root.
    pub plugin_data_dir: Option<&'a str>,
    /// User config values for `${user_config.KEY}` substitution.
    ///
    /// TS: `substituteUserConfigInContent()` in `pluginOptionsStorage.ts`.
    /// Keys are option names, values are (value, sensitive) pairs.
    /// Sensitive keys resolve to a placeholder instead of the actual value.
    pub user_config: Option<&'a [(&'a str, &'a str, bool)]>,
}

/// Expand placeholders in a skill prompt template.
///
/// Supports (in substitution order matching TS `substituteArguments()`):
/// 1. Named args: `$env`, `$region` — from `argument_names`
/// 2. Indexed: `$ARGUMENTS[0]`, `$ARGUMENTS[1]`
/// 3. Positional shorthand: `$0`, `$1`, `${1}` — word-boundary safe
/// 4. Full: `$ARGUMENTS` / `${ARGUMENTS}` — replaced with the full args string
/// 5. `${CLAUDE_SKILL_DIR}` — replaced with the skill's source directory
/// 6. `${CLAUDE_SESSION_ID}` — replaced with the current session ID
/// 7. `${CLAUDE_PLUGIN_ROOT}` — replaced with the plugin's install directory
/// 8. `${CLAUDE_PLUGIN_DATA}` — replaced with the plugin's persistent data dir
/// 9. `${user_config.KEY}` — replaced with user config values (sensitive masked)
///
/// If no argument placeholders are found, appends `ARGUMENTS: {args}` to the prompt.
///
/// TS: `substituteArguments()` + `substitutePluginVariables()` +
/// `substituteUserConfigInContent()` in `loadPluginCommands.ts`.
pub fn expand_skill_prompt(template: &str, opts: &ExpandOptions<'_>) -> String {
    let trimmed_args = opts.args.trim();
    let parts: Vec<&str> = trimmed_args.split_whitespace().collect();

    // Prepend base directory if provided (TS: prependBaseDir)
    let mut result = match opts.base_dir {
        Some(dir) if !dir.is_empty() => {
            format!("Base directory for this skill: {dir}\n\n{template}")
        }
        _ => template.to_string(),
    };

    // Replace ${CLAUDE_SKILL_DIR} with skill directory path
    if let Some(skill_dir) = opts.skill_dir {
        result = result.replace("${CLAUDE_SKILL_DIR}", skill_dir);
    }

    // Replace ${CLAUDE_SESSION_ID} with session ID
    if let Some(session_id) = opts.session_id {
        result = result.replace("${CLAUDE_SESSION_ID}", session_id);
    }

    // Replace ${CLAUDE_PLUGIN_ROOT} with plugin root directory
    // TS: substitutePluginVariables() in loadPluginCommands.ts
    if let Some(plugin_root) = opts.plugin_root {
        result = result.replace("${CLAUDE_PLUGIN_ROOT}", plugin_root);
    }

    // Replace ${CLAUDE_PLUGIN_DATA} with plugin data directory
    if let Some(plugin_data) = opts.plugin_data_dir {
        result = result.replace("${CLAUDE_PLUGIN_DATA}", plugin_data);
    }

    // Replace ${user_config.KEY} with user config values
    // TS: substituteUserConfigInContent() — sensitive keys resolve to a
    // descriptive placeholder instead of the actual value to prevent secrets
    // from leaking into model prompts.
    if let Some(config_entries) = opts.user_config {
        for &(key, value, sensitive) in config_entries {
            let placeholder = format!("${{user_config.{key}}}");
            if result.contains(&placeholder) {
                let replacement = if sensitive {
                    format!("[SENSITIVE:{key}]")
                } else {
                    value.to_string()
                };
                result = result.replace(&placeholder, &replacement);
            }
        }
    }

    // Track whether any argument placeholder was found
    let mut found_placeholder = false;

    // 1. Named args: $env, $region (TS: substituteArguments named args)
    for (i, name) in opts.argument_names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let placeholder = format!("${name}");
        if result.contains(&placeholder) {
            let value = parts.get(i).copied().unwrap_or("");
            result = result.replace(&placeholder, value);
            found_placeholder = true;
        }
    }

    // 2. Indexed: $ARGUMENTS[0], $ARGUMENTS[1] (TS: $ARGUMENTS[N])
    for i in 0..20 {
        let placeholder = format!("$ARGUMENTS[{i}]");
        if result.contains(&placeholder) {
            let value = parts.get(i).copied().unwrap_or("");
            result = result.replace(&placeholder, value);
            found_placeholder = true;
        }
    }

    // 3. Positional shorthand: $0, $1, ${0}, ${1} (TS: shorthand $N).
    //    Per TS `argumentSubstitution.ts:7` the shorthand is a
    //    zero-indexed alias for `$ARGUMENTS[N]`: `$0` = first arg,
    //    `$1` = second arg. This mirrors JS array indexing rather
    //    than shell positional parameters — deliberate TS choice
    //    (see doc comment on `substituteArguments`).
    let has_positional = (0..=20)
        .any(|i| result.contains(&format!("${i}")) || result.contains(&format!("${{{i}}}")));
    if has_positional {
        for (i, part) in parts.iter().enumerate() {
            result = result.replace(&format!("${{{i}}}"), part);
            result = result.replace(&format!("${i}"), part);
        }
        // Clear remaining positional placeholders (0..=20).
        for idx in 0..=20 {
            result = result.replace(&format!("${{{idx}}}"), "");
            result = result.replace(&format!("${idx}"), "");
        }
        found_placeholder = true;
    }

    // 4. Full: $ARGUMENTS / ${ARGUMENTS}
    if result.contains("$ARGUMENTS") || result.contains("${ARGUMENTS}") {
        result = result.replace("${ARGUMENTS}", trimmed_args);
        result = result.replace("$ARGUMENTS", trimmed_args);
        found_placeholder = true;
    }

    // If no placeholders found, append args with "ARGUMENTS:" prefix (TS behavior)
    if !found_placeholder && !trimmed_args.is_empty() {
        result = format!("{result}\n\nARGUMENTS: {trimmed_args}");
    }

    result
}

/// Convenience wrapper for expand_skill_prompt with only args (no dir/session/names).
pub fn expand_skill_prompt_simple(template: &str, args: &str) -> String {
    expand_skill_prompt(
        template,
        &ExpandOptions {
            args,
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    )
}

/// Normalize a skill name by stripping a leading slash and trimming.
pub fn normalize_skill_name(name: &str) -> &str {
    let trimmed = name.trim();
    trimmed.strip_prefix('/').unwrap_or(trimmed)
}

/// Validate a skill name for invocation.
///
/// Returns Ok(normalized_name) or Err(error_message).
pub fn validate_skill_name(name: &str) -> Result<&str, String> {
    let normalized = normalize_skill_name(name);
    if normalized.is_empty() {
        return Err(format!("Invalid skill format: {name}"));
    }
    // Disallow characters that could cause issues
    if normalized.contains("..") || normalized.contains('\0') {
        return Err(format!("Invalid characters in skill name: {name}"));
    }
    Ok(normalized)
}

/// Check if a skill name matches a permission rule pattern.
///
/// Supports exact match and prefix wildcards (e.g., "review:*" matches "review-pr").
pub fn skill_matches_rule(skill_name: &str, rule: &str) -> bool {
    let normalized_rule = rule.strip_prefix('/').unwrap_or(rule);

    // Exact match
    if normalized_rule == skill_name {
        return true;
    }
    // Prefix wildcard: "review:*" matches any skill starting with "review"
    if let Some(prefix) = normalized_rule.strip_suffix(":*") {
        return skill_name.starts_with(prefix);
    }
    false
}

/// Filter allowed tools for a skill invocation.
///
/// Starts with the skill's allowed_tools list, removes disallowed_tools,
/// then intersects with the globally available tools.
pub fn compute_effective_tools(skill: &ResolvedSkill, available_tools: &[String]) -> Vec<String> {
    let available_set: HashSet<&str> = available_tools.iter().map(String::as_str).collect();

    if skill.allowed_tools.is_empty() {
        // No restriction — use all available tools minus disallowed
        let disallowed: HashSet<&str> = skill.disallowed_tools.iter().map(String::as_str).collect();
        available_tools
            .iter()
            .filter(|t| !disallowed.contains(t.as_str()))
            .cloned()
            .collect()
    } else {
        // Intersect allowed with available, then remove disallowed
        let disallowed: HashSet<&str> = skill.disallowed_tools.iter().map(String::as_str).collect();
        skill
            .allowed_tools
            .iter()
            .filter(|t| available_set.contains(t.as_str()) && !disallowed.contains(t.as_str()))
            .cloned()
            .collect()
    }
}

/// Determine the execution mode for a skill based on its configuration
/// and whether a forked execution is beneficial.
pub fn determine_execution_mode(skill: &ResolvedSkill, force_inline: bool) -> SkillExecutionMode {
    if force_inline {
        return SkillExecutionMode::Inline;
    }
    skill.execution_mode
}

/// Build the skill invocation output for inline execution.
pub fn build_inline_output(skill: &ResolvedSkill) -> SkillOutput {
    SkillOutput {
        success: true,
        command_name: skill.name.clone(),
        status: SkillExecutionMode::Inline,
        result: None,
        agent_id: None,
        allowed_tools: if skill.allowed_tools.is_empty() {
            None
        } else {
            Some(skill.allowed_tools.clone())
        },
        model: skill.model_override.clone(),
    }
}

/// Build the skill invocation output for forked execution.
pub fn build_forked_output(skill_name: &str, agent_id: &str, result_text: &str) -> SkillOutput {
    SkillOutput {
        success: true,
        command_name: skill_name.to_string(),
        status: SkillExecutionMode::Forked,
        result: Some(result_text.to_string()),
        agent_id: Some(agent_id.to_string()),
        allowed_tools: None,
        model: None,
    }
}

#[cfg(test)]
#[path = "skill_advanced.test.rs"]
mod tests;
