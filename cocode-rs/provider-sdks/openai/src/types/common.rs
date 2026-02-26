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
        /// Search filters.
        #[serde(skip_serializing_if = "Option::is_none")]
        filters: Option<serde_json::Value>,
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
        /// Search filters.
        #[serde(skip_serializing_if = "Option::is_none")]
        filters: Option<serde_json::Value>,
    },
    /// Code interpreter tool.
    CodeInterpreter {
        /// Container for code execution (string ID or auto object).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        container: Option<serde_json::Value>,
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
        /// Background type.
        #[serde(skip_serializing_if = "Option::is_none")]
        background: Option<String>,
        /// Fidelity control.
        #[serde(skip_serializing_if = "Option::is_none")]
        input_fidelity: Option<String>,
        /// Inpainting mask.
        #[serde(skip_serializing_if = "Option::is_none")]
        input_image_mask: Option<serde_json::Value>,
        /// Moderation level.
        #[serde(skip_serializing_if = "Option::is_none")]
        moderation: Option<String>,
        /// Compression level.
        #[serde(skip_serializing_if = "Option::is_none")]
        output_compression: Option<i32>,
        /// Partial images count.
        #[serde(skip_serializing_if = "Option::is_none")]
        partial_images: Option<i32>,
    },
    /// Local shell tool.
    LocalShell,
    /// MCP tool.
    Mcp {
        /// MCP server label.
        server_label: String,
        /// Server URL.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_url: Option<String>,
        /// Allowed tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allowed_tools: Option<serde_json::Value>,
        /// Require approval.
        #[serde(skip_serializing_if = "Option::is_none")]
        require_approval: Option<String>,
        /// Authorization token.
        #[serde(skip_serializing_if = "Option::is_none")]
        authorization: Option<String>,
        /// Connector ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        connector_id: Option<String>,
        /// Custom headers.
        #[serde(skip_serializing_if = "Option::is_none")]
        headers: Option<std::collections::HashMap<String, String>>,
        /// Server description.
        #[serde(skip_serializing_if = "Option::is_none")]
        server_description: Option<String>,
    },
    /// Apply patch tool.
    ApplyPatch,
    /// Shell tool (function shell).
    #[serde(rename = "shell")]
    FunctionShell,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,

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
    /// Timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Location type (e.g., "approximate").
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub location_type: Option<String>,
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
    /// Hybrid search configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hybrid_search: Option<serde_json::Value>,
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
                parameters: Some(parameters),
                strict: None,
            },
        })
    }

    /// Create a web search tool.
    pub fn web_search() -> Self {
        Self::WebSearch {
            search_context_size: None,
            user_location: None,
            filters: None,
        }
    }

    /// Create a file search tool.
    pub fn file_search(vector_store_ids: Vec<String>) -> Self {
        Self::FileSearch {
            vector_store_ids,
            max_num_results: None,
            ranking_options: None,
            filters: None,
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
            background: None,
            input_fidelity: None,
            input_image_mask: None,
            moderation: None,
            output_compression: None,
            partial_images: None,
        }
    }

    /// Create a local shell tool.
    pub fn local_shell() -> Self {
        Self::LocalShell
    }

    /// Create an MCP tool.
    pub fn mcp(server_label: impl Into<String>) -> Self {
        Self::Mcp {
            server_label: server_label.into(),
            server_url: None,
            allowed_tools: None,
            require_approval: None,
            authorization: None,
            connector_id: None,
            headers: None,
            server_description: None,
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
            *container = Some(serde_json::Value::String(container_id.into()));
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
    pub fn with_allowed_tools(mut self, tools: serde_json::Value) -> Self {
        if let Self::Mcp {
            ref mut allowed_tools,
            ..
        } = self
        {
            *allowed_tools = Some(tools);
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
///
/// Aligns with the Python SDK's `ToolChoice` union type.
/// Serializes tagged objects (`{"type":"auto"}`), deserializes both plain strings
/// (`"auto"`) and tagged objects.
#[derive(Debug, Clone, Serialize)]
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
    #[serde(rename = "allowed_tools")]
    Allowed {
        /// Mode: "auto" or "required".
        mode: String,
        /// List of allowed tools (opaque objects from the API).
        tools: Vec<serde_json::Value>,
    },
    /// Force web search tool.
    #[serde(rename = "web_search_preview")]
    WebSearch,
    /// Force web search tool (2025-03-11 version).
    #[serde(rename = "web_search_preview_2025_03_11")]
    WebSearchPreview20250311,
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
        /// Optional tool name.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
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

impl<'de> serde::Deserialize<'de> for ToolChoice {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            // Plain string form: "auto", "none", "required"
            serde_json::Value::String(s) => match s.as_str() {
                "auto" => Ok(ToolChoice::Auto),
                "none" => Ok(ToolChoice::None),
                "required" => Ok(ToolChoice::Required),
                other => Err(serde::de::Error::unknown_variant(
                    other,
                    &["auto", "none", "required"],
                )),
            },
            // Object form: discriminated by "type" field
            serde_json::Value::Object(map) => {
                let type_str = map
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::missing_field("type"))?;
                match type_str {
                    "auto" => Ok(ToolChoice::Auto),
                    "none" => Ok(ToolChoice::None),
                    "required" => Ok(ToolChoice::Required),
                    "function" => {
                        let name = map
                            .get("name")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| serde::de::Error::missing_field("name"))?
                            .to_string();
                        Ok(ToolChoice::Function { name })
                    }
                    "allowed_tools" => {
                        let mode = map
                            .get("mode")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| serde::de::Error::missing_field("mode"))?
                            .to_string();
                        let tools = map
                            .get("tools")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        Ok(ToolChoice::Allowed { mode, tools })
                    }
                    "web_search_preview" => Ok(ToolChoice::WebSearch),
                    "web_search_preview_2025_03_11" => Ok(ToolChoice::WebSearchPreview20250311),
                    "file_search" => Ok(ToolChoice::FileSearch),
                    "code_interpreter" => Ok(ToolChoice::CodeInterpreter),
                    "computer_use_preview" => Ok(ToolChoice::ComputerUse),
                    "image_generation" => Ok(ToolChoice::ImageGeneration),
                    "mcp" => {
                        let server_label = map
                            .get("server_label")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| serde::de::Error::missing_field("server_label"))?
                            .to_string();
                        let name = map
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(std::string::ToString::to_string);
                        Ok(ToolChoice::Mcp { server_label, name })
                    }
                    "shell" => Ok(ToolChoice::Shell),
                    "apply_patch" => Ok(ToolChoice::ApplyPatch),
                    "custom" => {
                        let name = map
                            .get("name")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| serde::de::Error::missing_field("name"))?
                            .to_string();
                        Ok(ToolChoice::Custom { name })
                    }
                    other => Err(serde::de::Error::unknown_variant(
                        other,
                        &[
                            "auto",
                            "none",
                            "required",
                            "function",
                            "allowed_tools",
                            "web_search_preview",
                            "web_search_preview_2025_03_11",
                            "file_search",
                            "code_interpreter",
                            "computer_use_preview",
                            "image_generation",
                            "mcp",
                            "shell",
                            "apply_patch",
                            "custom",
                        ],
                    )),
                }
            }
            _ => Err(serde::de::Error::custom("expected string or object")),
        }
    }
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
