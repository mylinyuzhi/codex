//! Content part types for messages.
//!
//! Content is structured as typed parts that differ based on the message role:
//! - UserContentPart: Text, File
//! - AssistantContentPart: Text, File, Reasoning, ToolCall, ToolResult, Source, ToolApprovalRequest
//! - ToolContentPart: ToolResult, ToolApprovalResponse

use serde::Deserialize;
use serde::Serialize;

use crate::data_content::DataContent;
use crate::json_value::JSONValue;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;

/// A text content part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPart {
    /// The text content.
    pub text: String,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextPart {
    /// Create a new text part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// A file content part (image, document, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePart {
    /// The file data.
    pub data: DataContent,
    /// The MIME type of the file.
    pub media_type: String,
    /// Optional filename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl FilePart {
    /// Create a new file part.
    pub fn new(data: DataContent, media_type: impl Into<String>) -> Self {
        Self {
            data,
            media_type: media_type.into(),
            filename: None,
            provider_metadata: None,
        }
    }

    /// Create an image file part from bytes.
    pub fn image(bytes: Vec<u8>, media_type: impl Into<String>) -> Self {
        Self::new(DataContent::from_bytes(bytes), media_type)
    }

    /// Create an image file part from a URL.
    pub fn image_url(url: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::new(DataContent::from_url(url), media_type)
    }

    /// Create an image file part from base64.
    pub fn image_base64(base64: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::new(DataContent::from_base64(base64), media_type)
    }

    /// Set the filename.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// A reasoning content part (for thinking models).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningPart {
    /// The reasoning text.
    pub text: String,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ReasoningPart {
    /// Create a new reasoning part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// A tool call content part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallPart {
    /// The tool call ID.
    pub tool_call_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The tool arguments as JSON.
    pub input: JSONValue,
    /// Whether the tool call will be executed by the provider.
    /// If this flag is not set or is false, the tool call will be executed by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolCallPart {
    /// Create a new tool call part.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JSONValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            provider_executed: None,
            provider_metadata: None,
        }
    }

    /// Set whether the tool is executed by the provider.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// A tool result content part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultPart {
    /// The tool call ID this result is for.
    pub tool_call_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The result content.
    pub output: ToolResultContent,
    /// Whether the tool call was successful.
    #[serde(default)]
    pub is_error: bool,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolResultPart {
    /// Create a new tool result part.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        output: ToolResultContent,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            output,
            is_error: false,
            provider_metadata: None,
        }
    }

    /// Mark the result as an error.
    pub fn with_error(mut self) -> Self {
        self.is_error = true;
        self
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// Content of a tool result.
///
/// This matches the LanguageModelV4ToolResultOutput type from the v4 spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultContent {
    /// Text tool output that should be directly sent to the API.
    Text {
        /// The text content.
        value: String,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// JSON tool output.
    Json {
        /// The JSON content.
        value: JSONValue,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Type when the user has denied the execution of the tool call.
    ExecutionDenied {
        /// Optional reason for the execution denial.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Error text output.
    ErrorText {
        /// The error message.
        value: String,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Error JSON output.
    ErrorJson {
        /// The error JSON.
        value: JSONValue,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Multiple content parts.
    Content {
        /// The content parts.
        value: Vec<ToolResultContentPart>,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
}

impl ToolResultContent {
    /// Create text content.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            value: content.into(),
            provider_options: None,
        }
    }

    /// Create JSON content.
    pub fn json(content: JSONValue) -> Self {
        Self::Json {
            value: content,
            provider_options: None,
        }
    }

    /// Create execution denied content.
    pub fn execution_denied(reason: Option<String>) -> Self {
        Self::ExecutionDenied {
            reason,
            provider_options: None,
        }
    }

    /// Create error text content.
    pub fn error_text(message: impl Into<String>) -> Self {
        Self::ErrorText {
            value: message.into(),
            provider_options: None,
        }
    }

    /// Create error JSON content.
    pub fn error_json(error: JSONValue) -> Self {
        Self::ErrorJson {
            value: error,
            provider_options: None,
        }
    }

    /// Create from multiple parts.
    pub fn content_parts(parts: Vec<ToolResultContentPart>) -> Self {
        Self::Content {
            value: parts,
            provider_options: None,
        }
    }
}

impl From<String> for ToolResultContent {
    fn from(s: String) -> Self {
        Self::text(s)
    }
}

impl From<&str> for ToolResultContent {
    fn from(s: &str) -> Self {
        Self::text(s)
    }
}

/// A part of tool result content.
///
/// Matches the content array items in LanguageModelV4ToolResultOutput.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultContentPart {
    /// Text content.
    Text {
        /// The text content.
        text: String,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// File data (base64 encoded).
    FileData {
        /// Base-64 encoded media data.
        data: String,
        /// IANA media type.
        #[serde(rename = "mediaType")]
        media_type: String,
        /// Optional filename.
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// File URL reference.
    FileUrl {
        /// URL of the file.
        url: String,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// File ID reference.
    FileId {
        /// ID of the file, can be a string or a map of provider to ID.
        #[serde(rename = "fileId")]
        file_id: FileIdReference,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Image data (base64 encoded).
    ImageData {
        /// Base-64 encoded image data.
        data: String,
        /// IANA media type.
        #[serde(rename = "mediaType")]
        media_type: String,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Image URL reference.
    ImageUrl {
        /// URL of the image.
        url: String,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Image file ID reference.
    ImageFileId {
        /// Image file ID, can be a string or a map of provider to ID.
        #[serde(rename = "fileId")]
        file_id: FileIdReference,
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
    /// Custom content part for provider-specific content.
    Custom {
        /// Provider-specific options.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderMetadata>,
    },
}

impl ToolResultContentPart {
    /// Create a text content part.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            text: content.into(),
            provider_options: None,
        }
    }

    /// Create a file data part.
    pub fn file_data(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::FileData {
            data: data.into(),
            media_type: media_type.into(),
            filename: None,
            provider_options: None,
        }
    }

    /// Create an image data part.
    pub fn image_data(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::ImageData {
            data: data.into(),
            media_type: media_type.into(),
            provider_options: None,
        }
    }

    /// Create an image URL part.
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::ImageUrl {
            url: url.into(),
            provider_options: None,
        }
    }
}

/// File ID reference that can be either a single string or a provider-to-ID mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileIdReference {
    /// A single file ID string.
    Single(String),
    /// A mapping from provider name to file ID.
    Mapped(std::collections::HashMap<String, String>),
}

impl From<String> for FileIdReference {
    fn from(s: String) -> Self {
        Self::Single(s)
    }
}

impl From<&str> for FileIdReference {
    fn from(s: &str) -> Self {
        Self::Single(s.to_string())
    }
}

/// User message content parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UserContentPart {
    /// Text content.
    Text(TextPart),
    /// File content (image, document, etc.).
    File(FilePart),
}

impl UserContentPart {
    /// Create a text part.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text(TextPart::new(content))
    }

    /// Create a file part.
    pub fn file(data: DataContent, media_type: impl Into<String>) -> Self {
        Self::File(FilePart::new(data, media_type))
    }

    /// Create an image from bytes.
    pub fn image(bytes: Vec<u8>, media_type: impl Into<String>) -> Self {
        Self::File(FilePart::image(bytes, media_type))
    }

    /// Create an image from a URL.
    pub fn image_url(url: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::File(FilePart::image_url(url, media_type))
    }
}

/// A reasoning file content part (file data that is part of reasoning).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningFilePart {
    /// The file data.
    pub data: DataContent,
    /// The MIME type of the file.
    pub media_type: String,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ReasoningFilePart {
    /// Create a new reasoning file part.
    pub fn new(data: DataContent, media_type: impl Into<String>) -> Self {
        Self {
            data,
            media_type: media_type.into(),
            provider_metadata: None,
        }
    }

    /// Create from base64 data.
    pub fn from_base64(base64: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::new(DataContent::from_base64(base64), media_type)
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// A custom content part for provider-specific extensions.
///
/// Used in both prompts (with `provider_options`) and responses (with `provider_metadata`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPart {
    /// The kind of custom content.
    pub kind: String,
    /// Provider-specific options (prompt-side).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    /// Provider-specific metadata (response-side).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl CustomPart {
    /// Create a new custom part.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            provider_options: None,
            provider_metadata: None,
        }
    }

    /// Add provider options (prompt-side).
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Add provider metadata (response-side).
    pub fn with_provider_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// Assistant message content parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantContentPart {
    /// Text content.
    Text(TextPart),
    /// File content.
    File(FilePart),
    /// Reasoning content (for thinking models).
    Reasoning(ReasoningPart),
    /// Reasoning file content (file data that is part of reasoning).
    ReasoningFile(ReasoningFilePart),
    /// Custom content for provider-specific extensions.
    Custom(CustomPart),
    /// Tool call.
    ToolCall(ToolCallPart),
    /// Tool result (when assistant receives tool results).
    ToolResult(ToolResultPart),
    /// Source reference (for citations).
    Source(SourcePart),
    /// Tool approval request (for provider-executed tools).
    ToolApprovalRequest(ToolApprovalRequestPart),
}

impl AssistantContentPart {
    /// Create a text part.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text(TextPart::new(content))
    }

    /// Create a reasoning part.
    pub fn reasoning(content: impl Into<String>) -> Self {
        Self::Reasoning(ReasoningPart::new(content))
    }

    /// Create a tool call part.
    pub fn tool_call(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JSONValue,
    ) -> Self {
        Self::ToolCall(ToolCallPart::new(tool_call_id, tool_name, input))
    }

    /// Create a source part.
    pub fn source(id: impl Into<String>, source_type: SourceType) -> Self {
        Self::Source(SourcePart::new(id, source_type))
    }

    /// Create a custom content part.
    pub fn custom(kind: impl Into<String>) -> Self {
        Self::Custom(CustomPart::new(kind))
    }

    /// Create a tool approval request part.
    pub fn tool_approval_request(
        approval_id: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self::ToolApprovalRequest(ToolApprovalRequestPart::new(approval_id, tool_call_id))
    }
}

/// A source reference content part (for citations).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePart {
    /// The source type (url or document).
    pub source_type: SourceType,
    /// The source ID.
    pub id: String,
    /// The source URL (for URL sources).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The source title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The MIME type (for document sources).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Optional filename (for document sources).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl SourcePart {
    /// Create a new source part.
    pub fn new(id: impl Into<String>, source_type: SourceType) -> Self {
        Self {
            source_type,
            id: id.into(),
            url: None,
            title: None,
            media_type: None,
            filename: None,
            provider_metadata: None,
        }
    }

    /// Create a URL source.
    pub fn url(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            source_type: SourceType::Url,
            id: id.into(),
            url: Some(url.into()),
            title: None,
            media_type: None,
            filename: None,
            provider_metadata: None,
        }
    }

    /// Create a URL source (alias for [`url`](Self::url)).
    pub fn url_source(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self::url(id, url)
    }

    /// Create a document source with title and media type.
    pub fn document(
        id: impl Into<String>,
        title: impl Into<String>,
        media_type: impl Into<String>,
    ) -> Self {
        Self {
            source_type: SourceType::Document,
            id: id.into(),
            url: None,
            title: Some(title.into()),
            media_type: Some(media_type.into()),
            filename: None,
            provider_metadata: None,
        }
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }

    /// Set the filename.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

/// Types of sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    /// URL source referencing web content.
    Url,
    /// Document source referencing files/documents.
    Document,
}

/// A tool approval request content part (for provider-executed tools).
///
/// This is used for flows where the provider executes the tool (e.g. MCP tools)
/// but requires an explicit user approval before continuing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequestPart {
    /// ID of the approval request. This ID is referenced by the subsequent
    /// tool-approval-response (tool message) to approve or deny execution.
    pub approval_id: String,
    /// The tool call ID that this approval request is for.
    pub tool_call_id: String,
    /// The tool name (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Additional context about the tool call (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolApprovalRequestPart {
    /// Create a new tool approval request part.
    pub fn new(approval_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            tool_name: None,
            context: None,
            provider_metadata: None,
        }
    }

    /// Set the tool name.
    pub fn with_tool_name(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }

    /// Set additional context.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Add provider metadata.
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// Tool message content parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolContentPart {
    /// Tool result.
    ToolResult(ToolResultPart),
    /// Tool approval response.
    ToolApprovalResponse(ToolApprovalResponsePart),
}

/// A tool approval response part (for tools that require approval).
///
/// This contains the user's decision to approve or deny a provider-executed tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResponsePart {
    /// ID of the approval request that this response refers to.
    pub approval_id: String,
    /// Whether the approval was granted (true) or denied (false).
    pub approved: bool,
    /// Optional reason for approval or denial.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolApprovalResponsePart {
    /// Create a new tool approval response.
    pub fn new(approval_id: impl Into<String>, approved: bool) -> Self {
        Self {
            approval_id: approval_id.into(),
            approved,
            reason: None,
            provider_metadata: None,
        }
    }

    /// Add a reason for the approval or denial.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[cfg(test)]
#[path = "content.test.rs"]
mod tests;
