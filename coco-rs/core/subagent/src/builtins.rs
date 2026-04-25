//! TS-parity built-in agent catalog.
//!
//! TS source: `tools/AgentTool/built-in/*.ts` and `builtInAgents.ts:22-72`.
//! Each built-in here mirrors the TS contract for `agentType`, `whenToUse`,
//! `tools`/`disallowedTools`, `model`, `color`, `background`, and
//! `omitClaudeMd`. The system prompt body is left empty here — runtime
//! supplies the prompt text via a `getSystemPrompt` analogue. The `name`
//! field doubles as the catalog ID and the lookup key in store snapshots.
//!
//! Optional built-ins (Explore, Plan, verification, claude-code-guide) are
//! gated by booleans on `BuiltinAgentCatalog`; consumers (CLI bootstrap)
//! map these from feature flags / GrowthBook gates.

use coco_types::{
    AgentColorName, AgentDefinition, AgentSource, AgentTypeId, SubagentType, ToolName,
};

/// What the SDK / CLI / TUI passes in to choose which optional built-ins
/// load. Mirrors the gate logic in `builtInAgents.ts` (`getBuiltInAgents`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuiltinAgentCatalog {
    /// `BUILTIN_EXPLORE_PLAN_AGENTS` + `tengu_amber_stoat`.
    pub include_explore_plan: bool,
    /// `VERIFICATION_AGENT` + `tengu_hive_evidence`.
    pub include_verification: bool,
    /// `claude-code-guide` is included for non-SDK entrypoints (CLI/TUI).
    pub include_claude_code_guide: bool,
    /// SDK noninteractive mode disables the entire built-in roster.
    pub disable_all: bool,
}

impl BuiltinAgentCatalog {
    /// Sensible defaults for an interactive CLI/TUI session.
    pub fn interactive() -> Self {
        Self {
            include_explore_plan: true,
            include_verification: false,
            include_claude_code_guide: true,
            disable_all: false,
        }
    }

    /// SDK noninteractive mode (`CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS`).
    /// Disables the entire built-in roster — caller may then inject
    /// extension built-ins via `AgentDefinitionStore::insert_definition`.
    pub fn sdk_noninteractive() -> Self {
        Self {
            disable_all: true,
            ..Self::default()
        }
    }

    /// All TS-parity built-ins enabled. Useful for snapshots and tests.
    pub fn all_enabled() -> Self {
        Self {
            include_explore_plan: true,
            include_verification: true,
            include_claude_code_guide: true,
            disable_all: false,
        }
    }
}

/// Resolve the built-in roster for a session.
pub fn builtin_definitions(catalog: BuiltinAgentCatalog) -> Vec<AgentDefinition> {
    if catalog.disable_all {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(6);
    out.push(general_purpose());
    out.push(statusline_setup());
    if catalog.include_explore_plan {
        out.push(explore());
        out.push(plan());
    }
    if catalog.include_verification {
        out.push(verification());
    }
    if catalog.include_claude_code_guide {
        out.push(claude_code_guide());
    }
    out
}

/// Lookup a single built-in by canonical (case-sensitive) `agent_type`.
pub fn builtin_definition(agent_type: &str) -> Option<AgentDefinition> {
    SubagentType::ALL.iter().find_map(|s| {
        if s.as_str() == agent_type {
            Some(builtin_for(*s))
        } else {
            None
        }
    })
}

fn builtin_for(t: SubagentType) -> AgentDefinition {
    match t {
        SubagentType::GeneralPurpose => general_purpose(),
        SubagentType::StatusLine => statusline_setup(),
        SubagentType::Explore => explore(),
        SubagentType::Plan => plan(),
        SubagentType::Verification => verification(),
        SubagentType::ClaudeCodeGuide => claude_code_guide(),
    }
}

// ── individual built-ins ──
//
// Each builder mirrors the TS file `tools/AgentTool/built-in/<name>.ts`.
// The `system_prompt` is left None: built-in prompts are dynamic (parent
// context drives them), so the runtime renders them on spawn.

fn base(t: SubagentType, when_to_use: &str) -> AgentDefinition {
    AgentDefinition {
        agent_type: AgentTypeId::Builtin(t),
        name: t.as_str().to_owned(),
        when_to_use: Some(when_to_use.to_owned()),
        description: Some(when_to_use.to_owned()),
        source: AgentSource::BuiltIn,
        base_dir: Some("built-in".to_owned()),
        ..Default::default()
    }
}

fn general_purpose() -> AgentDefinition {
    base(
        SubagentType::GeneralPurpose,
        "General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks.",
    )
}

fn read_only_disallowed() -> Vec<String> {
    // TS exploreAgent.ts:67-73, planAgent.ts:77-83, verificationAgent.ts:139-145.
    // These are real ToolName variants — `Edit`/`Write`, NOT `FileEdit`/`FileWrite`.
    vec![
        ToolName::Agent.as_str().into(),
        ToolName::ExitPlanMode.as_str().into(),
        ToolName::Edit.as_str().into(),
        ToolName::Write.as_str().into(),
        ToolName::NotebookEdit.as_str().into(),
    ]
}

fn statusline_setup() -> AgentDefinition {
    AgentDefinition {
        model: Some("sonnet".into()),
        color: Some(AgentColorName::Orange),
        allowed_tools: vec![
            ToolName::Read.as_str().into(),
            ToolName::Edit.as_str().into(),
        ],
        ..base(
            SubagentType::StatusLine,
            "Use this agent to configure the user's Claude Code status line setting.",
        )
    }
}

fn explore() -> AgentDefinition {
    AgentDefinition {
        // TS exploreAgent.ts:78: `process.env.USER_TYPE === 'ant' ? 'inherit' : 'haiku'`.
        // Default 3P/SDK build → haiku (cheaper, cache-friendly fast explore).
        // The runtime can override via `model` parameter.
        model: Some("haiku".into()),
        omit_claude_md: true,
        disallowed_tools: read_only_disallowed(),
        ..base(
            SubagentType::Explore,
            "Read-only exploration agent for surveying code and answering targeted questions about the repo.",
        )
    }
}

fn plan() -> AgentDefinition {
    AgentDefinition {
        model: Some("inherit".into()),
        omit_claude_md: true,
        disallowed_tools: read_only_disallowed(),
        ..base(
            SubagentType::Plan,
            "Planning agent that drafts implementation strategies without modifying code.",
        )
    }
}

fn verification() -> AgentDefinition {
    AgentDefinition {
        model: Some("inherit".into()),
        color: Some(AgentColorName::Red),
        background: true,
        disallowed_tools: read_only_disallowed(),
        ..base(
            SubagentType::Verification,
            "Background verification agent that checks the most recent change for regressions.",
        )
    }
}

fn claude_code_guide() -> AgentDefinition {
    // TS claudeCodeGuideAgent.ts: default branch tools = [Glob, Grep, Read,
    // WebFetch, WebSearch], permissionMode: 'dontAsk'.
    AgentDefinition {
        model: Some("haiku".into()),
        permission_mode: Some("dontAsk".into()),
        allowed_tools: vec![
            ToolName::Glob.as_str().into(),
            ToolName::Grep.as_str().into(),
            ToolName::Read.as_str().into(),
            ToolName::WebFetch.as_str().into(),
            ToolName::WebSearch.as_str().into(),
        ],
        ..base(
            SubagentType::ClaudeCodeGuide,
            "Use this agent for questions about Claude Code, the Claude Agent SDK, or the Anthropic API.",
        )
    }
}
