//! Session state aggregate that wires together all components.
//!
//! [`SessionState`] is the main runtime container for an active session,
//! holding references to the API client, tool registry, hooks, and message history.

use std::sync::Arc;

use cocode_api::ApiClient;
use cocode_api::ModelResolver;
use cocode_api::MultiModel;
use cocode_config::ConfigManager;
use cocode_context::ConversationContext;
use cocode_context::EnvironmentInfo;
use cocode_hooks::HookRegistry;
use cocode_loop::AgentLoop;
use cocode_loop::CompactionConfig;
use cocode_loop::FallbackConfig;
use cocode_loop::LoopConfig;
use cocode_loop::LoopResult;
use cocode_message::MessageHistory;
use cocode_protocol::LoopEvent;
use cocode_protocol::ProviderType;
use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::TokenUsage;
use cocode_protocol::model::ModelRole;
use cocode_skill::SkillInterface;
use cocode_tools::ToolRegistry;

use cocode_api::thinking_convert;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;

use crate::session::Session;

/// Result of a single turn in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnResult {
    /// Final text response from the model.
    pub final_text: String,

    /// Number of turns completed so far.
    pub turns_completed: i32,

    /// Token usage for this turn.
    pub usage: TokenUsage,

    /// Whether the model requested more tool calls.
    pub has_pending_tools: bool,

    /// Whether the loop completed (model stop signal).
    pub is_complete: bool,
}

impl TurnResult {
    /// Create a turn result from a loop result.
    pub fn from_loop_result(result: &LoopResult) -> Self {
        Self {
            final_text: result.final_text.clone(),
            turns_completed: result.turns_completed,
            usage: TokenUsage::new(
                result.total_input_tokens as i64,
                result.total_output_tokens as i64,
            ),
            has_pending_tools: false,
            is_complete: true,
        }
    }
}

/// Session state aggregate for an active conversation.
///
/// This struct holds all the runtime components needed to drive a conversation:
/// - Session metadata
/// - Message history
/// - Tool registry
/// - Hook registry
/// - Skills
/// - API client
/// - Cancellation token
///
/// # Example
///
/// ```ignore
/// use cocode_session::{Session, SessionState};
/// use cocode_config::ConfigManager;
/// use cocode_protocol::ProviderType;
/// use std::path::PathBuf;
///
/// let session = Session::new(
///     PathBuf::from("."),
///     "gpt-5",
///     ProviderType::Openai,
/// );
///
/// let config = ConfigManager::from_default()?;
/// let mut state = SessionState::new(session, &config).await?;
///
/// // Run a turn
/// let result = state.run_turn("Hello!").await?;
/// println!("Response: {}", result.final_text);
///
/// // Cancel if needed
/// state.cancel();
/// ```
pub struct SessionState {
    /// Session metadata.
    pub session: Session,

    /// Message history for the conversation.
    pub message_history: MessageHistory,

    /// Tool registry (built-in + MCP tools).
    pub tool_registry: Arc<ToolRegistry>,

    /// Hook registry for event interception.
    pub hook_registry: Arc<HookRegistry>,

    /// Loaded skills.
    pub skills: Vec<SkillInterface>,

    /// API client for model inference.
    api_client: ApiClient,

    /// Model resolver with provider and model caching.
    /// This is the primary source for model resolution.
    model_resolver: Arc<ModelResolver>,

    /// Multi-model container for backward compatibility with AgentLoop.
    /// This is backed by the same ModelResolver and synced with current_selections.
    multi_model: Arc<MultiModel>,

    /// Cancellation token for graceful shutdown.
    cancel_token: CancellationToken,

    /// Loop configuration.
    loop_config: LoopConfig,

    /// Total turns run.
    total_turns: i32,

    /// Total input tokens consumed.
    total_input_tokens: i32,

    /// Total output tokens generated.
    total_output_tokens: i32,

    /// Context window size for the model.
    context_window: i32,

    /// Runtime role selections (model + thinking_level per role).
    current_selections: RoleSelections,

    /// Provider type for the current session.
    provider_type: ProviderType,
}

impl SessionState {
    /// Create a new session state from a session and configuration.
    ///
    /// This initializes all components including:
    /// - API client from the resolved provider/model
    /// - Tool registry with built-in tools
    /// - Hook registry (empty by default)
    /// - Skills (loaded from project/user directories)
    pub async fn new(session: Session, config: &ConfigManager) -> anyhow::Result<Self> {
        // Get the primary model info from session
        let primary_model = session
            .primary_model()
            .ok_or_else(|| anyhow::anyhow!("Session has no main model configured"))?;
        let provider_name = primary_model.provider();
        let model_name = primary_model.model_name();

        info!(
            session_id = %session.id,
            model = %model_name,
            provider = %provider_name,
            "Creating session state"
        );

        // Resolve provider info
        let provider_info = config.resolve_provider(provider_name)?;
        let provider_type = provider_info.provider_type;

        // Get model context window (default to 200k if not specified)
        let context_window = provider_info
            .get_model(model_name)
            .and_then(|m| m.info.context_window)
            .unwrap_or(200_000) as i32;

        // Create API client
        let api_client = ApiClient::new();

        // Create ModelResolver for provider and model caching
        let model_resolver = Arc::new(ModelResolver::new(Arc::new(config.clone())));

        // Create MultiModel backed by ModelResolver for AgentLoop compatibility
        let multi_model = Arc::new(MultiModel::with_resolver(model_resolver.clone()));

        // Set initial selections from session and config
        // current_selections is the SINGLE SOURCE OF TRUTH
        let mut current_selections = config.current_selections();
        current_selections.merge(&session.selections);

        // Sync selections to MultiModel for AgentLoop compatibility
        // Set main model selection
        let main_key = format!("{}/{}", provider_name, model_name);
        multi_model.set_selection(ModelRole::Main, &main_key);

        // Sync other role selections
        for role in ModelRole::all() {
            if *role != ModelRole::Main {
                if let Some(selection) = current_selections.get(*role) {
                    multi_model.set_selection(*role, selection.model.to_string());
                }
            }
        }

        // Create tool registry with built-in tools
        let mut tool_registry = ToolRegistry::new();
        cocode_tools::builtin::register_builtin_tools(&mut tool_registry);

        // Create hook registry (empty for now)
        let hook_registry = HookRegistry::new();

        // Load skills (empty for now, can be populated later)
        let skills = Vec::new();

        // Build loop config from session
        let loop_config = LoopConfig {
            max_turns: session.max_turns,
            ..LoopConfig::default()
        };

        Ok(Self {
            session,
            message_history: MessageHistory::new(),
            tool_registry: Arc::new(tool_registry),
            hook_registry: Arc::new(hook_registry),
            skills,
            api_client,
            model_resolver,
            multi_model,
            cancel_token: CancellationToken::new(),
            loop_config,
            total_turns: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            context_window,
            current_selections,
            provider_type,
        })
    }

    /// Run a single turn with the given user input.
    ///
    /// This creates an agent loop and runs it to completion,
    /// returning the result of the conversation turn.
    pub async fn run_turn(&mut self, user_input: &str) -> anyhow::Result<TurnResult> {
        info!(
            session_id = %self.session.id,
            input_len = user_input.len(),
            "Running turn"
        );

        // Update session activity
        self.session.touch();

        // Create event channel
        let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);

        // Spawn task to handle events (logging for now)
        let cancel_token = self.cancel_token.clone();
        let event_task = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if cancel_token.is_cancelled() {
                    break;
                }
                Self::handle_event(&event);
            }
        });

        // Build environment info
        let model_name = self
            .session
            .model()
            .ok_or_else(|| anyhow::anyhow!("Session has no main model"))?;
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .model(model_name)
            .context_window(self.context_window)
            .output_token_limit(16_384)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build environment: {e}"))?;

        // Build conversation context
        let context = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build context: {e}"))?;

        // Build and run the agent loop
        let mut loop_instance = AgentLoop::builder()
            .api_client(self.api_client.clone())
            .multi_model(self.multi_model.clone())
            .tool_registry(self.tool_registry.clone())
            .context(context)
            .config(self.loop_config.clone())
            .fallback_config(FallbackConfig::default())
            .compaction_config(CompactionConfig::default())
            .hooks(self.hook_registry.clone())
            .event_tx(event_tx)
            .cancel_token(self.cancel_token.clone())
            .build();

        let result = loop_instance.run(user_input).await?;

        // Drop the event sender to signal end of events, then wait for task to complete
        drop(loop_instance);
        let _ = event_task.await;

        // Update totals
        self.total_turns += result.turns_completed;
        self.total_input_tokens += result.total_input_tokens;
        self.total_output_tokens += result.total_output_tokens;

        Ok(TurnResult::from_loop_result(&result))
    }

    /// Handle a loop event (logging).
    fn handle_event(event: &LoopEvent) {
        match event {
            LoopEvent::TurnStarted {
                turn_id,
                turn_number,
            } => {
                debug!(turn_id, turn_number, "Turn started");
            }
            LoopEvent::TurnCompleted { turn_id, usage } => {
                debug!(
                    turn_id,
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    "Turn completed"
                );
            }
            LoopEvent::TextDelta { delta, .. } => {
                // In a real implementation, this would stream to UI
                debug!(delta_len = delta.len(), "Text delta");
            }
            LoopEvent::ToolUseQueued { name, call_id, .. } => {
                debug!(name, call_id, "Tool queued");
            }
            LoopEvent::Error { error } => {
                tracing::error!(code = %error.code, message = %error.message, "Loop error");
            }
            _ => {
                debug!(?event, "Loop event");
            }
        }
    }

    /// Cancel the current operation.
    pub fn cancel(&self) {
        info!(session_id = %self.session.id, "Cancelling session");
        self.cancel_token.cancel();
    }

    /// Check if the session is cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    /// Get the model name.
    ///
    /// Returns the main model name, or an empty string if not configured.
    pub fn model(&self) -> &str {
        self.session.model().unwrap_or("")
    }

    /// Get the provider name.
    ///
    /// Returns the main provider name, or an empty string if not configured.
    pub fn provider(&self) -> &str {
        self.session.provider().unwrap_or("")
    }

    /// Get total turns run.
    pub fn total_turns(&self) -> i32 {
        self.total_turns
    }

    /// Get total input tokens consumed.
    pub fn total_input_tokens(&self) -> i32 {
        self.total_input_tokens
    }

    /// Get total output tokens generated.
    pub fn total_output_tokens(&self) -> i32 {
        self.total_output_tokens
    }

    /// Get the message history.
    pub fn history(&self) -> &MessageHistory {
        &self.message_history
    }

    /// Get mutable access to the message history.
    pub fn history_mut(&mut self) -> &mut MessageHistory {
        &mut self.message_history
    }

    /// Set the hook registry.
    pub fn set_hooks(&mut self, hooks: Arc<HookRegistry>) {
        self.hook_registry = hooks;
    }

    /// Add a skill to the session.
    pub fn add_skill(&mut self, skill: SkillInterface) {
        self.skills.push(skill);
    }

    /// Get the loaded skills.
    pub fn skills(&self) -> &[SkillInterface] {
        &self.skills
    }

    /// Update the loop configuration.
    pub fn set_loop_config(&mut self, config: LoopConfig) {
        self.loop_config = config;
    }

    /// Get the loop configuration.
    pub fn loop_config(&self) -> &LoopConfig {
        &self.loop_config
    }

    // ==========================================================
    // Role Selection API
    // ==========================================================

    /// Get all current role selections.
    pub fn selections(&self) -> &RoleSelections {
        &self.current_selections
    }

    /// Get selection for a specific role.
    pub fn selection(&self, role: ModelRole) -> Option<&RoleSelection> {
        self.current_selections.get(role)
    }

    /// Get thinking level for a specific role.
    ///
    /// Returns the explicitly set thinking level for this role, or None
    /// if no override is set (model's default will be used).
    pub fn thinking_level(&self, role: ModelRole) -> Option<&ThinkingLevel> {
        self.current_selections
            .get(role)
            .and_then(|s| s.thinking_level.as_ref())
    }

    /// Get the provider type for this session.
    pub fn provider_type(&self) -> ProviderType {
        self.provider_type
    }

    /// Get the model resolver.
    ///
    /// The resolver provides access to cached providers and models.
    /// Use `get_for_role(role, &selections)` to get models with custom selections.
    pub fn model_resolver(&self) -> &Arc<ModelResolver> {
        &self.model_resolver
    }

    /// Get or create a model for a specific role.
    ///
    /// Get model for a specific role using the model resolver with current selections.
    /// Falls back to the main role if the requested role has no selection.
    ///
    /// # Returns
    ///
    /// A tuple of (model, provider_type) for the role, or None if no selection exists.
    pub fn get_model_for_role(
        &self,
        role: ModelRole,
    ) -> anyhow::Result<Option<(Arc<dyn hyper_sdk::Model>, ProviderType)>> {
        // Use ModelResolver with current_selections as the source of truth
        match self
            .model_resolver
            .get_for_role(role, &self.current_selections)
        {
            Ok((model, provider_type)) => Ok(Some((model, provider_type))),
            Err(e) => {
                // If error is "no model configured", return None instead of error
                if e.is_no_model_configured() {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("{}", e))
                }
            }
        }
    }

    /// Get the main model (shorthand for get_model_for_role(ModelRole::Main)).
    ///
    /// Returns the main model using the model resolver with current selections.
    pub fn main_model(&self) -> anyhow::Result<Arc<dyn hyper_sdk::Model>> {
        self.model_resolver
            .main(&self.current_selections)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Switch model for a specific role.
    ///
    /// This updates both the role selection and the multi-model container.
    pub fn switch_role(&mut self, role: ModelRole, selection: RoleSelection) {
        info!(
            role = %role,
            model = %selection.model,
            thinking = ?selection.thinking_level,
            "Switching role"
        );
        // Update selections
        self.current_selections.set(role, selection.clone());
        // Update multi-model selection
        self.multi_model
            .set_selection(role, selection.model.to_string());
    }

    /// Switch only the thinking level for a specific role.
    ///
    /// This updates the thinking level without changing the model.
    /// Returns `true` if the role selection exists and was updated.
    pub fn switch_thinking_level(&mut self, role: ModelRole, level: ThinkingLevel) -> bool {
        info!(
            role = %role,
            thinking = %level,
            "Switching thinking level for role"
        );
        self.current_selections.set_thinking_level(role, level)
    }

    /// Clear thinking level override for a specific role.
    ///
    /// Returns `true` if the role selection exists and was updated.
    pub fn clear_thinking_level(&mut self, role: ModelRole) -> bool {
        if let Some(selection) = self.current_selections.get_mut(role) {
            selection.clear_thinking_level();
            info!(role = %role, "Cleared thinking level for role");
            true
        } else {
            false
        }
    }

    /// Build provider options from current thinking level for a role.
    ///
    /// Returns the provider-specific options needed to configure thinking
    /// for the current session's provider, or None if no thinking is configured.
    ///
    /// Note: If `model_info` is None, default ModelInfo is used, which means
    /// no reasoning_summary or include_thoughts overrides will be applied.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Get options for main role with model info
    /// if let Some(opts) = state.build_thinking_options(ModelRole::Main, Some(&model_info)) {
    ///     request = request.provider_options(opts);
    /// }
    /// ```
    pub fn build_thinking_options(
        &self,
        role: ModelRole,
        model_info: Option<&cocode_protocol::ModelInfo>,
    ) -> Option<hyper_sdk::options::ProviderOptions> {
        let thinking_level = self.thinking_level(role)?;
        let default_model_info = cocode_protocol::ModelInfo::default();
        let model_info = model_info.unwrap_or(&default_model_info);
        thinking_convert::to_provider_options(thinking_level, model_info, self.provider_type)
    }

    // ==========================================================
    // Streaming Turn API
    // ==========================================================

    /// Run a single turn with the given user input, streaming events to the provided channel.
    ///
    /// This is similar to `run_turn` but forwards all events to the provided channel
    /// instead of handling them internally. This enables real-time streaming to a TUI
    /// or other consumer.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tokio::sync::mpsc;
    /// use cocode_protocol::LoopEvent;
    ///
    /// let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);
    ///
    /// // Spawn task to handle events
    /// tokio::spawn(async move {
    ///     while let Some(event) = event_rx.recv().await {
    ///         // Process event (update TUI, etc.)
    ///     }
    /// });
    ///
    /// let result = state.run_turn_streaming("Hello!", event_tx).await?;
    /// ```
    pub async fn run_turn_streaming(
        &mut self,
        user_input: &str,
        event_tx: mpsc::Sender<LoopEvent>,
    ) -> anyhow::Result<TurnResult> {
        info!(
            session_id = %self.session.id,
            input_len = user_input.len(),
            "Running turn with streaming"
        );

        // Update session activity
        self.session.touch();

        // Build environment info
        let model_name = self
            .session
            .model()
            .ok_or_else(|| anyhow::anyhow!("Session has no main model"))?;
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .model(model_name)
            .context_window(self.context_window)
            .output_token_limit(16_384)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build environment: {e}"))?;

        // Build conversation context
        let context = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build context: {e}"))?;

        // Build and run the agent loop with the provided event channel
        let mut loop_instance = AgentLoop::builder()
            .api_client(self.api_client.clone())
            .multi_model(self.multi_model.clone())
            .tool_registry(self.tool_registry.clone())
            .context(context)
            .config(self.loop_config.clone())
            .fallback_config(FallbackConfig::default())
            .compaction_config(CompactionConfig::default())
            .hooks(self.hook_registry.clone())
            .event_tx(event_tx)
            .cancel_token(self.cancel_token.clone())
            .build();

        let result = loop_instance.run(user_input).await?;

        // Update totals
        self.total_turns += result.turns_completed;
        self.total_input_tokens += result.total_input_tokens;
        self.total_output_tokens += result.total_output_tokens;

        Ok(TurnResult::from_loop_result(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_sdk::ContentBlock;

    #[test]
    fn test_turn_result_from_loop_result() {
        let loop_result = LoopResult::completed(
            3,
            1000,
            500,
            "Hello!".to_string(),
            vec![ContentBlock::text("Hello!")],
        );

        let turn = TurnResult::from_loop_result(&loop_result);
        assert_eq!(turn.final_text, "Hello!");
        assert_eq!(turn.turns_completed, 3);
        assert_eq!(turn.usage.input_tokens, 1000);
        assert_eq!(turn.usage.output_tokens, 500);
        assert!(turn.is_complete);
    }

    #[test]
    fn test_turn_result_serde() {
        let turn = TurnResult {
            final_text: "test".to_string(),
            turns_completed: 5,
            usage: TokenUsage::new(100, 50),
            has_pending_tools: false,
            is_complete: true,
        };

        let json = serde_json::to_string(&turn).expect("serialize");
        let parsed: TurnResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.final_text, turn.final_text);
        assert_eq!(parsed.turns_completed, turn.turns_completed);
        assert_eq!(parsed.usage.input_tokens, turn.usage.input_tokens);
    }
}
