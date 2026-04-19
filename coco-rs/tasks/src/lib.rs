//! Task subsystem for coco-rs.
//!
//! This crate owns **three distinct kinds of task state**, mirroring
//! what Claude Code actually has in TS. They're deliberately separate —
//! a running background shell task, a persistent plan-item stored on
//! disk, and an ephemeral conversation checklist are different things
//! with different lifecycles.
//!
//! | Module | TS source | Purpose |
//! |--------|-----------|---------|
//! | [`running`] | `Task.ts` + `tasks/` | Running background tasks (shell/agent) with lifecycle events for the SDK NDJSON stream. |
//! | [`task_list`] | `utils/tasks.ts` | Durable plan items, persisted per task-list-id on disk with file locking. Shared across a team. |
//! | [`todos`] | `utils/todo/types.ts` + `AppState.todos[agentId]` | Ephemeral per-agent TodoWrite (V1) checklist. |
//!
//! V1 (TodoWrite) and V2 (Task* tools) are gated by the CLI layer via
//! `is_todo_v2_enabled()` — they're never both active. Running tasks
//! are orthogonal and always on.

pub mod handle_impls;
pub mod running;
pub mod task_list;
pub mod todos;

// Re-export the canonical surface so callers don't have to pierce the
// module tree for every type.
pub use running::TaskManager;
pub use running::TaskOutput;
pub use task_list::ClaimResult;
pub use task_list::Task;
pub use task_list::TaskListStore;
pub use task_list::TaskStatus;
pub use task_list::TaskUpdate;
pub use task_list::resolve_task_list_id;
pub use todos::TodoItem;
pub use todos::TodoStore;
