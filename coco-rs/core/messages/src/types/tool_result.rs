use coco_types::AppStatePatch;
use coco_types::PermissionUpdate;

use super::Message;

/// Result of a tool execution.
///
/// **Effect channels**:
/// - `app_state_patch` — a queued mutation of shared `ToolAppState`
///   that the executor applies post-execute (serial) or post-batch
///   (concurrent). Tools MUST NOT mutate shared `ToolAppState` inline
///   during `execute()` — `ToolUseContext.app_state` is an
///   [`AppStateReadHandle`](coco_types::AppStateReadHandle) with no
///   `.write()` method, so the compiler enforces the discipline.
/// - `permission_updates` — declarative permission-rule deltas
///   (typically `PermissionUpdate::AddRules` with destination
///   `Command` for skill `allowed-tools`). Applied via the executor's
///   permission-rule handle so subsequent turns see the new rules.
///   TS parity: `SkillTool` returns a `contextModifier` that wraps
///   `getAppState` to inject `alwaysAllowRules.command`.
///
/// TS parity: `orchestration.ts:queuedContextModifiers` — per-tool
/// `(ctx) => newCtx` modifiers keyed by `tool_use_id` and applied
/// after the concurrent batch finishes.
///
/// Not `Clone` / `Serialize` / `Deserialize`: the `app_state_patch`
/// closure can't participate in those traits. `ToolResult<T>` is
/// always consumed by the executor (applied + converted to
/// `Message::ToolResult`); no call path clones or serializes the
/// whole struct.
pub struct ToolResult<T> {
    pub data: T,
    pub new_messages: Vec<Message>,
    /// Queued mutation of shared app_state. `None` for tools that
    /// don't need to mutate (the overwhelming majority — only
    /// `EnterPlanMode` / `ExitPlanMode` currently return a patch).
    pub app_state_patch: Option<AppStatePatch>,
    /// Declarative permission-rule deltas. Empty for the overwhelming
    /// majority of tools — only the `SkillTool` populates this today,
    /// to forward a skill's frontmatter `allowed-tools` as Command-source
    /// auto-allow rules.
    pub permission_updates: Vec<PermissionUpdate>,
}

impl<T> ToolResult<T> {
    /// Shorthand: plain data result, no extra messages, no app_state
    /// mutation. Matches the 90%+ of tool call sites.
    pub fn data(data: T) -> Self {
        Self {
            data,
            new_messages: Vec::new(),
            app_state_patch: None,
            permission_updates: Vec::new(),
        }
    }

    /// Construct with data + extra messages and no mutation.
    pub fn with_messages(data: T, new_messages: Vec<Message>) -> Self {
        Self {
            data,
            new_messages,
            app_state_patch: None,
            permission_updates: Vec::new(),
        }
    }

    /// Attach a post-execute app_state patch. Consumes and returns
    /// the result fluently.
    pub fn with_patch(mut self, patch: AppStatePatch) -> Self {
        self.app_state_patch = Some(patch);
        self
    }

    /// Attach permission-rule deltas the executor should fold into
    /// the running session config after this tool returns.
    pub fn with_permission_updates(mut self, updates: Vec<PermissionUpdate>) -> Self {
        self.permission_updates = updates;
        self
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for ToolResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolResult")
            .field("data", &self.data)
            .field("new_messages", &self.new_messages)
            .field(
                "app_state_patch",
                &self.app_state_patch.as_ref().map(|_| "<fn>"),
            )
            .field("permission_updates", &self.permission_updates)
            .finish()
    }
}
