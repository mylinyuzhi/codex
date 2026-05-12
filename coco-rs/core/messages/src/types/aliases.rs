//! Version-agnostic LLM type aliases — shielded through `coco-inference`.
//!
//! All Message-family structs in this `types/` tree reference these aliases;
//! never reach for `vercel_ai_provider` directly. Upgrading the underlying
//! SDK only requires changing `services/inference/src/lib.rs` — this file
//! stays unchanged.

pub use coco_inference::AssistantContentPart as AssistantContent;
pub use coco_inference::FilePart as FileContent;
pub use coco_inference::LanguageModelMessage as LlmMessage;
pub use coco_inference::LanguageModelPrompt as LlmPrompt;
pub use coco_inference::ReasoningPart as ReasoningContent;
pub use coco_inference::TextPart as TextContent;
pub use coco_inference::ToolCallPart as ToolCallContent;
pub use coco_inference::ToolContentPart as ToolContent;
pub use coco_inference::ToolResultContentPart;
pub use coco_inference::ToolResultPart as ToolResultContent;
pub use coco_inference::UserContentPart as UserContent;

/// Construct a `ToolResultContentPart` that carries an Anthropic
/// `tool_reference` content block. The Anthropic API server expands
/// the block into inline `<functions>...</functions>` markup before
/// the prompt reaches the model — the client-side `tools` array is
/// not modified, preserving prompt-cache prefix.
///
/// Encoded via the `Custom` variant + `provider_options.anthropic`
/// shape that `vercel-ai-anthropic::convert_tool_result_content_part`
/// already recognizes:
/// ```json
/// { "type": "custom",
///   "providerOptions": {
///     "anthropic": {
///       "type": "tool-reference",
///       "toolName": "WebFetch"
///     }
///   }}
/// ```
///
/// Non-Anthropic providers will emit a `Warning::Other` and skip the
/// block; callers must gate on the model's
/// `Capability::ServerSideToolReference` before using this builder.
///
/// TS source: `ToolSearchTool.ts:444-470` returns these content
/// blocks directly. Skipping multi-provider concerns is intentional —
/// only the Anthropic adapter knows how to unwrap them, and the
/// engine arranges to call this helper only when capable.
#[must_use]
pub fn tool_reference_content_part(tool_name: impl Into<String>) -> ToolResultContentPart {
    use std::collections::HashMap;
    let mut anthropic = HashMap::new();
    anthropic.insert(
        "type".to_string(),
        serde_json::Value::String("tool-reference".to_string()),
    );
    anthropic.insert(
        "toolName".to_string(),
        serde_json::Value::String(tool_name.into()),
    );
    let mut map = HashMap::new();
    map.insert("anthropic".to_string(), anthropic);
    ToolResultContentPart::Custom {
        provider_options: Some(coco_inference::ProviderOptions(map)),
    }
}
