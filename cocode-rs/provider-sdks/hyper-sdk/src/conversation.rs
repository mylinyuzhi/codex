//! Conversation context for multi-turn conversations.
//!
//! `ConversationContext` manages conversation state across multiple API calls,
//! including message history, previous response IDs for continuity, and hooks.
//!
//! # Multi-Turn Conversations
//!
//! The [`generate`] and [`stream`] methods **automatically prepend conversation
//! history** to each request. This means you only need to provide the new message(s)
//! for each turn, and the context will include all previous messages automatically.
//!
//! # Example
//!
//! ```no_run
//! use hyper_sdk::conversation::ConversationContext;
//! use hyper_sdk::session::SessionConfig;
//! use hyper_sdk::hooks::ResponseIdHook;
//! use hyper_sdk::{GenerateRequest, Message, OpenAIProvider, Provider};
//! use std::sync::Arc;
//!
//! # async fn example() -> hyper_sdk::Result<()> {
//! let provider = OpenAIProvider::from_env()?;
//! let model = provider.model("gpt-4o")?;
//!
//! // Create conversation with session config and hooks
//! let mut conversation = ConversationContext::new()
//!     .with_session_config(SessionConfig::new().temperature(0.7));
//!
//! conversation.add_request_hook(Arc::new(ResponseIdHook));
//!
//! // First turn - sends: [user: "Hello!"]
//! let response = conversation.generate(
//!     model.as_ref(),
//!     GenerateRequest::new(vec![Message::user("Hello!")]),
//! ).await?;
//!
//! // Second turn - automatically includes history
//! // Sends: [user: "Hello!", assistant: <response>, user: "What's 2+2?"]
//! let response = conversation.generate(
//!     model.as_ref(),
//!     GenerateRequest::new(vec![Message::user("What's 2+2?")]),
//! ).await?;
//!
//! // For single-turn requests without history, use generate_stateless()
//! let response = conversation.generate_stateless(
//!     model.as_ref(),
//!     GenerateRequest::new(vec![Message::user("Independent question")]),
//! ).await?;
//!
//! // Access full history
//! println!("Messages: {:?}", conversation.messages());
//! # Ok(())
//! # }
//! ```

use crate::error::HyperError;
use crate::hooks::HookChain;
use crate::hooks::HookContext;
use crate::hooks::RequestHook;
use crate::hooks::ResponseHook;
use crate::hooks::StreamHook;
use crate::messages::Message;
use crate::messages::Role;
use crate::model::Model;
use crate::request::GenerateRequest;
use crate::response::GenerateResponse;
use crate::session::SessionConfig;
use crate::stream::StreamResponse;
use std::sync::Arc;
use tracing::debug;
use tracing::instrument;
use uuid::Uuid;

/// Generate a unique request ID for hook correlation.
fn generate_request_id() -> String {
    format!("req_{}", Uuid::new_v4())
}

/// Manages conversation state across multiple API calls.
///
/// `ConversationContext` tracks message history, maintains conversation continuity
/// via `previous_response_id`, and executes hooks on requests and responses.
#[derive(Debug)]
pub struct ConversationContext {
    /// Unique conversation ID.
    id: String,
    /// Provider name for hook context.
    provider: String,
    /// Model ID for hook context.
    model_id: String,
    /// Previous response ID for conversation continuity.
    previous_response_id: Option<String>,
    /// Message history.
    messages: Vec<Message>,
    /// Session configuration (merged into requests).
    session_config: SessionConfig,
    /// Hook chain for this conversation.
    hooks: HookChain,
    /// Whether to track message history.
    track_history: bool,
}

impl Default for ConversationContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationContext {
    /// Create a new conversation context with a generated ID.
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            provider: String::new(),
            model_id: String::new(),
            previous_response_id: None,
            messages: Vec::new(),
            session_config: SessionConfig::default(),
            hooks: HookChain::new(),
            track_history: true,
        }
    }

    /// Create a conversation context with a specific ID.
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::new()
        }
    }

    /// Set the session configuration.
    pub fn with_session_config(mut self, config: SessionConfig) -> Self {
        self.session_config = config;
        self
    }

    /// Set the provider and model for hook context.
    pub fn with_provider_info(
        mut self,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        self.provider = provider.into();
        self.model_id = model_id.into();
        self
    }

    /// Disable message history tracking.
    ///
    /// When disabled, messages are not stored after each turn.
    pub fn without_history(mut self) -> Self {
        self.track_history = false;
        self
    }

    /// Get the conversation ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the previous response ID.
    pub fn previous_response_id(&self) -> Option<&str> {
        self.previous_response_id.as_deref()
    }

    /// Set the previous response ID manually.
    pub fn set_previous_response_id(&mut self, id: impl Into<String>) {
        self.previous_response_id = Some(id.into());
    }

    /// Clear the previous response ID.
    pub fn clear_previous_response_id(&mut self) {
        self.previous_response_id = None;
    }

    /// Get the message history.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Add a message to history.
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Clear message history.
    pub fn clear_history(&mut self) {
        self.messages.clear();
    }

    /// Get mutable reference to session config.
    pub fn session_config_mut(&mut self) -> &mut SessionConfig {
        &mut self.session_config
    }

    /// Get reference to session config.
    pub fn session_config(&self) -> &SessionConfig {
        &self.session_config
    }

    /// Add a request hook.
    pub fn add_request_hook(&mut self, hook: Arc<dyn RequestHook>) -> &mut Self {
        self.hooks.add_request_hook(hook);
        self
    }

    /// Add a response hook.
    pub fn add_response_hook(&mut self, hook: Arc<dyn ResponseHook>) -> &mut Self {
        self.hooks.add_response_hook(hook);
        self
    }

    /// Add a stream hook.
    pub fn add_stream_hook(&mut self, hook: Arc<dyn StreamHook>) -> &mut Self {
        self.hooks.add_stream_hook(hook);
        self
    }

    /// Get reference to the hook chain.
    pub fn hooks(&self) -> &HookChain {
        &self.hooks
    }

    /// Get mutable reference to the hook chain.
    pub fn hooks_mut(&mut self) -> &mut HookChain {
        &mut self.hooks
    }

    /// Prepare a request with conversation context.
    ///
    /// This method:
    /// 1. Optionally prepends conversation history to the request
    /// 2. Merges session config into the request
    /// 3. Builds hook context with previous_response_id
    /// 4. Runs request hooks
    ///
    /// # Arguments
    ///
    /// * `request` - The request to prepare
    /// * `model` - The model to use
    /// * `with_history` - Whether to prepend conversation history to the request
    #[must_use = "this returns a Result that must be handled"]
    #[instrument(skip(self, request, model, with_history), fields(conversation_id = %self.id, provider = %model.provider()))]
    pub async fn prepare_request(
        &mut self,
        mut request: GenerateRequest,
        model: &dyn Model,
        with_history: bool,
    ) -> Result<(GenerateRequest, HookContext), HyperError> {
        debug!(
            messages = request.messages.len(),
            with_history, "Preparing request"
        );
        // Update provider/model info from the model
        if self.provider.is_empty() {
            self.provider = model.provider().to_string();
        }
        if self.model_id.is_empty() {
            self.model_id = model.model_name().to_string();
        }

        // Prepend conversation history if requested
        if with_history && !self.messages.is_empty() {
            let mut combined = self.messages.clone();
            combined.extend(request.messages);
            request.messages = combined;
        }

        // Merge session config
        self.session_config.merge_into(&mut request);

        // Build hook context with unique request_id
        let mut hook_ctx =
            HookContext::with_provider(&self.provider, &self.model_id).conversation_id(&self.id);

        // Generate a unique request_id for correlation
        hook_ctx.request_id = Some(generate_request_id());

        if let Some(ref prev_id) = self.previous_response_id {
            hook_ctx = hook_ctx.previous_response_id(prev_id);
        }

        // Run request hooks
        self.hooks
            .run_request_hooks(&mut request, &mut hook_ctx)
            .await?;

        // Track user messages in history (only track new messages, not the ones from history)
        if self.track_history {
            // We need to only track the NEW user messages, not the ones we prepended from history
            let history_len = if with_history { self.messages.len() } else { 0 };
            for msg in request.messages.iter().skip(history_len) {
                if msg.role == Role::User {
                    self.messages.push(msg.clone());
                }
            }
        }

        Ok((request, hook_ctx))
    }

    /// Update context after receiving a response.
    ///
    /// This method:
    /// 1. Updates previous_response_id from the response
    /// 2. Adds assistant message to history (if tracking) with source metadata
    /// 3. Runs response hooks
    ///
    /// The source provider and model are taken from the hook context, ensuring
    /// that the message metadata correctly reflects which provider/model generated it.
    #[must_use = "this returns a Result that must be handled"]
    #[instrument(skip(self, response, hook_ctx), fields(conversation_id = %self.id, response_id = %response.id))]
    pub async fn process_response(
        &mut self,
        response: &mut GenerateResponse,
        hook_ctx: &HookContext,
    ) -> Result<(), HyperError> {
        debug!("Processing response");
        // Update previous response ID
        self.previous_response_id = Some(response.id.clone());

        // Add assistant response to history with source metadata
        if self.track_history {
            let mut assistant_msg = Message::new(Role::Assistant, response.content.clone());
            // Set source metadata from hook context (which has the provider/model info)
            // This ensures thinking signatures can be properly validated in cross-provider scenarios
            assistant_msg.metadata = crate::messages::ProviderMetadata::with_source(
                &hook_ctx.provider,
                &hook_ctx.model_id,
            );
            self.messages.push(assistant_msg);
        }

        // Run response hooks
        self.hooks.run_response_hooks(response, hook_ctx).await?;

        Ok(())
    }

    /// Generate a response with conversation context.
    ///
    /// This method automatically prepends the conversation history to the request,
    /// creating a multi-turn conversation experience. The history includes all
    /// previous user and assistant messages from this conversation.
    ///
    /// This is a convenience method that:
    /// 1. Prepends conversation history to the request
    /// 2. Prepares the request with hooks and session config
    /// 3. Calls the model's generate method
    /// 4. Processes the response with hooks
    ///
    /// For single-turn requests without history, use [`generate_stateless`].
    #[must_use = "this returns a Result that must be handled"]
    #[instrument(skip(self, model, request), fields(conversation_id = %self.id, provider = %model.provider(), model_id = %model.model_name()))]
    pub async fn generate(
        &mut self,
        model: &dyn Model,
        request: GenerateRequest,
    ) -> Result<GenerateResponse, HyperError> {
        debug!("Conversation turn starting");
        let (prepared_request, hook_ctx) = self.prepare_request(request, model, true).await?;
        let mut response = model.generate(prepared_request).await?;
        self.process_response(&mut response, &hook_ctx).await?;
        Ok(response)
    }

    /// Generate a response without auto-attaching conversation history.
    ///
    /// Unlike [`generate`], this method does NOT prepend the conversation history
    /// to the request. Only the messages in the provided request are sent.
    ///
    /// Use this when you want full control over the messages sent to the model,
    /// or when implementing custom history management.
    #[must_use = "this returns a Result that must be handled"]
    pub async fn generate_stateless(
        &mut self,
        model: &dyn Model,
        request: GenerateRequest,
    ) -> Result<GenerateResponse, HyperError> {
        let (prepared_request, hook_ctx) = self.prepare_request(request, model, false).await?;
        let mut response = model.generate(prepared_request).await?;
        self.process_response(&mut response, &hook_ctx).await?;
        Ok(response)
    }

    /// Stream a response with conversation context.
    ///
    /// This method automatically prepends the conversation history to the request,
    /// similar to [`generate`].
    ///
    /// Note: For streaming, hook context is built but response hooks are not
    /// automatically run. The caller should process stream events and call
    /// `process_response` manually with the final response.
    #[must_use = "this returns a Result that must be handled"]
    pub async fn stream(
        &mut self,
        model: &dyn Model,
        request: GenerateRequest,
    ) -> Result<(StreamResponse, HookContext), HyperError> {
        let (prepared_request, hook_ctx) = self.prepare_request(request, model, true).await?;
        let stream = model.stream(prepared_request).await?;
        Ok((stream, hook_ctx))
    }

    /// Stream a response without auto-attaching conversation history.
    ///
    /// Unlike [`stream`], this method does NOT prepend the conversation history
    /// to the request. Only the messages in the provided request are sent.
    #[must_use = "this returns a Result that must be handled"]
    pub async fn stream_stateless(
        &mut self,
        model: &dyn Model,
        request: GenerateRequest,
    ) -> Result<(StreamResponse, HookContext), HyperError> {
        let (prepared_request, hook_ctx) = self.prepare_request(request, model, false).await?;
        let stream = model.stream(prepared_request).await?;
        Ok((stream, hook_ctx))
    }

    /// Switch to a different provider/model for subsequent requests.
    ///
    /// This method sanitizes all history messages for the new provider,
    /// stripping provider-specific content that won't be understood.
    #[instrument(skip(self), fields(conversation_id = %self.id, from_provider = %self.provider, to_provider = %new_provider))]
    pub fn switch_provider(&mut self, new_provider: &str, new_model: &str) {
        debug!(from_model = %self.model_id, to_model = %new_model, "Switching provider");
        // Sanitize all history messages for new provider
        for msg in &mut self.messages {
            msg.convert_for_provider(new_provider, new_model);
        }

        // Clear provider-specific state
        self.previous_response_id = None; // OpenAI-specific, meaningless to others
        self.provider = new_provider.to_string();
        self.model_id = new_model.to_string();
    }

    /// Get the current provider name.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Get the current model ID.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Generate using a different model than the context's default.
    ///
    /// History messages are automatically sanitized for the target model.
    /// This is a temporary switch - the context's default provider is not changed.
    #[must_use = "this returns a Result that must be handled"]
    pub async fn generate_with_model(
        &mut self,
        model: &dyn Model,
        request: GenerateRequest,
    ) -> Result<GenerateResponse, HyperError> {
        // If provider differs, sanitize history
        if model.provider() != self.provider {
            let mut sanitized_history = self.messages.clone();
            for msg in &mut sanitized_history {
                msg.convert_for_provider(model.provider(), model.model_name());
            }

            // Build request with sanitized history
            let mut combined_messages = sanitized_history;
            combined_messages.extend(request.messages.clone());

            let modified_request = GenerateRequest {
                messages: combined_messages,
                ..request
            };

            // Prepare request with temporary provider context
            // Use with_history=false since we've already built the combined messages above
            let original_provider = std::mem::take(&mut self.provider);
            let original_model = std::mem::take(&mut self.model_id);
            self.provider = model.provider().to_string();
            self.model_id = model.model_name().to_string();

            let (prepared_request, hook_ctx) =
                self.prepare_request(modified_request, model, false).await?;

            // Restore original provider context
            self.provider = original_provider;
            self.model_id = original_model;

            let mut response = model.generate(prepared_request).await?;

            // Track response but keep source info from the actual provider
            response.model = model.model_name().to_string();

            // Add assistant response to history with source tracking
            if self.track_history {
                let mut assistant_msg = Message::new(Role::Assistant, response.content.clone());
                assistant_msg.metadata = crate::messages::ProviderMetadata::with_source(
                    model.provider(),
                    model.model_name(),
                );
                self.messages.push(assistant_msg);
            }

            // Run response hooks
            self.hooks
                .run_response_hooks(&mut response, &hook_ctx)
                .await?;

            Ok(response)
        } else {
            self.generate(model, request).await
        }
    }
}

/// Builder for ConversationContext.
#[derive(Debug, Default)]
pub struct ConversationContextBuilder {
    id: Option<String>,
    provider: Option<String>,
    model_id: Option<String>,
    session_config: Option<SessionConfig>,
    track_history: bool,
}

impl ConversationContextBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            track_history: true,
            ..Default::default()
        }
    }

    /// Set the conversation ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the provider name.
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Set the model ID.
    pub fn model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Set the session configuration.
    pub fn session_config(mut self, config: SessionConfig) -> Self {
        self.session_config = Some(config);
        self
    }

    /// Disable history tracking.
    pub fn without_history(mut self) -> Self {
        self.track_history = false;
        self
    }

    /// Build the ConversationContext.
    pub fn build(self) -> ConversationContext {
        let mut ctx = ConversationContext::new();

        if let Some(id) = self.id {
            ctx.id = id;
        }
        if let Some(provider) = self.provider {
            ctx.provider = provider;
        }
        if let Some(model_id) = self.model_id {
            ctx.model_id = model_id;
        }
        if let Some(config) = self.session_config {
            ctx.session_config = config;
        }
        ctx.track_history = self.track_history;

        ctx
    }
}

#[cfg(test)]
#[path = "conversation.test.rs"]
mod tests;
