//! Smoke tests for `runner_loop` after the task-storage unification.
//!
//! The pre-refactor suite exercised the deleted `InProcessTeammateTaskState`
//! mirror — every assertion read `state.is_idle`, `state.messages`,
//! `state.current_work_cancel` from the parallel store the
//! task-storage refactor removed. Those tests are no longer meaningful
//! against the unified `TaskManager`-only model; their replacements
//! live in `tasks/running.test.rs` (canonical row + control-handle
//! sibling map) and `agent_handle/mod.test.rs` (teammate dispatch).
//!
//! This file keeps the no-arg helpers (`AgentQueryConfig::default`,
//! `WaitResult` shape checks) as compile-time tripwires so accidental
//! API changes there fail loudly.

use super::*;

#[test]
fn agent_query_config_default_is_constructible() {
    let cfg = AgentQueryConfig::default();
    assert!(cfg.system_prompt.is_empty());
    assert!(cfg.allowed_tools.is_empty());
    assert!(cfg.disallowed_tools.is_empty());
    assert!(cfg.fork_context_messages.is_empty());
    assert!(cfg.cancel.is_none());
}

#[test]
fn wait_result_aborted_is_constructible() {
    let r = WaitResult::Aborted;
    assert!(matches!(r, WaitResult::Aborted));
}
