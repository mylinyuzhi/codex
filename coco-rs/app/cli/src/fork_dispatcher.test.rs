use super::*;

use clap::Parser;
use coco_config::CatalogPaths;
use coco_config::EnvSnapshot;
use coco_config::RuntimeOverrides;
use coco_config::Settings;
use coco_config::SettingsWithSource;
use coco_types::ForkLabel;
use tempfile::TempDir;

use crate::Cli;
use crate::session_runtime::SessionRuntimeBuildOpts;

async fn build_runtime(home: &TempDir) -> Arc<SessionRuntime> {
    let settings = SettingsWithSource {
        merged: Settings {
            model: Some("anthropic/claude-opus-4-7".into()),
            ..Default::default()
        },
        per_source: std::collections::HashMap::new(),
        source_paths: std::collections::HashMap::new(),
    };
    let runtime_config = coco_config::build_runtime_config_with(
        settings,
        EnvSnapshot::default(),
        RuntimeOverrides::default(),
        CatalogPaths::empty_in(home.path()),
    )
    .expect("runtime config");
    let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
    let model: Arc<dyn coco_inference::LanguageModel> = Arc::new(crate::headless::MockModel::new());
    let client = Arc::new(coco_inference::ApiClient::with_default_fingerprint(
        model, retry,
    ));
    let cli = Cli::try_parse_from(["coco"]).expect("parse default cli");

    SessionRuntime::build(SessionRuntimeBuildOpts {
        cli: &cli,
        runtime_config: Arc::new(runtime_config),
        cwd: home.path().to_path_buf(),
        model_id: "mock-model".into(),
        system_prompt: "test".to_string(),
        bypass_permissions_available: false,
        permission_mode: coco_types::PermissionMode::default(),
        client,
        fallback_clients: Vec::new(),
        recovery_policy: None,
        tools: Arc::new(coco_tool_runtime::ToolRegistry::new()),
        session_manager: Arc::new(coco_session::SessionManager::new(
            home.path().join("sessions"),
        )),
        fast_model_spec: None,
        permission_bridge: None,
        command_registry: Arc::new(tokio::sync::RwLock::new(Arc::new(
            coco_commands::CommandRegistry::new(),
        ))),
        skill_manager: Arc::new(coco_skills::SkillManager::new()),
        agent_search_paths: coco_subagent::definition_store::AgentSearchPaths::empty(),
        builtin_agent_catalog: coco_subagent::BuiltinAgentCatalog::interactive(),
    })
    .await
    .expect("build SessionRuntime")
}

#[tokio::test]
async fn dispatch_with_parent_history_uses_no_event_message_path() {
    let home = TempDir::new().expect("home tempdir");
    let runtime = build_runtime(&home).await;
    let dispatcher = SessionRuntimeForkDispatcher::new(runtime);
    let cache = CacheSafeParams {
        rendered_system_prompt: "test".into(),
        model_id: "mock-model".into(),
        provider: "mock".into(),
        prompt_cache: None,
        fork_context_messages: vec![Arc::new(coco_messages::create_user_message("parent turn"))],
    };
    let options = ForkedAgentOptions::for_label(ForkLabel::PromptSuggestion);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dispatcher.dispatch(&cache, &options, "fork turn", None),
    )
    .await
    .expect("fork dispatch must complete without a drained event receiver")
    .expect("fork dispatch should succeed");

    assert_eq!(result.messages.len(), 1);
    let text = coco_messages::wrapping::extract_text_from_message(&result.messages[0]);
    assert!(text.contains("parent turn"));
    assert!(text.contains("fork turn"));
}
