//! Common types used across the OpenAI SDK.

use serde::Deserialize;
use serde::Serialize;

use crate::error::OpenAIError;
use crate::error::Result;

/// Conversation role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role.
    User,
    /// Assistant role.
    Assistant,
    /// System role.
    System,
    /// Developer role (for system instructions).
    Developer,
}

/// Reason the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of turn.
    EndTurn,
    /// Maximum tokens reached.
    MaxTokens,
    /// Stop sequence matched.
    StopSequence,
    /// Tool use requested.
    ToolUse,
    /// Content was filtered.
    ContentFilter,
}

/// Response status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    /// Response completed successfully.
    Completed,
    /// Response is in progress.
    InProgress,
    /// Response is incomplete.
    Incomplete,
    /// Response failed.
    Failed,
    /// Response was cancelled.
    Cancelled,
    /// Response is queued for processing.
    Queued,
}

/// Input format for custom tools (discriminated by `type`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomToolInputFormat {
    /// Unconstrained free-form text.
    Text,
    /// A grammar-based format.
    Grammar {
        /// The grammar definition string.
        definition: String,
        /// The syntax: "lark" or "regex".
        syntax: String,
    },
}

/// Tool definition - supports both function tools and built-in tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Tool {
    /// Function tool.
    Function {
        /// Function definition.
        #[serde(flatten)]
        function: FunctionDefinition,
    },
    /// Web search tool.
    WebSearch {
        /// Search context size.
        #[serde(skip_serializing_if = "Option::is_none")]
        search_context_size: Option<String>,
        /// User location for search.
        #[serde(skip_serializing_if = "Option::is_none")]
        user_location: Option<UserLocation>,
    },
    /// File search tool.
    FileSearch {
        /// Vector store IDs.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vector_store_ids: Vec<String>,
        /// Maximum results.
        #[serde(skip_serializing_if = "Option::is_none")]
        max_num_results: Option<i32>,
        /// Ranking options.
        #[serde(skip_serializing_if = "Option::is_none")]
        ranking_options: Option<RankingOptions>,
    },
    /// Code interpreter tool.
    CodeInterpreter {
        /// Container for code execution.
        #[serde(skip_serializing_if = "Option::is_none")]
        container: Option<String>,
    },
    /// Computer use tool.
    #[serde(rename = "computer_use_preview")]
    ComputerUse {
        /// Display width.
        display_width: i32,
        /// Display height.
        display_height: i32,
        /// Environment type.
        #[serde(skip_serializing_if = "Option::is_none")]
        environment: Option<String>,
    },
    /// Image generation tool.
    ImageGeneration {
        /// Image model.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Image size.
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<String>,
        /// Image quality.
        #[serde(skip_serializing_if = "Option::is_none")]
        quality: Option<String>,
        /// Response format.
        #[serde(skip_serializing_if = "Option::is_none")]
        output_format: Option<String>,
    },
    /// Local shell tool.
    LocalShell {
        /// Allowed commands.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_commands: Vec<String>,
    },
    /// MCP tool.
    Mcp {
        /// MCP server label.
        server_label: String,
        /// Server URL.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_url: Option<String>,
        /// Allowed tools.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_tools: Vec<String>,
        /// Require approval.
        #[serde(skip_serializing_if = "Option::is_none")]
        require_approval: Option<String>,
    },
    /// Apply patch tool.
    ApplyPatch,
    /// Function shell tool.
    #[serde(rename = "function_shell")]
    FunctionShell {
        /// Shell command template.
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
    /// Custom tool.
    Custom {
        /// Custom tool name.
        name: String,
        /// Tool description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Input format constraint.
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<CustomToolInputFormat>,
    },
}

/// Function definition for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Name of the function.
    pub name: String,

    /// Description of the function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,

    /// Whether to enable strict schema adherence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// User location for web search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLocation {
    /// Country code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Region/state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// City.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
}

/// Ranking options for file search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingOptions {
    /// Ranker type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranker: Option<String>,
    /// Score threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
}

impl Tool {
    /// Create a new function tool.
    pub fn function(
        name: impl Into<String>,
        description: Option<String>,
        parameters: serde_json::Value,
    ) -> Result<Self> {
        let name = name.into();
        if name.is_empty() || name.len() > 64 {
            return Err(OpenAIError::Validation(
                "function name must be 1-64 characters".to_string(),
            ));
        }
        Ok(Self::Function {
            function: FunctionDefinition {
                name,
                description,
                parameters,
                strict: None,
            },
        })
    }

    /// Create a web search tool.
    pub fn web_search() -> Self {
        Self::WebSearch {
            search_context_size: None,
            user_location: None,
        }
    }

    /// Create a file search tool.
    pub fn file_search(vector_store_ids: Vec<String>) -> Self {
        Self::FileSearch {
            vector_store_ids,
            max_num_results: None,
            ranking_options: None,
        }
    }

    /// Create a code interpreter tool.
    pub fn code_interpreter() -> Self {
        Self::CodeInterpreter { container: None }
    }

    /// Create a computer use tool.
    pub fn computer_use(display_width: i32, display_height: i32) -> Self {
        Self::ComputerUse {
            display_width,
            display_height,
            environment: None,
        }
    }

    /// Create an image generation tool.
    pub fn image_generation() -> Self {
        Self::ImageGeneration {
            model: None,
            size: None,
            quality: None,
            output_format: None,
        }
    }

    /// Create a local shell tool.
    pub fn local_shell() -> Self {
        Self::LocalShell {
            allowed_commands: vec![],
        }
    }

    /// Create an MCP tool.
    pub fn mcp(server_label: impl Into<String>) -> Self {
        Self::Mcp {
            server_label: server_label.into(),
            server_url: None,
            allowed_tools: vec![],
            require_approval: None,
        }
    }

    /// Create an apply patch tool.
    pub fn apply_patch() -> Self {
        Self::ApplyPatch
    }

    /// Create a custom tool with grammar format.
    pub fn custom_with_grammar(
        name: impl Into<String>,
        description: impl Into<String>,
        syntax: impl Into<String>,
        definition: impl Into<String>,
    ) -> Self {
        Self::Custom {
            name: name.into(),
            description: Some(description.into()),
            format: Some(CustomToolInputFormat::Grammar {
                syntax: syntax.into(),
                definition: definition.into(),
            }),
        }
    }

    /// Create a custom tool with unconstrained text format.
    pub fn custom_text(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::Custom {
            name: name.into(),
            description: Some(description.into()),
            format: Some(CustomToolInputFormat::Text),
        }
    }

    /// Create a custom tool with no format constraint.
    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom {
            name: name.into(),
            description: None,
            format: None,
        }
    }

    /// Set strict mode for function tools.
    pub fn strict(mut self, strict: bool) -> Self {
        if let Self::Function { ref mut function } = self {
            function.strict = Some(strict);
        }
        self
    }

    /// Set search context size for web search tool.
    pub fn with_search_context_size(mut self, size: impl Into<String>) -> Self {
        if let Self::WebSearch {
            ref mut search_context_size,
            ..
        } = self
        {
            *search_context_size = Some(size.into());
        }
        self
    }

    /// Set user location for web search tool.
    pub fn with_user_location(mut self, location: UserLocation) -> Self {
        if let Self::WebSearch {
            ref mut user_location,
            ..
        } = self
        {
            *user_location = Some(location);
        }
        self
    }

    /// Set max results for file search tool.
    pub fn with_max_results(mut self, max: i32) -> Self {
        if let Self::FileSearch {
            ref mut max_num_results,
            ..
        } = self
        {
            *max_num_results = Some(max);
        }
        self
    }

    /// Set ranking options for file search tool.
    pub fn with_ranking_options(mut self, options: RankingOptions) -> Self {
        if let Self::FileSearch {
            ref mut ranking_options,
            ..
        } = self
        {
            *ranking_options = Some(options);
        }
        self
    }

    /// Set container for code interpreter tool.
    pub fn with_container(mut self, container_id: impl Into<String>) -> Self {
        if let Self::CodeInterpreter { ref mut container } = self {
            *container = Some(container_id.into());
        }
        self
    }

    /// Set environment for computer use tool.
    pub fn with_environment(mut self, env: impl Into<String>) -> Self {
        if let Self::ComputerUse {
            ref mut environment,
            ..
        } = self
        {
            *environment = Some(env.into());
        }
        self
    }

    /// Set model for image generation tool.
    pub fn with_model(mut self, model_name: impl Into<String>) -> Self {
        if let Self::ImageGeneration { ref mut model, .. } = self {
            *model = Some(model_name.into());
        }
        self
    }

    /// Set size for image generation tool.
    pub fn with_size(mut self, image_size: impl Into<String>) -> Self {
        if let Self::ImageGeneration { ref mut size, .. } = self {
            *size = Some(image_size.into());
        }
        self
    }

    /// Set quality for image generation tool.
    pub fn with_quality(mut self, image_quality: impl Into<String>) -> Self {
        if let Self::ImageGeneration {
            ref mut quality, ..
        } = self
        {
            *quality = Some(image_quality.into());
        }
        self
    }

    /// Set allowed commands for local shell tool.
    pub fn with_allowed_commands(mut self, commands: Vec<String>) -> Self {
        if let Self::LocalShell {
            ref mut allowed_commands,
        } = self
        {
            *allowed_commands = commands;
        }
        self
    }

    /// Set server URL for MCP tool.
    pub fn with_server_url(mut self, url: impl Into<String>) -> Self {
        if let Self::Mcp {
            ref mut server_url, ..
        } = self
        {
            *server_url = Some(url.into());
        }
        self
    }

    /// Set allowed tools for MCP tool.
    pub fn with_allowed_tools(mut self, tools: Vec<String>) -> Self {
        if let Self::Mcp {
            ref mut allowed_tools,
            ..
        } = self
        {
            *allowed_tools = tools;
        }
        self
    }

    /// Set require approval for MCP tool.
    pub fn with_require_approval(mut self, approval: impl Into<String>) -> Self {
        if let Self::Mcp {
            ref mut require_approval,
            ..
        } = self
        {
            *require_approval = Some(approval.into());
        }
        self
    }
}

/// Tool choice configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    /// Let the model decide whether to use tools.
    Auto,
    /// Do not use any tools.
    None,
    /// Require the model to use a tool.
    Required,
    /// Force use of a specific function.
    Function {
        /// Name of the function to call.
        name: String,
    },
    /// Constrain to a set of allowed tools.
    Allowed {
        /// List of allowed tool names.
        #[serde(default)]
        tools: Vec<String>,
        /// Mode: auto or required.
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
    },
    /// Force a specific built-in tool type.
    #[serde(rename = "web_search")]
    WebSearch,
    /// Force file search tool.
    #[serde(rename = "file_search")]
    FileSearch,
    /// Force code interpreter tool.
    #[serde(rename = "code_interpreter")]
    CodeInterpreter,
    /// Force computer use tool.
    #[serde(rename = "computer_use_preview")]
    ComputerUse,
    /// Force image generation tool.
    #[serde(rename = "image_generation")]
    ImageGeneration,
    /// Force MCP tool.
    Mcp {
        /// MCP server label.
        server_label: String,
        /// Tool name.
        tool_name: String,
    },
    /// Force shell tool.
    Shell,
    /// Force apply patch tool.
    #[serde(rename = "apply_patch")]
    ApplyPatch,
    /// Force custom tool.
    Custom {
        /// Custom tool name.
        name: String,
    },
}

/// Metadata for requests and responses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    /// Custom key-value pairs.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

impl Metadata {
    /// Create empty metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a key-value pair.
    pub fn insert(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
#[path = "common.test.rs"]
mod tests;
