use serde::Deserialize;
use serde::Serialize;

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
    /// Most recent agent-summary text. `None` until the first
    /// periodic summary fires. Surfaced in the compact reminder so
    /// the model sees "Progress: ..." text. TS:
    /// `LocalAgentTask.tsx:240-241` `progress.summary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_summary: Option<String>,
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
