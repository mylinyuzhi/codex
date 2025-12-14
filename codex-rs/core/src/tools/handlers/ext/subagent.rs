//! Subagent Tool Handlers
//!
//! Handlers for Task and TaskOutput tools that integrate with the subagent system.

use crate::function_tool::FunctionCallError;
use crate::model_provider_info::ModelProviderInfo;
use crate::subagent::AgentExecutor;
use crate::subagent::ModelClientBridge;
use crate::subagent::ModelConfig;
use crate::subagent::SubagentContext;
use crate::subagent::SubagentStatus;
use crate::subagent::get_or_create_stores;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

/// Arguments for Task tool invocation
#[derive(Debug, Clone, Deserialize)]
pub struct TaskArgs {
    pub subagent_type: String,
    pub prompt: String,
    pub description: String,
    /// Provider name override - references config.model_providers HashMap key.
    /// Takes highest priority for provider selection.
    #[serde(default)]
    pub model_provider: Option<String>,
    /// Model name override.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub run_in_background: bool,
    #[serde(default)]
    pub resume: Option<String>,
}

/// Task Tool Handler
///
/// Spawns subagents for complex, multi-step tasks.
/// Stores are obtained from the global registry using conversation_id.
#[derive(Debug, Default)]
pub struct TaskHandler;

impl TaskHandler {
    /// Create a new Task handler.
    /// Stores are obtained from global registry at runtime via conversation_id.
    pub fn new() -> Self {
        Self
    }

    /// Resolve provider using priority chain.
    ///
    /// Priority order:
    /// 1. Task tool parameter (override_provider)
    /// 2. Environment variable CODEX_SUBAGENT_PROVIDER
    /// 3. Agent definition model_config.provider
    /// 4. None (inherit parent session's provider)
    async fn resolve_provider(
        &self,
        invocation: &ToolInvocation,
        override_provider: Option<&str>,
        model_config: &ModelConfig,
    ) -> Result<Option<ModelProviderInfo>, FunctionCallError> {
        // Priority 1: Task tool parameter
        if let Some(provider_name) = override_provider {
            return self.get_provider_info(invocation, provider_name).await;
        }

        // Priority 2: Environment variable
        if let Ok(env_provider) = std::env::var("CODEX_SUBAGENT_PROVIDER") {
            return self.get_provider_info(invocation, &env_provider).await;
        }

        // Priority 3: Agent definition
        if let Some(provider_name) = &model_config.provider {
            return self.get_provider_info(invocation, provider_name).await;
        }

        // Priority 4: Inherit (return None)
        Ok(None)
    }

    /// Get ModelProviderInfo from config by provider name.
    async fn get_provider_info(
        &self,
        invocation: &ToolInvocation,
        provider_name: &str,
    ) -> Result<Option<ModelProviderInfo>, FunctionCallError> {
        let state = invocation.session.state.lock().await;
        let config = &state.session_configuration.original_config_do_not_use;

        config
            .model_providers
            .get(provider_name)
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                let available: Vec<&String> = config.model_providers.keys().collect();
                FunctionCallError::RespondToModel(format!(
                    "Unknown provider: '{}'. Available providers: {:?}",
                    provider_name, available
                ))
            })
    }

    /// Resolve model name using priority chain.
    ///
    /// Priority order:
    /// 1. Environment variable CODEX_SUBAGENT_MODEL
    /// 2. Task tool parameter (override_model)
    /// 3. Agent definition model_config.model
    /// 4. Provider's default model (from provider_info.ext.model_name)
    /// 5. Inherit parent session's model
    fn resolve_model(
        override_model: Option<&str>,
        model_config: &ModelConfig,
        provider_info: Option<&ModelProviderInfo>,
        parent_model: &str,
    ) -> String {
        // Priority 1: Environment variable
        if let Ok(env_model) = std::env::var("CODEX_SUBAGENT_MODEL") {
            return env_model;
        }

        // Priority 2: Task tool parameter
        if let Some(model) = override_model {
            return Self::resolve_model_name(model);
        }

        // Priority 3: Agent definition
        if let Some(def_model) = &model_config.model {
            return Self::resolve_model_name(def_model);
        }

        // Priority 4: Provider's default model
        if let Some(info) = provider_info {
            if let Some(model_name) = &info.ext.model_name {
                return model_name.clone();
            }
        }

        // Priority 5: Parent model (inherit)
        parent_model.to_string()
    }

    /// Map user-friendly model names to actual identifiers.
    fn resolve_model_name(name: &str) -> String {
        match name.to_lowercase().as_str() {
            "sonnet" | "claude-sonnet" => "claude-sonnet".to_string(),
            "haiku" | "claude-haiku" => "claude-haiku".to_string(),
            "opus" | "claude-opus" => "claude-opus".to_string(),
            _ => name.to_string(),
        }
    }

    /// Create a new ModelClient with a custom provider.
    ///
    /// Applies model parameters (temperature, top_p) from the agent's ModelConfig
    /// to the provider's configuration.
    async fn create_model_client(
        &self,
        invocation: &ToolInvocation,
        mut provider_info: ModelProviderInfo,
        model_name: &str,
        model_config: &ModelConfig,
    ) -> Result<crate::client::ModelClient, FunctionCallError> {
        use crate::client::ModelClient;
        use crate::openai_models::model_family::find_family_for_model;
        use codex_protocol::ConversationId;
        use codex_protocol::config_types_ext::ModelParameters;

        let parent_client = &invocation.turn.client;

        // Get config from parent client
        let config = parent_client.config();

        // Reuse auth_manager and otel_manager from parent
        let auth_manager = parent_client.auth_manager();
        let otel_manager = parent_client.otel_manager().clone();

        // Get model family from model name
        let model_family = find_family_for_model(model_name);

        // Generate new conversation_id for subagent (independent conversation)
        let conversation_id = ConversationId::new();

        // Apply model parameters from agent definition to provider
        // Only override if agent definition has non-default values (Fix #5)
        let existing_params = provider_info
            .ext
            .model_parameters
            .take()
            .unwrap_or_default();

        // Default values from subagent/definition/mod.rs
        const DEFAULT_TEMPERATURE: f32 = 0.7;
        const DEFAULT_TOP_P: f32 = 0.95;

        let merged_params = ModelParameters {
            // Only use agent's temperature if it differs from default
            temperature: if (model_config.temperature - DEFAULT_TEMPERATURE).abs() > f32::EPSILON {
                Some(model_config.temperature)
            } else {
                existing_params.temperature
            },
            // Only use agent's top_p if it differs from default
            top_p: if (model_config.top_p - DEFAULT_TOP_P).abs() > f32::EPSILON {
                Some(model_config.top_p)
            } else {
                existing_params.top_p
            },
            // Keep other parameters from provider
            frequency_penalty: existing_params.frequency_penalty,
            presence_penalty: existing_params.presence_penalty,
            max_tokens: existing_params.max_tokens,
            budget_tokens: existing_params.budget_tokens,
            include_thoughts: existing_params.include_thoughts,
        };
        provider_info.ext.model_parameters = Some(merged_params);

        // Reuse reasoning config from parent
        let effort = parent_client.reasoning_effort();
        let summary = parent_client.reasoning_summary();
        let session_source = parent_client.session_source().clone();

        Ok(ModelClient::new(
            config,
            auth_manager,
            model_family,
            otel_manager,
            provider_info,
            effort,
            summary,
            conversation_id,
            session_source,
        ))
    }
}

#[async_trait]
impl ToolHandler for TaskHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        // Parse arguments
        let arguments = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "Invalid payload type for Task".to_string(),
                ));
            }
        };

        let args: TaskArgs = serde_json::from_str(arguments)
            .map_err(|e| FunctionCallError::RespondToModel(format!("Invalid arguments: {e}")))?;

        // Get session-scoped stores from global registry
        let stores = get_or_create_stores(invocation.session.conversation_id);

        // Get agent definition
        let definition = stores
            .registry
            .get(&args.subagent_type)
            .await
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "Unknown subagent type '{}'. Available types: Explore, Plan",
                    args.subagent_type
                ))
            })?;

        // Get parent client and its model for inheritance
        let parent_client = &invocation.turn.client;
        let parent_model = parent_client.provider().name.as_str();

        // Resolve provider (returns None if inheriting parent)
        let resolved_provider = self
            .resolve_provider(
                &invocation,
                args.model_provider.as_deref(),
                &definition.model_config,
            )
            .await?;

        // Resolve model name using priority chain
        let resolved_model = Self::resolve_model(
            args.model.as_deref(),
            &definition.model_config,
            resolved_provider.as_ref(),
            parent_model,
        );

        // Create ModelClient - either new one with custom provider or clone parent's
        let model_client = if let Some(provider_info) = resolved_provider {
            // Create new ModelClient with custom provider and agent's model parameters
            self.create_model_client(
                &invocation,
                provider_info,
                &resolved_model,
                &definition.model_config,
            )
            .await?
        } else {
            // Inherit parent's provider but apply agent's model parameters
            let parent_provider = parent_client.provider().clone();
            self.create_model_client(
                &invocation,
                parent_provider,
                &resolved_model,
                &definition.model_config,
            )
            .await?
        };

        // Clone model name for use in bridge (before it's moved into context)
        let model_name_for_bridge = resolved_model.clone();

        // Subagent tool access is determined solely by agent definition - no parent
        // tool intersection. Subagents can use any tool their definition allows.
        // For background tasks, restrict to ASYNC_SAFE_TOOLS.
        let context = SubagentContext::new(
            definition,
            invocation.turn.cwd.clone(),
            invocation.cancellation_token.child_token(), // Propagate parent cancellation
            resolved_model,
        )
        .with_async(args.run_in_background);

        // Create model bridge from the ModelClient with actual model name
        let model_bridge = ModelClientBridge::new(model_client, model_name_for_bridge);
        let executor = AgentExecutor::new(context).with_model_bridge(Arc::new(model_bridge));

        if args.run_in_background {
            // Spawn background task with two-phase registration to prevent race conditions
            let agent_id = executor.context.agent_id.clone();
            let prompt = args.prompt.clone();
            let description = args.description.clone();
            let transcript_store = stores.transcript_store.clone();

            // Phase 1: Pre-register with Pending status (before spawn)
            // This ensures TaskOutput can find the task immediately after Task returns
            stores.background_store.register_pending(
                agent_id.clone(),
                description.clone(),
                prompt.clone(),
            );

            // Capture agent_id for error handling in closure (Fix #3)
            let agent_id_for_error = agent_id.clone();

            let handle = tokio::spawn(async move {
                executor
                    .run_with_resume(prompt, None, &transcript_store)
                    .await
                    .unwrap_or_else(|e| crate::subagent::SubagentResult {
                        status: SubagentStatus::Error,
                        result: format!("Spawn error: {e}"),
                        turns_used: 0,
                        duration: Duration::ZERO,
                        agent_id: agent_id_for_error, // Use captured agent_id instead of "unknown"
                        total_tool_use_count: 0,
                        total_duration_ms: 0,
                        total_tokens: 0,
                        usage: None,
                    })
            });

            // Phase 2: Set handle and transition to Running status
            stores.background_store.set_handle(&agent_id, handle);

            Ok(ToolOutput::Function {
                content: serde_json::json!({
                    "status": "async_launched",
                    "agent_id": agent_id,
                    "description": description,
                })
                .to_string(),
                content_items: None,
                success: Some(true),
            })
        } else {
            // Synchronous execution
            let result = executor
                .run_with_resume(
                    args.prompt,
                    args.resume.as_deref(),
                    &stores.transcript_store,
                )
                .await
                .map_err(|e| FunctionCallError::RespondToModel(format!("Execution failed: {e}")))?;

            Ok(ToolOutput::Function {
                content: serde_json::json!({
                    "status": result.status,
                    "result": result.result,
                    "turns_used": result.turns_used,
                    "duration_seconds": result.duration.as_secs_f32(),
                    "agent_id": result.agent_id,
                    "total_tool_use_count": result.total_tool_use_count,
                    "total_tokens": result.total_tokens,
                })
                .to_string(),
                content_items: None,
                success: Some(result.status == SubagentStatus::Goal),
            })
        }
    }
}

/// Arguments for TaskOutput tool invocation
#[derive(Debug, Clone, Deserialize)]
pub struct TaskOutputArgs {
    pub agent_id: String,
    #[serde(default = "default_block")]
    pub block: bool,
    #[serde(default = "default_timeout")]
    pub timeout: i32,
}

fn default_block() -> bool {
    true
}

fn default_timeout() -> i32 {
    300
}

/// TaskOutput Tool Handler
///
/// Retrieves results from background subagent tasks.
/// Stores are obtained from the global registry using conversation_id.
#[derive(Debug, Default)]
pub struct TaskOutputHandler;

/// Default cleanup duration for old tasks and transcripts (1 hour).
const CLEANUP_OLDER_THAN: Duration = Duration::from_secs(60 * 60);

impl TaskOutputHandler {
    /// Create a new TaskOutput handler.
    /// Stores are obtained from global registry at runtime via conversation_id.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolHandler for TaskOutputHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        // Parse arguments
        let arguments = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "Invalid payload type for TaskOutput".to_string(),
                ));
            }
        };

        let args: TaskOutputArgs = serde_json::from_str(arguments)
            .map_err(|e| FunctionCallError::RespondToModel(format!("Invalid arguments: {e}")))?;

        // Get session-scoped stores from global registry
        let stores = get_or_create_stores(invocation.session.conversation_id);

        let timeout = Duration::from_secs(args.timeout as u64);

        match stores
            .background_store
            .get_result(&args.agent_id, args.block, timeout)
            .await
        {
            Some(result) => {
                // Trigger cleanup opportunistically after retrieving a result
                stores
                    .background_store
                    .cleanup_old_tasks(CLEANUP_OLDER_THAN);
                stores
                    .transcript_store
                    .cleanup_old_transcripts(CLEANUP_OLDER_THAN);

                Ok(ToolOutput::Function {
                    content: serde_json::json!({
                        "status": result.status,
                        "result": result.result,
                        "turns_used": result.turns_used,
                        "duration_seconds": result.duration.as_secs_f32(),
                    })
                    .to_string(),
                    content_items: None,
                    success: Some(result.status == SubagentStatus::Goal),
                })
            }
            None => {
                let status = stores.background_store.get_status(&args.agent_id);
                Ok(ToolOutput::Function {
                    content: serde_json::json!({
                        "status": status.map(|s| format!("{:?}", s)).unwrap_or("not_found".to_string()),
                        "message": if status.is_some() {
                            "Task still running or timed out waiting"
                        } else {
                            "No task found with that agent_id"
                        },
                    })
                    .to_string(),
                    content_items: None,
                    success: Some(false),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_handler_kind() {
        let handler = TaskHandler::new();
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[test]
    fn test_task_output_handler_kind() {
        let handler = TaskOutputHandler::new();
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[test]
    fn test_parse_task_args() {
        let args: TaskArgs = serde_json::from_str(
            r#"{"subagent_type": "Explore", "prompt": "Find files", "description": "Finding files"}"#,
        )
        .expect("should parse");
        assert_eq!(args.subagent_type, "Explore");
        assert_eq!(args.prompt, "Find files");
        assert!(!args.run_in_background);
    }

    #[test]
    fn test_parse_task_output_args() {
        let args: TaskOutputArgs =
            serde_json::from_str(r#"{"agent_id": "agent-123"}"#).expect("should parse");
        assert_eq!(args.agent_id, "agent-123");
        assert!(args.block);
        assert_eq!(args.timeout, 300);
    }

    #[test]
    fn test_parse_task_args_with_model_provider() {
        let args: TaskArgs = serde_json::from_str(
            r#"{"subagent_type": "Explore", "prompt": "Find files", "description": "Finding files", "model_provider": "openai", "model": "gpt-4"}"#,
        )
        .expect("should parse");
        assert_eq!(args.subagent_type, "Explore");
        assert_eq!(args.model_provider, Some("openai".to_string()));
        assert_eq!(args.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_resolve_model_name() {
        // User-friendly names
        assert_eq!(TaskHandler::resolve_model_name("sonnet"), "claude-sonnet");
        assert_eq!(TaskHandler::resolve_model_name("haiku"), "claude-haiku");
        assert_eq!(TaskHandler::resolve_model_name("opus"), "claude-opus");

        // Already full names
        assert_eq!(
            TaskHandler::resolve_model_name("claude-sonnet"),
            "claude-sonnet"
        );

        // Unknown model - pass through
        assert_eq!(TaskHandler::resolve_model_name("gpt-4"), "gpt-4");
    }
}
