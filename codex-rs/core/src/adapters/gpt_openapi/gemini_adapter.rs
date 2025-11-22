//! Gemini adapter for OpenAPI-style Gemini models
//!
//! This adapter implements support for Google Gemini models via OpenAI-compatible
//! Chat Completions API format. It supports:
//!
//! - Text and multimodal messages (images)
//! - Standard function calling
//! - Gemini-specific thinking/reasoning with configurable token budgets
//! - Non-streaming mode (streaming support planned for future)
//!
//! # Gemini Thinking Budget
//!
//! Gemini models support a "thinking" parameter that controls reasoning:
//!
//! - **Default (unset)**: Dynamic thinking - model decides when and how much to think
//! - **`budget_tokens = -1`**: Explicit dynamic thinking
//! - **`budget_tokens = 0`**: Disable thinking (Gemini 2.5 Flash only)
//! - **`budget_tokens > 0`**: Fixed token budget (128-32768 for Pro, 0-24576 for Flash)
//!
//! Configure via `ModelParameters.budget_tokens` and `include_thoughts`.

use crate::adapters::AdapterContext;
use crate::adapters::ProviderAdapter;
use crate::adapters::RequestContext;
use crate::adapters::RequestMetadata;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::CodexErr;
use crate::error::Result;
use crate::model_family::derive_default_model_family;
use crate::model_provider_info::ModelProviderInfo;
use codex_protocol::config_types_ext::ModelParameters;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use serde_json::Value as JsonValue;
use serde_json::json;
use uuid::Uuid;

/// Gemini adapter for OpenAPI-compatible Chat Completions format
///
/// This adapter is designed for Gemini models accessed via OpenAI-compatible
/// Chat Completions API endpoints. It performs request/response transformation
/// while preserving Gemini-specific features like thinking budgets.
#[derive(Debug, Clone)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    /// Create a new Gemini adapter instance
    pub fn new() -> Self {
        Self
    }

    /// Build Gemini thinking parameter from ModelParameters
    ///
    /// Returns None if budget_tokens is not set (use Gemini default).
    /// Returns Some with thinking config if budget_tokens is set.
    ///
    /// # Validation
    ///
    /// - Accepts -1 (dynamic thinking)
    /// - Accepts 0 (disable thinking, Flash only)
    /// - Accepts 1-32768 (fixed budget)
    /// - Rejects values < -1 or > 32768
    fn build_thinking_param(params: &ModelParameters) -> Result<Option<JsonValue>> {
        let budget = match params.budget_tokens {
            None => return Ok(None), // Use Gemini default (dynamic)
            Some(b) => b,
        };

        // Validate range (union of Pro and Flash ranges)
        if budget < -1 || budget > 32768 {
            return Err(CodexErr::Fatal(format!(
                "Invalid budget_tokens: {}. Valid range: -1 (dynamic), 0 (disable for Flash), or 1-32768",
                budget
            )));
        }

        Ok(Some(json!({
            "include_thoughts": params.include_thoughts.unwrap_or(true),
            "budget_tokens": budget
        })))
    }

    /// Convert ResponseItem slice to Gemini messages array format
    ///
    /// Uses two-pass scanning to merge items with the same id into single assistant messages:
    /// - First pass: Collect all assistant items (Message, Reasoning, FunctionCall) by id
    ///   and collect FunctionCallOutput for later ordering
    /// - Second pass: Emit merged messages at first occurrence position of each id,
    ///   followed by sorted tool outputs
    ///
    /// Constraints:
    /// - id MUST exist for assistant Message, FunctionCall, Reasoning (Fatal error if missing)
    /// - Same id: max 1 Message, max 1 Reasoning, multiple FunctionCalls allowed
    /// - Output order matches input order based on first occurrence of each id
    /// - FunctionCallOutput is emitted after corresponding assistant message, sorted by index
    ///
    /// Transforms:
    /// - System instructions → {role: "system", content: "..."}
    /// - Same-id items → merged {role: "assistant", content?, reasoning?, tool_calls?}
    /// - User Message → {role: "user", content: [...]}
    /// - FunctionCallOutput → {role: "tool", tool_call_id, content} (sorted by index)
    fn transform_response_items_to_messages(
        items: &[ResponseItem],
        system_instructions: Option<&str>,
    ) -> Result<Vec<JsonValue>> {
        use std::collections::HashMap;
        use std::collections::HashSet;

        let mut messages = Vec::new();

        // Prepend system message if instructions are provided
        if let Some(instructions) = system_instructions {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }

        // ========== Strong-typed structs ==========

        /// Tool call information (from FunctionCall)
        struct ToolCallInfo {
            index: i32,
            call_id: String,
            name: String,
            arguments: String,
        }

        /// Tool output information (from FunctionCallOutput)
        struct ToolOutputInfo {
            call_id: String,
            output: FunctionCallOutputPayload,
        }

        /// Grouped assistant items with same message id
        struct AssistantGroup {
            message_content: Option<Vec<JsonValue>>,
            reasoning: Option<String>,
            tool_calls: Vec<ToolCallInfo>,
            message_signature: Option<String>,
            call_signatures: HashMap<String, String>,
        }

        // ========== First pass: Collect all assistant items and tool outputs ==========

        let mut groups: HashMap<String, AssistantGroup> = HashMap::new();
        // call_id -> (group_id, index) for FunctionCallOutput ordering
        let mut call_id_to_group: HashMap<String, (String, i32)> = HashMap::new();
        // Pending tool outputs: (group_id, index, info)
        let mut pending_outputs: Vec<(String, i32, ToolOutputInfo)> = Vec::new();

        for item in items {
            match item {
                // Assistant Message - requires id, max 1 per id
                ResponseItem::Message { id, role, content } if role == "assistant" => {
                    let item_id = id
                        .as_ref()
                        .ok_or_else(|| CodexErr::Fatal("Assistant Message missing id".into()))?;

                    let group = groups
                        .entry(item_id.clone())
                        .or_insert_with(|| AssistantGroup {
                            message_content: None,
                            reasoning: None,
                            tool_calls: Vec::new(),
                            message_signature: None,
                            call_signatures: HashMap::new(),
                        });

                    if group.message_content.is_some() {
                        return Err(CodexErr::Fatal(format!(
                            "Duplicate Message for id '{}'",
                            item_id
                        )));
                    }
                    group.message_content = Some(Self::transform_content_items(content)?);
                }

                // Reasoning - requires id, max 1 per id
                // Also extract signatures from encrypted_content JSON
                ResponseItem::Reasoning {
                    id,
                    content,
                    encrypted_content,
                    ..
                } => {
                    if id.is_empty() {
                        return Err(CodexErr::Fatal("Reasoning missing id".into()));
                    }

                    let group = groups.entry(id.clone()).or_insert_with(|| AssistantGroup {
                        message_content: None,
                        reasoning: None,
                        tool_calls: Vec::new(),
                        message_signature: None,
                        call_signatures: HashMap::new(),
                    });

                    // Extract reasoning text if present
                    if let Some(contents) = content {
                        if group.reasoning.is_some() {
                            return Err(CodexErr::Fatal(format!(
                                "Duplicate Reasoning for id '{}'",
                                id
                            )));
                        }

                        let text = contents
                            .iter()
                            .map(|c| match c {
                                ReasoningItemContent::ReasoningText { text }
                                | ReasoningItemContent::Text { text } => text.as_str(),
                            })
                            .collect::<Vec<_>>()
                            .join("");

                        group.reasoning = Some(text);
                    }

                    // Parse signatures from encrypted_content JSON
                    // Format: { "message_signature": "...", "call_signatures": { "call_id": "sig" } }
                    if let Some(enc) = encrypted_content {
                        if let Ok(sig_data) = serde_json::from_str::<JsonValue>(enc) {
                            // Extract message_signature
                            if let Some(msg_sig) = sig_data
                                .get("message_signature")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                            {
                                group.message_signature = Some(msg_sig.to_string());
                            }
                            // Extract call_signatures
                            if let Some(call_sigs) =
                                sig_data.get("call_signatures").and_then(|v| v.as_object())
                            {
                                for (call_id, sig_val) in call_sigs {
                                    if let Some(sig) = sig_val.as_str().filter(|s| !s.is_empty()) {
                                        group
                                            .call_signatures
                                            .insert(call_id.clone(), sig.to_string());
                                    }
                                }
                            }
                        }
                    }
                }

                // FunctionCall - requires id, multiple allowed per id
                ResponseItem::FunctionCall {
                    id,
                    name,
                    arguments,
                    call_id,
                } => {
                    let item_id = id
                        .as_ref()
                        .ok_or_else(|| CodexErr::Fatal("FunctionCall missing id".into()))?;

                    let group = groups
                        .entry(item_id.clone())
                        .or_insert_with(|| AssistantGroup {
                            message_content: None,
                            reasoning: None,
                            tool_calls: Vec::new(),
                            message_signature: None,
                            call_signatures: HashMap::new(),
                        });

                    // Use current length as index (0-based, per-group)
                    let index = group.tool_calls.len() as i32;
                    group.tool_calls.push(ToolCallInfo {
                        index,
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    });

                    // Build call_id -> (group_id, index) mapping for FunctionCallOutput ordering
                    call_id_to_group.insert(call_id.clone(), (item_id.clone(), index));
                }

                // FunctionCallOutput - collect for later ordering
                ResponseItem::FunctionCallOutput { call_id, output } => {
                    if let Some((group_id, index)) = call_id_to_group.get(call_id) {
                        pending_outputs.push((
                            group_id.clone(),
                            *index,
                            ToolOutputInfo {
                                call_id: call_id.clone(),
                                output: output.clone(),
                            },
                        ));
                    } else {
                        // Fallback: call_id has no matching FunctionCall, use -1 index
                        pending_outputs.push((
                            "_unknown_".to_string(),
                            -1,
                            ToolOutputInfo {
                                call_id: call_id.clone(),
                                output: output.clone(),
                            },
                        ));
                    }
                }

                _ => {} // User messages processed in second pass
            }
        }

        // Group tool outputs by group_id and sort by index
        let mut grouped_outputs: HashMap<String, Vec<(i32, ToolOutputInfo)>> = HashMap::new();
        for (group_id, index, info) in pending_outputs {
            grouped_outputs
                .entry(group_id)
                .or_default()
                .push((index, info));
        }
        for outputs in grouped_outputs.values_mut() {
            outputs.sort_by_key(|(idx, _)| *idx);
        }

        // ========== Helper functions ==========

        // Transform FunctionCallOutputPayload to JSON content
        // Prefers content_items (for images) over plain content string
        fn transform_tool_output_content(output: &FunctionCallOutputPayload) -> JsonValue {
            if let Some(items) = &output.content_items {
                let mapped: Vec<JsonValue> = items
                    .iter()
                    .map(|it| match it {
                        FunctionCallOutputContentItem::InputText { text } => {
                            json!({"type": "text", "text": text})
                        }
                        FunctionCallOutputContentItem::InputImage { image_url } => {
                            json!({"type": "image_url", "image_url": {"url": image_url}})
                        }
                    })
                    .collect();
                json!(mapped)
            } else {
                json!(output.content)
            }
        }

        // Build merged assistant message with signatures
        fn build_merged_message(group: &AssistantGroup) -> JsonValue {
            let mut msg = json!({ "role": "assistant" });
            let obj = msg.as_object_mut().expect("json object");

            // content (from Message)
            if let Some(content) = &group.message_content {
                obj.insert("content".to_string(), json!(content));
            } else {
                obj.insert("content".to_string(), json!(null));
            }

            // reasoning_content (from Reasoning) - Gemini uses "reasoning_content" field
            if let Some(reasoning) = &group.reasoning {
                obj.insert("reasoning_content".to_string(), json!(reasoning));
            }

            // message-level signature
            if let Some(sig) = &group.message_signature {
                obj.insert("signature".to_string(), json!(sig));
            }

            // tool_calls with per-call signatures
            if !group.tool_calls.is_empty() {
                let tool_calls_json: Vec<JsonValue> = group
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        let mut tc_obj = json!({
                            "index": tc.index,
                            "id": tc.call_id,
                            "type": "function",
                            "function": { "name": tc.name, "arguments": tc.arguments }
                        });
                        // Add signature if exists for this call_id
                        if let Some(sig) = group.call_signatures.get(&tc.call_id) {
                            if let Some(obj) = tc_obj.as_object_mut() {
                                obj.insert("signature".to_string(), json!(sig));
                            }
                        }
                        tc_obj
                    })
                    .collect();
                obj.insert("tool_calls".to_string(), json!(tool_calls_json));
            }

            msg
        }

        // ========== Second pass: Emit messages in original order ==========

        let mut emitted_ids: HashSet<String> = HashSet::new();

        for item in items {
            match item {
                // User Message - emit directly
                ResponseItem::Message { role, content, .. } if role != "assistant" => {
                    messages.push(json!({
                        "role": role,
                        "content": Self::transform_content_items(content)?
                    }));
                }

                // Assistant Message - emit merged on first occurrence, followed by tool outputs
                ResponseItem::Message {
                    id: Some(item_id),
                    role,
                    ..
                } if role == "assistant" => {
                    if !emitted_ids.contains(item_id) {
                        emitted_ids.insert(item_id.clone());
                        if let Some(group) = groups.get(item_id) {
                            messages.push(build_merged_message(group));

                            // Emit tool outputs in index order
                            if let Some(outputs) = grouped_outputs.get(item_id) {
                                for (index, info) in outputs {
                                    messages.push(json!({
                                        "role": "tool",
                                        "index": index,
                                        "tool_call_id": info.call_id,
                                        "content": transform_tool_output_content(&info.output)
                                    }));
                                }
                            }
                        }
                    }
                }

                // Reasoning - emit merged on first occurrence, followed by tool outputs
                ResponseItem::Reasoning { id, .. } if !id.is_empty() => {
                    if !emitted_ids.contains(id) {
                        emitted_ids.insert(id.clone());
                        if let Some(group) = groups.get(id) {
                            messages.push(build_merged_message(group));

                            // Emit tool outputs in index order
                            if let Some(outputs) = grouped_outputs.get(id) {
                                for (index, info) in outputs {
                                    messages.push(json!({
                                        "role": "tool",
                                        "index": index,
                                        "tool_call_id": info.call_id,
                                        "content": transform_tool_output_content(&info.output)
                                    }));
                                }
                            }
                        }
                    }
                }

                // FunctionCall - emit merged on first occurrence, followed by tool outputs
                ResponseItem::FunctionCall {
                    id: Some(item_id), ..
                } => {
                    if !emitted_ids.contains(item_id) {
                        emitted_ids.insert(item_id.clone());
                        if let Some(group) = groups.get(item_id) {
                            messages.push(build_merged_message(group));

                            // Emit tool outputs in index order
                            if let Some(outputs) = grouped_outputs.get(item_id) {
                                for (index, info) in outputs {
                                    messages.push(json!({
                                        "role": "tool",
                                        "index": index,
                                        "tool_call_id": info.call_id,
                                        "content": transform_tool_output_content(&info.output)
                                    }));
                                }
                            }
                        }
                    }
                }

                // FunctionCallOutput - already handled above, skip
                ResponseItem::FunctionCallOutput { .. } => continue,

                _ => continue,
            }
        }

        // Emit any orphan tool outputs (call_id not matching any FunctionCall)
        if let Some(outputs) = grouped_outputs.get("_unknown_") {
            for (_, info) in outputs {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": info.call_id,
                    "content": transform_tool_output_content(&info.output)
                }));
            }
        }

        Ok(messages)
    }

    /// Transform ContentItem slice to Gemini content format
    ///
    /// Supports:
    /// - Text (InputText, OutputText)
    /// - Images (InputImage with data URL or https URL)
    fn transform_content_items(items: &[ContentItem]) -> Result<Vec<JsonValue>> {
        let mut content = Vec::new();

        for item in items {
            match item {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                    content.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
                ContentItem::InputImage { image_url } => {
                    content.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": image_url
                        }
                    }));
                }
            }
        }

        Ok(content)
    }

    /// Parse complete (non-streaming) Chat Completions JSON response
    ///
    /// Extracts:
    /// 1. Validates finish_reason (only "stop" or "tool_calls" allowed)
    /// 2. Reasoning content (if present) → OutputItemDone(Reasoning)
    /// 3. Tool calls (if present) → OutputItemDone(FunctionCall) for each
    /// 4. Message content (if present and non-empty) → OutputItemDone(Message)
    /// 5. Token usage → Completed event
    ///
    /// All items from the same message share the response id as their item id.
    fn parse_complete_chat_json(body: &str) -> Result<Vec<ResponseEvent>> {
        let data: JsonValue = serde_json::from_str(body)?;
        let mut events = Vec::new();

        // Check for error
        if let Some(error) = data.get("error") {
            return Err(Self::parse_gemini_error(error)?);
        }

        // Extract choices array with strict validation
        let choices = data
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                CodexErr::Stream(
                    "Missing or invalid 'choices' array in response".into(),
                    None,
                )
            })?;

        // Ensure choices array is not empty
        if choices.is_empty() {
            return Err(CodexErr::Stream(
                "Empty 'choices' array in response".into(),
                None,
            ));
        }

        let choice = &choices[0];

        // 1. Validate finish_reason (Fatal error for invalid values)
        let finish_reason = choice
            .get("finish_reason")
            .and_then(|r| r.as_str())
            .unwrap_or("");

        if finish_reason != "stop" && finish_reason != "tool_calls" {
            return Err(CodexErr::Fatal(format!(
                "Invalid finish_reason: '{}'. Expected 'stop' or 'tool_calls'",
                finish_reason
            )));
        }

        // Extract message from choices[0]
        let message = choice.get("message").ok_or_else(|| {
            CodexErr::Stream("Missing 'message' field in choices[0]".into(), None)
        })?;

        // 2. Extract message_id from response (used for all items)
        let message_id = data
            .get("id")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // 3. Parse content (always generate Message, even if empty)
        // Use empty string as default if content is null or missing
        let content_str = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
            id: Some(message_id.clone()),
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: content_str.to_string(),
            }],
        }));

        // 4. Parse reasoning_content and signatures (message + tool_calls)
        // Note: Gemini uses "reasoning_content" not "reasoning"
        // Signatures are stored as JSON in encrypted_content:
        // { "message_signature": "...", "call_signatures": { "call_id": "sig", ... } }
        let reasoning_text = message
            .get("reasoning_content")
            .and_then(|r| r.as_str())
            .unwrap_or("");

        // Extract message-level signature
        let message_signature = message
            .get("signature")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // 5. Parse tool calls with signatures (merged logic to ensure call_id consistency)
        // This fixes a bug where empty call_id would cause signature loss:
        // - Previously: signature extraction skipped empty call_id, but FunctionCall generated UUID
        // - Now: unified call_id generation ensures signature uses the same ID
        let mut call_signatures: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut function_calls: Vec<ResponseItem> = Vec::new();

        if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
            for (idx, tool_call) in tool_calls.iter().enumerate() {
                // Unified call_id generation (generate UUID if empty/missing)
                let call_id = tool_call
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("call_{}", Uuid::new_v4()));

                // Extract signature using the same call_id (including generated UUID)
                if let Some(sig) = tool_call
                    .get("signature")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    call_signatures.insert(call_id.clone(), sig.to_string());
                }

                // Parse function info
                let function = tool_call.get("function").ok_or_else(|| {
                    CodexErr::Stream(
                        format!("Missing 'function' field in tool_calls[{}]", idx),
                        None,
                    )
                })?;

                let name = function
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| {
                        CodexErr::Stream(
                            format!("Missing 'name' in tool_calls[{}].function", idx),
                            None,
                        )
                    })?;

                let arguments = function
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");

                function_calls.push(ResponseItem::FunctionCall {
                    id: Some(message_id.clone()),
                    name: name.to_string(),
                    call_id,
                    arguments: arguments.to_string(),
                });
            }
        }

        // Build JSON for encrypted_content if any signature exists
        let encrypted_content = if message_signature.is_some() || !call_signatures.is_empty() {
            Some(
                json!({
                    "message_signature": message_signature,
                    "call_signatures": call_signatures
                })
                .to_string(),
            )
        } else {
            None
        };

        // Push Reasoning first (maintains original event order)
        if !reasoning_text.is_empty() || encrypted_content.is_some() {
            events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                id: message_id.clone(),
                summary: vec![ReasoningItemReasoningSummary::SummaryText {
                    text: reasoning_text.to_string(),
                }],
                content: None,
                encrypted_content,
            }));
        }

        // Then push all FunctionCalls
        for fc in function_calls {
            events.push(ResponseEvent::OutputItemDone(fc));
        }

        // 6. Extract token usage
        let token_usage = data.get("usage").map(|u| {
            crate::protocol::TokenUsage {
                input_tokens: u.get("prompt_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                cached_input_tokens: 0, // Gemini may not report this
                output_tokens: u
                    .get("completion_tokens")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0),
                reasoning_output_tokens: u
                    .get("reasoning_tokens")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0),
                total_tokens: u.get("total_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
            }
        });

        // 7. Completion event
        events.push(ResponseEvent::Completed {
            response_id: message_id,
            token_usage,
        });

        Ok(events)
    }

    /// Parse Gemini error response and classify into appropriate CodexErr
    fn parse_gemini_error(error: &JsonValue) -> Result<CodexErr> {
        let code = error.get("code").and_then(|c| c.as_str()).unwrap_or("");
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");

        Ok(match code {
            "context_length_exceeded" | "invalid_argument"
                if message.to_lowercase().contains("context") =>
            {
                CodexErr::ContextWindowExceeded
            }
            "resource_exhausted" | "insufficient_quota" => CodexErr::QuotaExceeded,
            "unauthenticated" | "permission_denied" => {
                CodexErr::Fatal(format!("Authentication error: {}", message))
            }
            _ => CodexErr::Stream(message.to_string(), None),
        })
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini_openapi"
    }

    fn supports_previous_response_id(&self) -> bool {
        // Gemini Chat API doesn't support conversation continuity via response_id
        false
    }

    fn validate_provider(&self, provider: &ModelProviderInfo) -> Result<()> {
        // Require Chat API (not Responses API)
        if provider.wire_api != crate::model_provider_info::WireApi::Chat {
            return Err(CodexErr::Fatal(format!(
                "GeminiAdapter requires wire_api = \"chat\". \
                 Current configuration uses wire_api = \"{:?}\". \
                 Please update your config to set wire_api = \"chat\" \
                 for provider '{}'.",
                provider.wire_api, provider.name
            )));
        }

        // Only support non-streaming for initial version
        if provider.ext.streaming {
            return Err(CodexErr::Fatal(
                "GeminiAdapter: streaming mode not yet supported. \
                 Set streaming = false in provider configuration."
                    .into(),
            ));
        }

        Ok(())
    }

    fn build_request_metadata(
        &self,
        _prompt: &Prompt,
        _provider: &ModelProviderInfo,
        _context: &RequestContext,
    ) -> Result<RequestMetadata> {
        // No special headers needed for Gemini Chat API
        Ok(RequestMetadata::default())
    }

    fn transform_request(
        &self,
        prompt: &Prompt,
        context: &RequestContext,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue> {
        let model = provider.ext.model_name.as_ref().ok_or_else(|| {
            CodexErr::Fatal(
                "Provider must specify model_name when using gemini_openapi adapter".into(),
            )
        })?;

        // Log input size and last item type
        let input_size = prompt.input.len();
        let last_item_type = prompt
            .input
            .last()
            .map(crate::adapters::get_item_type_name)
            .unwrap_or_else(|| "None".to_string());
        tracing::debug!(
            "Gemini adapter transform_request: input_size={}, last_item_type={}",
            input_size,
            last_item_type
        );

        // Get model_family: provider's own (derived from model_name) OR default (with BASE_INSTRUCTIONS)
        let default_family = derive_default_model_family("");
        let model_family = provider
            .ext
            .model_family
            .as_ref()
            .unwrap_or(&default_family);

        // Get system instructions with proper fallback:
        // 1. User override (Config.base_instructions) - highest priority
        // 2. Model family base_instructions (e.g., Gemini-specific or BASE_INSTRUCTIONS)
        let system_instructions = prompt.get_full_instructions(model_family);

        // Transform messages with system instructions
        let messages = Self::transform_response_items_to_messages(
            &prompt.input,
            Some(system_instructions.as_ref()),
        )?;

        // Build base request
        let mut request = json!({
            "model": model,
            "messages": messages,
            "stream": false
        });

        // Add tools if present
        let tools = crate::tools::spec::create_tools_json_for_chat_completions_api(&prompt.tools)?;
        if !tools.is_empty() {
            request["tools"] = json!(tools);
            request["tool_choice"] = json!("auto");
        }

        // Add thinking parameter (Gemini-specific)
        let params = &context.effective_parameters;
        if let Some(thinking) = Self::build_thinking_param(params)? {
            request["thinking"] = thinking;
        }

        // Add standard parameters
        if let Some(temp) = params.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = params.max_tokens {
            request["max_tokens"] = json!(max_tokens);
        }
        if let Some(top_p) = params.top_p {
            request["top_p"] = json!(top_p);
        }

        // Add output schema if present (response_format)
        if let Some(schema) = &prompt.output_schema {
            request["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "codex_output_schema",
                    "schema": schema,
                    "strict": true
                }
            });
        }

        Ok(request)
    }

    fn transform_response_chunk(
        &self,
        chunk: &str,
        _context: &mut AdapterContext,
        _provider: &ModelProviderInfo,
    ) -> Result<Vec<ResponseEvent>> {
        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Non-streaming mode: parse complete JSON
        Self::parse_complete_chat_json(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::FunctionCallOutputPayload;

    // ========== Provider Validation ==========

    #[test]
    fn test_validate_provider() {
        let adapter = GeminiAdapter::new();

        // Valid config: Chat API + non-streaming
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Chat;
        provider.ext.streaming = false;
        assert!(adapter.validate_provider(&provider).is_ok());

        // Invalid: Responses API
        provider.wire_api = crate::model_provider_info::WireApi::Responses;
        assert!(adapter.validate_provider(&provider).is_err());

        // Invalid: streaming enabled
        provider.wire_api = crate::model_provider_info::WireApi::Chat;
        provider.ext.streaming = true;
        assert!(adapter.validate_provider(&provider).is_err());
    }

    // ========== Thinking Parameter ==========

    #[test]
    fn test_thinking_param() {
        // None → use Gemini default
        let params = ModelParameters::default();
        assert!(
            GeminiAdapter::build_thinking_param(&params)
                .unwrap()
                .is_none()
        );

        // Dynamic thinking (-1)
        let mut params = ModelParameters::default();
        params.budget_tokens = Some(-1);
        let result = GeminiAdapter::build_thinking_param(&params)
            .unwrap()
            .unwrap();
        assert_eq!(result["budget_tokens"], -1);

        // Invalid range
        params.budget_tokens = Some(-2);
        assert!(GeminiAdapter::build_thinking_param(&params).is_err());
        params.budget_tokens = Some(50000);
        assert!(GeminiAdapter::build_thinking_param(&params).is_err());
    }

    // ========== Request Transform ==========

    #[test]
    fn test_transform_messages() {
        // User text message
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Hello".to_string(),
            }],
        }];
        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["text"], "Hello");

        // With system instructions
        let messages =
            GeminiAdapter::transform_response_items_to_messages(&items, Some("You are helpful"))
                .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn test_transform_tool_call_and_output() {
        // FunctionCall
        let items = vec![ResponseItem::FunctionCall {
            id: Some("resp-1".to_string()),
            name: "search".to_string(),
            call_id: "call_1".to_string(),
            arguments: "{}".to_string(),
        }];
        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "search");

        // FunctionCallOutput
        let items = vec![ResponseItem::FunctionCallOutput {
            call_id: "call_1".to_string(),
            output: FunctionCallOutputPayload {
                content: "result".to_string(),
                content_items: None,
                success: Some(true),
            },
        }];
        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["tool_call_id"], "call_1");
    }

    // ========== Response Parsing ==========

    #[test]
    fn test_parse_basic_response() {
        let json = r#"{
            "id": "resp-1",
            "choices": [{"message": {"role": "assistant", "content": "Hello"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
        }"#;

        let events = GeminiAdapter::parse_complete_chat_json(json).unwrap();
        assert_eq!(events.len(), 2); // Message + Completed

        match &events[0] {
            ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                match &content[0] {
                    ContentItem::OutputText { text } => assert_eq!(text, "Hello"),
                    _ => panic!("Expected OutputText"),
                }
            }
            _ => panic!("Expected Message"),
        }

        match &events[1] {
            ResponseEvent::Completed { token_usage, .. } => {
                let usage = token_usage.as_ref().unwrap();
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 20);
            }
            _ => panic!("Expected Completed"),
        }
    }

    #[test]
    fn test_parse_tool_call_response() {
        // With tool_calls, content is null → empty Message is still generated
        let json = r#"{
            "id": "resp-1",
            "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
                {"call_id": "call_1", "type": "function", "function": {"name": "search", "arguments": "{}"}}
            ]}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 10, "total_tokens": 15}
        }"#;

        let events = GeminiAdapter::parse_complete_chat_json(json).unwrap();
        assert_eq!(events.len(), 3); // Message (empty) + FunctionCall + Completed

        // Empty content Message
        match &events[0] {
            ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                match &content[0] {
                    ContentItem::OutputText { text } => assert_eq!(text, ""),
                    _ => panic!("Expected OutputText"),
                }
            }
            _ => panic!("Expected Message"),
        }

        // FunctionCall
        match &events[1] {
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { name, call_id, .. }) => {
                assert_eq!(name, "search");
                assert_eq!(call_id, "call_1");
            }
            _ => panic!("Expected FunctionCall"),
        }
    }

    #[test]
    fn test_parse_tool_call_generates_missing_id() {
        // Missing tool call id → generate UUID
        let json = r#"{
            "id": "resp-1",
            "choices": [{"message": {"role": "assistant", "tool_calls": [
                {"type": "function", "function": {"name": "test", "arguments": "{}"}}
            ]}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 10, "total_tokens": 15}
        }"#;

        let events = GeminiAdapter::parse_complete_chat_json(json).unwrap();
        match &events[1] {
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, .. }) => {
                assert!(call_id.starts_with("call_"));
            }
            _ => panic!("Expected FunctionCall"),
        }
    }

    // ========== Error Handling ==========

    #[test]
    fn test_parse_errors() {
        // Context exceeded (code + message contains "context")
        let json =
            r#"{"error": {"code": "context_length_exceeded", "message": "context exceeded"}}"#;
        assert!(matches!(
            GeminiAdapter::parse_complete_chat_json(json).unwrap_err(),
            CodexErr::ContextWindowExceeded
        ));

        // Quota exceeded
        let json = r#"{"error": {"code": "resource_exhausted", "message": "quota"}}"#;
        assert!(matches!(
            GeminiAdapter::parse_complete_chat_json(json).unwrap_err(),
            CodexErr::QuotaExceeded
        ));

        // Missing choices
        let json = r#"{"id": "test"}"#;
        assert!(GeminiAdapter::parse_complete_chat_json(json).is_err());
    }

    // ========== Two-Pass Merging ==========

    #[test]
    fn test_merge_same_id_items() {
        // Message + Reasoning + FunctionCall with same id → single merged message
        let items = vec![
            ResponseItem::Message {
                id: Some("id-A".to_string()),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "Hello".to_string(),
                }],
            },
            ResponseItem::Reasoning {
                id: "id-A".to_string(),
                summary: vec![],
                content: Some(vec![ReasoningItemContent::ReasoningText {
                    text: "Thinking...".to_string(),
                }]),
                encrypted_content: None,
            },
            ResponseItem::FunctionCall {
                id: Some("id-A".to_string()),
                name: "test".to_string(),
                call_id: "call_1".to_string(),
                arguments: "{}".to_string(),
            },
        ];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"][0]["text"], "Hello");
        assert_eq!(messages[0]["reasoning_content"], "Thinking...");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    }

    #[test]
    fn test_merge_multiple_function_calls() {
        // Multiple FunctionCalls with same id → merged tool_calls array
        let items = vec![
            ResponseItem::FunctionCall {
                id: Some("id-B".to_string()),
                name: "func_1".to_string(),
                call_id: "call_1".to_string(),
                arguments: "{}".to_string(),
            },
            ResponseItem::FunctionCall {
                id: Some("id-B".to_string()),
                name: "func_2".to_string(),
                call_id: "call_2".to_string(),
                arguments: "{}".to_string(),
            },
        ];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        let tool_calls = messages[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 2);
    }

    #[test]
    fn test_transform_missing_id_errors() {
        // Assistant Message without id → error
        let items = vec![ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: "Hello".to_string(),
            }],
        }];
        assert!(GeminiAdapter::transform_response_items_to_messages(&items, None).is_err());

        // FunctionCall without id → error
        let items = vec![ResponseItem::FunctionCall {
            id: None,
            name: "test".to_string(),
            call_id: "call_1".to_string(),
            arguments: "{}".to_string(),
        }];
        assert!(GeminiAdapter::transform_response_items_to_messages(&items, None).is_err());
    }

    // ========== FunctionCallOutput Ordering ==========

    #[test]
    fn test_function_call_output_sorted_by_index() {
        // FunctionCallOutput items out-of-order → should be sorted by index
        let items = vec![
            // FunctionCalls first (index 0, 1, 2)
            ResponseItem::FunctionCall {
                id: Some("resp-1".to_string()),
                name: "func_a".to_string(),
                call_id: "call_0".to_string(),
                arguments: "{}".to_string(),
            },
            ResponseItem::FunctionCall {
                id: Some("resp-1".to_string()),
                name: "func_b".to_string(),
                call_id: "call_1".to_string(),
                arguments: "{}".to_string(),
            },
            ResponseItem::FunctionCall {
                id: Some("resp-1".to_string()),
                name: "func_c".to_string(),
                call_id: "call_2".to_string(),
                arguments: "{}".to_string(),
            },
            // FunctionCallOutput out-of-order (2, 0, 1)
            ResponseItem::FunctionCallOutput {
                call_id: "call_2".to_string(),
                output: FunctionCallOutputPayload {
                    content: "result_2".to_string(),
                    content_items: None,
                    success: Some(true),
                },
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call_0".to_string(),
                output: FunctionCallOutputPayload {
                    content: "result_0".to_string(),
                    content_items: None,
                    success: Some(true),
                },
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call_1".to_string(),
                output: FunctionCallOutputPayload {
                    content: "result_1".to_string(),
                    content_items: None,
                    success: Some(true),
                },
            },
        ];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();

        // Should have: 1 assistant message + 3 tool messages
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "assistant");

        // Tool outputs should be in index order (0, 1, 2)
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_0");
        assert_eq!(messages[1]["index"], 0);

        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
        assert_eq!(messages[2]["index"], 1);

        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call_2");
        assert_eq!(messages[3]["index"], 2);
    }

    #[test]
    fn test_function_call_output_content_items() {
        // FunctionCallOutput with content_items (image) should be preserved
        let items = vec![
            ResponseItem::FunctionCall {
                id: Some("resp-1".to_string()),
                name: "screenshot".to_string(),
                call_id: "call_img".to_string(),
                arguments: "{}".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call_img".to_string(),
                output: FunctionCallOutputPayload {
                    content: "fallback text".to_string(),
                    content_items: Some(vec![
                        FunctionCallOutputContentItem::InputText {
                            text: "Caption".to_string(),
                        },
                        FunctionCallOutputContentItem::InputImage {
                            image_url: "data:image/png;base64,abc123".to_string(),
                        },
                    ]),
                    success: Some(true),
                },
            },
        ];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 2);

        // Tool output should use content_items, not fallback content
        let tool_content = messages[1]["content"].as_array().unwrap();
        assert_eq!(tool_content.len(), 2);
        assert_eq!(tool_content[0]["type"], "text");
        assert_eq!(tool_content[0]["text"], "Caption");
        assert_eq!(tool_content[1]["type"], "image_url");
        assert_eq!(
            tool_content[1]["image_url"]["url"],
            "data:image/png;base64,abc123"
        );
    }

    #[test]
    fn test_orphan_function_call_output() {
        // FunctionCallOutput without matching FunctionCall → fallback handling
        let items = vec![ResponseItem::FunctionCallOutput {
            call_id: "orphan_call".to_string(),
            output: FunctionCallOutputPayload {
                content: "orphan result".to_string(),
                content_items: None,
                success: Some(true),
            },
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["tool_call_id"], "orphan_call");
        assert_eq!(messages[0]["content"], "orphan result");
    }
}
