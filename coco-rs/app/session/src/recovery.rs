//! Conversation recovery for session resume/fork.
//!
//! TS: utils/conversationRecovery.ts — reload conversation from transcript.

use crate::storage::TranscriptEntry;
use coco_types::Message;
use std::path::Path;

/// Conversation recovery result.
#[derive(Debug)]
pub struct RecoveredConversation {
    pub messages: Vec<Message>,
    pub model: String,
    pub turn_count: i32,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    /// Plan slug extracted from transcript (for plan resume).
    pub plan_slug: Option<String>,
    /// Whether the session had sidechain entries.
    pub has_sidechain: bool,
}

/// Load a conversation from a session transcript for resume.
///
/// Reads the JSONL transcript and reconstructs the message history.
pub fn load_conversation_for_resume(
    transcript_path: &Path,
) -> anyhow::Result<RecoveredConversation> {
    if !transcript_path.exists() {
        anyhow::bail!("transcript not found: {}", transcript_path.display());
    }

    let content = std::fs::read_to_string(transcript_path)?;
    let mut model = String::new();
    let mut turn_count = 0;
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut messages = Vec::new();
    let mut plan_slug = None;
    let mut has_sidechain = false;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        // Skip metadata entries (they have a "type" field with metadata variant names)
        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if matches!(
            entry_type,
            "custom-title" | "tag" | "last-prompt" | "summary" | "cost-summary"
        ) {
            continue;
        }

        // Check for sidechain
        if entry
            .get("is_sidechain")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            has_sidechain = true;
            continue; // Skip sidechain entries
        }

        // Extract plan slug from messages
        if plan_slug.is_none() {
            if let Some(slug) = entry.get("slug").and_then(|v| v.as_str()) {
                plan_slug = Some(slug.to_string());
            }
        }

        // Try to parse as TranscriptEntry for structured access
        if let Ok(te) = serde_json::from_str::<TranscriptEntry>(line) {
            // Extract model (latest wins)
            if let Some(m) = &te.model {
                if !m.is_empty() {
                    model.clone_from(m);
                }
            }

            // Sum tokens
            if let Some(usage) = &te.usage {
                total_input += usage.input_tokens;
                total_output += usage.output_tokens;
            }

            // Reconstruct messages from transcript entries
            if let Some(msg) = reconstruct_message(&te) {
                if te.entry_type == "assistant" {
                    turn_count += 1;
                }
                messages.push(msg);
            }
        }
    }

    Ok(RecoveredConversation {
        messages,
        model,
        turn_count,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        plan_slug,
        has_sidechain,
    })
}

/// Reconstruct a `Message` from a `TranscriptEntry`.
///
/// Maps the JSONL transcript format back to the internal Message enum.
fn reconstruct_message(entry: &TranscriptEntry) -> Option<Message> {
    let msg_value = entry.message.as_ref()?;
    let content = msg_value.get("content")?;
    let uuid = uuid::Uuid::parse_str(&entry.uuid).unwrap_or_else(|_| uuid::Uuid::new_v4());

    match entry.entry_type.as_str() {
        "user" => {
            let text = extract_text_from_content(content);
            let llm_message = coco_types::LlmMessage::user_text(text);
            Some(Message::User(coco_types::UserMessage {
                message: llm_message,
                uuid,
                timestamp: entry.timestamp.clone(),
                is_meta: false,
                is_visible_in_transcript_only: false,
                is_virtual: false,
                is_compact_summary: false,
                permission_mode: None,
                origin: None,
            }))
        }
        "assistant" => {
            let text = extract_text_from_content(content);
            let llm_message = coco_types::LlmMessage::assistant_text(text);
            Some(Message::Assistant(coco_types::AssistantMessage {
                message: llm_message,
                uuid,
                model: entry.model.clone().unwrap_or_default(),
                stop_reason: None,
                usage: entry.usage.as_ref().map(|u| coco_types::TokenUsage {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    cache_read_input_tokens: u.cache_read_tokens.unwrap_or(0),
                    cache_creation_input_tokens: u.cache_creation_tokens.unwrap_or(0),
                }),
                cost_usd: entry.cost_usd,
                request_id: None,
                api_error: None,
            }))
        }
        "system" | "attachment" => {
            let text = extract_text_from_content(content);
            let llm_message = coco_types::LlmMessage::user_text(text);
            Some(Message::Attachment(coco_types::AttachmentMessage {
                uuid,
                message: llm_message,
                is_meta: true,
            }))
        }
        _ => None,
    }
}

/// Extract text from a JSON content field.
///
/// Handles both string content (`"content": "text"`) and array content
/// (`"content": [{"type": "text", "text": "..."}]`).
fn extract_text_from_content(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(text);
                }
            }
        }
        return parts.join("\n");
    }
    String::new()
}

/// Check if a session can be resumed (transcript exists and is valid).
pub fn can_resume_session(transcript_path: &Path) -> bool {
    if !transcript_path.exists() {
        return false;
    }
    // Check file isn't empty and has at least one valid JSON line
    std::fs::read_to_string(transcript_path)
        .map(|content| {
            content.lines().any(|line| {
                !line.trim().is_empty() && serde_json::from_str::<serde_json::Value>(line).is_ok()
            })
        })
        .unwrap_or(false)
}

/// Fork a conversation — create a copy of the transcript for a new session.
pub fn fork_conversation(source_path: &Path, dest_path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source_path, dest_path)?;
    Ok(())
}

#[cfg(test)]
#[path = "recovery.test.rs"]
mod tests;
