//! Reusable mock model harness for e2e testing.
//!
//! Provides `MockModelBuilder` to define a sequence of LLM responses
//! (text + tool calls) without writing boilerplate LanguageModelV4 impls.
//!
//! Usage:
//! ```ignore
//! let model = MockModelBuilder::new()
//!     .on_call(0, |_| MockResponse::tool_call("Read", json!({"file_path": "/tmp/x"})))
//!     .on_call(1, |_| MockResponse::text("Done!"))
//!     .build();
//! let result = run_with_mock(model, "read the file", tools).await;
//! assert_eq!(result.turns, 2);
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_query::QueryResult;
use coco_tool::ToolPermissionBridge;
use coco_tool::ToolPermissionBridgeRef;
use coco_tool::ToolPermissionDecision;
use coco_tool::ToolPermissionRequest;
use coco_tool::ToolPermissionResolution;
use coco_tool::ToolRegistry;
use coco_tools::BashTool;
use coco_tools::EditTool;
use coco_tools::EnterPlanModeTool;
use coco_tools::ExitPlanModeTool;
use coco_tools::GlobTool;
use coco_tools::GrepTool;
use coco_tools::ReadTool;
use coco_tools::WriteTool;
use coco_types::PermissionMode;
use coco_types::ToolAppState;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

// ─── MockResponse: declarative response builder ───

/// A declarative mock response.
pub enum MockResponse {
    /// Return text only (conversation ends).
    Text(String),
    /// Return a single tool call.
    ToolCall {
        tool_name: String,
        input: serde_json::Value,
    },
    /// Return text + tool calls.
    TextAndToolCalls {
        text: String,
        tool_calls: Vec<(String, serde_json::Value)>,
    },
    /// Return multiple tool calls (parallel execution).
    MultiToolCall(Vec<(String, serde_json::Value)>),
}

impl MockResponse {
    pub fn text(s: &str) -> Self {
        Self::Text(s.to_string())
    }

    pub fn tool_call(name: &str, input: serde_json::Value) -> Self {
        Self::ToolCall {
            tool_name: name.to_string(),
            input,
        }
    }

    pub fn multi_tool(calls: Vec<(&str, serde_json::Value)>) -> Self {
        Self::MultiToolCall(calls.into_iter().map(|(n, i)| (n.to_string(), i)).collect())
    }

    fn into_generate_result(self, call_idx: i32) -> LanguageModelV4GenerateResult {
        let (content, finish) = match self {
            Self::Text(text) => (
                vec![AssistantContentPart::Text(TextPart {
                    text,
                    provider_metadata: None,
                })],
                UnifiedFinishReason::Stop,
            ),
            Self::ToolCall { tool_name, input } => (
                vec![AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id: format!("call_{call_idx}"),
                    tool_name,
                    input,
                    provider_executed: None,
                    provider_metadata: None,
                })],
                UnifiedFinishReason::ToolCalls,
            ),
            Self::TextAndToolCalls { text, tool_calls } => {
                let mut parts = vec![AssistantContentPart::Text(TextPart {
                    text,
                    provider_metadata: None,
                })];
                for (i, (name, input)) in tool_calls.into_iter().enumerate() {
                    parts.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: format!("call_{call_idx}_{i}"),
                        tool_name: name,
                        input,
                        provider_executed: None,
                        provider_metadata: None,
                    }));
                }
                (parts, UnifiedFinishReason::ToolCalls)
            }
            Self::MultiToolCall(calls) => {
                let parts: Vec<_> = calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, input))| {
                        AssistantContentPart::ToolCall(ToolCallPart {
                            tool_call_id: format!("call_{call_idx}_{i}"),
                            tool_name: name,
                            input,
                            provider_executed: None,
                            provider_metadata: None,
                        })
                    })
                    .collect();
                (parts, UnifiedFinishReason::ToolCalls)
            }
        };

        LanguageModelV4GenerateResult {
            content,
            usage: Usage::new(50, 20),
            finish_reason: FinishReason::new(finish),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        }
    }
}

// ─── ScriptedMock: plays a sequence of responses ───

type ResponseFn = Box<dyn Fn(&LanguageModelV4CallOptions) -> MockResponse + Send + Sync>;

/// A mock model that plays a predefined script of responses.
pub struct ScriptedMock {
    call_count: AtomicI32,
    responses: Vec<ResponseFn>,
    #[allow(dead_code)]
    fallback: MockResponse,
}

impl ScriptedMock {
    fn get_response(&self, options: &LanguageModelV4CallOptions) -> MockResponse {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst) as usize;
        if idx < self.responses.len() {
            (self.responses[idx])(options)
        } else {
            MockResponse::text("(mock: no more scripted responses)")
        }
    }
}

#[async_trait::async_trait]
impl LanguageModelV4 for ScriptedMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "scripted-mock"
    }
    async fn do_generate(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let idx = self.call_count.load(Ordering::SeqCst);
        let response = self.get_response(&options);
        Ok(response.into_generate_result(idx))
    }
    async fn do_stream(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        let result = self.do_generate(options).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

// ─── MockModelBuilder ───

/// Builder for creating scripted mock models.
pub struct MockModelBuilder {
    responses: Vec<ResponseFn>,
}

impl Default for MockModelBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MockModelBuilder {
    pub fn new() -> Self {
        Self {
            responses: Vec::new(),
        }
    }

    /// Add a response for call N (0-indexed).
    pub fn on_call<F>(mut self, _idx: usize, f: F) -> Self
    where
        F: Fn(&LanguageModelV4CallOptions) -> MockResponse + Send + Sync + 'static,
    {
        self.responses.push(Box::new(f));
        self
    }

    /// Add a simple text response.
    pub fn then_text(self, text: &str) -> Self {
        let text = text.to_string();
        self.on_call(0, move |_| MockResponse::Text(text.clone()))
    }

    /// Add a tool call response.
    pub fn then_tool_call(self, name: &str, input: serde_json::Value) -> Self {
        let name = name.to_string();
        self.on_call(0, move |_| MockResponse::ToolCall {
            tool_name: name.clone(),
            input: input.clone(),
        })
    }

    pub fn build(self) -> Arc<ScriptedMock> {
        Arc::new(ScriptedMock {
            call_count: AtomicI32::new(0),
            responses: self.responses,
            fallback: MockResponse::text("(no more responses)"),
        })
    }
}

// ─── AllowAllPermissionBridge ───

/// Permission bridge that auto-approves every request. Used in
/// integration tests to let the mock model drive tools that return
/// `PermissionDecision::Ask` (e.g. `ExitPlanMode`) without an
/// interactive user. The real `NoOpPermissionBridge` rejects by
/// default, which is wrong for tests whose purpose is to exercise
/// the tool itself.
pub struct AllowAllPermissionBridge;

#[async_trait::async_trait]
impl ToolPermissionBridge for AllowAllPermissionBridge {
    async fn request_permission(
        &self,
        _request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String> {
        Ok(ToolPermissionResolution {
            decision: ToolPermissionDecision::Approved,
            feedback: None,
        })
    }
}

pub fn allow_all_bridge() -> ToolPermissionBridgeRef {
    Arc::new(AllowAllPermissionBridge)
}

// ─── Convenience runners ───

/// Register all 6 core tools.
pub fn core_tools() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    Arc::new(registry)
}

/// Core tools + EnterPlanMode + ExitPlanMode for plan-mode integration
/// tests. Built on top of [`core_tools`] so Read/Write/Grep/etc. are
/// available inside plan mode (model "explores the codebase").
pub fn tools_with_plan_mode() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(EnterPlanModeTool));
    registry.register(Arc::new(ExitPlanModeTool));
    Arc::new(registry)
}

/// Configuration knobs for [`run_plan_mode_turn`]. Wires the shared
/// `ToolAppState` + `config_home` (needed for plan-file I/O) into the
/// engine, and lets the caller drive multi-turn scenarios by threading
/// `final_messages` from the previous turn back in.
pub struct PlanModeTurnParams {
    pub session_id: String,
    pub config_home: std::path::PathBuf,
    pub app_state: Arc<RwLock<ToolAppState>>,
    pub tools: Arc<ToolRegistry>,
    /// Messages from prior turns (plus the new user prompt). When empty
    /// the helper creates a fresh user message from `prompt_if_empty`.
    pub messages: Vec<coco_types::Message>,
    /// Fallback prompt when `messages` is empty (first turn case).
    pub prompt_if_empty: String,
    /// Raise this for scenarios that need more than the default 10
    /// tool-iteration budget (e.g. tool_rounds_do_not_advance_cadence).
    pub max_turns: i32,
    /// Engine starting mode. Plan-mode tests start here as
    /// [`PermissionMode::Plan`] directly: the engine's plan-mode
    /// reminder snapshots `config.permission_mode` at construction
    /// time, so a model-driven `EnterPlanMode` mid-run wouldn't flip
    /// the reminder on. Matches how real sessions enter plan mode
    /// (Shift+Tab toggle BEFORE `engine.run`).
    pub permission_mode: PermissionMode,
}

impl PlanModeTurnParams {
    /// Convenience: turn 1 from scratch in Plan mode.
    pub fn plan_turn(
        session_id: impl Into<String>,
        config_home: std::path::PathBuf,
        app_state: Arc<RwLock<ToolAppState>>,
        tools: Arc<ToolRegistry>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            config_home,
            app_state,
            tools,
            messages: Vec::new(),
            prompt_if_empty: prompt.into(),
            max_turns: 20,
            permission_mode: PermissionMode::Plan,
        }
    }

    /// Override the starting mode (used for Reentry tests where the
    /// first run exits back to Default and the second run re-enters
    /// Plan).
    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Feed the prior turn's `final_messages` + a new user message into
    /// the next run.
    pub fn next_turn(mut self, prev_messages: Vec<coco_types::Message>, prompt: &str) -> Self {
        self.messages = prev_messages;
        self.messages
            .push(coco_messages::create_user_message(prompt));
        self
    }
}

/// Drive one engine run scripted for plan-mode integration tests.
///
/// Wires `app_state` (cross-turn plan cadence + exit flags) and
/// `config_home` (plan-file path resolution), registers the passed
/// tool set, and starts in the caller-specified permission mode.
pub async fn run_plan_mode_turn(
    model: Arc<dyn LanguageModelV4>,
    params: PlanModeTurnParams,
) -> QueryResult {
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let cancel = CancellationToken::new();
    let config = QueryEngineConfig {
        model_name: "scripted-mock".into(),
        permission_mode: params.permission_mode,
        max_turns: params.max_turns,
        session_id: params.session_id,
        ..Default::default()
    };
    let engine = QueryEngine::new(config, client, params.tools, cancel, None)
        .with_app_state(params.app_state)
        .with_config_home(params.config_home)
        // Auto-approve any `Ask` decision (ExitPlanMode, etc.) — tests
        // script the model flow, not user interaction.
        .with_permission_bridge(allow_all_bridge());

    if params.messages.is_empty() {
        engine
            .run(&params.prompt_if_empty)
            .await
            .expect("mock engine run failed")
    } else {
        // `run_with_messages` requires at least one `user` message at
        // the tail; callers that want a fresh turn should use
        // `next_turn()` which pushes one before handing off.
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        engine
            .run_with_messages(params.messages, tx)
            .await
            .expect("mock engine run_with_messages failed")
    }
}

/// Run the query engine with a mock model and default config.
pub async fn run_with_mock(
    model: Arc<dyn LanguageModelV4>,
    prompt: &str,
    tools: Arc<ToolRegistry>,
) -> QueryResult {
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let cancel = CancellationToken::new();
    let config = QueryEngineConfig {
        model_name: "scripted-mock".into(),
        permission_mode: PermissionMode::BypassPermissions,
        max_turns: 10,
        ..Default::default()
    };
    let engine = QueryEngine::new(config, client, tools, cancel, None);
    match engine.run(prompt).await {
        Ok(result) => result,
        Err(err) => panic!("mock engine should not fail: {err}"),
    }
}

// ─── Tests using the harness ───

#[tokio::test]
async fn test_harness_text_only() {
    let model = MockModelBuilder::new()
        .then_text("Hello from harness!")
        .build();

    let result = run_with_mock(model, "hi", core_tools()).await;
    assert_eq!(result.response_text, "Hello from harness!");
    assert_eq!(result.turns, 1);
}

#[tokio::test]
async fn test_harness_tool_call_then_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.txt");
    std::fs::write(&file, "harness test content").unwrap();
    let path = file.to_str().unwrap().to_string();

    let model = MockModelBuilder::new()
        .then_tool_call("Read", serde_json::json!({"file_path": path}))
        .then_text("I read the file.")
        .build();

    let result = run_with_mock(model, "read it", core_tools()).await;
    assert_eq!(result.turns, 2);
    assert_eq!(result.response_text, "I read the file.");
}

#[tokio::test]
async fn test_harness_multi_tool_parallel() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "aaa").unwrap();
    std::fs::write(dir.path().join("b.txt"), "bbb").unwrap();

    let path_a = dir.path().join("a.txt").to_str().unwrap().to_string();
    let path_b = dir.path().join("b.txt").to_str().unwrap().to_string();

    let model = MockModelBuilder::new()
        .on_call(0, move |_| {
            MockResponse::multi_tool(vec![
                ("Read", serde_json::json!({"file_path": path_a.clone()})),
                ("Read", serde_json::json!({"file_path": path_b.clone()})),
            ])
        })
        .then_text("Read both files.")
        .build();

    let result = run_with_mock(model, "read both", core_tools()).await;
    assert_eq!(result.turns, 2);
    assert_eq!(result.response_text, "Read both files.");
}

#[tokio::test]
async fn test_harness_write_edit_read_chain() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("chain.txt");
    let path = file.to_str().unwrap().to_string();

    let p1 = path.clone();
    let p2 = path.clone();
    let p3 = path.clone();

    let model = MockModelBuilder::new()
        // Step 1: Write file
        .on_call(0, move |_| {
            MockResponse::tool_call(
                "Write",
                serde_json::json!({"file_path": p1.clone(), "content": "original content"}),
            )
        })
        // Step 2: Edit file
        .on_call(1, move |_| {
            MockResponse::tool_call(
                "Edit",
                serde_json::json!({
                    "file_path": p2.clone(),
                    "old_string": "original",
                    "new_string": "modified"
                }),
            )
        })
        // Step 3: Read file back
        .on_call(2, move |_| {
            MockResponse::tool_call("Read", serde_json::json!({"file_path": p3.clone()}))
        })
        // Step 4: Final answer
        .then_text("File was written, edited, and verified.")
        .build();

    let result = run_with_mock(model, "write edit read", core_tools()).await;
    assert_eq!(result.turns, 4);
    assert_eq!(
        result.response_text,
        "File was written, edited, and verified."
    );

    // Verify the file has the edited content
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "modified content");
}

#[tokio::test]
async fn test_harness_bash_echo() {
    let model = MockModelBuilder::new()
        .then_tool_call("Bash", serde_json::json!({"command": "echo hello_e2e"}))
        .then_text("Command executed.")
        .build();

    let result = run_with_mock(model, "run echo", core_tools()).await;
    assert_eq!(result.turns, 2);
    assert_eq!(result.response_text, "Command executed.");
}
