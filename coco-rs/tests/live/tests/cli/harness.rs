//! In-process driver for the agent loop.
//!
//! Builds a `QueryEngine` mirroring what `coco -p "<prompt>"` constructs
//! (`run_chat` in `app/cli/src/main.rs`) but without a binary process —
//! tests get direct access to the `QueryResult` and the structured
//! `CoreEvent` stream.
//!
//! Hermetic by design: each `LiveSession` owns a tempdir used as `cwd`
//! and `project_dir` so `bypassPermissions` writes can't escape.

use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_query::QueryResult;
use coco_tool_runtime::ToolRegistry;
use coco_types::CoreEvent;
use coco_types::Features;
use coco_types::PermissionMode;
use coco_types::ToolOverrides;

/// Register a curated tool subset for live agent runs.
///
/// `coco_tools::register_all_tools` ships ~42 builtins; several
/// (`AskUserQuestion`, `Grep`, `TodoWrite`, `CronList`,
/// `ListMcpResources`, …) currently emit JSON schemas with
/// `type: null` for empty argument blocks, which DeepSeek's API
/// correctly rejects with HTTP 400. To keep the live tests focused
/// on agent-loop behavior — not on validating every tool's schema —
/// we register only the file/shell tools the scenarios actually
/// need. Upstream schema cleanup is tracked separately.
fn register_suite_tools(registry: &ToolRegistry) {
    registry.register(Arc::new(coco_tools::BashTool));
    registry.register(Arc::new(coco_tools::ReadTool));
    registry.register(Arc::new(coco_tools::WriteTool));
    registry.register(Arc::new(coco_tools::EditTool));
    registry.register(Arc::new(coco_tools::GlobTool));
}
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::common::runtime::build_client;

/// Knobs the CLI suite tweaks per scenario. Defaults aim at "tiny but
/// realistic" so live tests stay fast and cheap.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Triggers compaction earlier when set small. Default 200_000
    /// (matches the production `run_chat` default).
    pub context_window: i64,
    /// Per-call output cap. Default 2_048 keeps each turn cheap.
    pub max_output_tokens: i64,
    /// Hard upper bound on agent loop turns.
    pub max_turns: i32,
    /// Per-call max_tokens; `None` lets the model decide.
    pub max_tokens: Option<i64>,
    /// Capacity of the `CoreEvent` channel. Bigger window = less
    /// back-pressure on the engine; tests assert post-hoc so 1024 is plenty.
    pub event_buffer: usize,
    /// Optional system-prompt override — `None` lets the engine use
    /// its own composed system prompt.
    pub system_prompt: Option<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            context_window: 200_000,
            max_output_tokens: 2_048,
            max_turns: 8,
            max_tokens: None,
            event_buffer: 1024,
            system_prompt: None,
        }
    }
}

impl SessionConfig {
    /// Smaller window, fewer output tokens — used by the compaction
    /// scenario to force `ContextCompacted` to fire.
    pub fn small_window(window_tokens: i64) -> Self {
        Self {
            context_window: window_tokens,
            max_output_tokens: 1_024,
            max_turns: 12,
            ..Self::default()
        }
    }
}

/// Outcome of a single in-process agent run. Exposes the same surfaces a
/// black-box CLI test would inspect: final stdout (`response_text`),
/// turn count, accumulated usage, and the structured event stream.
pub struct SessionOutcome {
    pub result: QueryResult,
    pub events: Vec<CoreEvent>,
    /// Tempdir that backed `cwd`. Owned by the outcome so files the
    /// agent wrote stay readable for follow-up assertions; cleaned up
    /// when the outcome drops. Underscore-prefixed because tests
    /// generally read the agent's reply rather than the filesystem,
    /// but the lifetime tie is load-bearing.
    pub _workdir: TempDir,
}

/// Drive `QueryEngine::run_with_events` end-to-end. Spawns a task to
/// drain events into a `Vec` so the engine never blocks on the channel.
pub async fn run_session(
    provider_name: &str,
    model_id: &str,
    session_cfg: SessionConfig,
    prompt: &str,
) -> Result<SessionOutcome> {
    // Anchor the agent's cwd at `/tmp/coco-tests-cli-<rand>` so any
    // file the agent writes is visibly under /tmp (not under the
    // project directory or macOS's opaque `/var/folders/...`).
    let workdir = crate::common::tmpdir::make("coco-tests-cli-")
        .with_context(|| "create cwd tempdir under /tmp")?;
    let workdir_path = workdir.path().to_path_buf();

    let api_client = build_client(provider_name, model_id)
        .with_context(|| format!("build api client for {provider_name}/{model_id}"))?;

    let tool_registry = ToolRegistry::new();
    register_suite_tools(&tool_registry);
    let tools = Arc::new(tool_registry);

    let cancel = CancellationToken::new();

    let config = QueryEngineConfig {
        model_id: model_id.to_string(),
        permission_mode: PermissionMode::BypassPermissions,
        bypass_permissions_available: true,
        context_window: session_cfg.context_window,
        max_output_tokens: session_cfg.max_output_tokens,
        max_turns: session_cfg.max_turns,
        max_tokens: session_cfg.max_tokens,
        system_prompt: session_cfg.system_prompt,
        is_non_interactive: true,
        project_dir: Some(workdir_path.clone()),
        cwd_override: Some(workdir_path.clone()),
        features: Arc::new(Features::with_defaults()),
        tool_overrides: Arc::new(ToolOverrides::none()),
        ..QueryEngineConfig::default()
    };

    let engine = QueryEngine::new(config, api_client, tools, cancel, /*hooks*/ None);

    let (event_tx, mut event_rx) = mpsc::channel::<CoreEvent>(session_cfg.event_buffer);
    let drainer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(evt) = event_rx.recv().await {
            events.push(evt);
        }
        events
    });

    let result = engine
        .run_with_events(prompt, event_tx)
        .await
        .with_context(|| format!("engine.run_with_events on {provider_name}/{model_id}"))?;

    // CLI sessions can fan out to many LLM HTTP calls per prompt
    // (multi-turn agent loop, tool reflection, compaction). Use the
    // cost tracker's authoritative count so the report distinguishes
    // `record_calls` (one per session) from the real `llm_calls`.
    let llm_calls = result.cost_tracker.total_api_calls.max(0) as u64;
    crate::common::usage_report::record_with_llm_calls(
        provider_name,
        model_id,
        "cli.run_session",
        &result.total_usage,
        llm_calls,
    );

    let events = drainer.await.unwrap_or_default();
    Ok(SessionOutcome {
        result,
        events,
        _workdir: workdir,
    })
}
