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
    // Verbatim TS `built-in/generalPurposeAgent.ts:27-29`.
    base(
        SubagentType::GeneralPurpose,
        "General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks. When you are searching for a keyword or file and are not confident that you will find the right match in the first few tries use this agent to perform the search for you.",
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
            // Verbatim TS `built-in/exploreAgent.ts:60-61` `EXPLORE_WHEN_TO_USE`.
            "Fast agent specialized for exploring codebases. Use this when you need to quickly find files by patterns (eg. \"src/components/**/*.tsx\"), search code for keywords (eg. \"API endpoints\"), or answer questions about the codebase (eg. \"how do API endpoints work?\"). When calling this agent, specify the desired thoroughness level: \"quick\" for basic searches, \"medium\" for moderate exploration, or \"very thorough\" for comprehensive analysis across multiple locations and naming conventions.",
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
            // Verbatim TS `built-in/planAgent.ts:75-77` whenToUse.
            "Software architect agent for designing implementation plans. Use this when you need to plan the implementation strategy for a task. Returns step-by-step plans, identifies critical files, and considers architectural trade-offs.",
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
            // Verbatim TS `built-in/verificationAgent.ts:131-133`
            // `VERIFICATION_WHEN_TO_USE`.
            "Use this agent to verify that implementation work is correct before reporting completion. Invoke after non-trivial tasks (3+ file edits, backend/API changes, infrastructure changes). Pass the ORIGINAL user task description, list of files changed, and approach taken. The agent runs builds, tests, linters, and checks to produce a PASS/FAIL/PARTIAL verdict with evidence.",
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
            // Verbatim TS `built-in/claudeCodeGuideAgent.ts:100`. The TS
            // template references `${SEND_MESSAGE_TOOL_NAME}` (= "SendMessage")
            // — inlined here as the constant value for byte-faithful prompt
            // rendering.
            "Use this agent when the user asks questions (\"Can Claude...\", \"Does Claude...\", \"How do I...\") about: (1) Claude Code (the CLI tool) - features, hooks, slash commands, MCP servers, settings, IDE integrations, keyboard shortcuts; (2) Claude Agent SDK - building custom agents; (3) Claude API (formerly Anthropic API) - API usage, tool use, Anthropic SDK usage. **IMPORTANT:** Before spawning a new agent, check if there is already a running or recently completed claude-code-guide agent that you can continue via SendMessage.",
        )
    }
}
