//! Concrete `SideQuery` implementation that routes through `ApiClient`.
//!
//! Lives in `app/cli` (the binding layer) because it composes the two
//! halves: the `SideQuery` trait owned by `coco-tool-runtime` and the
//! `ApiClient` owned by `coco-inference`. Layer rules forbid the impl
//! from living in either of those crates (would create a back-edge).
//!
//! The adapter holds a `RuntimeConfig` Arc so `request.model_role` can
//! resolve through `ModelRoles::get(...)` — operator's
//! `settings.models.<role>` choice flows unchanged. When a role is set
//! and a different `ApiClient` is needed (e.g. `ModelRole::Memory`
//! pointing at a separate provider), the adapter builds a fresh client
//! via the same `coco_inference::model_factory` path the main session
//! uses; otherwise it falls through to the default client.

use std::sync::Arc;

use async_trait::async_trait;
use coco_config::RuntimeConfig;
use coco_inference::ApiClient;
use coco_inference::LanguageModelFunctionTool;
use coco_inference::LanguageModelTool;
use coco_inference::QueryParams;
use coco_messages::AssistantContent;
use coco_messages::LlmMessage;
use coco_tool_runtime::SideQuery;
use coco_types::SideQueryRequest;
use coco_types::SideQueryResponse;
use coco_types::SideQueryRole;
use coco_types::SideQueryStopReason;
use coco_types::SideQueryToolUse;
use coco_types::SideQueryUsage;

/// `SideQuery` adapter wrapping an `ApiClient` plus a `RuntimeConfig`
/// for role resolution. When `request.model_role` is set, the adapter
/// builds a per-role client; otherwise it uses the default. Per-role
/// clients are cached for the session — `build_api_client` is
/// non-trivial (auth resolution, retry config, fingerprint) and the
/// recall ranker hits this path on every turn.
pub struct SideQueryAdapter {
    default_client: Arc<ApiClient>,
    runtime_config: Arc<RuntimeConfig>,
    role_clients:
        tokio::sync::RwLock<std::collections::HashMap<coco_types::ModelRole, Arc<ApiClient>>>,
}

impl SideQueryAdapter {
    pub fn new(default_client: Arc<ApiClient>, runtime_config: Arc<RuntimeConfig>) -> Self {
        Self {
            default_client,
            runtime_config,
            role_clients: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    async fn resolve_client(
        &self,
        request: &SideQueryRequest,
    ) -> Result<Arc<ApiClient>, anyhow::Error> {
        // Role wins over `model` string — operator picks the provider
        // via `settings.models.<role>`.
        let Some(role) = request.model_role else {
            return Ok(self.default_client.clone());
        };
        // Fast path: cached client.
        if let Some(client) = self.role_clients.read().await.get(&role) {
            return Ok(client.clone());
        }
        // Slow path: build, then cache.
        let Some(spec) = self.runtime_config.model_roles.get(role).cloned() else {
            return Ok(self.default_client.clone());
        };
        let client = coco_inference::model_factory::build_api_client(
            &self.runtime_config,
            &spec,
            self.runtime_config.api.retry.clone().into(),
        )
        .map_err(|e| anyhow::anyhow!("build_api_client for role {role:?}: {e}"))?;
        self.role_clients.write().await.insert(role, client.clone());
        Ok(client)
    }
}

impl std::fmt::Debug for SideQueryAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SideQueryAdapter").finish_non_exhaustive()
    }
}

#[async_trait]
impl SideQuery for SideQueryAdapter {
    async fn query(
        &self,
        request: SideQueryRequest,
    ) -> Result<SideQueryResponse, coco_error::BoxedError> {
        let client = self.resolve_client(&request).await.map_err(|e| {
            Box::new(coco_error::PlainError::new(
                e.to_string(),
                coco_error::StatusCode::Internal,
            )) as coco_error::BoxedError
        })?;

        // Build the prompt: system message + user/assistant turns from
        // `request.messages`. Tool definitions are forwarded when
        // `forced_tool` is set (TS structured-output path).
        let mut prompt: Vec<LlmMessage> = Vec::with_capacity(request.messages.len() + 1);
        if !request.system.is_empty() {
            prompt.push(LlmMessage::system(&request.system));
        }
        for m in &request.messages {
            match m.role {
                SideQueryRole::User => {
                    prompt.push(LlmMessage::user_text(&m.content));
                }
                SideQueryRole::Assistant => {
                    prompt.push(LlmMessage::assistant_text(&m.content));
                }
            }
        }

        // Convert `request.tools` (`SideQueryToolDef`) into the vercel-ai
        // tool shape via the version-agnostic re-exports owned by
        // `coco-inference`. Only the function-tool subset is used —
        // that's all `SideQuery` exposes.
        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| {
                        LanguageModelTool::Function(LanguageModelFunctionTool {
                            name: t.name.clone(),
                            description: Some(t.description.clone()),
                            input_schema: t.input_schema.clone(),
                            input_examples: None,
                            strict: None,
                            provider_options: None,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        };

        let params = QueryParams {
            prompt,
            max_tokens: request.max_tokens.map(i64::from),
            thinking_level: None,
            fast_mode: false,
            tools,
            context_management: None,
            query_source: Some(request.query_source.clone()),
            agent_id: None,
            time_since_last_assistant_ms: None,
            // SDK side-query helper — not the agent loop. Per-call cache
            // strategy could be wired through `request` later if SDK
            // surface adds it.
            agentic: false,
            cache: None,
        };

        let result = client.query(&params).await.map_err(|e| {
            Box::new(coco_error::PlainError::new(
                e.to_string(),
                coco_error::StatusCode::ProviderError,
            )) as coco_error::BoxedError
        })?;

        // Marshal the `AssistantContent` blocks back into the
        // structured `SideQueryResponse` shape. Text blocks
        // concatenate; `tool-call` blocks become `tool_uses`.
        let mut text_buf = String::new();
        let mut tool_uses = Vec::new();
        for c in &result.content {
            match c {
                AssistantContent::Text(t) => {
                    text_buf.push_str(&t.text);
                }
                AssistantContent::ToolCall(tc) => {
                    tool_uses.push(SideQueryToolUse {
                        name: tc.tool_name.clone(),
                        input: tc.input.clone(),
                    });
                }
                _ => {}
            }
        }

        let stop_reason = match result.stop_reason.as_deref() {
            Some("end_turn") | Some("stop") => SideQueryStopReason::EndTurn,
            Some("max_tokens") | Some("length") => SideQueryStopReason::MaxTokens,
            Some("tool_use") | Some("tool_calls") => SideQueryStopReason::ToolUse,
            Some("stop_sequence") => SideQueryStopReason::StopSequence,
            Some(other) => SideQueryStopReason::Other(other.to_string()),
            None => SideQueryStopReason::EndTurn,
        };

        Ok(SideQueryResponse {
            text: if text_buf.is_empty() {
                None
            } else {
                Some(text_buf)
            },
            tool_uses,
            stop_reason,
            usage: SideQueryUsage {
                input_tokens: result.usage.input_tokens,
                output_tokens: result.usage.output_tokens,
            },
            model_used: result.model,
        })
    }

    fn model_id(&self) -> &str {
        self.default_client.model_id()
    }
}
