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

struct ForkMockModel;

#[async_trait::async_trait]
impl coco_inference::LanguageModel for ForkMockModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "mock-model"
    }

    async fn do_generate(
        &self,
        options: &coco_inference::LanguageModelCallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<coco_inference::LanguageModelGenerateResult, coco_inference::AISdkError> {
        let text = options
            .prompt
            .iter()
            .flat_map(|message| match message {
                coco_llm_types::LlmMessage::System { content, .. }
                | coco_llm_types::LlmMessage::Developer { content, .. }
                | coco_llm_types::LlmMessage::User { content, .. } => content
                    .iter()
                    .filter_map(|part| match part {
                        coco_llm_types::UserContentPart::Text(text) => Some(text.text.as_str()),
                        coco_llm_types::UserContentPart::File(_) => None,
                    })
                    .collect::<Vec<_>>(),
                coco_llm_types::LlmMessage::Assistant { content, .. } => content
                    .iter()
                    .filter_map(|part| match part {
                        coco_llm_types::AssistantContentPart::Text(text) => {
                            Some(text.text.as_str())
                        }
                        coco_llm_types::AssistantContentPart::File(_)
                        | coco_llm_types::AssistantContentPart::Reasoning(_)
                        | coco_llm_types::AssistantContentPart::ReasoningFile(_)
                        | coco_llm_types::AssistantContentPart::Custom(_)
                        | coco_llm_types::AssistantContentPart::ToolCall(_)
                        | coco_llm_types::AssistantContentPart::ToolResult(_)
                        | coco_llm_types::AssistantContentPart::Source(_)
                        | coco_llm_types::AssistantContentPart::ToolApprovalRequest(_) => None,
                    })
                    .collect::<Vec<_>>(),
                coco_llm_types::LlmMessage::Tool { .. } => Vec::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(coco_inference::LanguageModelGenerateResult {
            content: vec![coco_llm_types::AssistantContentPart::Text(
                coco_llm_types::TextPart {
                    text,
                    provider_metadata: None,
                },
            )],
            usage: coco_llm_types::Usage::new(1, 1),
            finish_reason: coco_llm_types::FinishReason::new(coco_llm_types::StopReason::EndTurn),
            warnings: Vec::new(),
            provider_metadata: None,
            request: None,
            response: None,
        })
    }

    async fn do_stream(
        &self,
        options: &coco_inference::LanguageModelCallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<coco_inference::LanguageModelStreamResult, coco_inference::AISdkError> {
        let result = self.do_generate(options, None).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

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

    let cli = Cli::try_parse_from(["coco"]).expect("parse default cli");
    let model_runtimes = Arc::new(
        coco_inference::ModelRuntimeRegistry::from_prebuilt_language_model(
            coco_types::ModelRole::Main,
            coco_inference::PrebuiltLanguageModelSlot::new(
                Arc::new(ForkMockModel),
                coco_inference::RetryConfig::default(),
            ),
        ),
    );

    SessionRuntime::build(SessionRuntimeBuildOpts {
        cli: &cli,
        runtime_config: Arc::new(runtime_config),
        cwd: home.path().to_path_buf(),
        model_id: "mock-model".into(),
        system_prompt: "test".to_string(),
        bypass_permissions_available: false,
        permission_mode: coco_types::PermissionMode::default(),
        model_runtimes: Some(model_runtimes),
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
        session_id_override: None,
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
