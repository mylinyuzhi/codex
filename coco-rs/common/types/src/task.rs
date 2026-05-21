use serde::Deserialize;
use serde::Serialize;

/// Progress snapshot for a running LocalAgent task. TS:
/// `tools/AgentTool/agentToolUtils.ts` ProgressTracker + `tasks/LocalAgentTask/
/// LocalAgentTask.tsx:127` AgentProgress.
///
/// Token counts come from the engine's UsageAccumulator; `last_tool_name` and
/// `recent_activities` are recorded as the assistant emits tool_use blocks.
/// `summary` is the periodic AgentSummary 1-2 sentence text (separate path).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProgress {
    /// Input tokens used so far (prompt + context). TS: `latestInputTokens`.
    #[serde(default)]
    pub input_tokens: i64,
    /// Cumulative output tokens generated. TS: `cumulativeOutputTokens`.
    #[serde(default)]
    pub output_tokens: i64,
    /// `total_tokens = input + output` cached for convenience. TS:
    /// `getTokenCountFromTracker`.
    #[serde(default)]
    pub total_tokens: i64,
    /// Number of tool invocations observed. TS: `toolUseCount`.
    #[serde(default)]
    pub tool_use_count: i32,
    /// Most-recent tool name (for the `task_status` reminder's "last action"
    /// field). TS: `tracker.recentActivities[length-1].tool_name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
    /// Up to 5 most-recent activities (`{tool_name, summary}`), FIFO. TS:
    /// `recentActivities` queue clamped to length 5.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_activities: Vec<TaskActivity>,
    /// 1-2 sentence summary from the periodic AgentSummary timer. Independent
    /// of token deltas — `updateAgentSummary` writes this; `updateAgentProgress`
    /// preserves it (TS `LocalAgentTask.tsx:339-353`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// One entry in [`TaskProgress::recent_activities`]. TS:
/// `agentToolUtils.ts:Activity { tool_name, summary }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskActivity {
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

/// TS parity — five lifecycle states matching `Task.ts:15-21`. There
/// is no `Cancelled` variant: cancel-token cascades and explicit
/// `TaskStop` invocations both end the task in [`Self::Killed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// LocalAgent-specific sidecar fields. TS source:
/// `tasks/LocalAgentTask/LocalAgentTask.tsx:128-148` (the
/// `LocalAgentTaskState` extras beyond `TaskStateBase`).
///
/// Carried inside [`TaskExtras::LocalAgent`] so non-LocalAgent task
/// variants don't pay storage cost for fields that have no meaning
/// for them (TS uses a union type to express this; the Rust analog
/// is the [`TaskExtras`] enum).
///
/// **W6 (A5 merge)**: these fields used to live in a sparse
/// `HashMap<id, LocalAgentExtra>` separate from `TaskStateBase`,
/// which created a two-lock window during `update_status` where a UI
/// `set_retain(true)` could race the `evict_after` stamp, silently
/// evicting a panel-pinned task. They are now stored on the same
/// `TaskStateBase` instance so the entire transition happens under
/// the same lock.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalAgentExtras {
    /// Live progress snapshot for the LocalAgent task. `None` until
    /// the first message arrives / summary fires. Surfaced in the
    /// compact reminder so the model sees "Progress: ..." text.
    ///
    /// TS parity: `AgentProgress` carrying input/output/cumulative
    /// token counts plus a 5-deep `recentActivities` FIFO and
    /// `lastToolName` — `LocalAgentTask.tsx:127, 240-241, 339-353`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<TaskProgress>,
    /// True once `TaskOutputTool` reads the terminal output. Stops
    /// the compact reminder from announcing the same agent
    /// repeatedly. TS: `compact.ts:1578` `agent.retrieved`.
    #[serde(default)]
    pub retrieved: bool,
    /// When set by the TUI panel viewer, blocks eviction. TS:
    /// `LocalAgentTask.tsx:140`.
    #[serde(default)]
    pub retain: bool,
    /// Unix-ms deadline after which the panel may evict the task.
    /// Set to `current_time_ms() + PANEL_GRACE_MS` (30 s) at terminal
    /// transition unless `retain` is true. TS:
    /// `LocalAgentTask.tsx:294, 424, 448`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evict_after: Option<i64>,
    /// Session-backgrounded flag for Ctrl+B fg/bg switching. Used
    /// by the TUI panel filter. TS: `LocalAgentTask.tsx:134` /
    /// `:499` (async init = true) / `:564` (fg init = false).
    #[serde(default)]
    pub is_backgrounded: bool,
    /// Error text from a `Failed` terminal transition, surfaced by the
    /// post-compact `task_status` reminder so the model rediscovers
    /// failure context after compaction wiped the original
    /// `<task-notification>` envelope. `None` for non-Failed terminals
    /// and pre-terminal states.
    ///
    /// TS parity: `compact.ts:1591-1594` reads `agent.error` as
    /// `deltaSummary` for terminal tasks; renderer at
    /// `messages.ts:4005-4006` outputs `"Delta: <error>"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Per-`TaskType` sidecar extras. The Rust analog of TS's union
/// `LocalShellTaskState | LocalAgentTaskState | DreamTaskState | …`
/// where each variant carries variant-specific fields. Non-agent
/// task types currently carry no extras; the enum leaves room for
/// shell-specific (e.g. `exit_code` once promoted out of disk) and
/// dream-specific (e.g. consolidation stats) state.
///
/// Default: [`Self::None`] so existing constructors that don't set
/// extras don't carry LocalAgent-specific dead fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskExtras {
    /// LocalAgent extras — meaningful for `TaskType::LocalAgent` and
    /// (with the same shape) `TaskType::Dream`.
    LocalAgent(LocalAgentExtras),
    /// Shell / workflow / teammate / monitor / etc. — no
    /// variant-specific fields today. Kept as a distinct variant
    /// (vs. folding into [`Self::None`]) so a future extension for
    /// e.g. `LocalBash(LocalShellExtras)` doesn't require a wire-format
    /// migration.
    #[default]
    None,
}

impl TaskExtras {
    /// Pre-populate a LocalAgent variant with the supplied initial
    /// backgrounded flag. Used by [`TaskStateBase::new_running`] so
    /// the caller doesn't have to construct `LocalAgentExtras` by
    /// hand for the common bootstrap case.
    pub fn local_agent(is_backgrounded: bool) -> Self {
        Self::LocalAgent(LocalAgentExtras {
            is_backgrounded,
            ..LocalAgentExtras::default()
        })
    }

    /// Borrow the LocalAgent extras if this variant carries them.
    pub fn local_agent_ref(&self) -> Option<&LocalAgentExtras> {
        match self {
            Self::LocalAgent(e) => Some(e),
            _ => None,
        }
    }

    /// Mutably borrow the LocalAgent extras if this variant carries them.
    pub fn local_agent_mut(&mut self) -> Option<&mut LocalAgentExtras> {
        match self {
            Self::LocalAgent(e) => Some(e),
            _ => None,
        }
    }
}

/// Base state shared by all task types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStateBase {
    /// prefix + 8 random base36 chars
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub start_time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_paused_ms: Option<i64>,
    pub output_file: String,
    #[serde(default)]
    pub output_offset: i64,
    /// Per-variant sidecar fields. `TaskExtras::LocalAgent` carries
    /// the 5 LocalAgent-only fields (progress / retrieved / retain /
    /// evict_after / is_backgrounded); other task types use
    /// `TaskExtras::None` (Default). TS uses a union type for the
    /// same separation — Rust expresses it as an enum.
    #[serde(default)]
    pub extras: TaskExtras,
}

impl TaskStateBase {
    /// Borrow the LocalAgent extras if this task carries them. Returns
    /// `None` for shell / dream / teammate / monitor task types.
    /// Shorthand for `self.extras.local_agent_ref()`.
    pub fn local_agent_extras(&self) -> Option<&LocalAgentExtras> {
        self.extras.local_agent_ref()
    }

    /// Mutably borrow the LocalAgent extras if this task carries them.
    pub fn local_agent_extras_mut(&mut self) -> Option<&mut LocalAgentExtras> {
        self.extras.local_agent_mut()
    }

    /// `true` when the task is a LocalAgent (or Dream) AND has been
    /// marked backgrounded (either at registration via
    /// `registerAsyncAgent` or post-creation via `signal_detach`).
    /// Returns `false` for non-LocalAgent types — they have no
    /// `is_backgrounded` semantic.
    pub fn is_backgrounded(&self) -> bool {
        self.local_agent_extras()
            .map(|e| e.is_backgrounded)
            .unwrap_or(false)
    }

    /// `true` when the panel viewer has pinned this task open.
    pub fn retain(&self) -> bool {
        self.local_agent_extras().map(|e| e.retain).unwrap_or(false)
    }

    /// `true` when `TaskOutputTool` has consumed the terminal output.
    pub fn retrieved(&self) -> bool {
        self.local_agent_extras()
            .map(|e| e.retrieved)
            .unwrap_or(false)
    }

    /// The panel-grace deadline if set.
    pub fn evict_after(&self) -> Option<i64> {
        self.local_agent_extras().and_then(|e| e.evict_after)
    }

    /// The live progress snapshot if set.
    pub fn progress(&self) -> Option<&TaskProgress> {
        self.local_agent_extras().and_then(|e| e.progress.as_ref())
    }
}

/// Generate a task ID with type prefix + 8 random base36 chars.
pub fn generate_task_id(task_type: TaskType) -> String {
    let prefix = match task_type {
        TaskType::LocalBash => "tb",
        TaskType::LocalAgent => "ta",
        TaskType::RemoteAgent => "tr",
        TaskType::InProcessTeammate => "tt",
        TaskType::LocalWorkflow => "tw",
        TaskType::MonitorMcp => "tm",
        TaskType::Dream => "td",
    };
    let random: String = (0..8)
        .map(|_| {
            let idx = rand_u8() % 36;
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    format!("{prefix}{random}")
}

/// Simple random byte using uuid v4 as entropy source.
fn rand_u8() -> u8 {
    uuid::Uuid::new_v4().as_bytes()[0]
}
