//! System reminder wrapping — inject messages as `<system-reminder>` XML tags.

use coco_types::LlmMessage;
use coco_types::Message;

/// Wrap text in system-reminder XML tags.
/// These messages are hidden from UI (is_meta=true) but visible to the model.
pub fn wrap_in_system_reminder(text: &str) -> String {
    format!("<system-reminder>\n{text}\n</system-reminder>")
}

/// Create a system-reminder meta message.
pub fn create_system_reminder_message(text: &str) -> Message {
    Message::Attachment(coco_types::AttachmentMessage::api(
        coco_types::AttachmentKind::CriticalSystemReminder,
        LlmMessage::user_text(wrap_in_system_reminder(text)),
    ))
}

/// Extract plain text content from a message (for display/logging).
pub fn extract_text_from_message(msg: &Message) -> String {
    match msg {
        Message::User(m) => extract_text_from_llm_message(&m.message),
        Message::Assistant(m) => extract_text_from_llm_message(&m.message),
        Message::ToolResult(m) => extract_text_from_llm_message(&m.message),
        Message::Attachment(m) => m.as_text_for_display(),
        Message::System(s) => match s {
            coco_types::SystemMessage::Informational(m) => m.message.clone(),
            coco_types::SystemMessage::ApiError(m) => m.error.clone(),
            coco_types::SystemMessage::LocalCommand(m) => m.output.clone(),
            _ => String::new(),
        },
        Message::ToolUseSummary(m) => m.summary.clone(),
        Message::Progress(_) | Message::Tombstone(_) => String::new(),
    }
}

fn extract_text_from_llm_message(msg: &LlmMessage) -> String {
    match msg {
        LlmMessage::User { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                coco_types::UserContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        LlmMessage::Assistant { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                coco_types::AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        LlmMessage::System { content, .. } => content.clone(),
        LlmMessage::Tool { .. } => String::new(),
    }
}

#[cfg(test)]
#[path = "wrapping.test.rs"]
mod tests;
