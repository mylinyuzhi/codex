//! Teammate identity resolution — 3-tier priority system.
//!
//! TS: utils/teammate.ts, utils/teammateContext.ts
//!
//! Resolution priority:
//! 1. Thread-local context (in-process teammates via `tokio::task_local!`)
//! 2. Dynamic team context (set at runtime for tmux teammates)
//! 3. Environment variables (legacy/fallback)

use std::sync::RwLock;

use super::TeamContext;
use super::swarm_constants::AGENT_ID_ENV_VAR;
use super::swarm_constants::AGENT_NAME_ENV_VAR;
use super::swarm_constants::PARENT_SESSION_ID_ENV_VAR;
use super::swarm_constants::PLAN_MODE_REQUIRED_ENV_VAR;
use super::swarm_constants::TEAM_NAME_ENV_VAR;
use super::swarm_constants::TEAMMATE_COLOR_ENV_VAR;

// ── Thread-local Context (tier 1) ──

tokio::task_local! {
    /// Thread-local teammate context for in-process teammates.
    /// Set via `run_with_teammate_context()`.
    static TEAMMATE_CONTEXT: TeammateContextData;
}

/// Runtime context for in-process teammates (stored in task-local).
///
/// TS: `TeammateContext` in utils/teammateContext.ts
#[derive(Debug, Clone)]
pub struct TeammateContextData {
    pub agent_id: String,
    pub agent_name: String,
    pub team_name: String,
    pub color: Option<String>,
    pub plan_mode_required: bool,
    pub parent_session_id: String,
}

/// Run a future with teammate context set in task-local storage.
///
/// TS: `runWithTeammateContext(context, fn)`
pub async fn run_with_teammate_context<F, T>(context: TeammateContextData, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TEAMMATE_CONTEXT.scope(context, f).await
}

/// Get the current teammate context from task-local (if any).
///
/// TS: `getTeammateContext()`
pub fn get_teammate_context() -> Option<TeammateContextData> {
    TEAMMATE_CONTEXT.try_with(std::clone::Clone::clone).ok()
}

/// Check if running as an in-process teammate.
///
/// TS: `isInProcessTeammate()`
pub fn is_in_process_teammate() -> bool {
    TEAMMATE_CONTEXT.try_with(|_| ()).is_ok()
}

// ── Dynamic Context (tier 2) ──

/// Module-scoped dynamic team context (for tmux teammates).
static DYNAMIC_CONTEXT: RwLock<Option<DynamicTeamContext>> = RwLock::new(None);

/// Dynamic team context set at runtime (not via task-local).
///
/// TS: `dynamicTeamContext` in utils/teammate.ts
#[derive(Debug, Clone)]
pub struct DynamicTeamContext {
    pub agent_id: String,
    pub agent_name: String,
    pub team_name: String,
    pub color: Option<String>,
    pub plan_mode_required: bool,
    pub parent_session_id: Option<String>,
}

/// Set the dynamic team context.
///
/// TS: `setDynamicTeamContext(context)`
pub fn set_dynamic_team_context(ctx: DynamicTeamContext) {
    if let Ok(mut guard) = DYNAMIC_CONTEXT.write() {
        *guard = Some(ctx);
    }
}

/// Clear the dynamic team context.
///
/// TS: `clearDynamicTeamContext()`
pub fn clear_dynamic_team_context() {
    if let Ok(mut guard) = DYNAMIC_CONTEXT.write() {
        *guard = None;
    }
}

/// Get the dynamic team context.
///
/// TS: `getDynamicTeamContext()`
pub fn get_dynamic_team_context() -> Option<DynamicTeamContext> {
    DYNAMIC_CONTEXT.read().ok().and_then(|g| g.clone())
}

// ── Identity Resolution (3-tier) ──

/// Get the current agent ID (3-tier priority).
///
/// TS: `getAgentId()`
pub fn get_agent_id() -> Option<String> {
    // Tier 1: task-local
    if let Some(ctx) = get_teammate_context() {
        return Some(ctx.agent_id);
    }
    // Tier 2: dynamic context
    if let Some(ctx) = get_dynamic_team_context() {
        return Some(ctx.agent_id);
    }
    // Tier 3: env var (cross-process fallback).
    std::env::var(AGENT_ID_ENV_VAR).ok()
}

/// Get the current agent display name (3-tier priority).
///
/// TS: `getAgentName()`
pub fn get_agent_name() -> Option<String> {
    if let Some(ctx) = get_teammate_context() {
        return Some(ctx.agent_name);
    }
    if let Some(ctx) = get_dynamic_team_context() {
        return Some(ctx.agent_name);
    }
    std::env::var(AGENT_NAME_ENV_VAR).ok()
}

/// Get the current team name (3-tier priority).
///
/// TS: `getTeamName(teamContext?)`
pub fn get_team_name(team_context: Option<&TeamContext>) -> Option<String> {
    if let Some(ctx) = get_teammate_context() {
        return Some(ctx.team_name);
    }
    if let Some(ctx) = get_dynamic_team_context() {
        return Some(ctx.team_name);
    }
    if let Some(tc) = team_context {
        return Some(tc.team_name.clone());
    }
    std::env::var(TEAM_NAME_ENV_VAR).ok()
}

/// Get the parent session ID.
///
/// TS: `getParentSessionId()`
pub fn get_parent_session_id() -> Option<String> {
    if let Some(ctx) = get_teammate_context() {
        return Some(ctx.parent_session_id);
    }
    if let Some(ctx) = get_dynamic_team_context() {
        return ctx.parent_session_id;
    }
    std::env::var(PARENT_SESSION_ID_ENV_VAR).ok()
}

/// Check if currently running as a teammate (not leader).
///
/// TS: `isTeammate()`
pub fn is_teammate() -> bool {
    get_agent_id().is_some()
}

/// Get the teammate's assigned UI color.
///
/// TS: `getTeammateColor()`
pub fn get_teammate_color() -> Option<String> {
    if let Some(ctx) = get_teammate_context() {
        return ctx.color;
    }
    if let Some(ctx) = get_dynamic_team_context() {
        return ctx.color;
    }
    std::env::var(TEAMMATE_COLOR_ENV_VAR).ok()
}

/// Check if plan mode is required.
///
/// TS: `isPlanModeRequired()`
pub fn is_plan_mode_required() -> bool {
    if let Some(ctx) = get_teammate_context() {
        return ctx.plan_mode_required;
    }
    if let Some(ctx) = get_dynamic_team_context() {
        return ctx.plan_mode_required;
    }
    std::env::var(PLAN_MODE_REQUIRED_ENV_VAR)
        .ok()
        .is_some_and(|v| v == "1" || v == "true")
}

/// Check if this session is the team leader.
///
/// TS: `isTeamLead(teamContext?)`
pub fn is_team_lead(team_context: Option<&TeamContext>) -> bool {
    let Some(tc) = team_context else {
        return false;
    };
    tc.is_leader
}

/// Check if there are any active in-process teammates.
///
/// TS: `hasActiveInProcessTeammates(appState)`
pub fn has_active_in_process_teammates(
    tasks: &std::collections::HashMap<String, super::swarm_task::InProcessTeammateTaskState>,
) -> bool {
    tasks.values().any(|t| !t.is_idle && !t.shutdown_requested)
}

/// Check if there are any working (non-idle) in-process teammates.
///
/// TS: `hasWorkingInProcessTeammates(appState)`
pub fn has_working_in_process_teammates(
    tasks: &std::collections::HashMap<String, super::swarm_task::InProcessTeammateTaskState>,
) -> bool {
    tasks.values().any(|t| !t.is_idle)
}

/// Wait for all in-process teammates to become idle.
///
/// TS: `waitForTeammatesToBecomeIdle(setAppState, appState)`
///
/// Polls every 500ms until all teammates are idle or all tasks complete.
pub async fn wait_for_teammates_to_become_idle(
    tasks: &tokio::sync::RwLock<
        std::collections::HashMap<String, super::swarm_task::InProcessTeammateTaskState>,
    >,
) {
    loop {
        {
            let guard = tasks.read().await;
            if !has_working_in_process_teammates(&guard) {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Create a `TeammateContextData` for spawning an in-process agent.
pub fn create_teammate_context(
    agent_name: &str,
    team_name: &str,
    color: Option<String>,
    plan_mode_required: bool,
    parent_session_id: &str,
) -> TeammateContextData {
    TeammateContextData {
        agent_id: format!("{agent_name}@{team_name}"),
        agent_name: agent_name.to_string(),
        team_name: team_name.to_string(),
        color,
        plan_mode_required,
        parent_session_id: parent_session_id.to_string(),
    }
}

#[cfg(test)]
#[path = "swarm_identity.test.rs"]
mod tests;
