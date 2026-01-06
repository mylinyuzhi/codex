//! Type conversion between codex-api and google-genai types.
//!
//! **IMPORTANT**: This module must align with `google-genai/src/chat.rs` to ensure
//! round-trip consistency. The sendback format from this adapter should match what
//! `chat.rs` produces when building conversation history.
//!
//! See: `google-genai/src/chat.rs` for reference implementation of:
//! - `send_function_response_with_id()` - FunctionResponse with call ID pairing
//! - `send_function_responses_with_ids()` - Multiple function responses
//! - History management via `add_to_history()` / `curated_history`
//!
//! # Conversion Rules
//!
//! ## Input (codex-api → google-genai)
//!
//! | codex-api ResponseItem | google-genai Content/Part |
//! |------------------------|---------------------------|
//! | Message(role="user")   | Content(role="user", parts=[Part::text]) |
//! | Message(role="assistant") | Content(role="model", parts=[Part::text]) |
//! | FunctionCall           | Part::function_call (in model Content) |
//! | FunctionCallOutput     | Content(role="user", parts=[Part::function_response]) |
//! | Reasoning              | Part with thought=true, thought_signature |
//!
//! ## Output (google-genai → codex-api)
//!
//! | google-genai Part      | codex-api ResponseItem |
//! |------------------------|------------------------|
//! | Part.text (thought=false) | Message(role="assistant", OutputText) |
//! | Part.function_call     | FunctionCall |
//! | Part.thought=true      | Reasoning |

use crate::common::Prompt;
use crate::common::ResponseEvent;
use base64::Engine;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use google_genai::types::Content;
use google_genai::types::FunctionCall;
use google_genai::types::FunctionDeclaration;
use google_genai::types::FunctionResponse;
use google_genai::types::GenerateContentResponse;
use google_genai::types::Part;
use google_genai::types::Schema;
use google_genai::types::SchemaType;
use std::collections::HashMap;

/// Convert a Prompt to a list of Gemini Contents.
///
/// This is a complex function that handles multiple concerns:
///
/// ## Alignment with chat.rs
/// The output format must match what `google-genai/src/chat.rs` produces when building
/// `curated_history`. Specifically:
/// - FunctionCall → Content(role="model", Part::function_call with id)
/// - FunctionCallOutput → Content(role="user", Part::function_response with id)
/// - Signatures are attached via `Part::thought_signature` for round-trip preservation
///
/// ## Merging Strategy
/// Parts are merged into Contents based on:
/// 1. **Role**: Parts with the same role are grouped together
/// 2. **response_id**: Model parts from the same response_id are grouped together
///
/// ## Signature Application
/// Signatures stored in `Reasoning.encrypted_content` are applied to parts:
/// - If `parts_order` is present: Use type-based lookup (new approach)
/// - Otherwise: Use position-based lookup (backwards compatibility)
///
/// ## FunctionCall/FunctionCallOutput Pairing
/// - **Server-provided call_ids**: Preserved as-is for FunctionResponse.id
/// - **Client-generated call_ids** (prefix `call_genai_@cligen_`): Stripped (set to None)
///   because Gemini API doesn't recognize them. Matching is by order in this case.
///
/// ## Order Tracking
/// For client-generated call_ids, we track the order of FunctionCalls and verify that
/// FunctionCallOutputs arrive in the same order for correct pairing.
pub fn prompt_to_contents(prompt: &Prompt) -> Vec<Content> {
    let mut contents: Vec<Content> = Vec::new();
    let mut current_parts: Vec<Part> = Vec::new();
    let mut current_role: Option<String> = None;
    // Track response_id to merge parts from the same model response
    let mut current_response_id: Option<String> = None;

    // ========== Pre-scan: Build FunctionCall order map for client-generated IDs ==========
    // When server doesn't provide FunctionCall.id, we generate client-side IDs with prefix
    // `call_genai_@cligen_`. These must be stripped when creating FunctionResponse because
    // Gemini API doesn't recognize them. Instead, we rely on order-based matching.
    let mut client_call_id_order: HashMap<String, usize> = HashMap::new();
    let mut client_call_index: usize = 0;

    for item in &prompt.input {
        if let ResponseItem::FunctionCall { call_id, .. } = item {
            if is_client_generated_call_id(call_id) {
                client_call_id_order.insert(call_id.clone(), client_call_index);
                client_call_index += 1;
            }
        }
    }

    // Track which client-generated FunctionCallOutput we're processing (for order verification)
    let mut client_output_index: usize = 0;

    // ========== Pre-scan: Collect signatures and parts_order ==========
    // Track part signatures and order for type-based application
    let mut part_signatures: Vec<Option<String>> = Vec::new();
    let mut parts_order: Vec<String> = Vec::new();

    // Track occurrence counts per part type for signature lookup (new type-based approach)
    let mut text_occurrence: usize = 0;
    let mut thought_occurrence: usize = 0;
    let mut function_call_occurrence: usize = 0;

    // Track global part index for fallback position-based lookup (backwards compatibility)
    let mut part_index: usize = 0;

    // Pre-collect parts_order and part_signatures from Reasoning items
    for item in &prompt.input {
        if let ResponseItem::Reasoning {
            encrypted_content: Some(enc),
            ..
        } = item
        {
            if let Ok(sig_data) = serde_json::from_str::<serde_json::Value>(enc) {
                // Extract parts_order for type-based signature lookup
                if let Some(order) = sig_data.get("parts_order").and_then(|v| v.as_array()) {
                    for o in order {
                        parts_order.push(o.as_str().unwrap_or("").to_string());
                    }
                }
                // Extract part_signatures
                if let Some(sigs) = sig_data.get("part_signatures").and_then(|v| v.as_array()) {
                    for sig_val in sigs {
                        let sig = sig_val.as_str().map(|s| s.to_string());
                        part_signatures.push(sig);
                    }
                }
            }
        }
    }

    // Check if we have parts_order for type-based lookup, otherwise fall back to position-based
    let use_type_based_lookup = !parts_order.is_empty();

    // Helper to get signature by part type and occurrence index (new type-based approach)
    // This finds the nth occurrence of part_type in parts_order and returns its signature
    let get_sig_for_part = |part_type: &str,
                            occurrence: usize,
                            order: &[String],
                            sigs: &[Option<String>]|
     -> Option<Vec<u8>> {
        let mut count = 0;
        for (i, pt) in order.iter().enumerate() {
            if pt == part_type {
                if count == occurrence {
                    return sigs
                        .get(i)
                        .and_then(|s| s.as_ref())
                        .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
                }
                count += 1;
            }
        }
        None
    };

    // Helper to get signature by position (backwards compatibility fallback)
    let get_sig_at = |index: usize, sigs: &[Option<String>]| -> Option<Vec<u8>> {
        sigs.get(index)
            .and_then(|s| s.as_ref())
            .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
    };

    // Helper to flush accumulated parts into a Content
    let flush = |contents: &mut Vec<Content>, parts: &mut Vec<Part>, role: &Option<String>| {
        if !parts.is_empty() {
            contents.push(Content {
                parts: Some(std::mem::take(parts)),
                role: role.clone(),
            });
        }
    };

    // ========== Main processing loop ==========
    for item in &prompt.input {
        match item {
            ResponseItem::Message {
                id: item_id,
                role,
                content,
            } => {
                let gemini_role = if role == "assistant" {
                    "model".to_string()
                } else {
                    role.clone()
                };
                let is_model = gemini_role == "model";

                // Flush if role changes OR if response_id changes (for model role)
                // This ensures parts from the same model response are grouped together
                let should_flush = current_role.as_ref() != Some(&gemini_role)
                    || (is_model
                        && item_id.is_some()
                        && current_response_id.as_ref() != item_id.as_ref());

                if should_flush {
                    flush(&mut contents, &mut current_parts, &current_role);
                    current_role = Some(gemini_role.clone());
                    if is_model {
                        current_response_id = item_id.clone();
                    }
                }

                // Convert content items to parts
                // Only apply signatures for model role (signatures are from model response)
                for content_item in content {
                    match content_item {
                        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                            let mut part = Part::text(text);
                            // Apply signature (only for model role)
                            if is_model {
                                let sig_bytes = if use_type_based_lookup {
                                    get_sig_for_part(
                                        "text",
                                        text_occurrence,
                                        &parts_order,
                                        &part_signatures,
                                    )
                                } else {
                                    // Fallback to position-based lookup for backwards compatibility
                                    get_sig_at(part_index, &part_signatures)
                                };
                                if let Some(sig) = sig_bytes {
                                    part.thought_signature = Some(sig);
                                }
                                text_occurrence += 1;
                                part_index += 1;
                            }
                            current_parts.push(part);
                        }
                        ContentItem::InputImage { image_url } => {
                            // Handle data URLs or regular URLs
                            let part = if let Some(data_url) = parse_data_url(image_url) {
                                // Use inline_data with the base64 string directly
                                Part {
                                    inline_data: Some(google_genai::types::Blob::new(
                                        &data_url.base64_data,
                                        &data_url.mime_type,
                                    )),
                                    ..Default::default()
                                }
                            } else {
                                // Regular URL - use file_data
                                Part::from_uri(image_url, "image/*")
                            };
                            // Note: Images don't have signatures in parts_order tracking
                            current_parts.push(part);
                        }
                    }
                }
            }

            ResponseItem::FunctionCall {
                id: item_id,
                name,
                arguments,
                call_id,
            } => {
                // Function calls belong to model role
                // Flush if role changes OR if response_id changes
                let should_flush = current_role.as_ref() != Some(&"model".to_string())
                    || (item_id.is_some()
                        && current_response_id.as_ref() != item_id.as_ref());

                if should_flush {
                    flush(&mut contents, &mut current_parts, &current_role);
                    current_role = Some("model".to_string());
                    current_response_id = item_id.clone();
                }

                // Parse arguments as JSON
                let args = serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);

                // Build Part with function_call
                let mut part = Part {
                    function_call: Some(FunctionCall {
                        id: Some(call_id.clone()),
                        name: Some(name.clone()),
                        args: Some(args),
                        partial_args: None,
                        will_continue: None,
                    }),
                    ..Default::default()
                };

                // Apply signature
                let sig_bytes = if use_type_based_lookup {
                    get_sig_for_part(
                        "function_call",
                        function_call_occurrence,
                        &parts_order,
                        &part_signatures,
                    )
                } else {
                    // Fallback to position-based lookup for backwards compatibility
                    get_sig_at(part_index, &part_signatures)
                };
                if let Some(sig) = sig_bytes {
                    part.thought_signature = Some(sig);
                }
                function_call_occurrence += 1;
                part_index += 1;

                current_parts.push(part);
            }

            ResponseItem::FunctionCallOutput { call_id, output } => {
                // Function outputs are user role - flush current model parts first
                flush(&mut contents, &mut current_parts, &current_role);
                current_role = Some("user".to_string());
                current_response_id = None; // Reset response_id for user role

                // Convert output to response value, preferring content_items for multimodal
                let response_value = if let Some(items) = &output.content_items {
                    // Multimodal content - map to array of parts
                    let mapped: Vec<serde_json::Value> = items
                        .iter()
                        .map(|item| match item {
                            FunctionCallOutputContentItem::InputText { text } => {
                                serde_json::json!({"type": "text", "text": text})
                            }
                            FunctionCallOutputContentItem::InputImage { image_url } => {
                                serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {"url": image_url}
                                })
                            }
                        })
                        .collect();
                    serde_json::json!(mapped)
                } else {
                    // Plain text - try to parse as JSON, otherwise wrap
                    serde_json::from_str(&output.content)
                        .unwrap_or_else(|_| serde_json::json!({ "result": output.content.clone() }))
                };

                // Determine the FunctionResponse.id to use:
                // - Server-provided call_ids: Keep as-is for proper pairing
                // - Client-generated call_ids (prefix `call_genai_@cligen_`): Strip (use None)
                //   because Gemini API doesn't recognize them. Matching is by order instead.
                let response_id = if is_client_generated_call_id(call_id) {
                    // Verify order matches expected position (for debugging/validation)
                    // FunctionCallOutputs should arrive in same order as FunctionCalls
                    if let Some(&expected_idx) = client_call_id_order.get(call_id) {
                        if expected_idx != client_output_index {
                            // Order mismatch warning - this shouldn't happen in normal usage
                            // but we continue anyway as order-based matching is best effort
                            tracing::warn!(
                                "FunctionCallOutput order mismatch: expected index {}, got {} for call_id {}",
                                expected_idx,
                                client_output_index,
                                call_id
                            );
                        }
                    }
                    client_output_index += 1;
                    None // Strip client-generated ID - Gemini will match by order
                } else {
                    Some(call_id.clone()) // Keep server-provided ID for exact matching
                };

                current_parts.push(Part {
                    function_response: Some(FunctionResponse {
                        id: response_id,
                        name: None, // Name is optional in response
                        response: Some(response_value),
                        will_continue: None,
                        scheduling: None,
                        parts: None,
                    }),
                    ..Default::default()
                });

                // Flush immediately after function response
                flush(&mut contents, &mut current_parts, &current_role);
                current_role = None;
            }

            ResponseItem::Reasoning {
                id: item_id,
                summary: _,
                content,
                encrypted_content: _,
            } => {
                // Reasoning belongs to model role
                // Flush if role changes OR if response_id changes
                let should_flush = current_role.as_ref() != Some(&"model".to_string())
                    || current_response_id.as_ref() != Some(item_id);

                if should_flush {
                    flush(&mut contents, &mut current_parts, &current_role);
                    current_role = Some("model".to_string());
                    current_response_id = Some(item_id.clone());
                }

                // Convert reasoning content to thought parts
                if let Some(content_items) = content {
                    for item in content_items {
                        match item {
                            ReasoningItemContent::ReasoningText { text }
                            | ReasoningItemContent::Text { text } => {
                                let mut part = Part {
                                    text: Some(text.clone()),
                                    thought: Some(true),
                                    ..Default::default()
                                };

                                // Apply signature
                                let sig_bytes = if use_type_based_lookup {
                                    get_sig_for_part(
                                        "thought",
                                        thought_occurrence,
                                        &parts_order,
                                        &part_signatures,
                                    )
                                } else {
                                    // Fallback to position-based lookup for backwards compatibility
                                    get_sig_at(part_index, &part_signatures)
                                };
                                if let Some(sig) = sig_bytes {
                                    part.thought_signature = Some(sig);
                                }
                                thought_occurrence += 1;
                                part_index += 1;

                                current_parts.push(part);
                            }
                        }
                    }
                }
            }

            // Skip items that don't translate to Gemini format
            ResponseItem::LocalShellCall { .. }
            | ResponseItem::CustomToolCall { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::GhostSnapshot { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::Other => {}
        }
    }

    // Flush remaining parts
    flush(&mut contents, &mut current_parts, &current_role);

    contents
}

/// Convert Gemini response to codex-api ResponseEvents.
///
/// **Alignment with chat.rs**: The output ResponseItems must be convertible back to
/// Contents that match `chat.rs` format. Key points:
/// - FunctionCall.call_id uses `call_genai_@cligen_{UUID}` prefix when server doesn't provide ID
/// - Signatures are stored in Reasoning.encrypted_content as JSON for round-trip
///
/// Returns (events, response_id) where response_id is generated for the response.
pub fn response_to_events(response: &GenerateContentResponse) -> (Vec<ResponseEvent>, String) {
    let mut events = Vec::new();

    // Use server-provided response_id if available, otherwise generate one
    let response_id = response.response_id.clone().unwrap_or_else(generate_uuid);

    // Get parts from first candidate
    let Some(parts) = response.parts() else {
        // Even with no parts, emit Created and Completed events
        events.push(ResponseEvent::Created);
        events.push(ResponseEvent::Completed {
            response_id: response_id.clone(),
            token_usage: extract_usage(response),
        });
        return (events, response_id);
    };

    // Emit Created event first
    events.push(ResponseEvent::Created);

    // Collect parts by type
    let mut text_parts: Vec<String> = Vec::new();
    let mut reasoning_texts: Vec<String> = Vec::new();
    let mut part_signatures: Vec<Option<String>> = Vec::new();
    let mut parts_order: Vec<String> = Vec::new(); // Track part types in original order
    let mut function_calls: Vec<ResponseItem> = Vec::new();

    for part in parts {
        // Collect signature for every part (in order, None if absent)
        let sig = part
            .thought_signature
            .as_ref()
            .map(|s| base64::engine::general_purpose::STANDARD.encode(s));
        part_signatures.push(sig);

        // Track part type in original order for signature roundtrip
        // Priority: thought > function_call > text (mutually exclusive tracking)
        if part.thought == Some(true) {
            parts_order.push("thought".to_string());
            if let Some(text) = &part.text {
                reasoning_texts.push(text.clone());
            }
        } else if part.function_call.is_some() {
            parts_order.push("function_call".to_string());
            let fc = part.function_call.as_ref().unwrap();
            let call_id = fc.id.clone().unwrap_or_else(generate_call_id);
            let name = fc.name.clone().unwrap_or_default();
            let arguments = fc
                .args
                .as_ref()
                .map(|a| serde_json::to_string(a).unwrap_or_default())
                .unwrap_or_default();

            function_calls.push(ResponseItem::FunctionCall {
                id: Some(response_id.clone()), // Use response_id as message_id
                name,
                arguments,
                call_id,
            });
        } else if part.text.is_some() {
            parts_order.push("text".to_string());
            text_parts.push(part.text.as_ref().unwrap().clone());
        }
    }

    // Emit message item first (if we have text content)
    if !text_parts.is_empty() {
        events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
            id: Some(response_id.clone()),
            role: "assistant".to_string(),
            content: text_parts
                .iter()
                .map(|t| ContentItem::OutputText { text: t.clone() })
                .collect(),
        }));
    }

    // Check if any part has a signature
    let has_signatures = part_signatures.iter().any(|s| s.is_some());

    // Emit reasoning item (if we have reasoning content or signatures)
    if !reasoning_texts.is_empty() || has_signatures {
        let summary: Vec<ReasoningItemReasoningSummary> = reasoning_texts
            .iter()
            .map(|t| ReasoningItemReasoningSummary::SummaryText { text: t.clone() })
            .collect();

        let content: Option<Vec<ReasoningItemContent>> = if !reasoning_texts.is_empty() {
            Some(
                reasoning_texts
                    .iter()
                    .map(|t| ReasoningItemContent::ReasoningText { text: t.clone() })
                    .collect(),
            )
        } else {
            None
        };

        // Build encrypted_content with parts_order and part_signatures for round-trip
        // parts_order tracks the original part types in order for signature alignment
        let encrypted_content = if has_signatures || !parts_order.is_empty() {
            Some(
                serde_json::json!({
                    "parts_order": parts_order,
                    "part_signatures": part_signatures
                })
                .to_string(),
            )
        } else {
            None
        };

        events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
            id: response_id.clone(), // Use same response_id for consistency
            summary,
            content,
            encrypted_content,
        }));
    }

    // Emit function calls
    for fc in function_calls {
        events.push(ResponseEvent::OutputItemDone(fc));
    }

    // Emit Completed event at the end
    events.push(ResponseEvent::Completed {
        response_id: response_id.clone(),
        token_usage: extract_usage(response),
    });

    (events, response_id)
}

/// Extract token usage from Gemini response.
pub fn extract_usage(response: &GenerateContentResponse) -> Option<TokenUsage> {
    let usage = response.usage_metadata.as_ref()?;

    Some(TokenUsage {
        input_tokens: usage.prompt_token_count.unwrap_or(0) as i64,
        cached_input_tokens: usage.cached_content_token_count.unwrap_or(0) as i64,
        output_tokens: usage.candidates_token_count.unwrap_or(0) as i64,
        reasoning_output_tokens: usage.thoughts_token_count.unwrap_or(0) as i64,
        total_tokens: usage.total_token_count.unwrap_or(0) as i64,
    })
}

/// Convert a JSON tool definition to Gemini FunctionDeclaration.
pub fn tool_json_to_declaration(tool: &serde_json::Value) -> Option<FunctionDeclaration> {
    // Handle OpenAI-style function tool format
    let function = if tool.get("type").and_then(|t| t.as_str()) == Some("function") {
        tool.get("function")?
    } else {
        tool
    };

    let name = function.get("name")?.as_str()?;
    let description = function.get("description").and_then(|d| d.as_str());

    let mut decl = FunctionDeclaration::new(name);

    if let Some(desc) = description {
        decl = decl.with_description(desc);
    }

    // Convert parameters schema
    if let Some(params) = function.get("parameters") {
        if let Some(schema) = json_schema_to_gemini(params) {
            decl = decl.with_parameters(schema);
        }
    }

    Some(decl)
}

/// Convert JSON Schema to Gemini Schema.
fn json_schema_to_gemini(json: &serde_json::Value) -> Option<Schema> {
    let schema_type = match json.get("type").and_then(|t| t.as_str()) {
        Some("string") => SchemaType::String,
        Some("number") => SchemaType::Number,
        Some("integer") => SchemaType::Integer,
        Some("boolean") => SchemaType::Boolean,
        Some("array") => SchemaType::Array,
        Some("object") => SchemaType::Object,
        Some("null") => SchemaType::Null,
        _ => return None,
    };

    let mut schema = Schema {
        schema_type: Some(schema_type),
        ..Default::default()
    };

    // Add description
    if let Some(desc) = json.get("description").and_then(|d| d.as_str()) {
        schema.description = Some(desc.to_string());
    }

    // Handle enum values
    if let Some(enum_vals) = json.get("enum").and_then(|e| e.as_array()) {
        schema.enum_values = Some(
            enum_vals
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }

    // Handle object properties
    if let Some(props) = json.get("properties").and_then(|p| p.as_object()) {
        let mut properties = HashMap::new();
        for (key, value) in props {
            if let Some(prop_schema) = json_schema_to_gemini(value) {
                properties.insert(key.clone(), prop_schema);
            }
        }
        if !properties.is_empty() {
            schema.properties = Some(properties);
        }
    }

    // Handle required fields
    if let Some(required) = json.get("required").and_then(|r| r.as_array()) {
        schema.required = Some(
            required
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }

    // Handle array items
    if let Some(items) = json.get("items") {
        if let Some(items_schema) = json_schema_to_gemini(items) {
            schema.items = Some(Box::new(items_schema));
        }
    }

    Some(schema)
}

/// Parse a data URL into mime type and base64 data string.
/// Returns (mime_type, base64_data) if valid.
struct DataUrl {
    mime_type: String,
    base64_data: String,
}

fn parse_data_url(url: &str) -> Option<DataUrl> {
    if !url.starts_with("data:") {
        return None;
    }

    let rest = url.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;

    let mime_type = if header.contains(';') {
        header.split(';').next()?.to_string()
    } else {
        header.to_string()
    };

    // Only support base64-encoded data URLs
    if !header.contains("base64") {
        return None;
    }

    Some(DataUrl {
        mime_type,
        base64_data: data.to_string(),
    })
}

/// Prefix for client-generated call IDs (distinguishes from server-provided IDs).
const CLIENT_GENERATED_CALL_ID_PREFIX: &str = "call_genai_@cligen_";

/// Check if a call_id was generated by the client (adapter).
pub fn is_client_generated_call_id(call_id: &str) -> bool {
    call_id.starts_with(CLIENT_GENERATED_CALL_ID_PREFIX)
}

/// Generate a call_id with special prefix to mark it as client-generated.
fn generate_call_id() -> String {
    format!(
        "{}{}",
        CLIENT_GENERATED_CALL_ID_PREFIX,
        uuid::Uuid::new_v4()
    )
}

/// Generate a proper UUID v4 string (for response_id, not call_id).
fn generate_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::FunctionCallOutputContentItem;
    use codex_protocol::models::FunctionCallOutputPayload;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_prompt_to_contents_simple_message() {
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Hello".to_string(),
                }],
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, Some("user".to_string()));
        let parts = contents[0].parts.as_ref().unwrap();
        assert_eq!(parts[0].text, Some("Hello".to_string()));
    }

    #[test]
    fn test_prompt_to_contents_assistant_becomes_model() {
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "Hi there".to_string(),
                }],
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, Some("model".to_string()));
    }

    #[test]
    fn test_prompt_to_contents_function_call() {
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![ResponseItem::FunctionCall {
                id: None,
                name: "get_weather".to_string(),
                arguments: r#"{"location":"Tokyo"}"#.to_string(),
                call_id: "call-123".to_string(),
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, Some("model".to_string()));
        let parts = contents[0].parts.as_ref().unwrap();
        let fc = parts[0].function_call.as_ref().unwrap();
        assert_eq!(fc.id, Some("call-123".to_string()));
        assert_eq!(fc.name, Some("get_weather".to_string()));
    }

    #[test]
    fn test_prompt_to_contents_function_output() {
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![ResponseItem::FunctionCallOutput {
                call_id: "call-123".to_string(),
                output: FunctionCallOutputPayload {
                    content: r#"{"temp": 20}"#.to_string(),
                    content_items: None,
                    success: Some(true),
                },
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, Some("user".to_string()));
        let parts = contents[0].parts.as_ref().unwrap();
        let fr = parts[0].function_response.as_ref().unwrap();
        assert_eq!(fr.id, Some("call-123".to_string()));
    }

    #[test]
    fn test_prompt_to_contents_function_output_multimodal() {
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![ResponseItem::FunctionCallOutput {
                call_id: "call-img".to_string(),
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
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        assert_eq!(contents.len(), 1);
        let parts = contents[0].parts.as_ref().unwrap();
        let fr = parts[0].function_response.as_ref().unwrap();
        assert_eq!(fr.id, Some("call-img".to_string()));

        // Verify response uses content_items, not fallback
        let response = fr.response.as_ref().unwrap();
        let items = response.as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["type"], "text");
        assert_eq!(items[0]["text"], "Caption");
        assert_eq!(items[1]["type"], "image_url");
        assert_eq!(items[1]["image_url"]["url"], "data:image/png;base64,abc123");
    }

    #[test]
    fn test_tool_json_to_declaration() {
        let tool = serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city name"
                        }
                    },
                    "required": ["location"]
                }
            }
        });

        let decl = tool_json_to_declaration(&tool).unwrap();

        assert_eq!(decl.name, Some("get_weather".to_string()));
        assert_eq!(
            decl.description,
            Some("Get the weather for a location".to_string())
        );
        assert!(decl.parameters.is_some());
    }

    #[test]
    fn test_parse_data_url() {
        let url = "data:image/png;base64,iVBORw0KGgo=";
        let result = parse_data_url(url).unwrap();

        assert_eq!(result.mime_type, "image/png");
        assert_eq!(result.base64_data, "iVBORw0KGgo=");
    }

    #[test]
    fn test_parse_non_data_url_returns_none() {
        let url = "https://example.com/image.png";
        assert!(parse_data_url(url).is_none());
    }

    #[test]
    fn test_extract_usage() {
        let response = GenerateContentResponse {
            candidates: None,
            prompt_feedback: None,
            usage_metadata: Some(google_genai::types::UsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(20),
                total_token_count: Some(30),
                cached_content_token_count: None,
                thoughts_token_count: None,
                tool_use_prompt_token_count: None,
                prompt_tokens_details: None,
                cache_tokens_details: None,
                candidates_tokens_details: None,
            }),
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        let usage = extract_usage(&response).unwrap();

        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    // ========== P2 Tests: call_id format and round-trip consistency ==========

    #[test]
    fn test_generated_call_id_has_prefix() {
        let call_id = generate_call_id();
        assert!(call_id.starts_with("call_genai_@cligen_"));
        assert!(is_client_generated_call_id(&call_id));
    }

    #[test]
    fn test_server_call_id_not_detected_as_client() {
        assert!(!is_client_generated_call_id("server_call_123"));
        assert!(!is_client_generated_call_id("call_abc")); // No @cligen
        assert!(!is_client_generated_call_id("genai_call_123")); // Wrong prefix
    }

    #[test]
    fn test_round_trip_single_function_call() {
        // Input: ResponseItem::FunctionCall with call_id "call_abc"
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![ResponseItem::FunctionCall {
                id: Some("resp-1".to_string()),
                name: "get_weather".to_string(),
                arguments: r#"{"location":"Tokyo"}"#.to_string(),
                call_id: "call_abc".to_string(),
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        // Convert to Contents
        let contents = prompt_to_contents(&prompt);

        // Verify Part.function_call.id == "call_abc"
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, Some("model".to_string()));
        let parts = contents[0].parts.as_ref().unwrap();
        assert_eq!(parts.len(), 1);
        let fc = parts[0].function_call.as_ref().unwrap();
        assert_eq!(fc.id, Some("call_abc".to_string()));
        assert_eq!(fc.name, Some("get_weather".to_string()));
    }

    #[test]
    fn test_round_trip_multiple_function_calls_with_signatures() {
        use base64::Engine;

        // Create Reasoning with encrypted_content containing part_signatures (position-based)
        // Signatures are applied by position: index 0 -> first part, index 1 -> second part
        let sig_1_b64 = base64::engine::general_purpose::STANDARD.encode(b"sig_1");
        let sig_2_b64 = base64::engine::general_purpose::STANDARD.encode(b"sig_2");

        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                // Reasoning with part_signatures for position-based application
                ResponseItem::Reasoning {
                    id: "resp-1".to_string(),
                    summary: vec![],
                    content: None,
                    encrypted_content: Some(
                        serde_json::json!({
                            "part_signatures": [sig_1_b64, sig_2_b64]
                        })
                        .to_string(),
                    ),
                },
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_a".to_string(),
                    arguments: "{}".to_string(),
                    call_id: "call_1".to_string(),
                },
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_b".to_string(),
                    arguments: "{}".to_string(),
                    call_id: "call_2".to_string(),
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Should have model content with function calls
        assert!(!contents.is_empty());
        let model_content = contents
            .iter()
            .find(|c| c.role == Some("model".to_string()));
        assert!(model_content.is_some());

        let parts = model_content.unwrap().parts.as_ref().unwrap();

        // Find function call parts and verify signatures by position
        let fc_parts: Vec<_> = parts.iter().filter(|p| p.function_call.is_some()).collect();
        assert_eq!(fc_parts.len(), 2);

        // First function call (position 0) gets sig_1
        assert_eq!(
            fc_parts[0].thought_signature,
            Some(b"sig_1".to_vec()),
            "First function call should have signature sig_1"
        );

        // Second function call (position 1) gets sig_2
        assert_eq!(
            fc_parts[1].thought_signature,
            Some(b"sig_2".to_vec()),
            "Second function call should have signature sig_2"
        );
    }

    #[test]
    fn test_function_call_missing_id_gets_client_generated() {
        use google_genai::types::Candidate;

        // Create GenerateContentResponse with FunctionCall.id = None
        let response = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![Part {
                        function_call: Some(FunctionCall {
                            id: None, // No server-provided ID
                            name: Some("get_weather".to_string()),
                            args: Some(serde_json::json!({"location": "Tokyo"})),
                            partial_args: None,
                            will_continue: None,
                        }),
                        ..Default::default()
                    }]),
                }),
                finish_reason: None,
                safety_ratings: None,
                index: None,
                token_count: None,
                avg_logprobs: None,
                citation_metadata: None,
                finish_message: None,
                grounding_metadata: None,
                logprobs_result: None,
            }]),
            prompt_feedback: None,
            usage_metadata: None,
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        // Convert via response_to_events
        let (events, _response_id) = response_to_events(&response);

        // Find FunctionCall event
        let fc_event = events.iter().find(|e| {
            matches!(
                e,
                crate::common::ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })
            )
        });
        assert!(fc_event.is_some(), "Should have FunctionCall event");

        if let crate::common::ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
            call_id,
            ..
        }) = fc_event.unwrap()
        {
            // Verify call_id starts with client-generated prefix
            assert!(
                call_id.starts_with("call_genai_@cligen_"),
                "Missing server ID should get client-generated prefix, got: {}",
                call_id
            );
        } else {
            panic!("Expected FunctionCall event");
        }
    }

    #[test]
    fn test_multi_turn_conversation_consistency() {
        use base64::Engine;

        // Turn 1: User "Hello" → Model "Let me search" + FunctionCall(get_weather)
        // Turn 2: FunctionCallOutput(temp=20) → continue conversation

        // Base64 encode signatures for proper roundtrip
        // Position 0: text part from Message, Position 1: function_call part
        let sig_weather_b64 = base64::engine::general_purpose::STANDARD.encode(b"sig_weather");

        let prompt = Prompt {
            instructions: "You are a helpful assistant.".to_string(),
            input: vec![
                // User message
                ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText {
                        text: "What's the weather in Tokyo?".to_string(),
                    }],
                },
                // Model response with function call
                ResponseItem::Message {
                    id: Some("resp-1".to_string()),
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: "Let me check the weather for you.".to_string(),
                    }],
                },
                // Reasoning with part_signatures (position-based)
                // Position 0: text part (no sig), Position 1: function_call part (sig_weather)
                ResponseItem::Reasoning {
                    id: "resp-1".to_string(),
                    summary: vec![],
                    content: None,
                    encrypted_content: Some(
                        serde_json::json!({
                            "part_signatures": [null, sig_weather_b64]
                        })
                        .to_string(),
                    ),
                },
                // Function call
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "get_weather".to_string(),
                    arguments: r#"{"location":"Tokyo"}"#.to_string(),
                    call_id: "call_weather".to_string(),
                },
                // Function output
                ResponseItem::FunctionCallOutput {
                    call_id: "call_weather".to_string(),
                    output: FunctionCallOutputPayload {
                        content: r#"{"temperature": 20, "condition": "sunny"}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        // Convert to Contents
        let contents = prompt_to_contents(&prompt);

        // Verify structure:
        // 1. User content with question
        // 2. Model content with text + function call with signature
        // 3. User content with function response

        // Should have at least 3 contents (user, model, user for function response)
        assert!(contents.len() >= 2, "Should have multiple contents");

        // First content should be user role
        assert_eq!(contents[0].role, Some("user".to_string()));

        // Model content should have function call with signature
        let model_content = contents
            .iter()
            .find(|c| c.role == Some("model".to_string()));
        assert!(model_content.is_some(), "Should have model content");

        let model_parts = model_content.unwrap().parts.as_ref().unwrap();
        let fc_part = model_parts.iter().find(|p| p.function_call.is_some());
        assert!(fc_part.is_some(), "Model should have function call");

        // Verify function call has signature from part_signatures (position 1)
        let fc_part = fc_part.unwrap();
        assert_eq!(
            fc_part.thought_signature,
            Some(b"sig_weather".to_vec()),
            "Function call should have its signature"
        );

        // Verify function response is in user role
        let fn_response_content = contents.iter().find(|c| {
            c.parts.as_ref().map_or(false, |parts| {
                parts.iter().any(|p| p.function_response.is_some())
            })
        });
        assert!(
            fn_response_content.is_some(),
            "Should have function response"
        );
        assert_eq!(
            fn_response_content.unwrap().role,
            Some("user".to_string()),
            "Function response should be in user role"
        );
    }

    #[test]
    fn test_sendback_format_matches_chat_rs() {
        // Build a history that matches what chat.rs would produce
        // via send_function_response_with_id

        // Create FunctionCall response then convert back to Contents
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                // Previous model response with function call
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "get_weather".to_string(),
                    arguments: r#"{"location":"Tokyo"}"#.to_string(),
                    call_id: "call_123".to_string(),
                },
                // Function response (like chat.rs send_function_response_with_id)
                ResponseItem::FunctionCallOutput {
                    call_id: "call_123".to_string(),
                    output: FunctionCallOutputPayload {
                        content: r#"{"temp": 20}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // chat.rs would produce:
        // - Content(role="model", parts=[Part{function_call: {id: "call_123", ...}}])
        // - Content(role="user", parts=[Part{function_response: {id: "call_123", ...}}])

        // Verify model content with function_call
        let model_content = contents
            .iter()
            .find(|c| c.role == Some("model".to_string()));
        assert!(model_content.is_some());
        let model_parts = model_content.unwrap().parts.as_ref().unwrap();
        let fc = model_parts
            .iter()
            .find(|p| p.function_call.is_some())
            .unwrap()
            .function_call
            .as_ref()
            .unwrap();
        assert_eq!(fc.id, Some("call_123".to_string()));

        // Verify user content with function_response (matches chat.rs format)
        let user_content = contents
            .iter()
            .find(|c| {
                c.role == Some("user".to_string())
                    && c.parts.as_ref().map_or(false, |p| {
                        p.iter().any(|part| part.function_response.is_some())
                    })
            })
            .expect("Should have user content with function_response");

        let fr_part = user_content
            .parts
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.function_response.is_some())
            .unwrap();
        let fr = fr_part.function_response.as_ref().unwrap();
        assert_eq!(
            fr.id,
            Some("call_123".to_string()),
            "FunctionResponse.id should match call_id"
        );
    }

    // ========== Python SDK Alignment Tests ==========

    #[test]
    fn test_reasoning_roundtrip_with_part_signatures() {
        use google_genai::types::Candidate;

        // Simulate Gemini response with parts ordered to match emission order:
        // Emission order: Message (text), Reasoning (thought), FunctionCall
        // So parts should be: [text, thought, function_call]
        // This ensures position-based signature matching works in roundtrip
        let gemini_response = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![
                        // Text output (position 0, no signature)
                        Part {
                            text: Some("Here's what I found.".to_string()),
                            ..Default::default()
                        },
                        // Thought part with signature (position 1)
                        Part {
                            thought: Some(true),
                            thought_signature: Some(b"thought_sig".to_vec()),
                            text: Some("I'm reasoning about this...".to_string()),
                            ..Default::default()
                        },
                        // Function call part with its own signature (position 2)
                        Part {
                            function_call: Some(FunctionCall {
                                id: Some("call_1".to_string()),
                                name: Some("search".to_string()),
                                args: Some(serde_json::json!({"query": "rust programming"})),
                                partial_args: None,
                                will_continue: None,
                            }),
                            thought_signature: Some(b"fc_sig".to_vec()),
                            ..Default::default()
                        },
                    ]),
                }),
                ..Default::default()
            }]),
            prompt_feedback: None,
            usage_metadata: None,
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        // Convert Gemini response -> codex-api events
        let (events, response_id) = response_to_events(&gemini_response);

        // Verify we get expected events
        assert!(!response_id.is_empty(), "Should generate response_id");

        // Extract ResponseItems from events
        let items: Vec<ResponseItem> = events
            .iter()
            .filter_map(|e| match e {
                ResponseEvent::OutputItemDone(item) => Some(item.clone()),
                _ => None,
            })
            .collect();

        // Should have: Message (text), Reasoning (thought), FunctionCall
        assert!(
            items.len() >= 2,
            "Should have multiple items, got: {:?}",
            items.len()
        );

        // Find Reasoning item and verify encrypted_content has part_signatures
        let reasoning = items
            .iter()
            .find(|item| matches!(item, ResponseItem::Reasoning { .. }));
        assert!(reasoning.is_some(), "Should have Reasoning item");

        if let Some(ResponseItem::Reasoning {
            encrypted_content, ..
        }) = reasoning
        {
            assert!(encrypted_content.is_some(), "Should have encrypted_content");
            let enc = encrypted_content.as_ref().unwrap();
            let sig_data: serde_json::Value = serde_json::from_str(enc).expect("valid JSON");

            // Verify part_signatures is present as array
            let part_sigs = sig_data.get("part_signatures").and_then(|v| v.as_array());
            assert!(part_sigs.is_some(), "Should have part_signatures array");

            // Should have 3 entries (for 3 parts)
            let part_sigs = part_sigs.unwrap();
            assert_eq!(part_sigs.len(), 3, "Should have 3 part signatures");

            // Position 0 (text) should be null, positions 1 and 2 should have signatures
            assert!(part_sigs[0].is_null(), "Position 0 (text) should be null");
            assert!(
                part_sigs[1].is_string(),
                "Position 1 (thought) should have signature"
            );
            assert!(
                part_sigs[2].is_string(),
                "Position 2 (fc) should have signature"
            );
        }

        // Now convert back: codex-api items -> Gemini Contents
        let prompt = Prompt {
            instructions: String::new(),
            input: items,
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Find model content with function call
        let model_content = contents
            .iter()
            .find(|c| c.role == Some("model".to_string()));
        assert!(
            model_content.is_some(),
            "Should have model content in roundtrip"
        );

        let model_parts = model_content.unwrap().parts.as_ref().unwrap();

        // Find function call part and verify signature is preserved
        let fc_part = model_parts.iter().find(|p| p.function_call.is_some());
        assert!(fc_part.is_some(), "Should have function call part");

        let fc_part = fc_part.unwrap();
        // The signature should be preserved in the roundtrip
        // Position 2 in part_signatures has fc_sig
        assert_eq!(
            fc_part.thought_signature,
            Some(b"fc_sig".to_vec()),
            "Function call should preserve its signature in roundtrip"
        );
    }

    #[test]
    fn test_parallel_function_calls_response() {
        use google_genai::types::Candidate;

        // Gemini response with multiple parallel function calls
        let gemini_response = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![
                        Part {
                            function_call: Some(FunctionCall {
                                id: Some("call_a".to_string()),
                                name: Some("get_weather".to_string()),
                                args: Some(serde_json::json!({"city": "Tokyo"})),
                                partial_args: None,
                                will_continue: None,
                            }),
                            ..Default::default()
                        },
                        Part {
                            function_call: Some(FunctionCall {
                                id: Some("call_b".to_string()),
                                name: Some("get_time".to_string()),
                                args: Some(serde_json::json!({"timezone": "JST"})),
                                partial_args: None,
                                will_continue: None,
                            }),
                            ..Default::default()
                        },
                    ]),
                }),
                ..Default::default()
            }]),
            prompt_feedback: None,
            usage_metadata: None,
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        let (events, _) = response_to_events(&gemini_response);

        // Count FunctionCall events
        let fc_events: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })
                )
            })
            .collect();

        assert_eq!(fc_events.len(), 2, "Should have 2 function call events");

        // Verify call_ids are preserved
        let call_ids: Vec<String> = fc_events
            .iter()
            .filter_map(|e| {
                if let ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    call_id, ..
                }) = e
                {
                    Some(call_id.clone())
                } else {
                    None
                }
            })
            .collect();

        assert!(call_ids.contains(&"call_a".to_string()));
        assert!(call_ids.contains(&"call_b".to_string()));
    }

    #[test]
    fn test_function_call_without_server_id() {
        use google_genai::types::Candidate;

        // Gemini response where FunctionCall has no id (server didn't provide one)
        let gemini_response = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![Part {
                        function_call: Some(FunctionCall {
                            id: None, // No server-provided ID
                            name: Some("my_tool".to_string()),
                            args: Some(serde_json::json!({})),
                            partial_args: None,
                            will_continue: None,
                        }),
                        ..Default::default()
                    }]),
                }),
                ..Default::default()
            }]),
            prompt_feedback: None,
            usage_metadata: None,
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        let (events, _) = response_to_events(&gemini_response);

        // Find FunctionCall event
        let fc_event = events.iter().find(|e| {
            matches!(
                e,
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })
            )
        });
        assert!(fc_event.is_some());

        if let Some(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, .. })) =
            fc_event
        {
            // Should have client-generated ID with special prefix
            assert!(
                is_client_generated_call_id(call_id),
                "Missing server ID should result in client-generated ID: {}",
                call_id
            );
            assert!(call_id.starts_with("call_genai_@cligen_"));
        }
    }

    #[test]
    fn test_binary_signature_roundtrip() {
        use google_genai::types::Candidate;

        // Binary signature with non-UTF8 bytes (would be corrupted by from_utf8_lossy)
        let binary_sig = vec![0x00, 0x01, 0xFF, 0xFE, 0x80, 0x90, 0xAB, 0xCD];

        // Create Gemini response with binary signature
        // Parts order: [thought, function_call] -> part_signatures: [sig, sig]
        let gemini_response = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![
                        // Thought part with binary signature (position 0)
                        Part {
                            thought: Some(true),
                            thought_signature: Some(binary_sig.clone()),
                            text: Some("Thinking...".to_string()),
                            ..Default::default()
                        },
                        // Function call with binary signature (position 1)
                        Part {
                            function_call: Some(FunctionCall {
                                id: Some("call_binary".to_string()),
                                name: Some("test_tool".to_string()),
                                args: Some(serde_json::json!({})),
                                partial_args: None,
                                will_continue: None,
                            }),
                            thought_signature: Some(binary_sig.clone()),
                            ..Default::default()
                        },
                    ]),
                }),
                ..Default::default()
            }]),
            prompt_feedback: None,
            usage_metadata: None,
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        // Convert to codex-api events
        let (events, _) = response_to_events(&gemini_response);

        // Extract ResponseItems
        let items: Vec<ResponseItem> = events
            .iter()
            .filter_map(|e| match e {
                ResponseEvent::OutputItemDone(item) => Some(item.clone()),
                _ => None,
            })
            .collect();

        // Find Reasoning item and verify encrypted_content has base64-encoded signatures
        let reasoning = items
            .iter()
            .find(|item| matches!(item, ResponseItem::Reasoning { .. }));
        assert!(reasoning.is_some(), "Should have Reasoning item");

        let encrypted_content = if let ResponseItem::Reasoning {
            encrypted_content, ..
        } = reasoning.unwrap()
        {
            encrypted_content.clone()
        } else {
            panic!("Not a Reasoning item")
        };

        assert!(encrypted_content.is_some(), "Should have encrypted_content");
        let enc = encrypted_content.unwrap();

        // Verify it's valid JSON with part_signatures array
        let sig_data: serde_json::Value = serde_json::from_str(&enc).expect("valid JSON");
        let part_sigs = sig_data
            .get("part_signatures")
            .and_then(|v| v.as_array())
            .expect("Should have part_signatures array");

        // Should have 2 entries (for 2 parts)
        assert_eq!(part_sigs.len(), 2, "Should have 2 part signatures");

        // Both should have signatures
        let sig0_base64 = part_sigs[0]
            .as_str()
            .expect("Position 0 should have signature");
        let sig1_base64 = part_sigs[1]
            .as_str()
            .expect("Position 1 should have signature");

        // Verify base64 can be decoded back to original bytes
        use base64::Engine;
        let decoded_sig0 = base64::engine::general_purpose::STANDARD
            .decode(sig0_base64)
            .expect("decode sig0");
        let decoded_sig1 = base64::engine::general_purpose::STANDARD
            .decode(sig1_base64)
            .expect("decode sig1");

        assert_eq!(
            decoded_sig0, binary_sig,
            "Position 0 signature should roundtrip correctly"
        );
        assert_eq!(
            decoded_sig1, binary_sig,
            "Position 1 signature should roundtrip correctly"
        );

        // Now test the full roundtrip: convert back to Contents
        let prompt = Prompt {
            instructions: String::new(),
            input: items,
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Find model content with function call and verify signature is preserved
        let model_content = contents
            .iter()
            .find(|c| c.role == Some("model".to_string()));
        assert!(model_content.is_some(), "Should have model content");

        let parts = model_content.unwrap().parts.as_ref().unwrap();
        let fc_part = parts.iter().find(|p| p.function_call.is_some());
        assert!(fc_part.is_some(), "Should have function call part");

        // Verify the binary signature was preserved through the full roundtrip
        assert_eq!(
            fc_part.unwrap().thought_signature,
            Some(binary_sig),
            "Binary signature should be preserved through full roundtrip"
        );
    }

    #[test]
    fn test_signature_roundtrip_thought_first_order() {
        use google_genai::types::Candidate;

        // This test verifies that signatures are correctly preserved when the original
        // Gemini part order differs from the emission order.
        //
        // Original order: [thought(sig_A), text(no_sig), function_call(sig_B)]
        // Emission order: [Message(text), Reasoning(thought), FunctionCall]
        //
        // With the old position-based signature lookup, this would fail because:
        // - Message (first emitted) would incorrectly get sig_A
        // - Reasoning (second emitted) would incorrectly get null
        //
        // With the new type-based signature lookup, signatures are correctly matched.
        let gemini_response = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![
                        // THOUGHT FIRST (position 0) - has signature
                        Part {
                            thought: Some(true),
                            thought_signature: Some(b"sig_A_thought".to_vec()),
                            text: Some("I'm thinking about this problem...".to_string()),
                            ..Default::default()
                        },
                        // TEXT SECOND (position 1) - no signature
                        Part {
                            text: Some("Here's my answer.".to_string()),
                            ..Default::default()
                        },
                        // FUNCTION CALL THIRD (position 2) - has signature
                        Part {
                            function_call: Some(FunctionCall {
                                id: Some("call_xyz".to_string()),
                                name: Some("search".to_string()),
                                args: Some(serde_json::json!({"query": "test"})),
                                partial_args: None,
                                will_continue: None,
                            }),
                            thought_signature: Some(b"sig_B_function".to_vec()),
                            ..Default::default()
                        },
                    ]),
                }),
                ..Default::default()
            }]),
            prompt_feedback: None,
            usage_metadata: None,
            model_version: None,
            response_id: None,
            create_time: None,
            sdk_http_response: None,
        };

        // Convert Gemini response -> codex-api events
        let (events, _response_id) = response_to_events(&gemini_response);

        // Extract ResponseItems from events
        let items: Vec<ResponseItem> = events
            .iter()
            .filter_map(|e| match e {
                ResponseEvent::OutputItemDone(item) => Some(item.clone()),
                _ => None,
            })
            .collect();

        // Verify Reasoning item has parts_order stored
        let reasoning = items
            .iter()
            .find(|item| matches!(item, ResponseItem::Reasoning { .. }));
        assert!(reasoning.is_some(), "Should have Reasoning item");

        if let Some(ResponseItem::Reasoning {
            encrypted_content, ..
        }) = reasoning
        {
            let enc = encrypted_content
                .as_ref()
                .expect("Should have encrypted_content");
            let sig_data: serde_json::Value = serde_json::from_str(enc).expect("valid JSON");

            // Verify parts_order is stored
            let parts_order = sig_data.get("parts_order").and_then(|v| v.as_array());
            assert!(parts_order.is_some(), "Should have parts_order array");
            let parts_order = parts_order.unwrap();
            assert_eq!(parts_order.len(), 3, "Should have 3 parts");
            assert_eq!(parts_order[0], "thought", "First part should be thought");
            assert_eq!(parts_order[1], "text", "Second part should be text");
            assert_eq!(
                parts_order[2], "function_call",
                "Third part should be function_call"
            );

            // Verify part_signatures
            let part_sigs = sig_data
                .get("part_signatures")
                .and_then(|v| v.as_array())
                .unwrap();
            assert!(
                part_sigs[0].is_string(),
                "Position 0 (thought) should have signature"
            );
            assert!(part_sigs[1].is_null(), "Position 1 (text) should be null");
            assert!(
                part_sigs[2].is_string(),
                "Position 2 (function_call) should have signature"
            );
        }

        // Now convert back: codex-api items -> Gemini Contents
        let prompt = Prompt {
            instructions: String::new(),
            input: items,
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Find model content
        let model_content = contents
            .iter()
            .find(|c| c.role == Some("model".to_string()));
        assert!(model_content.is_some(), "Should have model content");

        let parts = model_content.unwrap().parts.as_ref().unwrap();

        // Find thought part and verify it has the CORRECT signature (sig_A_thought)
        let thought_part = parts.iter().find(|p| p.thought == Some(true));
        assert!(thought_part.is_some(), "Should have thought part");
        assert_eq!(
            thought_part.unwrap().thought_signature,
            Some(b"sig_A_thought".to_vec()),
            "Thought part should have its original signature"
        );

        // Find text part and verify it has NO signature
        let text_part = parts
            .iter()
            .find(|p| p.text.is_some() && p.thought != Some(true) && p.function_call.is_none());
        assert!(text_part.is_some(), "Should have text part");
        assert_eq!(
            text_part.unwrap().thought_signature,
            None,
            "Text part should have no signature"
        );

        // Find function call part and verify it has the CORRECT signature (sig_B_function)
        let fc_part = parts.iter().find(|p| p.function_call.is_some());
        assert!(fc_part.is_some(), "Should have function call part");
        assert_eq!(
            fc_part.unwrap().thought_signature,
            Some(b"sig_B_function".to_vec()),
            "Function call part should have its original signature"
        );
    }

    // ========== FunctionResponse ID Handling Tests ==========

    #[test]
    fn test_function_response_server_id_preserved() {
        // When call_id is server-provided (no @cligen prefix), FunctionResponse.id should keep it
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "get_weather".to_string(),
                    arguments: r#"{"location":"Tokyo"}"#.to_string(),
                    call_id: "server_call_abc123".to_string(), // Server-provided ID
                },
                ResponseItem::FunctionCallOutput {
                    call_id: "server_call_abc123".to_string(), // Same server-provided ID
                    output: FunctionCallOutputPayload {
                        content: r#"{"temp": 20}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Find the user content with function_response
        let user_content = contents.iter().find(|c| {
            c.role == Some("user".to_string())
                && c.parts.as_ref().map_or(false, |p| {
                    p.iter().any(|part| part.function_response.is_some())
                })
        });
        assert!(
            user_content.is_some(),
            "Should have user content with function_response"
        );

        let fr_part = user_content
            .unwrap()
            .parts
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.function_response.is_some())
            .unwrap();
        let fr = fr_part.function_response.as_ref().unwrap();

        // Server-provided ID should be preserved
        assert_eq!(
            fr.id,
            Some("server_call_abc123".to_string()),
            "FunctionResponse.id should preserve server-provided call_id"
        );
    }

    #[test]
    fn test_function_response_client_id_stripped() {
        // When call_id is client-generated (@cligen prefix), FunctionResponse.id should be None
        let client_call_id = format!("{}test_uuid_12345", CLIENT_GENERATED_CALL_ID_PREFIX);
        assert!(
            is_client_generated_call_id(&client_call_id),
            "Test setup: should be client-generated ID"
        );

        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "get_weather".to_string(),
                    arguments: r#"{"location":"Tokyo"}"#.to_string(),
                    call_id: client_call_id.clone(), // Client-generated ID
                },
                ResponseItem::FunctionCallOutput {
                    call_id: client_call_id, // Same client-generated ID
                    output: FunctionCallOutputPayload {
                        content: r#"{"temp": 20}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Find the user content with function_response
        let user_content = contents.iter().find(|c| {
            c.role == Some("user".to_string())
                && c.parts.as_ref().map_or(false, |p| {
                    p.iter().any(|part| part.function_response.is_some())
                })
        });
        assert!(
            user_content.is_some(),
            "Should have user content with function_response"
        );

        let fr_part = user_content
            .unwrap()
            .parts
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.function_response.is_some())
            .unwrap();
        let fr = fr_part.function_response.as_ref().unwrap();

        // Client-generated ID should be stripped (set to None)
        assert_eq!(
            fr.id, None,
            "FunctionResponse.id should be None for client-generated call_id"
        );
    }

    #[test]
    fn test_function_response_order_matching() {
        // When multiple FunctionCalls have client-generated IDs, FunctionResponses
        // should be in the same order for correct matching by position
        let call_id_1 = format!("{}uuid_first", CLIENT_GENERATED_CALL_ID_PREFIX);
        let call_id_2 = format!("{}uuid_second", CLIENT_GENERATED_CALL_ID_PREFIX);
        let call_id_3 = format!("{}uuid_third", CLIENT_GENERATED_CALL_ID_PREFIX);

        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                // Three FunctionCalls with client-generated IDs
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_a".to_string(),
                    arguments: r#"{"arg": "first"}"#.to_string(),
                    call_id: call_id_1.clone(),
                },
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_b".to_string(),
                    arguments: r#"{"arg": "second"}"#.to_string(),
                    call_id: call_id_2.clone(),
                },
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_c".to_string(),
                    arguments: r#"{"arg": "third"}"#.to_string(),
                    call_id: call_id_3.clone(),
                },
                // Three FunctionCallOutputs in the same order
                ResponseItem::FunctionCallOutput {
                    call_id: call_id_1,
                    output: FunctionCallOutputPayload {
                        content: r#"{"result": "output_1"}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
                ResponseItem::FunctionCallOutput {
                    call_id: call_id_2,
                    output: FunctionCallOutputPayload {
                        content: r#"{"result": "output_2"}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
                ResponseItem::FunctionCallOutput {
                    call_id: call_id_3,
                    output: FunctionCallOutputPayload {
                        content: r#"{"result": "output_3"}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Collect all function_response parts
        let fr_parts: Vec<_> = contents
            .iter()
            .filter_map(|c| c.parts.as_ref())
            .flat_map(|parts| parts.iter())
            .filter(|p| p.function_response.is_some())
            .collect();

        assert_eq!(fr_parts.len(), 3, "Should have 3 FunctionResponse parts");

        // All should have id=None (client-generated IDs stripped)
        for (i, fr_part) in fr_parts.iter().enumerate() {
            let fr = fr_part.function_response.as_ref().unwrap();
            assert_eq!(
                fr.id, None,
                "FunctionResponse {} should have id=None for client-generated call_id",
                i + 1
            );
        }

        // Verify responses are in correct order by checking their content
        let responses: Vec<serde_json::Value> = fr_parts
            .iter()
            .map(|p| {
                p.function_response
                    .as_ref()
                    .unwrap()
                    .response
                    .clone()
                    .unwrap()
            })
            .collect();

        assert_eq!(responses[0]["result"], "output_1", "First response");
        assert_eq!(responses[1]["result"], "output_2", "Second response");
        assert_eq!(responses[2]["result"], "output_3", "Third response");
    }

    #[test]
    fn test_function_response_mixed_ids() {
        // Mix of server-provided and client-generated IDs
        let server_call_id = "server_call_xyz".to_string();
        let client_call_id = format!("{}uuid_client", CLIENT_GENERATED_CALL_ID_PREFIX);

        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                // Server-provided ID
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_server".to_string(),
                    arguments: "{}".to_string(),
                    call_id: server_call_id.clone(),
                },
                // Client-generated ID
                ResponseItem::FunctionCall {
                    id: Some("resp-1".to_string()),
                    name: "tool_client".to_string(),
                    arguments: "{}".to_string(),
                    call_id: client_call_id.clone(),
                },
                // Outputs in same order
                ResponseItem::FunctionCallOutput {
                    call_id: server_call_id.clone(),
                    output: FunctionCallOutputPayload {
                        content: r#"{"from": "server"}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
                ResponseItem::FunctionCallOutput {
                    call_id: client_call_id,
                    output: FunctionCallOutputPayload {
                        content: r#"{"from": "client"}"#.to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let contents = prompt_to_contents(&prompt);

        // Collect all function_response parts
        let fr_parts: Vec<_> = contents
            .iter()
            .filter_map(|c| c.parts.as_ref())
            .flat_map(|parts| parts.iter())
            .filter(|p| p.function_response.is_some())
            .collect();

        assert_eq!(fr_parts.len(), 2, "Should have 2 FunctionResponse parts");

        // Find the server response (should have id preserved)
        let server_fr = fr_parts.iter().find(|p| {
            let fr = p.function_response.as_ref().unwrap();
            fr.response.as_ref().map_or(false, |r| r["from"] == "server")
        });
        assert!(server_fr.is_some(), "Should find server function response");
        assert_eq!(
            server_fr
                .unwrap()
                .function_response
                .as_ref()
                .unwrap()
                .id,
            Some(server_call_id),
            "Server-provided ID should be preserved"
        );

        // Find the client response (should have id=None)
        let client_fr = fr_parts.iter().find(|p| {
            let fr = p.function_response.as_ref().unwrap();
            fr.response.as_ref().map_or(false, |r| r["from"] == "client")
        });
        assert!(client_fr.is_some(), "Should find client function response");
        assert_eq!(
            client_fr
                .unwrap()
                .function_response
                .as_ref()
                .unwrap()
                .id,
            None,
            "Client-generated ID should be stripped"
        );
    }
}
