//! Render the dynamic AgentTool prompt string.
//!
//! TS source: `tools/AgentTool/prompt.ts`. We reproduce two contracts here:
//!
//! 1. **Agent line format** (`prompt.ts:43-45`):
//!    `- {agentType}: {whenToUse} (Tools: {toolsDescription})`
//! 2. **Tools description branches** (`prompt.ts:15-37`):
//!    - allow + deny: comma-separated allow-list minus deny-list (or `"None"`)
//!    - allow only: comma-separated allow-list
//!    - deny only: `"All tools except {deny-list}"`
//!    - neither: `"All tools"`

use coco_types::AgentDefinition;

use crate::snapshot::AgentCatalogSnapshot;

/// Inputs that shape the AgentTool prompt for a given turn.
#[derive(Debug, Default, Clone)]
pub struct PromptOptions {
    /// Restrict the listed agents to this set (e.g. user's
    /// `Agent(type1,type2)` permission rule). `None` = no restriction.
    pub allowed_agent_types: Option<Vec<String>>,
    /// Pre-filter MCP-required agents whose servers are not yet ready.
    /// `None` = no MCP filtering.
    pub ready_mcp_servers: Option<Vec<String>>,
    /// When true, render slim coordinator prompt (no usage notes).
    pub coordinator_mode: bool,
    /// When true, render fork guidance section.
    pub fork_enabled: bool,
}

pub struct AgentToolPromptRenderer<'a> {
    snapshot: &'a AgentCatalogSnapshot,
}

impl<'a> AgentToolPromptRenderer<'a> {
    pub fn new(snapshot: &'a AgentCatalogSnapshot) -> Self {
        Self { snapshot }
    }

    /// Format the agent listing block. Order matches the snapshot's active
    /// iteration order, which is deterministic (alphabetical by
    /// `agent_type` via `BTreeMap`). TS preserves source-load order in
    /// `prompt.ts:198-199`; coco-rs uses alphabetical so multi-source
    /// reloads stay stable across `BuiltinAgentCatalog` toggles.
    pub fn agent_list(&self, opts: &PromptOptions) -> String {
        self.snapshot
            .active()
            .filter(|def| visible_to_prompt(def, opts))
            .map(format_agent_line)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Render the full AgentTool description that goes into the tool schema.
    /// Coordinator mode keeps it slim; non-coordinator mode includes usage
    /// notes, fork guidance, and parallel-spawn examples.
    pub fn full_prompt(&self, opts: &PromptOptions) -> String {
        let header = if self.snapshot.active_count() == 0 {
            "Launch a new agent to handle complex, multi-step tasks. (No agent types are currently available.)".to_owned()
        } else {
            "Launch a new agent to handle complex, multi-step tasks. Each agent type has specific capabilities and tools available to it.\n\nAvailable agent types and the tools they have access to:".to_owned()
        };

        let listing = self.agent_list(opts);

        let mut sections = Vec::with_capacity(4);
        sections.push(header);
        if !listing.is_empty() {
            sections.push(listing);
        }

        if !opts.coordinator_mode {
            if opts.fork_enabled {
                sections.push(fork_section());
            }
            sections.push(usage_notes_section(opts.fork_enabled));
        }

        sections.join("\n\n")
    }
}

fn visible_to_prompt(def: &AgentDefinition, opts: &PromptOptions) -> bool {
    if let Some(allowed) = opts.allowed_agent_types.as_ref()
        && !allowed.iter().any(|t| t == &def.name)
    {
        return false;
    }
    if !def.required_mcp_servers.is_empty() {
        let Some(ready) = opts.ready_mcp_servers.as_ref() else {
            return false;
        };
        // TS `loadAgentsDir.ts:237-241` does case-INsensitive substring match.
        let all_ready = def.required_mcp_servers.iter().all(|required| {
            let needle = required.to_ascii_lowercase();
            ready
                .iter()
                .any(|name| name.to_ascii_lowercase().contains(&needle))
        });
        if !all_ready {
            return false;
        }
    }
    true
}

fn format_agent_line(def: &AgentDefinition) -> String {
    let when = def
        .when_to_use
        .as_deref()
        .or(def.description.as_deref())
        .unwrap_or("");
    format!(
        "- {name}: {when} (Tools: {tools})",
        name = def.name,
        when = when,
        tools = format_tools_description(&def.allowed_tools, &def.disallowed_tools),
    )
}

/// Reproduces TS `getToolsDescription` (`prompt.ts:15-37`) verbatim.
pub fn format_tools_description(allowed: &[String], disallowed: &[String]) -> String {
    let allow_empty = allowed.is_empty();
    let deny_empty = disallowed.is_empty();
    if !allow_empty && !deny_empty {
        let effective: Vec<&String> = allowed.iter().filter(|t| !disallowed.contains(t)).collect();
        if effective.is_empty() {
            return "None".to_owned();
        }
        return effective
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    if !allow_empty {
        return allowed.join(", ");
    }
    if !deny_empty {
        return format!("All tools except {}", disallowed.join(", "));
    }
    "All tools".to_owned()
}

fn fork_section() -> String {
    // TS prompt.ts: "When to fork" block, paraphrased here. Keep tone aligned
    // with the TS guidance — the exact wording is consumed by the model.
    "When to fork: omit `subagent_type` to fork the current context. Forks inherit \
parent context and share the prompt cache; supply a directive-style prompt rather \
than re-briefing the agent."
        .to_owned()
}

fn usage_notes_section(fork_enabled: bool) -> String {
    let mut notes = String::new();
    notes.push_str("Usage notes:\n");
    notes.push_str("- Pick the agent whose listed tools match what the task needs; do not ask a read-only agent to write.\n");
    notes.push_str(
        "- Use `run_in_background: true` for long-running work; query status via the agent id.\n",
    );
    notes.push_str("- Spawn multiple agents in parallel when the work is independent.\n");
    notes.push_str("- Use `isolation: \"worktree\"` when the agent must edit files without disturbing the parent's working tree.\n");
    if fork_enabled {
        notes.push_str("- Fork (omit `subagent_type`) when you need the parent's context but cannot complete the task in the current turn.\n");
    }
    notes
}
