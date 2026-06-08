//! Skill-listing reminder gating.
//!
//! The reminder should be model-visible only when the current filtered
//! loaded tool set includes `Skill`. Otherwise it teaches the model to
//! call a tool that is not actually available on this turn.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use coco_inference::AISdkError;
use coco_inference::LanguageModel;
use coco_inference::LanguageModelCallOptions;
use coco_inference::LanguageModelGenerateResult;
use coco_inference::LanguageModelStreamResult;
use coco_llm_types::AssistantContentPart;
use coco_llm_types::FinishReason;
use coco_llm_types::LlmMessage;
use coco_llm_types::StopReason;
use coco_llm_types::TextPart;
use coco_llm_types::Usage;
use coco_system_reminder::InvokedSkillEntry;
use coco_system_reminder::ReminderSources;
use coco_system_reminder::SkillsSource;
use coco_tool_runtime::ToolRegistry;
use coco_types::AttachmentKind;
use coco_types::PermissionMode;
use coco_types::ToolFilter;
use coco_types::ToolName;
use tokio_util::sync::CancellationToken;

use crate::QueryEngine;
use crate::QueryEngineConfig;

const LISTING_MARKER: &str = "SKILL-LISTING-MARKER";

#[derive(Debug)]
struct CapturingTextModel {
    captured_prompts: Arc<Mutex<Vec<Vec<LlmMessage>>>>,
}

#[async_trait]
impl LanguageModel for CapturingTextModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "skill-listing-mock"
    }

    async fn do_generate(
        &self,
        options: &LanguageModelCallOptions,
        _abort_signal: Option<CancellationToken>,
    ) -> Result<LanguageModelGenerateResult, AISdkError> {
        self.captured_prompts
            .lock()
            .expect("captured prompts lock")
            .push(options.prompt.clone());
        Ok(LanguageModelGenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: "done".into(),
                provider_metadata: None,
            })],
            usage: Usage::new(10, 3),
            finish_reason: FinishReason::new(StopReason::EndTurn),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
    }

    async fn do_stream(
        &self,
        options: &LanguageModelCallOptions,
        abort_signal: Option<CancellationToken>,
    ) -> Result<LanguageModelStreamResult, AISdkError> {
        let result = self.do_generate(options, abort_signal).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

#[derive(Debug)]
struct SpySkillsSource {
    listing_calls: AtomicUsize,
}

impl SpySkillsSource {
    fn new() -> Self {
        Self {
            listing_calls: AtomicUsize::new(0),
        }
    }

    fn listing_calls(&self) -> usize {
        self.listing_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SkillsSource for SpySkillsSource {
    async fn listing(
        &self,
        _agent_id: Option<&str>,
        _tiers: &coco_config::SkillOverrideTiers,
    ) -> Option<String> {
        self.listing_calls.fetch_add(1, Ordering::SeqCst);
        Some(format!("- review: {LISTING_MARKER}"))
    }

    async fn invoked(&self, _agent_id: Option<&str>) -> Vec<InvokedSkillEntry> {
        Vec::new()
    }

    async fn activate_skills_for_paths(
        &self,
        _file_paths: &[std::path::PathBuf],
        _cwd: &std::path::Path,
    ) -> Vec<String> {
        Vec::new()
    }
}

fn skill_tools() -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(coco_tools::SkillTool));
    Arc::new(registry)
}

async fn run_case(config: QueryEngineConfig) -> (Vec<Vec<LlmMessage>>, usize, bool) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let model = Arc::new(CapturingTextModel {
        captured_prompts: captured.clone(),
    });
    let source = Arc::new(SpySkillsSource::new());
    let client = crate::test_support::model_runtime_registry(model);
    let engine = QueryEngine::new(
        config,
        client,
        skill_tools(),
        CancellationToken::new(),
        None,
    )
    .with_reminder_sources(ReminderSources {
        skills: Some(source.clone()),
        ..Default::default()
    });

    let result = engine.run("hello").await.expect("engine run");
    let has_skill_listing = result.final_messages.iter().any(|message| {
        matches!(
            message.as_ref(),
            coco_messages::Message::Attachment(att) if att.kind == AttachmentKind::SkillListing
        )
    });
    let prompts = captured.lock().expect("captured prompts lock").clone();
    (prompts, source.listing_calls(), has_skill_listing)
}

fn prompt_text(prompt: &[LlmMessage]) -> String {
    prompt
        .iter()
        .map(extract_all_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_all_text(msg: &LlmMessage) -> String {
    use coco_llm_types::AssistantContentPart;
    use coco_llm_types::ToolContentPart;
    use coco_llm_types::ToolResultContent;
    use coco_llm_types::UserContentPart;

    let mut out = String::new();
    let mut push = |s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    match msg {
        LlmMessage::User { content, .. }
        | LlmMessage::System { content, .. }
        | LlmMessage::Developer { content, .. } => {
            for part in content {
                if let UserContentPart::Text(t) = part {
                    push(&t.text);
                }
            }
        }
        LlmMessage::Assistant { content, .. } => {
            for part in content {
                if let AssistantContentPart::Text(t) = part {
                    push(&t.text);
                }
            }
        }
        LlmMessage::Tool { content, .. } => {
            for part in content {
                if let ToolContentPart::ToolResult(result) = part {
                    match &result.output {
                        ToolResultContent::Text { value, .. } => push(value),
                        ToolResultContent::Content { value, .. } => {
                            for part in value {
                                if let coco_llm_types::ToolResultContentPart::Text {
                                    text, ..
                                } = part
                                {
                                    push(text);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

#[tokio::test]
async fn skill_listing_default_mode_with_skill_tool_injects() {
    let config = QueryEngineConfig {
        model_id: "skill-listing-mock".into(),
        permission_mode: PermissionMode::Default,
        max_turns: Some(1),
        ..Default::default()
    };

    let (prompts, listing_calls, has_skill_listing) = run_case(config).await;

    assert_eq!(listing_calls, 1);
    assert!(has_skill_listing);
    assert!(
        prompt_text(&prompts[0]).contains(LISTING_MARKER),
        "skill listing should reach the model when Skill is loaded"
    );
}

#[tokio::test]
async fn skill_listing_plan_mode_keeps_skill_tool_and_injects() {
    // Mirror TS: plan mode no longer strips the Skill tool from the
    // schema (layer-3 removal), so Skill stays in the loaded set and the
    // skill-listing reminder is injected exactly as in Default mode.
    // Skill execution is gated at call time by the permission layer, not
    // by hiding the tool — so teaching the model about skills is correct.
    let config = QueryEngineConfig {
        model_id: "skill-listing-mock".into(),
        permission_mode: PermissionMode::Plan,
        max_turns: Some(1),
        ..Default::default()
    };

    let (prompts, listing_calls, has_skill_listing) = run_case(config).await;

    assert_eq!(listing_calls, 1);
    assert!(has_skill_listing);
    assert!(
        prompt_text(&prompts[0]).contains(LISTING_MARKER),
        "skill listing should reach the model in plan mode (Skill is no longer stripped)"
    );
}

#[tokio::test]
async fn skill_listing_tool_filter_excluding_skill_suppresses() {
    let config = QueryEngineConfig {
        model_id: "skill-listing-mock".into(),
        permission_mode: PermissionMode::Default,
        tool_filter: ToolFilter::new(Vec::new(), vec![ToolName::Skill.as_str().into()]),
        max_turns: Some(1),
        ..Default::default()
    };

    let (prompts, listing_calls, has_skill_listing) = run_case(config).await;

    assert_eq!(listing_calls, 0);
    assert!(!has_skill_listing);
    assert!(
        !prompt_text(&prompts[0]).contains(LISTING_MARKER),
        "skill listing should not reach the model when Skill is filtered out"
    );
}
