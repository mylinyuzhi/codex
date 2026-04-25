//! Backend traits and detection for teammate execution.
//!
//! TS: utils/swarm/backends/types.ts, registry.ts, detection.ts
//!
//! Three execution modes:
//! - **tmux**: Terminal panes visible to leader, supports hide/show
//! - **iTerm2**: Native split panes with automatic layout
//! - **in-process**: Same process, isolated via `tokio::task_local!`

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::swarm::BackendType;
use super::swarm_constants::AgentColorName;
use super::swarm_mailbox;

// ── Pane Types ──

/// Pane identifier (opaque string).
pub type PaneId = String;

/// Result from creating a teammate pane.
///
/// TS: `CreatePaneResult`
#[derive(Debug, Clone)]
pub struct CreatePaneResult {
    pub pane_id: PaneId,
    pub is_first_teammate: bool,
}

/// Result from backend detection.
///
/// TS: `BackendDetectionResult`
#[derive(Debug, Clone)]
pub struct BackendDetectionResult {
    pub backend_type: BackendType,
    /// Whether we're natively inside this backend (vs. launching external).
    pub is_native: bool,
    /// iTerm2-specific: needs Python API setup.
    pub needs_it2_setup: bool,
}

// ── PaneBackend Trait ──

/// Interface for pane management backends (tmux, iTerm2).
///
/// TS: `PaneBackend` in utils/swarm/backends/types.ts
#[async_trait]
pub trait PaneBackend: Send + Sync {
    fn backend_type(&self) -> BackendType;
    fn display_name(&self) -> &str;
    fn supports_hide_show(&self) -> bool;

    async fn is_available(&self) -> bool;
    async fn is_running_inside(&self) -> bool;

    async fn create_teammate_pane(
        &self,
        name: &str,
        color: AgentColorName,
    ) -> anyhow::Result<CreatePaneResult>;

    async fn send_command_to_pane(&self, pane_id: &PaneId, command: &str) -> anyhow::Result<()>;

    async fn set_pane_border_color(
        &self,
        pane_id: &PaneId,
        color: AgentColorName,
    ) -> anyhow::Result<()>;

    async fn set_pane_title(
        &self,
        pane_id: &PaneId,
        name: &str,
        color: AgentColorName,
    ) -> anyhow::Result<()>;

    /// Enable pane border status display (shows pane titles).
    ///
    /// TS: `enablePaneBorderStatus(windowTarget?, useExternalSession?)`
    async fn enable_pane_border_status(&self, window_target: Option<&str>) -> anyhow::Result<()>;

    async fn rebalance_panes(&self, window_target: &str, has_leader: bool) -> anyhow::Result<()>;

    async fn kill_pane(&self, pane_id: &PaneId) -> anyhow::Result<bool>;

    async fn hide_pane(&self, pane_id: &PaneId) -> anyhow::Result<bool>;

    async fn show_pane(
        &self,
        pane_id: &PaneId,
        target_window_or_pane: &str,
    ) -> anyhow::Result<bool>;
}

// ── TeammateExecutor Trait ──

/// Configuration for spawning a teammate.
///
/// TS: `TeammateSpawnConfig` in utils/swarm/backends/types.ts
#[derive(Debug, Clone)]
pub struct TeammateSpawnConfig {
    pub name: String,
    pub team_name: String,
    pub color: Option<AgentColorName>,
    pub plan_mode_required: bool,
    pub prompt: String,
    pub cwd: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub system_prompt_mode: SystemPromptMode,
    pub worktree_path: Option<String>,
    pub parent_session_id: String,
    pub permissions: Vec<String>,
    pub allow_permission_prompts: bool,
}

/// System prompt assembly mode.
///
/// TS: `systemPromptMode?: 'default' | 'replace' | 'append'`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemPromptMode {
    /// System prompt + teammate addendum.
    #[default]
    Default,
    /// Use only the provided system prompt.
    Replace,
    /// System prompt + addendum + provided prompt.
    Append,
}

/// Result from spawning a teammate.
///
/// TS: `TeammateSpawnResult`
#[derive(Debug)]
pub struct TeammateSpawnResult {
    pub success: bool,
    pub agent_id: String,
    pub error: Option<String>,
    /// Task ID (in-process only).
    pub task_id: Option<String>,
    /// Pane ID (pane-based only).
    pub pane_id: Option<PaneId>,
}

/// Common interface for teammate execution.
///
/// TS: `TeammateExecutor` in utils/swarm/backends/types.ts
#[async_trait]
pub trait TeammateExecutor: Send + Sync {
    fn backend_type(&self) -> BackendType;

    async fn is_available(&self) -> bool;

    async fn spawn(&self, config: TeammateSpawnConfig) -> TeammateSpawnResult;

    async fn send_message(
        &self,
        agent_id: &str,
        message: swarm_mailbox::TeammateMessage,
    ) -> anyhow::Result<()>;

    async fn terminate(&self, agent_id: &str, reason: Option<&str>) -> anyhow::Result<bool>;

    async fn kill(&self, agent_id: &str) -> anyhow::Result<bool>;

    async fn is_active(&self, agent_id: &str) -> bool;
}

// ── InProcessBackend ──

/// In-process teammate executor — wraps `InProcessAgentRunner`.
///
/// TS: `InProcessBackend` in utils/swarm/backends/InProcessBackend.ts
pub struct InProcessBackend {
    runner: Arc<super::swarm_runner::InProcessAgentRunner>,
}

impl InProcessBackend {
    pub fn new(runner: Arc<super::swarm_runner::InProcessAgentRunner>) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl TeammateExecutor for InProcessBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::InProcess
    }

    async fn is_available(&self) -> bool {
        true // Always available
    }

    async fn spawn(&self, config: TeammateSpawnConfig) -> TeammateSpawnResult {
        use coco_types::AgentIsolation;

        let spawn_config = super::swarm_runner::SpawnConfig {
            name: config.name.clone(),
            team_name: config.team_name.clone(),
            prompt: config.prompt,
            color: config.color.map(|c| c.as_str().to_string()),
            plan_mode_required: config.plan_mode_required,
            model: config.model,
            working_dir: Some(config.cwd),
            system_prompt: config.system_prompt,
            allowed_tools: config.permissions,
            allow_permission_prompts: config.allow_permission_prompts,
            effort: None,
            use_exact_tools: false,
            isolation: AgentIsolation::None,
            memory_scope: None,
            mcp_servers: Vec::new(),
            disallowed_tools: Vec::new(),
            max_turns: None,
        };

        let result = self.runner.register_agent(spawn_config).await;
        let task_id = if result.success {
            Some(format!("task-{}", result.agent_id))
        } else {
            None
        };

        TeammateSpawnResult {
            success: result.success,
            agent_id: result.agent_id,
            error: result.error,
            task_id,
            pane_id: None,
        }
    }

    async fn send_message(
        &self,
        agent_id: &str,
        message: swarm_mailbox::TeammateMessage,
    ) -> anyhow::Result<()> {
        // Extract agent name from "name@team" format
        let agent_name = agent_id.split('@').next().unwrap_or(agent_id);
        let team_name = agent_id.split('@').nth(1).unwrap_or("default");
        swarm_mailbox::write_to_mailbox(agent_name, message, team_name)
    }

    async fn terminate(&self, agent_id: &str, reason: Option<&str>) -> anyhow::Result<bool> {
        let agent_name = agent_id.split('@').next().unwrap_or(agent_id);
        let team_name = agent_id.split('@').nth(1).unwrap_or("default");
        let from = super::swarm_constants::TEAM_LEAD_NAME;
        swarm_mailbox::send_shutdown_request(agent_name, team_name, from, reason)?;
        Ok(true)
    }

    async fn kill(&self, agent_id: &str) -> anyhow::Result<bool> {
        Ok(self.runner.cancel_agent(agent_id).await)
    }

    async fn is_active(&self, agent_id: &str) -> bool {
        self.runner
            .get_context(agent_id)
            .await
            .is_some_and(|ctx| !ctx.is_cancelled())
    }
}

// ── Backend Registry ──

/// Backend registry — detects, caches, and provides backends.
///
/// TS: `registry.ts` — detectAndGetBackend, getTeammateExecutor, etc.
pub struct BackendRegistry {
    detection_result: RwLock<Option<BackendDetectionResult>>,
    in_process_backend: RwLock<Option<Arc<dyn TeammateExecutor>>>,
    pane_backend: RwLock<Option<Arc<dyn PaneBackend>>>,
    in_process_fallback_active: RwLock<bool>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self {
            detection_result: RwLock::new(None),
            in_process_backend: RwLock::new(None),
            pane_backend: RwLock::new(None),
            in_process_fallback_active: RwLock::new(false),
        }
    }

    /// Detect the best available pane backend.
    ///
    /// TS: `detectAndGetBackend()`
    ///
    /// Priority:
    /// 1. Inside tmux → use tmux
    /// 2. iTerm2 + it2 CLI → use iTerm2
    /// 3. tmux available → use tmux (external session)
    /// 4. Otherwise → in-process fallback
    pub async fn detect_backend(&self) -> BackendDetectionResult {
        {
            let cached = self.detection_result.read().await;
            if let Some(result) = cached.as_ref() {
                return result.clone();
            }
        }

        let result = detect_backend_impl().await;
        *self.detection_result.write().await = Some(result.clone());
        result
    }

    /// Register a pane backend.
    pub async fn register_pane_backend(&self, backend: Arc<dyn PaneBackend>) {
        *self.pane_backend.write().await = Some(backend);
    }

    /// Register an in-process backend.
    pub async fn register_in_process_backend(&self, backend: Arc<dyn TeammateExecutor>) {
        *self.in_process_backend.write().await = Some(backend);
    }

    /// Mark in-process fallback as active (no pane backend available).
    pub async fn mark_in_process_fallback(&self) {
        *self.in_process_fallback_active.write().await = true;
    }

    /// Check if in-process mode is active.
    ///
    /// TS: `isInProcessEnabled()`
    pub async fn is_in_process_enabled(&self) -> bool {
        *self.in_process_fallback_active.read().await
    }

    /// Get the cached pane backend.
    pub async fn get_pane_backend(&self) -> Option<Arc<dyn PaneBackend>> {
        self.pane_backend.read().await.clone()
    }

    /// Get the in-process backend.
    pub async fn get_in_process_backend(&self) -> Option<Arc<dyn TeammateExecutor>> {
        self.in_process_backend.read().await.clone()
    }

    /// Get the appropriate teammate executor.
    ///
    /// TS: `getTeammateExecutor(preferInProcess?)`
    pub async fn get_teammate_executor(
        &self,
        prefer_in_process: bool,
    ) -> Option<Arc<dyn TeammateExecutor>> {
        if prefer_in_process || *self.in_process_fallback_active.read().await {
            return self.in_process_backend.read().await.clone();
        }
        // Pane backends are not TeammateExecutor directly;
        // they need PaneBackendExecutor wrapping.
        // For now, fall back to in-process.
        self.in_process_backend.read().await.clone()
    }

    /// Get the resolved teammate mode string.
    ///
    /// TS: `getResolvedTeammateMode()`
    pub async fn get_resolved_teammate_mode(&self) -> &'static str {
        if *self.in_process_fallback_active.read().await {
            "in-process"
        } else {
            "tmux"
        }
    }

    /// Reset all caches (for testing).
    ///
    /// TS: `resetBackendDetection()`
    pub async fn reset(&self) {
        *self.detection_result.write().await = None;
        *self.in_process_backend.write().await = None;
        *self.pane_backend.write().await = None;
        *self.in_process_fallback_active.write().await = false;
    }
}

impl Default for BackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Detection Logic ──

/// Check if running inside tmux.
///
/// TS: `isInsideTmuxSync()` — checks TMUX env var captured at load.
pub fn is_inside_tmux() -> bool {
    std::env::var("TMUX").ok().is_some_and(|v| !v.is_empty())
}

/// Get the leader's tmux pane ID.
///
/// TS: `getLeaderPaneId()` — returns TMUX_PANE env var.
pub fn get_leader_pane_id() -> Option<String> {
    std::env::var("TMUX_PANE").ok()
}

/// Check if tmux is available on the system.
///
/// TS: `isTmuxAvailable()`
pub async fn is_tmux_available() -> bool {
    tokio::process::Command::new("tmux")
        .arg("-V")
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// Check if running inside iTerm2.
///
/// TS: `isInITerm2()`
pub fn is_in_iterm2() -> bool {
    std::env::var("TERM_PROGRAM")
        .ok()
        .is_some_and(|v| v == "iTerm.app")
        || std::env::var("ITERM_SESSION_ID").ok().is_some()
}

/// Check if the it2 CLI is available.
///
/// TS: `isIt2CliAvailable()`
pub async fn is_it2_cli_available() -> bool {
    tokio::process::Command::new("it2")
        .arg("session")
        .arg("list")
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// Run backend detection.
async fn detect_backend_impl() -> BackendDetectionResult {
    // Priority 1: Inside tmux
    if is_inside_tmux() {
        return BackendDetectionResult {
            backend_type: BackendType::Tmux,
            is_native: true,
            needs_it2_setup: false,
        };
    }

    // Priority 2: iTerm2 with it2 CLI
    if is_in_iterm2() && is_it2_cli_available().await {
        return BackendDetectionResult {
            backend_type: BackendType::Iterm2,
            is_native: true,
            needs_it2_setup: false,
        };
    }

    // Priority 3: iTerm2 without it2, but tmux available
    if is_in_iterm2() && is_tmux_available().await {
        return BackendDetectionResult {
            backend_type: BackendType::Tmux,
            is_native: false,
            needs_it2_setup: true,
        };
    }

    // Priority 4: tmux available (external session)
    if is_tmux_available().await {
        return BackendDetectionResult {
            backend_type: BackendType::Tmux,
            is_native: false,
            needs_it2_setup: false,
        };
    }

    // Fallback: in-process
    BackendDetectionResult {
        backend_type: BackendType::InProcess,
        is_native: true,
        needs_it2_setup: false,
    }
}

#[cfg(test)]
#[path = "swarm_backend.test.rs"]
mod tests;
