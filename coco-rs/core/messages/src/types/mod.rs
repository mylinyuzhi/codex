//! Types relocated from `coco-types` to live with the message machinery.
//!
//! Foundational, provider-agnostic types still live in `coco-types`; anything
//! that embeds an LLM body — `Message`, `ToolResult::new_messages`,
//! `HookResult.message`, `TranscriptMessage.message`, … — lives here.
//!
//! All LLM types are aliased through `coco-inference` (the seam crate).
//! Never reach for `vercel_ai_provider` directly.

pub mod aliases;
pub mod attachment_body;
pub mod attachment_emitter;
pub mod hook_result;
pub mod message;
pub mod serialized_message;
pub mod tool_result;
pub mod transcript;

pub use aliases::AssistantContent;
pub use aliases::FileContent;
pub use aliases::LlmMessage;
pub use aliases::LlmPrompt;
pub use aliases::ReasoningContent;
pub use aliases::TextContent;
pub use aliases::ToolCallContent;
pub use aliases::ToolContent;
pub use aliases::ToolResultContent;
pub use aliases::UserContent;

pub use attachment_body::AlreadyReadFilePayload;
pub use attachment_body::AttachmentBody;
pub use attachment_body::CommandPermissionsPayload;
pub use attachment_body::DynamicSkillPayload;
pub use attachment_body::EditedImageFilePayload;
pub use attachment_body::HookCancelledPayload;
pub use attachment_body::HookErrorDuringExecutionPayload;
pub use attachment_body::HookNonBlockingErrorPayload;
pub use attachment_body::HookPermissionDecision;
pub use attachment_body::HookPermissionDecisionPayload;
pub use attachment_body::HookSystemMessagePayload;
pub use attachment_body::SilentPayload;
pub use attachment_body::StructuredOutputPayload;

pub use attachment_emitter::AttachmentEmitter;

pub use hook_result::HookResult;

pub use message::ApiError;
pub use message::AssistantMessage;
pub use message::AttachmentMessage;
pub use message::Message;
pub use message::MessageKind;
pub use message::MessageOrigin;
pub use message::PartialCompactDirection;
pub use message::PreservedSegment;
pub use message::ProgressMessage;
pub use message::StopReason;
pub use message::SystemAgentsKilledMessage;
pub use message::SystemApiErrorMessage;
pub use message::SystemApiMetricsMessage;
pub use message::SystemAwaySummaryMessage;
pub use message::SystemBridgeStatusMessage;
pub use message::SystemCompactBoundaryMessage;
pub use message::SystemInformationalMessage;
pub use message::SystemLocalCommandMessage;
pub use message::SystemMemorySavedMessage;
pub use message::SystemMessage;
pub use message::SystemMessageLevel;
pub use message::SystemMicrocompactBoundaryMessage;
pub use message::SystemPermissionRetryMessage;
pub use message::SystemScheduledTaskFireMessage;
pub use message::SystemStopHookSummaryMessage;
pub use message::SystemTurnDurationMessage;
pub use message::TombstoneMessage;
pub use message::ToolResultMessage;
pub use message::ToolUseSummaryMessage;
pub use message::UserMessage;
pub use message::Visibility;

pub use serialized_message::SerializedMessage;

pub use tool_result::ToolResult;

pub use transcript::TranscriptEntry;
pub use transcript::TranscriptMessage;
