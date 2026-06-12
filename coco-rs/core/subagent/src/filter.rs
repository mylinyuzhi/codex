//! Tool filtering and nested-agent restriction parsing.
//!
//! Pure logic: takes (parent tool catalog, agent definition, runtime flags)
//! and returns a `ToolFilterPlan`. The plan is then applied to the child
//! `ToolRegistry` by `app/state` — this crate never touches the registry.

use coco_types::{AgentDefinition, MCP_TOOL_PREFIX, ToolName};

/// Tools blocked for every spawned agent.
///
/// Note: internally `Agent` can be re-allowed for ant builds to enable
/// nested-agent recursion. The default 3P/SDK build keeps `Agent` blocked;
/// the runtime can override the list per-spawn for ant builds.
pub const ALL_AGENT_DISALLOWED_TOOLS: &[&str] = &[
    ToolName::TaskOutput.as_str(),
    ToolName::ExitPlanMode.as_str(),
    ToolName::EnterPlanMode.as_str(),
    ToolName::Agent.as_str(),
    ToolName::AskUserQuestion.as_str(),
    ToolName::TaskStop.as_str(),
];

/// Custom agents inherit the universal block-list with no extras.
pub const CUSTOM_AGENT_DISALLOWED_TOOLS: &[&str] = ALL_AGENT_DISALLOWED_TOOLS;

/// Tools that are safe inside a background (async) agent.
///
/// Shell tools include `Bash` and `PowerShell` only — REPL is intentionally
/// excluded from the async-safe set (REPL is a long-lived stateful process
/// the runtime can't safely background).
pub const ASYNC_AGENT_ALLOWED_TOOLS: &[&str] = &[
    ToolName::Read.as_str(),
    ToolName::WebSearch.as_str(),
    ToolName::TodoWrite.as_str(),
    ToolName::Grep.as_str(),
    ToolName::WebFetch.as_str(),
    ToolName::Glob.as_str(),
    ToolName::Bash.as_str(),
    ToolName::PowerShell.as_str(),
    ToolName::Edit.as_str(),
    ToolName::Write.as_str(),
    ToolName::NotebookEdit.as_str(),
    ToolName::Skill.as_str(),
    ToolName::StructuredOutput.as_str(),
    ToolName::ToolSearch.as_str(),
    ToolName::EnterWorktree.as_str(),
    ToolName::ExitWorktree.as_str(),
];

/// Tools allowed only for in-process teammates (in addition to
/// [`ASYNC_AGENT_ALLOWED_TOOLS`]).
///
/// In-process teammates need these to coordinate via the shared task
/// list and inter-teammate mailbox. The `AGENT_TRIGGERS`-gated cron
/// tools (`CronCreate` / `CronDelete` / `CronList`) are included
/// unconditionally here — the feature gate is enforced upstream by
/// `Tool::is_enabled` so listing them costs nothing when the gate is
/// off.
pub const IN_PROCESS_TEAMMATE_ALLOWED_TOOLS: &[&str] = &[
    ToolName::TaskCreate.as_str(),
    ToolName::TaskGet.as_str(),
    ToolName::TaskList.as_str(),
    ToolName::TaskUpdate.as_str(),
    ToolName::SendMessage.as_str(),
    ToolName::CronCreate.as_str(),
    ToolName::CronDelete.as_str(),
    ToolName::CronList.as_str(),
];

/// The universal subagent tool block as deny-list names — the tools every
/// spawned subagent is denied regardless of its allow-list. Applied
/// *before* the allow-list intersection. `ExitPlanMode` is re-admitted
/// when `plan_mode` so a plan-mode subagent can still exit the plan.
///
/// coco-rs enforces tool visibility per-id via
/// [`coco_types::ToolFilter::allows`] (`tool-runtime/registry.rs`), so a
/// deny entry simply drops that tool from the model's list — no
/// `available_tools` snapshot is required (unlike the concrete-list
/// [`AgentToolFilter::plan`]). The caller merges these into the child
/// `ToolFilter`'s disallowed set.
pub fn subagent_disallowed_tools(plan_mode: bool) -> Vec<&'static str> {
    let exit_plan_mode = ToolName::ExitPlanMode.as_str();
    ALL_AGENT_DISALLOWED_TOOLS
        .iter()
        .copied()
        .filter(|name| !(plan_mode && *name == exit_plan_mode))
        .collect()
}

/// Inputs that drive `AgentToolFilter::plan`.
#[derive(Debug, Clone)]
pub struct ToolFilterContext<'a> {
    pub available_tools: &'a [String],
    pub is_builtin: bool,
    pub is_async: bool,
    pub plan_mode: bool,
    /// coco-rs extension: caller-supplied extra allow-list, e.g. a slash
    /// command's `allowed_tools`. Intersected on top of the agent's own
    /// allow-list. Used by the slash command runtime to over-restrict an
    /// agent for a specific invocation. Set to `None` for default behavior.
    pub extra_allow_list: Option<&'a [String]>,
    /// True when the spawn target is an in-process teammate AND
    /// agent-teams (`Feature::AgentTeams`) is on. When set, the async
    /// filter re-admits `Agent` plus [`IN_PROCESS_TEAMMATE_ALLOWED_TOOLS`]
    /// (Task* + SendMessage + Cron*) so teammates can coordinate via the
    /// shared task list and mailbox.
    pub is_in_process_teammate: bool,
}

/// Output of the filter plan: ready to feed into a child `ToolRegistry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFilterPlan {
    /// Tools that the child registry should expose. Order is stable (input
    /// order from `available_tools`).
    pub allowed_tools: Vec<String>,
    /// Tools the agent listed but that did not match anything available.
    pub unknown_tools: Vec<String>,
    /// True when the agent supplied no allow-list, i.e. "all tools default".
    pub uses_default_allow_list: bool,
}

pub struct AgentToolFilter;

impl AgentToolFilter {
    /// Compute the effective tool list for a child agent.
    ///
    /// Applied per candidate (short-circuit on first match):
    /// 1. MCP tools (`mcp__*`) — always allowed.
    /// 2. `ExitPlanMode` in `plan_mode` — bypasses both universal block
    ///    and async filter (sync OR async).
    /// 3. `ALL_AGENT_DISALLOWED_TOOLS` — universal block.
    /// 4. `CUSTOM_AGENT_DISALLOWED_TOOLS` — block for non-built-in agents.
    /// 5. Async agents: keep only `ASYNC_AGENT_ALLOWED_TOOLS`.
    ///
    /// Then the definition allow-list / deny-list are applied on the
    /// surviving set, and the optional `extra_allow_list` (coco-rs
    /// extension) intersects further.
    pub fn plan(def: &AgentDefinition, ctx: ToolFilterContext<'_>) -> ToolFilterPlan {
        let exit_plan_mode = ToolName::ExitPlanMode.as_str();
        let agent_tool = ToolName::Agent.as_str();
        let allowed_by_first_pass = |name: &&str| -> bool {
            // 1. MCP tools always pass.
            if name.starts_with(MCP_TOOL_PREFIX) {
                return true;
            }
            // 2. Plan-mode bypass for ExitPlanMode.
            if ctx.plan_mode && *name == exit_plan_mode {
                return true;
            }
            // 3. Universal block.
            if ALL_AGENT_DISALLOWED_TOOLS.contains(name) {
                // In-process teammates re-admit `Agent` so they can
                // spawn synchronous subagents (validated upstream by
                // `AgentTool::execute` to prevent background / teammate
                // spawning). The teammate MUST itself be running async
                // — sync teammates don't trigger this exception.
                if ctx.is_async && ctx.is_in_process_teammate && *name == agent_tool {
                    return true;
                }
                return false;
            }
            // 4. Custom agent extras.
            if !ctx.is_builtin && CUSTOM_AGENT_DISALLOWED_TOOLS.contains(name) {
                if ctx.is_async && ctx.is_in_process_teammate && *name == agent_tool {
                    return true;
                }
                return false;
            }
            // 5. Async allow-list.
            if ctx.is_async && !ASYNC_AGENT_ALLOWED_TOOLS.contains(name) {
                // In-process teammates also keep the
                // IN_PROCESS_TEAMMATE_ALLOWED_TOOLS set
                // (TaskCreate/Get/List/Update + SendMessage + Cron*)
                // so teammates can coordinate via the shared task list
                // and the inter-teammate mailbox.
                if ctx.is_in_process_teammate
                    && (*name == agent_tool || IN_PROCESS_TEAMMATE_ALLOWED_TOOLS.contains(name))
                {
                    return true;
                }
                return false;
            }
            true
        };
        let mut candidates: Vec<&str> = ctx
            .available_tools
            .iter()
            .map(String::as_str)
            .filter(allowed_by_first_pass)
            .collect();

        // Apply def.disallowed_tools BEFORE the def.allowed_tools intersection.
        // `allowedAvailableTools = filteredAvailableTools - disallowedToolSet`
        // is the catalog the allow-list is matched against — so a tool listed
        // in BOTH allow and deny is reported as `invalidTools`.
        if !def.disallowed_tools.is_empty() {
            let denied: Vec<&str> = def.disallowed_tools.iter().map(String::as_str).collect();
            candidates.retain(|name| !denied.contains(name));
        }

        // Agent's allow-list intersection. `Wildcard` = keep everything.
        // MCP tools do NOT bypass the allow-list — they are only
        // auto-included when the agent gave no allow-list at all.
        let uses_default_allow_list = def.allowed_tools.is_wildcard();
        let mut unknown_tools: Vec<String> = Vec::new();
        if let Some(explicit) = def.allowed_tools.as_explicit() {
            let allowed = parse_tool_allow_list(explicit);
            unknown_tools = allowed
                .iter()
                .filter(|name| !candidates.contains(*name))
                .map(|s| (*s).to_owned())
                .collect();
            candidates.retain(|name| allowed.contains(name));
        }

        // coco-rs extension: caller-supplied extra allow-list (e.g. slash
        // command `allowed_tools`). Same intersection semantics — no MCP
        // bypass, since callers pass an explicit set.
        if let Some(extra) = ctx.extra_allow_list
            && !extra.is_empty()
        {
            let extra_allowed: Vec<&str> = extra.iter().map(String::as_str).collect();
            candidates.retain(|name| extra_allowed.contains(name));
        }

        let allowed_tools: Vec<String> = candidates.into_iter().map(str::to_owned).collect();
        ToolFilterPlan {
            allowed_tools,
            unknown_tools,
            uses_default_allow_list,
        }
    }
}

/// Strip parenthesized arguments from allow-list entries: `Bash(*)` ↦ `Bash`.
///
/// Public so the subagent spawn path can normalise an
/// `AgentDefinition.allowed_tools` `Explicit` list into bare tool names
/// before handing them to a `ToolFilter` (which matches by `ToolId`, so a
/// raw `Bash(*)` would parse to `Custom("Bash(*)")` and never match).
pub fn parse_tool_allow_list(items: &[String]) -> Vec<&str> {
    items
        .iter()
        .map(|s| match s.find('(') {
            Some(i) => s[..i].trim(),
            None => s.trim(),
        })
        .collect()
}

// ── AllowedAgentTypes ──

/// Parsed `Agent(type1, type2, ...)` / `Task(type1, type2, ...)` restriction
/// from a permission rule. Both tool names are accepted (`Task` is a
/// permanent alias).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedAgentTypes {
    pub names: Vec<String>,
}

impl AllowedAgentTypes {
    /// A parsed entry with no listed types means the rule did not restrict
    /// types (it was effectively `Agent` or `Agent()` in the user's
    /// permissions), so every agent_type is allowed.
    pub fn matches(&self, agent_type: &str) -> bool {
        self.names.is_empty() || self.names.iter().any(|n| n == agent_type)
    }
}

/// Parse one allow-list entry like `Agent(Explore,plan)` or `Task(Plan)`.
///
/// Returns:
/// - `None` if the entry is not an `Agent`/`Task` restriction at all
///   (e.g. `Bash(npm test)` — caller should ignore those for this purpose).
/// - `None` for bare `Agent` / `Agent()` — the runtime treats this as
///   "no restriction"; returning `None` lets callers skip the matching
///   step entirely. To match this with a parsed value, use
///   `AllowedAgentTypes { names: vec![] }` whose `matches()` returns true
///   for every agent_type.
/// - `Some(AllowedAgentTypes { names })` with the listed types when an
///   explicit list is given (e.g. `Agent(Explore,Plan)`).
pub fn parse_allowed_agent_types(rule: &str) -> Option<AllowedAgentTypes> {
    let trimmed = rule.trim();
    let (head, paren_body) = match trimmed.find('(') {
        Some(i) => (&trimmed[..i], Some(&trimmed[i + 1..])),
        None => (trimmed, None),
    };
    if head.trim() != "Agent" && head.trim() != "Task" {
        return None;
    }
    let Some(body) = paren_body else {
        // Bare `Agent` / `Task` — no restriction. Caller treats as unrestricted.
        return None;
    };
    let inner = body.trim_end_matches(')');
    let names: Vec<String> = inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        // `Agent()` with empty parens — also unrestricted.
        return None;
    }
    Some(AllowedAgentTypes { names })
}
