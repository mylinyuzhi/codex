mod bash;
mod explore;
mod general;
mod guide;
mod plan;
mod statusline;

use crate::definition::AgentDefinition;
use cocode_config::BuiltinAgentOverride;
use cocode_config::BuiltinAgentsConfig;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Returns the complete set of built-in agent definitions.
///
/// These agents cover the most common subagent use-cases: shell execution,
/// file exploration, planning, general-purpose coding, guided reading, and
/// status-line updates.
pub fn builtin_agents() -> Vec<AgentDefinition> {
    vec![
        bash::bash_agent(),
        general::general_agent(),
        explore::explore_agent(),
        plan::plan_agent(),
        guide::guide_agent(),
        statusline::statusline_agent(),
    ]
}

/// Returns builtin agents with config overrides applied.
///
/// Loads configuration from `~/.cocode/builtin-agents.json` and applies
/// any overrides to the hardcoded agent definitions.
///
/// # Example
///
/// ```ignore
/// use cocode_subagent::definitions::builtin_agents_with_overrides;
///
/// let agents = builtin_agents_with_overrides();
/// // Agents now have any user-configured overrides applied
/// ```
pub fn builtin_agents_with_overrides() -> Vec<AgentDefinition> {
    let config = cocode_config::load_builtin_agents_config();
    builtin_agents_with_config(&config)
}

/// Returns builtin agents with the given config overrides applied.
///
/// This is the lower-level function that takes an explicit config,
/// useful for testing or when config is already loaded.
pub fn builtin_agents_with_config(config: &BuiltinAgentsConfig) -> Vec<AgentDefinition> {
    builtin_agents()
        .into_iter()
        .map(|mut def| {
            if let Some(override_cfg) = config.get(&def.agent_type) {
                apply_override(&mut def, override_cfg);
            }
            def
        })
        .collect()
}

/// Apply override configuration to an agent definition.
fn apply_override(def: &mut AgentDefinition, cfg: &BuiltinAgentOverride) {
    if let Some(max_turns) = cfg.max_turns {
        def.max_turns = Some(max_turns);
    }
    if let Some(ref identity) = cfg.identity {
        def.identity = Some(parse_identity(identity));
    }
    if let Some(ref tools) = cfg.tools {
        def.tools = tools.clone();
    }
    if let Some(ref disallowed) = cfg.disallowed_tools {
        def.disallowed_tools = disallowed.clone();
    }
}

/// Parse an identity string into an ExecutionIdentity.
///
/// Supported values:
/// - "main", "fast", "explore", "plan", "vision", "review", "compact" -> Role(ModelRole::*)
/// - "inherit" or unknown -> Inherit
fn parse_identity(s: &str) -> ExecutionIdentity {
    match s.to_lowercase().as_str() {
        "main" => ExecutionIdentity::Role(ModelRole::Main),
        "fast" => ExecutionIdentity::Role(ModelRole::Fast),
        "explore" => ExecutionIdentity::Role(ModelRole::Explore),
        "plan" => ExecutionIdentity::Role(ModelRole::Plan),
        "vision" => ExecutionIdentity::Role(ModelRole::Vision),
        "review" => ExecutionIdentity::Role(ModelRole::Review),
        "compact" => ExecutionIdentity::Role(ModelRole::Compact),
        "inherit" | _ => ExecutionIdentity::Inherit,
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
