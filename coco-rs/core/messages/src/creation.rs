use coco_types::ApiError;
use coco_types::AssistantContent;
use coco_types::AssistantMessage;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::MessageOrigin;
use coco_types::ProgressMessage;
use coco_types::SystemCompactBoundaryMessage;
use coco_types::SystemInformationalMessage;
use coco_types::SystemMessage;
use coco_types::SystemMessageLevel;
use coco_types::TokenUsage;
use coco_types::ToolContent;
use coco_types::ToolId;
use coco_types::ToolResultMessage;
use coco_types::UserMessage;
use uuid::Uuid;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::UserContentPart;

/// Create a user message from text content.
pub fn create_user_message(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_meta: false,
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: Some(MessageOrigin::UserInput),
    })
}

/// Create a user message with mixed content parts (text + images).
///
/// Used when the user input includes @-mentioned images or pasted images
/// alongside text. The provider layer (e.g. Anthropic) already handles
/// `UserContentPart::File` with image/* media types.
pub fn create_user_message_with_parts(parts: Vec<UserContentPart>) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user(parts),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_meta: false,
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: Some(MessageOrigin::UserInput),
    })
}

/// Create a system-injected meta message (hidden from UI, visible to model).
pub fn create_meta_message(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_meta: true,
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: Some(MessageOrigin::SystemInjected),
    })
}

/// Create an informational system message.
pub fn create_info_message(title: &str, message: &str) -> Message {
    Message::System(SystemMessage::Informational(SystemInformationalMessage {
        uuid: Uuid::new_v4(),
        level: SystemMessageLevel::Info,
        title: title.to_string(),
        message: message.to_string(),
    }))
}

/// Create an assistant message with content parts, model, and usage.
pub fn create_assistant_message(
    content: Vec<AssistantContent>,
    model: &str,
    usage: TokenUsage,
) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::assistant(content),
        uuid: Uuid::new_v4(),
        model: model.to_string(),
        stop_reason: None,
        usage: Some(usage),
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

/// Create a tool result message.
pub fn create_tool_result_message(
    tool_call_id: &str,
    tool_name: &str,
    tool_id: ToolId,
    output: &str,
    is_error: bool,
) -> Message {
    let result_content = if is_error {
        ToolResultContent::error_text(output)
    } else {
        ToolResultContent::text(output)
    };
    let tool_result = coco_types::ToolResultContent {
        tool_call_id: tool_call_id.to_string(),
        tool_name: tool_name.to_string(),
        output: result_content,
        is_error,
        provider_metadata: None,
    };
    Message::ToolResult(ToolResultMessage {
        uuid: Uuid::new_v4(),
        message: LlmMessage::tool(vec![ToolContent::ToolResult(tool_result)]),
        tool_use_id: tool_call_id.to_string(),
        tool_id,
        is_error,
    })
}

/// Create an error tool result (shorthand for `create_tool_result_message` with `is_error=true`).
pub fn create_error_tool_result(
    tool_call_id: &str,
    tool_name: &str,
    tool_id: ToolId,
    error: &str,
) -> Message {
    create_tool_result_message(
        tool_call_id,
        tool_name,
        tool_id,
        error,
        /*is_error*/ true,
    )
}

/// Create a compact boundary system message recording token counts before/after compaction.
pub fn create_compact_boundary_message(tokens_before: i64, tokens_after: i64) -> Message {
    Message::System(SystemMessage::CompactBoundary(
        SystemCompactBoundaryMessage {
            uuid: Uuid::new_v4(),
            tokens_before,
            tokens_after,
            trigger: coco_types::CompactTrigger::Auto,
            user_context: None,
            messages_summarized: None,
            pre_compact_discovered_tools: vec![],
            preserved_segment: None,
        },
    ))
}

/// Create a progress message for streaming tool output.
pub fn create_progress_message(tool_use_id: &str, data: serde_json::Value) -> Message {
    Message::Progress(ProgressMessage {
        tool_use_id: tool_use_id.to_string(),
        data,
        parent_message_uuid: None,
    })
}

/// Create a system informational message about operation cancellation.
pub fn create_cancellation_message() -> Message {
    Message::System(SystemMessage::Informational(SystemInformationalMessage {
        uuid: Uuid::new_v4(),
        level: SystemMessageLevel::Warning,
        title: "Cancelled".to_string(),
        message: "Operation was cancelled by the user.".to_string(),
    }))
}

/// Create a system message about a tool permission denial.
pub fn create_permission_denied_message(tool_name: &str, reason: &str) -> Message {
    Message::System(SystemMessage::Informational(SystemInformationalMessage {
        uuid: Uuid::new_v4(),
        level: SystemMessageLevel::Warning,
        title: format!("Permission denied: {tool_name}"),
        message: reason.to_string(),
    }))
}

/// Create an assistant error message with an attached API error.
pub fn create_assistant_error_message(error: &str, request_id: Option<&str>) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::assistant(vec![]),
        uuid: Uuid::new_v4(),
        model: String::new(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: request_id.map(str::to_string),
        api_error: Some(ApiError {
            message: error.to_string(),
            status_code: None,
        }),
    })
}

#[cfg(test)]
#[path = "creation.test.rs"]
mod tests;
