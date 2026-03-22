//! Response API types.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use super::InputContentBlock;
use super::Metadata;
use super::OutputContentBlock;
use super::ResponseStatus;
use super::Role;
use super::Tool;
use super::ToolChoice;
use super::Usage;
use crate::error::OpenAIError;
use crate::error::Result;

// ============================================================================
// Prompt caching configuration
// ============================================================================

/// Prompt caching retention policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheRetention {
    /// Session-based cache (in-memory).
    #[serde(rename = "in-memory")]
    InMemory,
    /// Extended retention up to 24 hours.
    #[serde(rename = "24h")]
    TwentyFourHours,
}

/// Streaming options for response creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    /// Whether to include obfuscation in the stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_obfuscation: Option<bool>,
}

// ============================================================================
// Reasoning configuration
// ============================================================================

/// Minimum budget tokens for extended thinking.
pub const MIN_THINKING_BUDGET_TOKENS: i32 = 1024;

/// Extended thinking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    /// Enable extended thinking with a budget.
    Enabled {
        /// Maximum tokens for thinking (must be >= 1024).
        budget_tokens: i32,
    },
    /// Disable extended thinking.
    Disabled,
    /// Auto mode - let the model decide.
    Auto,
}

impl ThinkingConfig {
    /// Create an enabled thinking config with the given budget.
    pub fn enabled(budget_tokens: i32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Create an enabled thinking config with validation.
    pub fn enabled_checked(budget_tokens: i32) -> Result<Self> {
        if budget_tokens < MIN_THINKING_BUDGET_TOKENS {
            return Err(OpenAIError::Validation(format!(
                "budget_tokens must be >= {MIN_THINKING_BUDGET_TOKENS}, got {budget_tokens}"
            )));
        }
        Ok(Self::Enabled { budget_tokens })
    }

    /// Create a disabled thinking config.
    pub fn disabled() -> Self {
        Self::Disabled
    }

    /// Create an auto thinking config.
    pub fn auto() -> Self {
        Self::Auto
    }
}

/// Reasoning effort level for model inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    /// No reasoning.
    None,
    /// Minimal reasoning effort.
    Minimal,
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra high reasoning effort.
    Xhigh,
}

/// Reasoning configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    /// Effort level for reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    /// Whether to generate a summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_summary: Option<String>,
    /// Summary mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl ReasoningConfig {
    /// Create a reasoning config with the given effort level.
    pub fn with_effort(effort: ReasoningEffort) -> Self {
        Self {
            effort: Some(effort),
            generate_summary: None,
            summary: None,
        }
    }

    /// Enable summary generation.
    pub fn with_summary(mut self, mode: impl Into<String>) -> Self {
        self.generate_summary = Some(mode.into());
        self
    }
}

// ============================================================================
// Service and configuration types
// ============================================================================

/// Service tier for request processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceTier {
    /// Auto-select tier.
    Auto,
    /// Default tier.
    Default,
    /// Flex tier.
    Flex,
    /// Scale tier.
    Scale,
    /// Priority tier.
    Priority,
}

/// Truncation strategy for input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Truncation {
    /// Auto-truncate if needed.
    Auto,
    /// Disable truncation.
    Disabled,
}

/// Items to include in the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseIncludable {
    /// Include file search call results.
    #[serde(rename = "file_search_call.results")]
    FileSearchCallResults,
    /// Include message input image URL detail.
    #[serde(rename = "message.input_image.image_url.detail")]
    MessageInputImageUrlDetail,
    /// Include computer call output.
    #[serde(rename = "computer_call_output")]
    ComputerCallOutput,
    /// Include reasoning encrypted content.
    #[serde(rename = "reasoning.encrypted_content")]
    ReasoningEncryptedContent,
    /// Include web search call results.
    #[serde(rename = "web_search_call.results")]
    WebSearchCallResults,
    /// Include web search action sources.
    #[serde(rename = "web_search_call.action.sources")]
    WebSearchCallActionSources,
    /// Include code interpreter call outputs.
    #[serde(rename = "code_interpreter_call.outputs")]
    CodeInterpreterCallOutputs,
    /// Include message output text logprobs.
    #[serde(rename = "message.output_text.logprobs")]
    MessageOutputTextLogprobs,
}

/// Text format configuration for structured outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextFormat {
    /// Plain text output.
    Text,
    /// JSON object output.
    JsonObject,
    /// JSON schema output with strict validation.
    JsonSchema {
        /// The JSON schema definition.
        schema: serde_json::Value,
        /// Name of the schema.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Whether to use strict mode.
        #[serde(skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
}

/// Text/structured output configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextConfig {
    /// Format for text output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
    /// Verbosity level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
}

impl TextConfig {
    /// Create a plain text config.
    pub fn text() -> Self {
        Self {
            format: Some(TextFormat::Text),
            verbosity: None,
        }
    }

    /// Create a JSON object config.
    pub fn json_object() -> Self {
        Self {
            format: Some(TextFormat::JsonObject),
            verbosity: None,
        }
    }

    /// Create a JSON schema config.
    pub fn json_schema(schema: serde_json::Value) -> Self {
        Self {
            format: Some(TextFormat::JsonSchema {
                schema,
                name: None,
                strict: None,
            }),
            verbosity: None,
        }
    }

    /// Create a JSON schema config with name and strict mode.
    pub fn json_schema_strict(schema: serde_json::Value, name: impl Into<String>) -> Self {
        Self {
            format: Some(TextFormat::JsonSchema {
                schema,
                name: Some(name.into()),
                strict: Some(true),
            }),
            verbosity: None,
        }
    }
}

/// Reason why a response is incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    /// Hit the maximum output token limit.
    MaxOutputTokens,
    /// Content was filtered.
    ContentFilter,
    /// Interrupted by user or system.
    Interrupted,
    /// Other reason (catch-all).
    #[serde(other)]
    Other,
}

/// Details about why a response is incomplete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncompleteDetails {
    /// The reason the response is incomplete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<IncompleteReason>,
}

// ============================================================================
// Response input item (flat item list for multi-turn conversations)
// ============================================================================

/// A single item in the flat input list for the Responses API.
///
/// The OpenAI Responses API `input` field accepts a flat list of heterogeneous
/// items (messages, function calls, function call outputs, etc.). This enum
/// models the `ResponseInputItemParam` discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseInputItem {
    /// A message (user, assistant, system, or developer).
    Message {
        /// Role of the message author.
        role: Role,
        /// Content blocks of the message.
        content: Vec<InputContentBlock>,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional status (e.g. "completed" for assistant messages).
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// A function call from a previous assistant turn.
    FunctionCall {
        /// Call ID to correlate with the function call output.
        call_id: String,
        /// Function name.
        name: String,
        /// Arguments as JSON string.
        arguments: String,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional status.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Output from a function call (tool result).
    FunctionCallOutput {
        /// Call ID of the function call this responds to.
        call_id: String,
        /// Output of the function call.
        output: String,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional status.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// A custom tool call from a previous assistant turn.
    #[serde(rename = "custom_tool_call")]
    CustomToolCall {
        /// Call ID to correlate with the custom tool call output.
        call_id: String,
        /// Tool name.
        name: String,
        /// Tool input (free-form text).
        input: String,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Output from a custom tool call.
    #[serde(rename = "custom_tool_call_output")]
    CustomToolCallOutput {
        /// Call ID of the custom tool call this responds to.
        call_id: String,
        /// Output from the custom tool.
        output: String,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Item reference (reference a previous conversation item by ID).
    ItemReference {
        /// ID of the item to reference.
        id: String,
    },
    /// MCP tool call (from a previous assistant turn).
    #[serde(rename = "mcp_call")]
    McpCall {
        /// Unique ID.
        id: String,
        /// Arguments (JSON string).
        arguments: String,
        /// Tool name.
        name: String,
        /// MCP server label.
        server_label: String,
        /// Approval request ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        approval_request_id: Option<String>,
        /// Error if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Tool output.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Status.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// MCP approval request (from a previous assistant turn).
    #[serde(rename = "mcp_approval_request")]
    McpApprovalRequest {
        /// Unique ID.
        id: String,
        /// Arguments (JSON string).
        arguments: String,
        /// Tool name requiring approval.
        name: String,
        /// MCP server label.
        server_label: String,
    },
    /// MCP approval response.
    #[serde(rename = "mcp_approval_response")]
    McpApprovalResponse {
        /// Approval request ID.
        approval_request_id: String,
        /// Whether to approve the request.
        approve: bool,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional reason for denial.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Local shell call (from a previous assistant turn).
    #[serde(rename = "local_shell_call")]
    LocalShellCall {
        /// Unique ID.
        id: String,
        /// Action details.
        action: serde_json::Value,
        /// Call ID.
        call_id: String,
        /// Status.
        status: String,
    },
    /// Local shell call output.
    #[serde(rename = "local_shell_call_output")]
    LocalShellCallOutput {
        /// Unique ID.
        id: String,
        /// Output string.
        output: String,
        /// Optional status.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Shell tool call (from a previous assistant turn).
    #[serde(rename = "shell_call")]
    ShellCall {
        /// Call ID.
        call_id: String,
        /// Action details.
        action: serde_json::Value,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional status.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Shell call output.
    #[serde(rename = "shell_call_output")]
    ShellCallOutput {
        /// Call ID.
        call_id: String,
        /// Output items.
        output: Vec<serde_json::Value>,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Maximum output length.
        #[serde(skip_serializing_if = "Option::is_none")]
        max_output_length: Option<i64>,
    },
    /// Apply patch call (from a previous assistant turn).
    #[serde(rename = "apply_patch_call")]
    ApplyPatchCall {
        /// Call ID.
        call_id: String,
        /// Operation details.
        operation: serde_json::Value,
        /// Status.
        status: String,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Apply patch call output.
    #[serde(rename = "apply_patch_call_output")]
    ApplyPatchCallOutput {
        /// Call ID.
        call_id: String,
        /// Status (e.g., "completed" or "failed").
        status: String,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional output text.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Computer call output.
    #[serde(rename = "computer_call_output")]
    ComputerCallOutput {
        /// Call ID.
        call_id: String,
        /// Output details.
        output: serde_json::Value,
        /// Optional item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Acknowledged safety checks.
        #[serde(skip_serializing_if = "Option::is_none")]
        acknowledged_safety_checks: Option<Vec<serde_json::Value>>,
        /// Optional status.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Image generation call (from a previous assistant turn).
    #[serde(rename = "image_generation_call")]
    ImageGenerationCall {
        /// Unique ID.
        id: String,
        /// Status.
        status: String,
        /// Generated result (URL or base64). Always serialized (may be null).
        result: Option<String>,
    },
    /// Reasoning item (from a previous assistant turn).
    #[serde(rename = "reasoning")]
    Reasoning {
        /// Unique ID.
        id: String,
        /// Reasoning summaries (required by the API; serialize even when empty).
        #[serde(default)]
        summary: Vec<ReasoningSummary>,
        /// Reasoning content items.
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Vec<ReasoningContent>>,
        /// Encrypted reasoning content (opaque token for multi-turn replay).
        #[serde(skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        /// Status of the reasoning item.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ReasoningStatus>,
    },
    /// Compaction item (from a previous assistant turn).
    #[serde(rename = "compaction")]
    Compaction {
        /// Unique ID (optional per Python SDK `Optional[str]`).
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Encrypted content.
        encrypted_content: String,
    },
    /// File search tool call (from a previous assistant turn).
    #[serde(rename = "file_search_call")]
    FileSearchCall {
        /// Unique ID.
        id: String,
        /// Search queries.
        queries: Vec<String>,
        /// Status of the call.
        status: String,
        /// Search results.
        #[serde(skip_serializing_if = "Option::is_none")]
        results: Option<Vec<FileSearchResult>>,
    },
    /// Computer tool call (from a previous assistant turn).
    #[serde(rename = "computer_call")]
    ComputerCall {
        /// Unique ID.
        id: String,
        /// Call ID.
        call_id: String,
        /// Action to perform.
        action: ComputerAction,
        /// Pending safety checks.
        pending_safety_checks: Vec<SafetyCheck>,
        /// Status of the call.
        status: String,
    },
    /// Web search tool call (from a previous assistant turn).
    #[serde(rename = "web_search_call")]
    WebSearchCall {
        /// Unique ID.
        id: String,
        /// Status of the call.
        status: String,
        /// Action details.
        action: serde_json::Value,
    },
    /// Code interpreter tool call (from a previous assistant turn).
    #[serde(rename = "code_interpreter_call")]
    CodeInterpreterCall {
        /// Unique ID.
        id: String,
        /// Container ID.
        container_id: String,
        /// Status of the call.
        status: String,
        /// Code to execute.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// Execution outputs.
        #[serde(skip_serializing_if = "Option::is_none")]
        outputs: Option<Vec<CodeInterpreterOutput>>,
    },
    /// MCP list tools response (from a previous assistant turn).
    #[serde(rename = "mcp_list_tools")]
    McpListTools {
        /// Unique ID.
        id: String,
        /// MCP server label.
        server_label: String,
        /// Available tools.
        tools: Vec<McpToolInfo>,
        /// Error if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

impl ResponseInputItem {
    /// Create a user message with content blocks.
    pub fn user_message(content: Vec<InputContentBlock>) -> Self {
        Self::Message {
            role: Role::User,
            content,
            id: None,
            status: None,
        }
    }

    /// Create a user message with a single text block.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::Message {
            role: Role::User,
            content: vec![InputContentBlock::text(text)],
            id: None,
            status: None,
        }
    }

    /// Create an assistant message with content blocks.
    pub fn assistant_message(
        content: Vec<InputContentBlock>,
        id: Option<String>,
        status: Option<String>,
    ) -> Self {
        Self::Message {
            role: Role::Assistant,
            content,
            id,
            status,
        }
    }

    /// Create an assistant message with a single text block.
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::Message {
            role: Role::Assistant,
            content: vec![InputContentBlock::output_text(text)],
            id: None,
            status: Some("completed".to_string()),
        }
    }

    /// Create a system message with a single text block.
    pub fn system_message(text: impl Into<String>) -> Self {
        Self::Message {
            role: Role::System,
            content: vec![InputContentBlock::text(text)],
            id: None,
            status: None,
        }
    }

    /// Create a developer message with a single text block.
    pub fn developer_message(text: impl Into<String>) -> Self {
        Self::Message {
            role: Role::Developer,
            content: vec![InputContentBlock::text(text)],
            id: None,
            status: None,
        }
    }

    /// Create a function call item (from a previous assistant turn).
    pub fn function_call(
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self::FunctionCall {
            call_id: call_id.into(),
            name: name.into(),
            arguments: arguments.into(),
            id: None,
            status: None,
        }
    }

    /// Create a function call output item (tool result).
    pub fn function_call_output(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self::FunctionCallOutput {
            call_id: call_id.into(),
            output: output.into(),
            id: None,
            status: None,
        }
    }

    /// Create a custom tool call item (from a previous assistant turn).
    pub fn custom_tool_call(
        call_id: impl Into<String>,
        name: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        Self::CustomToolCall {
            call_id: call_id.into(),
            name: name.into(),
            input: input.into(),
            id: None,
        }
    }

    /// Create a custom tool call output item (tool result).
    pub fn custom_tool_call_output(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self::CustomToolCallOutput {
            call_id: call_id.into(),
            output: output.into(),
            id: None,
        }
    }

    /// Create an item reference.
    pub fn item_reference(id: impl Into<String>) -> Self {
        Self::ItemReference { id: id.into() }
    }

    /// Create an MCP call input item.
    pub fn mcp_call(
        id: impl Into<String>,
        arguments: impl Into<String>,
        name: impl Into<String>,
        server_label: impl Into<String>,
    ) -> Self {
        Self::McpCall {
            id: id.into(),
            arguments: arguments.into(),
            name: name.into(),
            server_label: server_label.into(),
            approval_request_id: None,
            error: None,
            output: None,
            status: None,
        }
    }

    /// Create an MCP approval request input item.
    pub fn mcp_approval_request(
        id: impl Into<String>,
        arguments: impl Into<String>,
        name: impl Into<String>,
        server_label: impl Into<String>,
    ) -> Self {
        Self::McpApprovalRequest {
            id: id.into(),
            arguments: arguments.into(),
            name: name.into(),
            server_label: server_label.into(),
        }
    }

    /// Create an MCP approval response input item.
    pub fn mcp_approval_response(approval_request_id: impl Into<String>, approve: bool) -> Self {
        Self::McpApprovalResponse {
            approval_request_id: approval_request_id.into(),
            approve,
            id: None,
            reason: None,
        }
    }

    /// Create a local shell call input item.
    pub fn local_shell_call(
        id: impl Into<String>,
        action: serde_json::Value,
        call_id: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        Self::LocalShellCall {
            id: id.into(),
            action,
            call_id: call_id.into(),
            status: status.into(),
        }
    }

    /// Create a local shell call output input item.
    pub fn local_shell_call_output(id: impl Into<String>, output: impl Into<String>) -> Self {
        Self::LocalShellCallOutput {
            id: id.into(),
            output: output.into(),
            status: None,
        }
    }

    /// Create a shell call input item.
    pub fn shell_call(call_id: impl Into<String>, action: serde_json::Value) -> Self {
        Self::ShellCall {
            call_id: call_id.into(),
            action,
            id: None,
            status: None,
        }
    }

    /// Create a shell call output input item.
    pub fn shell_call_output(call_id: impl Into<String>, output: Vec<serde_json::Value>) -> Self {
        Self::ShellCallOutput {
            call_id: call_id.into(),
            output,
            id: None,
            max_output_length: None,
        }
    }

    /// Create an apply patch call input item.
    pub fn apply_patch_call(
        call_id: impl Into<String>,
        operation: serde_json::Value,
        status: impl Into<String>,
    ) -> Self {
        Self::ApplyPatchCall {
            call_id: call_id.into(),
            operation,
            status: status.into(),
            id: None,
        }
    }

    /// Create an apply patch call output input item.
    pub fn apply_patch_call_output(call_id: impl Into<String>, status: impl Into<String>) -> Self {
        Self::ApplyPatchCallOutput {
            call_id: call_id.into(),
            status: status.into(),
            id: None,
            output: None,
        }
    }

    /// Create a computer call output input item.
    pub fn computer_call_output(call_id: impl Into<String>, output: serde_json::Value) -> Self {
        Self::ComputerCallOutput {
            call_id: call_id.into(),
            output,
            id: None,
            acknowledged_safety_checks: None,
            status: None,
        }
    }

    /// Create an image generation call input item.
    pub fn image_generation_call(id: impl Into<String>, status: impl Into<String>) -> Self {
        Self::ImageGenerationCall {
            id: id.into(),
            status: status.into(),
            result: None,
        }
    }

    /// Create a reasoning input item from content items.
    pub fn reasoning(id: impl Into<String>, content: Vec<ReasoningContent>) -> Self {
        Self::Reasoning {
            id: id.into(),
            summary: vec![],
            content: Some(content),
            encrypted_content: None,
            status: None,
        }
    }

    /// Create a reasoning input item from encrypted content (for multi-turn replay).
    pub fn reasoning_encrypted(
        id: impl Into<String>,
        encrypted_content: impl Into<String>,
    ) -> Self {
        Self::Reasoning {
            id: id.into(),
            summary: vec![],
            content: None,
            encrypted_content: Some(encrypted_content.into()),
            status: None,
        }
    }

    /// Create a compaction input item.
    pub fn compaction(id: impl Into<String>, encrypted_content: impl Into<String>) -> Self {
        Self::Compaction {
            id: Some(id.into()),
            encrypted_content: encrypted_content.into(),
        }
    }

    /// Create a file search call input item.
    pub fn file_search_call(
        id: impl Into<String>,
        queries: Vec<String>,
        status: impl Into<String>,
    ) -> Self {
        Self::FileSearchCall {
            id: id.into(),
            queries,
            status: status.into(),
            results: None,
        }
    }

    /// Create a computer call input item.
    pub fn computer_call(
        id: impl Into<String>,
        call_id: impl Into<String>,
        action: ComputerAction,
        pending_safety_checks: Vec<SafetyCheck>,
        status: impl Into<String>,
    ) -> Self {
        Self::ComputerCall {
            id: id.into(),
            call_id: call_id.into(),
            action,
            pending_safety_checks,
            status: status.into(),
        }
    }

    /// Create a web search call input item.
    pub fn web_search_call(
        id: impl Into<String>,
        status: impl Into<String>,
        action: serde_json::Value,
    ) -> Self {
        Self::WebSearchCall {
            id: id.into(),
            status: status.into(),
            action,
        }
    }

    /// Create a code interpreter call input item.
    pub fn code_interpreter_call(
        id: impl Into<String>,
        container_id: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        Self::CodeInterpreterCall {
            id: id.into(),
            container_id: container_id.into(),
            status: status.into(),
            code: None,
            outputs: None,
        }
    }

    /// Create an MCP list tools input item.
    pub fn mcp_list_tools(
        id: impl Into<String>,
        server_label: impl Into<String>,
        tools: Vec<McpToolInfo>,
    ) -> Self {
        Self::McpListTools {
            id: id.into(),
            server_label: server_label.into(),
            tools,
            error: None,
        }
    }
}

// ============================================================================
// Response input (text or messages)
// ============================================================================

/// Input for response creation - can be simple text or a flat item list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInput {
    /// Simple text input.
    Text(String),
    /// Flat list of input items (messages, function calls, outputs, etc.).
    Items(Vec<ResponseInputItem>),
}

impl From<String> for ResponseInput {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for ResponseInput {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<Vec<ResponseInputItem>> for ResponseInput {
    fn from(items: Vec<ResponseInputItem>) -> Self {
        Self::Items(items)
    }
}

// ============================================================================
// Reasoning types
// ============================================================================

/// A summary item in reasoning output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningSummary {
    /// The summary text.
    pub text: String,
    /// The type of summary.
    #[serde(rename = "type")]
    pub summary_type: String,
}

impl ReasoningSummary {
    /// Create a new reasoning summary.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            summary_type: "summary_text".to_string(),
        }
    }
}

/// A content item in reasoning output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningContent {
    /// The reasoning text.
    pub text: String,
    /// The type of content (always "reasoning_text").
    #[serde(rename = "type")]
    pub content_type: String,
}

impl ReasoningContent {
    /// Create a new reasoning content item.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            content_type: "reasoning_text".to_string(),
        }
    }
}

/// Status of a reasoning item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningStatus {
    /// Reasoning is in progress.
    InProgress,
    /// Reasoning is completed.
    Completed,
    /// Reasoning is incomplete.
    Incomplete,
}

// ============================================================================
// Output item
// ============================================================================

/// Output item from a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputItem {
    /// Message output.
    Message {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Role (always "assistant").
        role: String,
        /// Content blocks.
        content: Vec<OutputContentBlock>,
        /// Status of the message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Function call output.
    FunctionCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID to reference this call.
        call_id: String,
        /// Function name.
        name: String,
        /// Arguments as JSON string.
        arguments: String,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Reasoning output from reasoning models.
    Reasoning {
        /// Unique ID.
        id: String,
        /// Reasoning content items.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Vec<ReasoningContent>>,
        /// Reasoning summaries.
        #[serde(default)]
        summary: Vec<ReasoningSummary>,
        /// Encrypted reasoning content (opaque token).
        #[serde(skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        /// Status of the reasoning.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ReasoningStatus>,
    },
    /// File search tool call.
    #[serde(rename = "file_search_call")]
    FileSearchCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Search queries.
        #[serde(default)]
        queries: Vec<String>,
        /// Search results.
        #[serde(skip_serializing_if = "Option::is_none")]
        results: Option<Vec<FileSearchResult>>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Web search tool call.
    #[serde(rename = "web_search_call")]
    WebSearchCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Action details.
        #[serde(default)]
        action: Option<serde_json::Value>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Computer tool call for UI automation.
    #[serde(rename = "computer_call")]
    ComputerCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Action to perform.
        action: ComputerAction,
        /// Pending safety checks.
        #[serde(default)]
        pending_safety_checks: Vec<SafetyCheck>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Code interpreter tool call.
    #[serde(rename = "code_interpreter_call")]
    CodeInterpreterCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Container ID.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        container_id: Option<String>,
        /// Code to execute.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// Execution outputs.
        #[serde(skip_serializing_if = "Option::is_none")]
        outputs: Option<Vec<CodeInterpreterOutput>>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Image generation tool call.
    #[serde(rename = "image_generation_call")]
    ImageGenerationCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Generated image result (URL or base64).
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Local shell tool call.
    #[serde(rename = "local_shell_call")]
    LocalShellCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Action details.
        #[serde(default)]
        action: Option<serde_json::Value>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// MCP tool call.
    #[serde(rename = "mcp_call")]
    McpCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// MCP server label.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_label: Option<String>,
        /// Tool name.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Tool arguments (JSON string).
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<String>,
        /// Tool output.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Error if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        /// Approval request ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        approval_request_id: Option<String>,
    },
    /// MCP list tools response.
    #[serde(rename = "mcp_list_tools")]
    McpListTools {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// MCP server label.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_label: Option<String>,
        /// Available tools.
        #[serde(default)]
        tools: Vec<McpToolInfo>,
        /// Error if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// MCP approval request.
    #[serde(rename = "mcp_approval_request")]
    McpApprovalRequest {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// MCP server label.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_label: Option<String>,
        /// Tool name requiring approval.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Arguments for the tool (JSON string).
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<String>,
    },
    /// Apply patch tool call.
    #[serde(rename = "apply_patch_call")]
    ApplyPatchCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Operation details.
        #[serde(default)]
        operation: Option<serde_json::Value>,
        /// Created by identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        created_by: Option<String>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Shell tool call (function shell).
    #[serde(rename = "shell_call")]
    FunctionShellCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Action details.
        #[serde(default)]
        action: Option<serde_json::Value>,
        /// Created by identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        created_by: Option<String>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Custom tool call.
    #[serde(rename = "custom_tool_call")]
    CustomToolCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Tool name.
        name: String,
        /// Tool input (free-form text).
        input: String,
    },
    /// Response compaction item.
    #[serde(rename = "compaction")]
    Compaction {
        /// Unique ID.
        #[serde(default)]
        id: String,
        /// Encrypted content.
        encrypted_content: String,
        /// Created by identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        created_by: Option<String>,
    },
    /// Shell call output.
    #[serde(rename = "shell_call_output")]
    ShellCallOutput {
        /// Unique ID.
        id: String,
        /// Call ID.
        call_id: String,
        /// Output items.
        #[serde(default)]
        output: Vec<serde_json::Value>,
        /// Maximum output length.
        #[serde(skip_serializing_if = "Option::is_none")]
        max_output_length: Option<i64>,
        /// Created by identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        created_by: Option<String>,
    },
    /// Apply patch call output.
    #[serde(rename = "apply_patch_call_output")]
    ApplyPatchCallOutput {
        /// Unique ID.
        id: String,
        /// Call ID.
        call_id: String,
        /// Status.
        status: String,
        /// Created by identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        created_by: Option<String>,
        /// Output text.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
}

impl OutputItem {
    /// Serialize this output item as a JSON value.
    ///
    /// Useful for inspecting or logging output items. To feed a previous
    /// turn's output back into the model, convert the relevant `OutputItem`
    /// variants into typed `ResponseInputItem` variants and include them in
    /// the next request's `input` array.
    pub fn to_input_item_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

// ============================================================================
// Tool call result types
// ============================================================================

/// File search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchResult {
    /// File ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    /// File name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Relevance score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Web search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    /// Result title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Result URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Result snippet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// A point in a drag path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragPathPoint {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
}

/// Computer action for UI automation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputerAction {
    /// Click action.
    Click {
        /// X coordinate.
        x: i32,
        /// Y coordinate.
        y: i32,
        /// Button (left, right, middle).
        #[serde(skip_serializing_if = "Option::is_none")]
        button: Option<String>,
    },
    /// Double click action.
    DoubleClick {
        /// X coordinate.
        x: i32,
        /// Y coordinate.
        y: i32,
    },
    /// Scroll action.
    Scroll {
        /// X coordinate.
        x: i32,
        /// Y coordinate.
        y: i32,
        /// Horizontal scroll amount.
        scroll_x: i32,
        /// Vertical scroll amount.
        scroll_y: i32,
    },
    /// Type text action.
    Type {
        /// Text to type.
        text: String,
    },
    /// Key press action.
    #[serde(rename = "keypress")]
    Keypress {
        /// Keys to press.
        keys: Vec<String>,
    },
    /// Screenshot action.
    Screenshot,
    /// Wait action.
    Wait,
    /// Drag action.
    Drag {
        /// Path of points.
        path: Vec<DragPathPoint>,
    },
    /// Move action.
    Move {
        /// X coordinate.
        x: i32,
        /// Y coordinate.
        y: i32,
    },
}

/// Safety check for computer actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    /// Check ID.
    #[serde(default)]
    pub id: String,
    /// Check code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Check message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Code interpreter output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeInterpreterOutput {
    /// Log output.
    Logs {
        /// Log content.
        logs: String,
    },
    /// Image output.
    Image {
        /// Image URL.
        #[serde(default)]
        url: String,
    },
}

/// MCP tool information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    /// Tool name.
    pub name: String,
    /// Tool description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Tool input schema.
    #[serde(default)]
    pub input_schema: serde_json::Value,
    /// Tool annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
}

// ============================================================================
// Request parameters
// ============================================================================

/// Parameters for creating a response.
#[derive(Debug, Clone, Serialize)]
pub struct ResponseCreateParams {
    /// Model ID to use (e.g., "gpt-4o", "o3").
    pub model: String,

    /// Input (text string or message array).
    pub input: ResponseInput,

    /// System instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,

    /// Tool definitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Tool choice configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// Extended thinking configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,

    /// Reasoning configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,

    /// Previous response ID for multi-turn conversations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,

    /// Sampling temperature (0.0 to 2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus sampling probability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Whether to store the response server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    /// Streaming options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,

    /// Prompt cache retention policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<PromptCacheRetention>,

    /// Request metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// User identifier for abuse monitoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Items to include in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<ResponseIncludable>>,

    /// Maximum number of tool calls per turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<i32>,

    /// Whether to allow parallel tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,

    /// Service tier for processing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,

    /// Text/structured output configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextConfig>,

    /// Truncation strategy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,

    /// Number of top logprobs to return (0-20).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<i32>,

    /// Conversation state for multi-turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ConversationParam>,

    /// Run model response in background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,

    /// Safety identifier for policy violation detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,

    /// Prompt template reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptParam>,

    /// Stable cache identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,

    /// Extra parameters passed through to the API request body.
    #[serde(flatten, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// Conversation parameter for multi-turn state.
///
/// Mirrors Python's `Union[str, ResponseConversationParam]`:
/// - `Id(String)` serializes as a plain JSON string: `"conv_abc123"`
/// - `Object { id }` serializes as an object: `{"id": "conv_abc123"}`
///
/// To feed previous output items back into the next turn, add them to the
/// `input` array as `ResponseInputItem` variants instead of using this field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConversationParam {
    /// Reference a prior conversation by plain ID string.
    Id(String),
    /// Reference a prior conversation using an object with an `id` field.
    Object {
        /// Conversation ID.
        id: String,
    },
}

/// Prompt template parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptParam {
    /// Prompt template ID (required).
    pub id: String,
    /// Template variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<serde_json::Value>,
    /// Prompt template version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl ResponseCreateParams {
    /// Create new response parameters with item input.
    pub fn new(model: impl Into<String>, input: Vec<ResponseInputItem>) -> Self {
        Self {
            model: model.into(),
            input: ResponseInput::Items(input),
            instructions: None,
            max_output_tokens: None,
            tools: None,
            tool_choice: None,
            thinking: None,
            reasoning: None,
            previous_response_id: None,
            temperature: None,
            top_p: None,
            store: None,
            stream_options: None,
            prompt_cache_retention: None,
            metadata: None,
            user: None,
            include: None,
            max_tool_calls: None,
            parallel_tool_calls: None,
            service_tier: None,
            text: None,
            truncation: None,
            top_logprobs: None,
            conversation: None,
            background: None,
            safety_identifier: None,
            prompt: None,
            prompt_cache_key: None,
            extra: std::collections::HashMap::new(),
        }
    }

    /// Create new response parameters with simple text input.
    pub fn with_text(model: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: ResponseInput::Text(text.into()),
            instructions: None,
            max_output_tokens: None,
            tools: None,
            tool_choice: None,
            thinking: None,
            reasoning: None,
            previous_response_id: None,
            temperature: None,
            top_p: None,
            store: None,
            stream_options: None,
            prompt_cache_retention: None,
            metadata: None,
            user: None,
            include: None,
            max_tool_calls: None,
            parallel_tool_calls: None,
            service_tier: None,
            text: None,
            truncation: None,
            top_logprobs: None,
            conversation: None,
            background: None,
            safety_identifier: None,
            prompt: None,
            prompt_cache_key: None,
            extra: std::collections::HashMap::new(),
        }
    }

    /// Set system instructions.
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Set maximum output tokens.
    pub fn max_output_tokens(mut self, tokens: i32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// Set tools.
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set tool choice.
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set thinking configuration.
    pub fn thinking(mut self, config: ThinkingConfig) -> Self {
        self.thinking = Some(config);
        self
    }

    /// Set reasoning configuration.
    pub fn reasoning(mut self, config: ReasoningConfig) -> Self {
        self.reasoning = Some(config);
        self
    }

    /// Set previous response ID for multi-turn conversations.
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Some(id.into());
        self
    }

    /// Set temperature (unchecked).
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set temperature with validation.
    pub fn temperature_checked(mut self, temp: f64) -> Result<Self> {
        if !(0.0..=2.0).contains(&temp) {
            return Err(OpenAIError::Validation(format!(
                "temperature must be in range [0.0, 2.0], got {temp}"
            )));
        }
        self.temperature = Some(temp);
        Ok(self)
    }

    /// Set top_p.
    pub fn top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Set whether to store the response.
    pub fn store(mut self, store: bool) -> Self {
        self.store = Some(store);
        self
    }

    /// Set streaming options.
    pub fn stream_options(mut self, options: StreamOptions) -> Self {
        self.stream_options = Some(options);
        self
    }

    /// Set prompt cache retention policy.
    pub fn prompt_cache_retention(mut self, retention: PromptCacheRetention) -> Self {
        self.prompt_cache_retention = Some(retention);
        self
    }

    /// Set metadata.
    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set user identifier.
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set items to include in the response.
    pub fn include(mut self, items: Vec<ResponseIncludable>) -> Self {
        self.include = Some(items);
        self
    }

    /// Set maximum tool calls per turn.
    pub fn max_tool_calls(mut self, max: i32) -> Self {
        self.max_tool_calls = Some(max);
        self
    }

    /// Set whether to allow parallel tool calls.
    pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
        self.parallel_tool_calls = Some(enabled);
        self
    }

    /// Set service tier.
    pub fn service_tier(mut self, tier: ServiceTier) -> Self {
        self.service_tier = Some(tier);
        self
    }

    /// Set text/structured output configuration.
    pub fn text_config(mut self, config: TextConfig) -> Self {
        self.text = Some(config);
        self
    }

    /// Set truncation strategy.
    pub fn truncation(mut self, strategy: Truncation) -> Self {
        self.truncation = Some(strategy);
        self
    }

    /// Set top logprobs (unchecked).
    pub fn top_logprobs(mut self, n: i32) -> Self {
        self.top_logprobs = Some(n);
        self
    }

    /// Set top logprobs with validation (0-20).
    pub fn top_logprobs_checked(mut self, n: i32) -> Result<Self> {
        if !(0..=20).contains(&n) {
            return Err(OpenAIError::Validation(format!(
                "top_logprobs must be in range [0, 20], got {n}"
            )));
        }
        self.top_logprobs = Some(n);
        Ok(self)
    }

    /// Set conversation state for multi-turn.
    pub fn conversation(mut self, conv: ConversationParam) -> Self {
        self.conversation = Some(conv);
        self
    }

    /// Set conversation by ID.
    pub fn conversation_id(mut self, id: impl Into<String>) -> Self {
        self.conversation = Some(ConversationParam::Id(id.into()));
        self
    }

    /// Run model response in background.
    pub fn background(mut self, enabled: bool) -> Self {
        self.background = Some(enabled);
        self
    }

    /// Set safety identifier for policy violation detection.
    pub fn safety_identifier(mut self, id: impl Into<String>) -> Self {
        self.safety_identifier = Some(id.into());
        self
    }

    /// Set prompt template reference.
    pub fn prompt(mut self, prompt: PromptParam) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// Set stable cache identifier.
    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.prompt_cache_key = Some(key.into());
        self
    }
}

// ============================================================================
// SDK HTTP Response (for round-trip preservation)
// ============================================================================

/// HTTP response metadata (not serialized, populated by client).
#[derive(Debug, Clone, Default)]
pub struct SdkHttpResponse {
    /// HTTP status code.
    pub status_code: Option<i32>,
    /// Response headers.
    pub headers: Option<HashMap<String, String>>,
    /// Raw response body.
    pub body: Option<String>,
}

impl SdkHttpResponse {
    /// Create a new SdkHttpResponse with all fields.
    pub fn new(status_code: i32, headers: HashMap<String, String>, body: String) -> Self {
        Self {
            status_code: Some(status_code),
            headers: Some(headers),
            body: Some(body),
        }
    }

    /// Create from status code and body only.
    pub fn from_status_and_body(status_code: i32, body: String) -> Self {
        Self {
            status_code: Some(status_code),
            headers: None,
            body: Some(body),
        }
    }
}

// ============================================================================
// Response
// ============================================================================

/// Error details in a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    /// Error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Error message.
    pub message: String,
}

impl ResponseError {
    /// Get the optional error code.
    pub fn code_opt(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Get the error code, defaulting to an empty string when missing.
    pub fn code_or_empty(&self) -> &str {
        self.code.as_deref().unwrap_or("")
    }
}

/// Prompt template information in response (echoed back).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsePrompt {
    /// Prompt template ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Prompt version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Template variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<serde_json::Value>,
}

/// Response from the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Unique response ID.
    pub id: String,

    /// Response status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseStatus>,

    /// Output items.
    pub output: Vec<OutputItem>,

    /// Token usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,

    /// Creation timestamp (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,

    /// Model used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Object type (always "response").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,

    /// Error details if status is Failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,

    /// Completion timestamp (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    /// Details about why the response is incomplete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<IncompleteDetails>,

    /// System instructions (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    /// Request metadata (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Service tier used for processing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,

    /// Temperature used (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Whether parallel tool calls are allowed (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,

    /// Tools used in this response (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Tool choice configuration (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// Maximum output tokens (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,

    /// Maximum tool calls per turn (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<i32>,

    /// Top-p sampling parameter (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Reasoning configuration (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,

    /// Text configuration (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextConfig>,

    /// Truncation strategy (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,

    /// Top logprobs setting (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<i32>,

    /// Prompt template used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<ResponsePrompt>,

    /// Prompt cache key used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,

    /// Prompt cache retention policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,

    /// Safety identifier used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,

    /// Previous response ID for multi-turn conversations (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,

    /// Whether the response was run in the background (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,

    /// Conversation state for multi-turn (echoed back). This is intentionally
    /// left as raw JSON to stay forward-compatible with the server and to
    /// mirror the Python SDK's `Conversation` type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<serde_json::Value>,

    /// User identifier for abuse monitoring (deprecated but still echoed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// HTTP response metadata (not serialized, populated by client).
    #[serde(skip)]
    pub sdk_http_response: Option<SdkHttpResponse>,
}

impl Response {
    /// Get the optional status value.
    pub fn status_opt(&self) -> Option<ResponseStatus> {
        self.status
    }

    /// Get the status, defaulting to `ResponseStatus::Completed` when missing.
    ///
    /// This mirrors the Python SDK where `status` is `Optional[ResponseStatus]`
    /// and may be absent in some edge cases, but most callers expect a
    /// "completed" status for successful responses.
    pub fn status_or_completed(&self) -> ResponseStatus {
        self.status.unwrap_or(ResponseStatus::Completed)
    }

    /// Get concatenated text from all message outputs.
    pub fn text(&self) -> String {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::Message { content, .. } = item {
                    Some(
                        content
                            .iter()
                            .filter_map(|c| c.as_text())
                            .collect::<Vec<_>>()
                            .join(""),
                    )
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get all function calls from the response.
    pub fn function_calls(&self) -> Vec<(&str, &str, &str)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::FunctionCall {
                    call_id,
                    name,
                    arguments,
                    ..
                } = item
                {
                    Some((call_id.as_str(), name.as_str(), arguments.as_str()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if response contains function calls.
    pub fn has_function_calls(&self) -> bool {
        self.output
            .iter()
            .any(|item| matches!(item, OutputItem::FunctionCall { .. }))
    }

    /// Get reasoning content if present, concatenating all content items.
    pub fn reasoning(&self) -> Option<String> {
        self.output.iter().find_map(|item| {
            if let OutputItem::Reasoning { content, .. } = item {
                content
                    .as_ref()
                    .map(|items| items.iter().map(|c| c.text.as_str()).collect::<String>())
            } else {
                None
            }
        })
    }

    /// Get encrypted reasoning content if present.
    pub fn encrypted_reasoning(&self) -> Option<&str> {
        self.output.iter().find_map(|item| {
            if let OutputItem::Reasoning {
                encrypted_content, ..
            } = item
            {
                encrypted_content.as_deref()
            } else {
                None
            }
        })
    }

    /// Get optional usage information.
    pub fn usage_opt(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }

    /// Get usage information, defaulting to zero tokens when missing.
    pub fn usage_or_default(&self) -> Usage {
        self.usage.clone().unwrap_or_default()
    }

    /// Get cached tokens used (from prompt caching).
    pub fn cached_tokens(&self) -> i32 {
        self.usage_opt()
            .map(super::usage::Usage::cached_tokens)
            .unwrap_or(0)
    }

    /// Check if response contains any tool calls (including function calls).
    pub fn has_tool_calls(&self) -> bool {
        self.output.iter().any(|item| {
            matches!(
                item,
                OutputItem::FunctionCall { .. }
                    | OutputItem::FileSearchCall { .. }
                    | OutputItem::WebSearchCall { .. }
                    | OutputItem::ComputerCall { .. }
                    | OutputItem::CodeInterpreterCall { .. }
                    | OutputItem::ImageGenerationCall { .. }
                    | OutputItem::LocalShellCall { .. }
                    | OutputItem::McpCall { .. }
                    | OutputItem::ApplyPatchCall { .. }
                    | OutputItem::FunctionShellCall { .. }
                    | OutputItem::CustomToolCall { .. }
            )
        })
    }

    /// Get all web search calls from the response.
    pub fn web_search_calls(&self) -> Vec<(Option<&str>, Option<&serde_json::Value>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::WebSearchCall { id, action, .. } = item {
                    Some((id.as_deref(), action.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all file search calls from the response.
    #[allow(clippy::type_complexity)]
    pub fn file_search_calls(
        &self,
    ) -> Vec<(Option<&str>, &[String], Option<&Vec<FileSearchResult>>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::FileSearchCall {
                    id,
                    queries,
                    results,
                    ..
                } = item
                {
                    Some((id.as_deref(), queries.as_slice(), results.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all computer calls from the response.
    pub fn computer_calls(&self) -> Vec<(&str, &ComputerAction)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::ComputerCall {
                    call_id, action, ..
                } = item
                {
                    Some((call_id.as_str(), action))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all code interpreter calls from the response.
    #[allow(clippy::type_complexity)]
    pub fn code_interpreter_calls(
        &self,
    ) -> Vec<(
        Option<&str>,
        Option<&str>,
        Option<&Vec<CodeInterpreterOutput>>,
    )> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::CodeInterpreterCall {
                    id, code, outputs, ..
                } = item
                {
                    Some((id.as_deref(), code.as_deref(), outputs.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all MCP calls from the response.
    pub fn mcp_calls(&self) -> Vec<MpcCallRef<'_>> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::McpCall {
                    id,
                    server_label,
                    name,
                    arguments,
                    output,
                    error,
                    ..
                } = item
                {
                    Some(MpcCallRef {
                        id: id.as_deref(),
                        server_label: server_label.as_deref(),
                        name: name.as_deref(),
                        arguments: arguments.as_deref(),
                        output: output.as_deref(),
                        error: error.as_deref(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all image generation calls from the response.
    pub fn image_generation_calls(&self) -> Vec<(Option<&str>, Option<&str>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::ImageGenerationCall { id, result, .. } = item {
                    Some((id.as_deref(), result.as_deref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all custom tool calls from the response.
    pub fn custom_tool_calls(&self) -> Vec<(&str, &str, &str)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::CustomToolCall {
                    call_id,
                    name,
                    input,
                    ..
                } = item
                {
                    Some((call_id.as_str(), name.as_str(), input.as_str()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all local shell calls from the response.
    pub fn local_shell_calls(&self) -> Vec<(&str, Option<&serde_json::Value>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::LocalShellCall {
                    call_id, action, ..
                } = item
                {
                    Some((call_id.as_str(), action.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Reference to an MCP call in a response.
#[derive(Debug, Clone)]
pub struct MpcCallRef<'a> {
    /// Unique ID.
    pub id: Option<&'a str>,
    /// MCP server label.
    pub server_label: Option<&'a str>,
    /// Tool name.
    pub name: Option<&'a str>,
    /// Tool arguments (JSON string).
    pub arguments: Option<&'a str>,
    /// Tool output.
    pub output: Option<&'a str>,
    /// Error if any.
    pub error: Option<&'a str>,
}

#[cfg(test)]
#[path = "responses.test.rs"]
mod tests;
