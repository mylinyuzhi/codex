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
