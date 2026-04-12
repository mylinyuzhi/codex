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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Killed | Self::Cancelled
        )
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
    #[serde(default)]
    pub notified: bool,
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
