//! Tests for [`session_bootstrap`].
//!
//! `install_session_late_binds` is the parity-enforcement helper —
//! these tests pin its contract: every late-bind slot is populated
//! after a successful call, and `mcp_handle` is honored as `Some` /
//! ignored as `None`.

use std::sync::Arc;

use coco_config::CatalogPaths;
use coco_config::EnvSnapshot;
use coco_config::RuntimeOverrides;
use coco_config::Settings;
use coco_config::SettingsWithSource;
use tempfile::TempDir;

use crate::Cli;
use crate::session_bootstrap::install_session_late_binds;
use crate::session_runtime::SessionRuntime;
use crate::session_runtime::SessionRuntimeBuildOpts;

/// Build a fresh `SessionRuntime` against a tempdir-backed runtime
/// config so the test runs hermetically (no `~/.coco` reads/writes).
async fn build_runtime(home: &TempDir) -> Arc<SessionRuntime> {
    use clap::Parser;

    let settings = SettingsWithSource {
        merged: Settings::default(),
        per_source: std::collections::HashMap::new(),
    };
    let runtime_config = coco_config::build_runtime_config_with(
        settings,
        EnvSnapshot::default(),
        RuntimeOverrides::default(),
        CatalogPaths::empty_in(home.path()),
    )
    .expect("runtime config");

    // Resources via `headless::create_api_client` — falls back to
    // mock when no provider key is configured.
    let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
    let (client, _provider, model_id) = crate::headless::create_api_client(&runtime_config, retry);

    let registry = coco_tool_runtime::ToolRegistry::new();
    let tools = Arc::new(registry);

    let cwd = home.path().to_path_buf();
    let cli = Cli::try_parse_from(["coco"]).expect("parse default cli");
    let session_manager = Arc::new(coco_session::SessionManager::new(
        home.path().join("sessions"),
    ));

    let command_registry = Arc::new(coco_commands::CommandRegistry::new());

    SessionRuntime::build(SessionRuntimeBuildOpts {
        cli: &cli,
        runtime_config: Arc::new(runtime_config),
        cwd,
        model_id,
        system_prompt: "test".to_string(),
        bypass_permissions_available: false,
        permission_mode: coco_types::PermissionMode::default(),
        client,
        fallback_clients: Vec::new(),
        recovery_policy: None,
        tools,
        session_manager,
        fast_model_spec: None,
        permission_bridge: None,
        command_registry,
    })
    .await
    .expect("build SessionRuntime")
}

#[tokio::test]
async fn install_session_late_binds_populates_every_slot_without_mcp() {
    let home = TempDir::new().expect("home tempdir");
    let runtime = build_runtime(&home).await;
    let cwd = home.path().to_path_buf();

    install_session_late_binds(runtime.clone(), &cwd, None)
        .await
        .expect("install_session_late_binds");

    assert!(
        runtime.current_task_runtime().await.is_some(),
        "task_runtime slot must be populated"
    );
    assert!(
        runtime.current_agent_transcript_store().await.is_some(),
        "agent_transcript_store slot must be populated"
    );
    assert!(
        runtime.current_fork_dispatcher().await.is_some(),
        "fork_dispatcher slot must be populated"
    );
    assert!(
        runtime.current_mcp_handle().await.is_none(),
        "mcp_handle slot must stay None when caller passes None"
    );
}

#[tokio::test]
async fn install_session_late_binds_attaches_mcp_when_some() {
    let home = TempDir::new().expect("home tempdir");
    let runtime = build_runtime(&home).await;
    let cwd = home.path().to_path_buf();

    let mcp_handle: coco_tool_runtime::McpHandleRef = Arc::new(coco_tool_runtime::NoOpMcpHandle);

    install_session_late_binds(runtime.clone(), &cwd, Some(mcp_handle))
        .await
        .expect("install_session_late_binds");

    assert!(
        runtime.current_mcp_handle().await.is_some(),
        "mcp_handle slot must be Some when caller passes Some"
    );
}
