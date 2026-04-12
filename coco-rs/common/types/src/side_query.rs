//! Side-query types — data types for LLM side-queries.
//!
//! TS: utils/sideQuery.ts (SideQueryOptions, response types)
//!
//! These are pure data types (no async). The async `SideQuery` trait
//! that uses these types lives in `coco-tool` (which has async-trait).
//! This split lets both `coco-permissions` and `coco-tool` share the
//! same request/response types without circular dependencies.

use serde::Deserialize;
use serde::Serialize;

// ── Request ──

/// A side-query request to the LLM.
///
/// Deliberately matches the TS `SideQueryOptions` common denominator.
/// Provider-specific details (beta headers, cache control, attribution)
/// are handled by the implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryRequest {
    /// Model to use. If `None`, uses the implementation's default.
    pub model: Option<String>,

    /// System prompt.
    pub system: String,

    /// Conversation messages.
    pub messages: Vec<SideQueryMessage>,

    /// Tool definitions for structured output.
    pub tools: Vec<SideQueryToolDef>,

    /// Force the LLM to call a specific tool (by name).
    /// Corresponds to `tool_choice: { type: "tool", name: "..." }`.
    pub forced_tool: Option<String>,

    /// Max output tokens (default: 1024).
    pub max_tokens: Option<i32>,

    /// Temperature override.
    pub temperature: Option<f64>,

    /// Thinking budget tokens. `None` = no thinking.
    pub thinking_budget: Option<i32>,

    /// Custom stop sequences.
    pub stop_sequences: Vec<String>,

    /// Skip the CLI system prompt prefix (for internal classifiers).
    pub skip_system_prefix: bool,

    /// Source label for telemetry (e.g. "permission_explainer", "auto_mode").
    pub query_source: String,
}

/// A message in a side-query conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryMessage {
    pub role: SideQueryRole,
    pub content: String,
}

/// Role in a side-query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideQueryRole {
    User,
    Assistant,
}

/// A tool definition for structured output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// ── Response ──

/// Response from a side-query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryResponse {
    /// Text content blocks concatenated.
    pub text: Option<String>,

    /// Tool use blocks from the response.
    pub tool_uses: Vec<SideQueryToolUse>,

    /// Stop reason.
    pub stop_reason: SideQueryStopReason,

    /// Token usage.
    pub usage: SideQueryUsage,

    /// Which model actually served the request.
    pub model_used: String,
}

/// A tool use block in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryToolUse {
    pub name: String,
    pub input: serde_json::Value,
}

/// Why the LLM stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideQueryStopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    /// Unknown or provider-specific reason.
    Other(String),
}

/// Token usage from a side-query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SideQueryUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

// ── Convenience constructors ──

impl SideQueryRequest {
    /// Simple single-turn text query.
    pub fn simple(system: &str, user_prompt: &str, query_source: &str) -> Self {
        Self {
            model: None,
            system: system.to_string(),
            messages: vec![SideQueryMessage {
                role: SideQueryRole::User,
                content: user_prompt.to_string(),
            }],
            tools: Vec::new(),
            forced_tool: None,
            max_tokens: None,
            temperature: None,
            thinking_budget: None,
            stop_sequences: Vec::new(),
            skip_system_prefix: false,
            query_source: query_source.to_string(),
        }
    }

    /// Query with forced tool use (structured output).
    pub fn with_forced_tool(
        system: &str,
        user_prompt: &str,
        tool: SideQueryToolDef,
        query_source: &str,
    ) -> Self {
        let tool_name = tool.name.clone();
        Self {
            model: None,
            system: system.to_string(),
            messages: vec![SideQueryMessage {
                role: SideQueryRole::User,
                content: user_prompt.to_string(),
            }],
            tools: vec![tool],
            forced_tool: Some(tool_name),
            max_tokens: None,
            temperature: None,
            thinking_budget: None,
            stop_sequences: Vec::new(),
            skip_system_prefix: false,
            query_source: query_source.to_string(),
        }
    }
}

impl SideQueryResponse {
    /// Get the first tool use input, if any.
    pub fn first_tool_input(&self) -> Option<&serde_json::Value> {
        self.tool_uses.first().map(|tu| &tu.input)
    }
}
