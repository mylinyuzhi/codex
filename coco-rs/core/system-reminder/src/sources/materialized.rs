//! `MaterializeContext` + `MaterializedSources` — the transient
//! input/output of `ReminderSources::materialize()`.
//!
//! Separating these from `TurnReminderInput` keeps individual source
//! impls decoupled from the full reminder-input shape — a source
//! cannot accidentally read fields it shouldn't, and `TurnReminderInput`
//! can evolve without breaking source impls.

use std::collections::HashMap;
use std::time::Duration;

use coco_config::SystemReminderConfig;

use crate::generator::AgentPendingMessage;
use crate::generator::DiagnosticFileSummary;
use crate::generator::HookEvent;
use crate::generator::InvokedSkillEntry;
use crate::generator::TaskStatusSnapshot;
use crate::generator::TeamContextSnapshot;
use crate::generator::TeammateMailboxInfo;
use crate::generators::memory::NestedMemoryInfo;
use crate::generators::memory::RelevantMemoryInfo;
use crate::generators::user_input::IdeOpenedFileSnapshot;
use crate::generators::user_input::IdeSelectionSnapshot;
use crate::generators::user_input::McpResourceEntry;

/// Per-turn context handed to [`super::ReminderSources::materialize`].
pub struct MaterializeContext<'a> {
    /// Current reminder config — sources can skip work when their
    /// reminder is disabled. Materializer also uses this to config-gate
    /// each arm.
    pub config: &'a SystemReminderConfig,

    /// Current agent id (`None` = main thread).
    pub agent_id: Option<&'a str>,

    /// User's raw prompt text this turn (for MCP resource resolution,
    /// relevant-memories prefetch, etc.). `None` on tool-result
    /// iterations.
    pub user_input: Option<&'a str>,

    /// @-mentioned file paths extracted from `user_input` — passed to
    /// nested-memory traversal.
    pub mentioned_paths: &'a [std::path::PathBuf],

    /// True when the engine crossed a compaction boundary on this
    /// turn. Gates `task_status` (TS emits post-compaction only).
    pub just_compacted: bool,

    /// Per-source timeout (mirrors `SystemReminderConfig::timeout_ms`).
    /// Sources exceeding this time yield defaults.
    pub per_source_timeout: Duration,
}

/// Output of [`super::ReminderSources::materialize`] — flat data,
/// spread into `TurnReminderInput` fields by the engine.
#[derive(Default, Debug)]
pub struct MaterializedSources {
    pub hook_events: Vec<HookEvent>,
    pub diagnostics: Vec<DiagnosticFileSummary>,
    pub task_statuses: Vec<TaskStatusSnapshot>,
    pub skill_listing: Option<String>,
    pub invoked_skills: Vec<InvokedSkillEntry>,
    pub mcp_instructions_current: HashMap<String, String>,
    pub mcp_resources: Vec<McpResourceEntry>,
    pub teammate_mailbox: Option<TeammateMailboxInfo>,
    pub team_context: Option<TeamContextSnapshot>,
    pub agent_pending_messages: Vec<AgentPendingMessage>,
    pub ide_selection: Option<IdeSelectionSnapshot>,
    pub ide_opened_file: Option<IdeOpenedFileSnapshot>,
    pub nested_memories: Vec<NestedMemoryInfo>,
    pub relevant_memories: Vec<RelevantMemoryInfo>,
}
