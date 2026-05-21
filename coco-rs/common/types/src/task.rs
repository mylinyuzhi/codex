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

/// Base state shared by all task types.
///
/// **W6 (A5 merge)**: the five `LocalAgentExtra` sidecar fields
/// (`progress_summary`, `retrieved`, `retain`, `evict_after`,
/// `is_backgrounded`) now live directly on `TaskStateBase`. The
/// previous sparse-map design (`local_agent_extras: HashMap<id,
/// LocalAgentExtra>`) created a two-lock window during
/// `update_status` where a UI `set_retain(true)` could race the
/// `evict_after` stamp, silently evicting a panel-pinned task. The
/// merged layout reads + writes under a single `tasks` RwLock so the
/// entire transition is atomic.
///
/// These fields are meaningful only for `TaskType::LocalAgent` (and
/// `TaskType::Dream` — same model). They serialize with
/// `skip_serializing_if` so transcripts for shell tasks stay
/// equally compact as before.
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

    // ── LocalAgent / Dream sidecar fields (W6 / A5) ────────────────
    /// Live progress snapshot for the LocalAgent task. `None` until
    /// the first message arrives / summary fires. Surfaced in the
    /// compact reminder so the model sees "Progress: ..." text.
    ///
    /// TS parity: TaskStateBase.progress is an `AgentProgress` struct
    /// carrying input/output/cumulative token counts plus a 5-deep
    /// `recentActivities` FIFO and `lastToolName` — `LocalAgentTask.tsx:127,
    /// 240-241, 339-353`. The legacy `progress_summary: Option<String>`
    /// shape collapsed all of that into one freeform string, losing
    /// granularity the `task_status` reminder needs.
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
    /// by the TUI panel filter. TS: `LocalAgentTask.tsx:134`.
    #[serde(default)]
    pub is_backgrounded: bool,
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
