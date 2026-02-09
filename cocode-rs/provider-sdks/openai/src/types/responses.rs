//! Response API types.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use super::InputContentBlock;
use super::Metadata;
use super::OutputContentBlock;
use super::ResponseStatus;
use super::Role;
use super::StopReason;
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
    InMemory,
    /// Extended retention up to 24 hours.
    #[serde(rename = "24h")]
    TwentyFourHours,
}

/// Prompt caching configuration for requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCachingConfig {
    /// Cache key for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    /// Cache retention policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<PromptCacheRetention>,
}

impl PromptCachingConfig {
    /// Create a new prompt caching config with a cache key.
    pub fn with_key(key: impl Into<String>) -> Self {
        Self {
            cache_key: Some(key.into()),
            retention: None,
        }
    }

    /// Set the retention policy.
    pub fn retention(mut self, retention: PromptCacheRetention) -> Self {
        self.retention = Some(retention);
        self
    }
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
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
}

/// Reasoning configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    /// Effort level for reasoning.
    pub effort: ReasoningEffort,
    /// Whether to generate a summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_summary: Option<String>,
}

impl ReasoningConfig {
    /// Create a reasoning config with the given effort level.
    pub fn with_effort(effort: ReasoningEffort) -> Self {
        Self {
            effort,
            generate_summary: None,
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
    pub format: TextFormat,
}

impl TextConfig {
    /// Create a plain text config.
    pub fn text() -> Self {
        Self {
            format: TextFormat::Text,
        }
    }

    /// Create a JSON object config.
    pub fn json_object() -> Self {
        Self {
            format: TextFormat::JsonObject,
        }
    }

    /// Create a JSON schema config.
    pub fn json_schema(schema: serde_json::Value) -> Self {
        Self {
            format: TextFormat::JsonSchema {
                schema,
                name: None,
                strict: None,
            },
        }
    }

    /// Create a JSON schema config with name and strict mode.
    pub fn json_schema_strict(schema: serde_json::Value, name: impl Into<String>) -> Self {
        Self {
            format: TextFormat::JsonSchema {
                schema,
                name: Some(name.into()),
                strict: Some(true),
            },
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
// Input message
// ============================================================================

/// Input message for the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    /// Role of the message author.
    pub role: Role,

    /// Content blocks of the message.
    pub content: Vec<InputContentBlock>,
}

impl InputMessage {
    /// Create a user message with content blocks.
    pub fn user(content: Vec<InputContentBlock>) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    /// Create a user message with a single text block.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![InputContentBlock::text(text)],
        }
    }

    /// Create an assistant message with content blocks.
    pub fn assistant(content: Vec<InputContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// Create an assistant message with a single text block.
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![InputContentBlock::text(text)],
        }
    }

    /// Create a system message with a single text block.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: vec![InputContentBlock::text(text)],
        }
    }

    /// Create a developer message with a single text block.
    pub fn developer(text: impl Into<String>) -> Self {
        Self {
            role: Role::Developer,
            content: vec![InputContentBlock::text(text)],
        }
    }
}

// ============================================================================
// Response input (text or messages)
// ============================================================================

/// Input for response creation - can be simple text or messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInput {
    /// Simple text input.
    Text(String),
    /// Array of input messages.
    Messages(Vec<InputMessage>),
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

impl From<Vec<InputMessage>> for ResponseInput {
    fn from(messages: Vec<InputMessage>) -> Self {
        Self::Messages(messages)
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
    },
    /// Reasoning output from reasoning models.
    Reasoning {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Reasoning content.
        content: String,
        /// Reasoning summaries.
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<Vec<ReasoningSummary>>,
    },
    /// File search tool call.
    #[serde(rename = "file_search_call")]
    FileSearchCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
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
        /// Call ID.
        call_id: String,
        /// Search query.
        #[serde(skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        /// Search results.
        #[serde(skip_serializing_if = "Option::is_none")]
        results: Option<Vec<WebSearchResult>>,
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
        /// Call ID.
        call_id: String,
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
        /// Call ID.
        call_id: String,
        /// Generation prompt.
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        /// Generated image result.
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<ImageGenerationResult>,
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
        /// Shell command.
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// Command output.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
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
        /// Call ID.
        call_id: String,
        /// MCP server label.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_label: Option<String>,
        /// Tool name.
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        /// Tool arguments.
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<serde_json::Value>,
        /// Tool output.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Error if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
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
        tool_name: Option<String>,
        /// Arguments for the tool.
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<serde_json::Value>,
    },
    /// Apply patch tool call.
    #[serde(rename = "apply_patch_call")]
    ApplyPatchCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Patch content.
        #[serde(skip_serializing_if = "Option::is_none")]
        patch: Option<String>,
        /// Output result.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Function shell tool call.
    #[serde(rename = "function_shell_call")]
    FunctionShellCall {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Call ID.
        call_id: String,
        /// Shell command.
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// Command output.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
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
        /// Status of the call.
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Response compaction item.
    #[serde(rename = "compaction")]
    Compaction {
        /// Unique ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Compacted data.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
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
        /// Scroll direction.
        direction: String,
        /// Scroll amount.
        #[serde(skip_serializing_if = "Option::is_none")]
        amount: Option<i32>,
    },
    /// Type text action.
    Type {
        /// Text to type.
        text: String,
    },
    /// Key press action.
    KeyPress {
        /// Key to press.
        key: String,
    },
    /// Screenshot action.
    Screenshot,
    /// Wait action.
    Wait {
        /// Milliseconds to wait.
        #[serde(skip_serializing_if = "Option::is_none")]
        ms: Option<i32>,
    },
    /// Drag action.
    Drag {
        /// Start X coordinate.
        start_x: i32,
        /// Start Y coordinate.
        start_y: i32,
        /// End X coordinate.
        end_x: i32,
        /// End Y coordinate.
        end_y: i32,
    },
}

/// Safety check for computer actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    /// Check ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
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
        /// Image data (base64 or URL).
        #[serde(skip_serializing_if = "Option::is_none")]
        image: Option<String>,
        /// Image file ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
    },
}

/// Image generation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationResult {
    /// Generated image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Generated image base64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    /// Revised prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
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

    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,

    /// Whether to store the response server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    /// Prompt caching configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_caching: Option<PromptCachingConfig>,

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConversationParam {
    /// Reference by ID.
    Id(String),
    /// Inline conversation items.
    Items {
        /// Items to prepend.
        #[serde(default)]
        items: Vec<serde_json::Value>,
    },
}

/// Prompt template parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptParam {
    /// Prompt template ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Template variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<std::collections::HashMap<String, String>>,
}

impl ResponseCreateParams {
    /// Create new response parameters with message input.
    pub fn new(model: impl Into<String>, input: Vec<InputMessage>) -> Self {
        Self {
            model: model.into(),
            input: ResponseInput::Messages(input),
            instructions: None,
            max_output_tokens: None,
            tools: None,
            tool_choice: None,
            thinking: None,
            reasoning: None,
            previous_response_id: None,
            temperature: None,
            top_p: None,
            stop: None,
            store: None,
            prompt_caching: None,
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
            stop: None,
            store: None,
            prompt_caching: None,
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

    /// Set stop sequences.
    pub fn stop(mut self, sequences: Vec<String>) -> Self {
        self.stop = Some(sequences);
        self
    }

    /// Set whether to store the response.
    pub fn store(mut self, store: bool) -> Self {
        self.store = Some(store);
        self
    }

    /// Set prompt caching configuration.
    pub fn prompt_caching(mut self, config: PromptCachingConfig) -> Self {
        self.prompt_caching = Some(config);
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
    pub code: String,
    /// Error message.
    pub message: String,
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
}

/// Response from the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Unique response ID.
    pub id: String,

    /// Response status.
    pub status: ResponseStatus,

    /// Output items.
    pub output: Vec<OutputItem>,

    /// Token usage.
    pub usage: Usage,

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

    /// Reason generation stopped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,

    /// Completion timestamp (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    /// Details about why the response is incomplete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<IncompleteDetails>,

    /// System instructions (echoed back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

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

    /// HTTP response metadata (not serialized, populated by client).
    #[serde(skip)]
    pub sdk_http_response: Option<SdkHttpResponse>,
}

impl Response {
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

    /// Get reasoning content if present.
    pub fn reasoning(&self) -> Option<&str> {
        self.output.iter().find_map(|item| {
            if let OutputItem::Reasoning { content, .. } = item {
                Some(content.as_str())
            } else {
                None
            }
        })
    }

    /// Get cached tokens used (from prompt caching).
    pub fn cached_tokens(&self) -> i32 {
        self.usage.cached_tokens()
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
    pub fn web_search_calls(&self) -> Vec<(&str, Option<&str>, Option<&Vec<WebSearchResult>>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::WebSearchCall {
                    call_id,
                    query,
                    results,
                    ..
                } = item
                {
                    Some((call_id.as_str(), query.as_deref(), results.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all file search calls from the response.
    pub fn file_search_calls(&self) -> Vec<(&str, &[String], Option<&Vec<FileSearchResult>>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::FileSearchCall {
                    call_id,
                    queries,
                    results,
                    ..
                } = item
                {
                    Some((call_id.as_str(), queries.as_slice(), results.as_ref()))
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
    pub fn code_interpreter_calls(
        &self,
    ) -> Vec<(&str, Option<&str>, Option<&Vec<CodeInterpreterOutput>>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::CodeInterpreterCall {
                    call_id,
                    code,
                    outputs,
                    ..
                } = item
                {
                    Some((call_id.as_str(), code.as_deref(), outputs.as_ref()))
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
                    call_id,
                    server_label,
                    tool_name,
                    arguments,
                    output,
                    error,
                    ..
                } = item
                {
                    Some(MpcCallRef {
                        call_id: call_id.as_str(),
                        server_label: server_label.as_deref(),
                        tool_name: tool_name.as_deref(),
                        arguments: arguments.as_ref(),
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
    pub fn image_generation_calls(
        &self,
    ) -> Vec<(&str, Option<&str>, Option<&ImageGenerationResult>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::ImageGenerationCall {
                    call_id,
                    prompt,
                    result,
                    ..
                } = item
                {
                    Some((call_id.as_str(), prompt.as_deref(), result.as_ref()))
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
    pub fn local_shell_calls(&self) -> Vec<(&str, Option<&str>, Option<&str>)> {
        self.output
            .iter()
            .filter_map(|item| {
                if let OutputItem::LocalShellCall {
                    call_id,
                    command,
                    output,
                    ..
                } = item
                {
                    Some((call_id.as_str(), command.as_deref(), output.as_deref()))
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
    /// Call ID.
    pub call_id: &'a str,
    /// MCP server label.
    pub server_label: Option<&'a str>,
    /// Tool name.
    pub tool_name: Option<&'a str>,
    /// Tool arguments.
    pub arguments: Option<&'a serde_json::Value>,
    /// Tool output.
    pub output: Option<&'a str>,
    /// Error if any.
    pub error: Option<&'a str>,
}

#[cfg(test)]
#[path = "responses.test.rs"]
mod tests;
