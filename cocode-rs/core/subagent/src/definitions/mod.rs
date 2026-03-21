mod bash;
mod code_simplifier;
mod explore;
mod general;
mod guide;
mod plan;
mod statusline;

use crate::definition::AgentDefinition;
use cocode_config::BuiltinAgentOverride;
use cocode_config::BuiltinAgentsConfig;
use cocode_protocol::execution::ExecutionIdentity;

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
        code_simplifier::code_simplifier_agent(),
    ]
}

/// Returns builtin agents with config overrides applied.
///
/// Loads configuration from `{cocode_home}/builtin-agents.json` and applies
/// any overrides to the hardcoded agent definitions.
///
/// # Example
///
/// ```ignore
/// use cocode_subagent::definitions::builtin_agents_with_overrides;
///
/// let agents = builtin_agents_with_overrides(&cocode_home);
/// // Agents now have any user-configured overrides applied
/// ```
pub fn builtin_agents_with_overrides(cocode_home: &std::path::Path) -> Vec<AgentDefinition> {
    let config = cocode_config::load_builtin_agents_config(cocode_home);
    builtin_agents_with_config(&config)
}

/// Returns all agents: builtins (with config overrides) + custom agents.
///
/// Custom agents are loaded from:
/// - `{cocode_home}/agents/` (user-level, lower priority)
/// - `{project_root}/.cocode/agents/` (project-level, higher priority)
///
/// Custom agents with the same `agent_type` as a builtin will override it.
pub fn all_agents(
    cocode_home: &std::path::Path,
    project_root: Option<&std::path::Path>,
) -> Vec<AgentDefinition> {
    let mut agents = builtin_agents_with_overrides(cocode_home);
    let custom = crate::loader::load_custom_agents(cocode_home, project_root);
    crate::loader::merge_custom_agents(&mut agents, custom);
    agents
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
        def.identity = Some(ExecutionIdentity::parse_loose(identity));
    }
    if let Some(ref tools) = cfg.tools {
        def.tools = tools.clone();
    }
    if let Some(ref disallowed) = cfg.disallowed_tools {
        def.disallowed_tools = disallowed.clone();
    }
    if let Some(fork_context) = cfg.fork_context {
        def.fork_context = fork_context;
    }
    if let Some(ref color) = cfg.color {
        def.color = Some(color.clone());
    }
    if let Some(ref reminder) = cfg.critical_reminder {
        def.critical_reminder = Some(reminder.clone());
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
