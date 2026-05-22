//! Conversation recovery for session resume/fork.
//!
//! TS: utils/conversationRecovery.ts — reload conversation from
//! transcript JSONL, build the message chain by walking parent_uuid
//! from the newest non-sidechain leaf back to the root, then
//! reconstruct typed `coco_messages::Message` values preserving
//! `tool_use` / `tool_result` content blocks so the resumed model
//! sees the same DAG it left.

use crate::storage::TranscriptEntry;
use crate::storage::entry_kind;
use coco_messages::Message;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

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
/// Reads the JSONL transcript, walks the `parent_uuid` chain backward
/// from the newest non-sidechain leaf, then reconstructs the message
/// list in chronological order. Falls back to top-to-bottom read order
/// when no parent_uuid links are present (transcripts written by
/// older builds, fixture data).
///
/// TS parity: `loadConversationForResume` →
/// `loadTranscriptFile` → `buildConversationChain`
/// (`utils/conversationRecovery.ts`).
pub fn load_conversation_for_resume(
    transcript_path: &Path,
) -> crate::Result<RecoveredConversation> {
    if !transcript_path.exists() {
        return Err(crate::SessionError::TranscriptNotFound {
            path: transcript_path.to_path_buf(),
        });
    }

    let content = std::fs::read_to_string(transcript_path)?;

    // Pass 1: parse every JSONL line into either a TranscriptEntry or
    // discard. Track sidechain flag and plan slug; collect transcript
    // entries in disk order.
    let mut entries: Vec<TranscriptEntry> = Vec::new();
    let mut plan_slug: Option<String> = None;
    let mut has_sidechain = false;

    // Metadata `type` discriminators we filter out before the leaf
    // walk. Mirrors the kebab-case TS values; centralised here so a
    // new metadata variant only needs adding in one place.
    const METADATA_DISCRIMINATORS: &[&str] = &[
        "custom-title",
        "tag",
        "last-prompt",
        "summary",
        "cost-summary",
        "file-history-snapshot",
        "marble-origami-commit",
        "marble-origami-snapshot",
        "content-replacement",
    ];

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let entry_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if METADATA_DISCRIMINATORS.contains(&entry_type) {
            continue;
        }
        // TS Claude Code writes `isSidechain` (camelCase). Tolerate
        // legacy snake_case for transcripts authored by an older
        // build of coco-rs.
        if value
            .get("isSidechain")
            .or_else(|| value.get("is_sidechain"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            has_sidechain = true;
            continue;
        }
        if plan_slug.is_none()
            && let Some(slug) = value.get("slug").and_then(|v| v.as_str())
            && !slug.is_empty()
        {
            plan_slug = Some(slug.to_string());
        }
        let Ok(te) = serde_json::from_value::<TranscriptEntry>(value) else {
            continue;
        };
        entries.push(te);
    }

    // Pass 2: build a uuid → entry index and the set of parent uuids
    // so we can identify leaves (uuids that no other entry points at).
    // The walk picks the latest non-sidechain leaf by timestamp; on
    // tie or empty index we fall back to disk order.
    let mut by_uuid: HashMap<String, usize> = HashMap::new();
    let mut parent_uuids: HashSet<String> = HashSet::new();
    for (idx, e) in entries.iter().enumerate() {
        if !e.uuid.is_empty() {
            by_uuid.insert(e.uuid.clone(), idx);
        }
        if let Some(p) = &e.parent_uuid
            && !p.is_empty()
        {
            parent_uuids.insert(p.clone());
        }
    }

    // Find leaves: entries whose uuid is not a parent of any other
    // entry. Pick the latest by timestamp string (RFC3339 sorts
    // lexicographically). When no parent_uuid links exist (older
    // fixtures), every entry is a "leaf" by this definition — fall
    // back to disk order to preserve the TS-aligned behavior of
    // returning every persisted message.
    let any_parent_link = !parent_uuids.is_empty();
    let chain_indices: Vec<usize> = if any_parent_link {
        let leaf_idx = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.uuid.is_empty() && !parent_uuids.contains(&e.uuid))
            .max_by(|(_, a), (_, b)| a.timestamp.cmp(&b.timestamp))
            .map(|(idx, _)| idx);
        match leaf_idx {
            Some(idx) => {
                let mut walked: Vec<usize> = Vec::new();
                let mut visited: HashSet<String> = HashSet::new();
                let mut cursor = Some(idx);
                while let Some(i) = cursor {
                    let entry = &entries[i];
                    if !entry.uuid.is_empty() && !visited.insert(entry.uuid.clone()) {
                        break;
                    }
                    walked.push(i);
                    cursor = entry
                        .parent_uuid
                        .as_deref()
                        .filter(|p| !p.is_empty())
                        .and_then(|p| by_uuid.get(p).copied());
                }
                walked.reverse();
                walked
            }
            None => (0..entries.len()).collect(),
        }
    } else {
        (0..entries.len()).collect()
    };

    // Pass 3: reconstruct typed messages, aggregating model + token +
    // turn counters along the way. `latest_model` mirrors TS's "newest
    // assistant model wins" rule used by the resume picker.
    let mut messages: Vec<Message> = Vec::with_capacity(chain_indices.len());
    let mut latest_model = String::new();
    let mut total_input: i64 = 0;
    let mut total_output: i64 = 0;
    let mut turn_count: i32 = 0;

    for idx in chain_indices {
        let te = &entries[idx];
        if let Some(m) = &te.model
            && !m.is_empty()
        {
            latest_model.clone_from(m);
        }
        if let Some(usage) = &te.usage {
            total_input += usage.input_tokens;
            total_output += usage.output_tokens;
        }
        if let Some(msg) = reconstruct_message(te) {
            if te.entry_type == "assistant" {
                turn_count += 1;
            }
            messages.push(msg);
        }
    }

    Ok(RecoveredConversation {
        messages,
        model: latest_model,
        turn_count,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        plan_slug,
        has_sidechain,
    })
}

/// Reconstruct a `Message` from a `TranscriptEntry`.
///
/// Round-trips full content blocks (`tool_use`, `tool_result`,
/// `reasoning`, `image`) by deserializing the persisted `message`
/// field directly into `LlmMessage`. Falls back to a text-only
/// reconstruction when the persisted shape is missing or malformed.
/// TS parity: `deserializeMessages` in
/// `utils/conversationRecovery.ts` — preserves the DAG so resumed
/// turns don't surface orphan tool_use blocks.
fn reconstruct_message(entry: &TranscriptEntry) -> Option<Message> {
    let uuid = entry
        .uuid
        .parse::<Uuid>()
        .unwrap_or_else(|_| Uuid::new_v4());
    let msg_value = entry.message.clone()?;

    let kind = entry.entry_type.as_str();
    if kind == entry_kind::USER {
        let llm = match serde_json::from_value::<coco_messages::LlmMessage>(msg_value.clone()) {
            Ok(m) => m,
            Err(_) => coco_messages::LlmMessage::user_text(extract_text_from_content(
                msg_value.get("content").unwrap_or(&serde_json::Value::Null),
            )),
        };
        return Some(Message::User(coco_messages::UserMessage {
            message: llm,
            uuid,
            timestamp: entry.timestamp.clone(),
            is_visible_in_transcript_only: false,
            is_virtual: false,
            is_compact_summary: false,
            permission_mode: None,
            origin: None,
            parent_tool_use_id: None,
        }));
    }
    if kind == entry_kind::ASSISTANT {
        let llm = match serde_json::from_value::<coco_messages::LlmMessage>(msg_value.clone()) {
            Ok(m) => m,
            Err(_) => coco_messages::LlmMessage::assistant_text(extract_text_from_content(
                msg_value.get("content").unwrap_or(&serde_json::Value::Null),
            )),
        };
        let usage = entry.usage.as_ref().map(|u| coco_types::TokenUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            input_token_details: coco_types::InputTokenDetails {
                cache_read_tokens: u.cache_read_tokens.unwrap_or(0),
                cache_write_tokens: u.cache_creation_tokens.unwrap_or(0),
                ..Default::default()
            },
            ..Default::default()
        });
        return Some(Message::Assistant(coco_messages::AssistantMessage {
            message: llm,
            uuid,
            model: entry.model.clone().unwrap_or_default(),
            stop_reason: None,
            usage,
            cost_usd: entry.cost_usd,
            request_id: None,
            api_error: None,
        }));
    }
    if kind == entry_kind::TOOL_RESULT {
        // Persisted as the bare `ToolResultMessage` JSON. Inject the
        // outer `Message` discriminator and deserialize via the enum
        // so derived fields (`parent_tool_use_id`, `is_error`,
        // structured content) survive round-trip.
        return tag_and_deserialize_message(msg_value, entry_kind::TOOL_RESULT);
    }
    if kind == entry_kind::SYSTEM {
        // Same shape as ToolResult: standalone struct, needs the
        // outer enum tag re-injected. Fall back to a synthetic
        // CriticalSystemReminder attachment when the original
        // sub-variant doesn't round-trip (e.g. a renamed variant
        // from a prior build).
        return tag_and_deserialize_message(msg_value.clone(), entry_kind::SYSTEM).or_else(|| {
            let llm = coco_messages::LlmMessage::user_text(extract_text_from_content(&msg_value));
            let mut att = coco_messages::AttachmentMessage::api(
                coco_types::AttachmentKind::CriticalSystemReminder,
                llm,
            );
            att.uuid = uuid;
            Some(Message::Attachment(att))
        });
    }
    // entry_kind::ATTACHMENT and unknown kinds: skip on resume.
    // Attachments are reminder-driven and re-injected by the
    // per-turn reminder pipeline (TS `deserializeMessages` filters
    // them for the same reason).
    None
}

/// Inject a `type` discriminator into a stored payload and deserialize
/// it as a `Message`. Lifts the duplication that used to live in the
/// system / tool_result arms.
fn tag_and_deserialize_message(payload: serde_json::Value, kind: &str) -> Option<Message> {
    let mut map = match payload {
        serde_json::Value::Object(m) => m,
        _ => return None,
    };
    map.insert(
        "type".to_string(),
        serde_json::Value::String(kind.to_string()),
    );
    serde_json::from_value::<Message>(serde_json::Value::Object(map)).ok()
}

/// Extract a plain-text fallback from a JSON content field. Handles
/// both string content and array-of-blocks content shapes.
fn extract_text_from_content(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text")
                && let Some(text) = item.get("text").and_then(|v| v.as_str())
            {
                parts.push(text);
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
    std::fs::read_to_string(transcript_path)
        .map(|content| {
            content.lines().any(|line| {
                !line.trim().is_empty() && serde_json::from_str::<serde_json::Value>(line).is_ok()
            })
        })
        .unwrap_or(false)
}

/// Fork a conversation — create a copy of the transcript for a new session.
pub fn fork_conversation(source_path: &Path, dest_path: &Path) -> crate::Result<()> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source_path, dest_path)?;
    Ok(())
}

#[cfg(test)]
#[path = "recovery.test.rs"]
mod tests;
