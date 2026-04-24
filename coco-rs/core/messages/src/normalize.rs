//! Message normalization for API consumption.
//!
//! TS: normalizeMessagesForAPI() — 10-step pipeline that transforms
//! internal messages into the format expected by the LLM API.
//!
//! Port from cocode-rs: [`NormalizationOptions`] presets
//! ([`for_api`](NormalizationOptions::for_api) /
//! [`for_ui`](NormalizationOptions::for_ui) /
//! [`for_persist`](NormalizationOptions::for_persist)) expose the same
//! pipeline under different axis filters, so callers don't re-derive
//! predicates per use case.

use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::Visibility;

use crate::predicates;

/// Configurable filter knobs for the normalization pipeline.
///
/// Callers pick a preset via the constructors below. Fields are public so
/// one-offs can override a single flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizationOptions {
    /// Drop messages where `Visibility::api == false` (silent / UI-only).
    pub require_api_visible: bool,
    /// Drop messages where `Visibility::ui == false` (API-only / hidden).
    pub require_ui_visible: bool,
    /// Drop `is_virtual=true` user messages (never sent anywhere).
    pub skip_virtual: bool,
    /// Drop tombstoned messages (summary markers, etc.).
    pub skip_tombstones: bool,
    /// Drop whitespace-only user messages (unless `is_meta=true`).
    pub skip_whitespace_user: bool,
}

impl NormalizationOptions {
    /// Preset for outgoing API requests: strip anything API-hidden, keep
    /// is_meta user messages, enforce tool-result pairing + ordering.
    pub const fn for_api() -> Self {
        Self {
            require_api_visible: true,
            require_ui_visible: false,
            skip_virtual: true,
            skip_tombstones: true,
            skip_whitespace_user: true,
        }
    }

    /// Preset for UI rendering: strip anything UI-hidden (silent events,
    /// meta user messages, API-only system messages).
    pub const fn for_ui() -> Self {
        Self {
            require_api_visible: false,
            require_ui_visible: true,
            skip_virtual: true,
            skip_tombstones: true,
            skip_whitespace_user: false,
        }
    }

    /// Preset for transcript persistence: keep everything the session
    /// history carried, drop nothing — mirrors TS's unfiltered
    /// `appendEntryToFile`.
    pub const fn for_persist() -> Self {
        Self {
            require_api_visible: false,
            require_ui_visible: false,
            skip_virtual: false,
            skip_tombstones: false,
            skip_whitespace_user: false,
        }
    }
}

/// Apply visibility / virtual / tombstone / whitespace filters per `opts`.
///
/// Returns borrowed references preserving original message order. Callers
/// that need further API-specific steps (tool-result pairing, consecutive
/// merging, role-first enforcement) use [`normalize_messages_for_api`];
/// callers that want a pre-filter for UI / persistence use this directly.
pub fn filter_by_options<'a>(
    messages: &'a [Message],
    opts: NormalizationOptions,
) -> Vec<&'a Message> {
    messages
        .iter()
        .filter(|m| {
            if opts.skip_virtual && predicates::is_virtual_message(m) {
                return false;
            }
            if opts.skip_tombstones && predicates::is_tombstone(m) {
                return false;
            }
            let Visibility { api, ui } = m.visibility();
            if opts.require_api_visible && !api {
                return false;
            }
            if opts.require_ui_visible && !ui {
                return false;
            }
            if opts.skip_whitespace_user && predicates::is_user_message(m) {
                let has_content = predicates::has_text_content(m) || predicates::is_meta_message(m);
                if !has_content {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// Normalize messages for API consumption.
///
/// 10-step pipeline:
/// 1. Filter out virtual messages (not sent to API)
/// 2. Filter out tombstoned messages
/// 3. Filter out progress messages (UI-only)
/// 4. Filter out tool use summaries (UI-only)
/// 5. Filter out whitespace-only user messages
/// 6. Ensure tool result pairing (orphaned results → remove)
/// 7. Strip empty assistant messages
/// 8. Ensure alternating user/assistant roles (merge consecutive same-role)
/// 9. Ensure conversation starts with user message
/// 10. Extract LlmMessage from each surviving message
pub fn normalize_messages_for_api(messages: &[Message]) -> Vec<LlmMessage> {
    // Steps 1–5 collapse into one visibility-driven filter.
    //
    // - `require_api_visible` covers steps 3 (progress — UI_ONLY) and 4
    //   (tool-use-summary — UI_ONLY) via their Visibility mapping. Any
    //   `Message::Attachment` whose kind is API-hidden (silent events,
    //   `AlreadyReadFile`, feature-gated kinds, etc.) is stripped too —
    //   which is new correct behavior post-Phase-1.
    // - `skip_virtual` covers step 1.
    // - `skip_tombstones` covers step 2.
    // - `skip_whitespace_user` covers step 5.
    let mut filtered: Vec<&Message> = filter_by_options(messages, NormalizationOptions::for_api());

    // Step 6: Ensure tool result pairing
    // Collect tool_use_ids from assistant messages
    let tool_use_ids: std::collections::HashSet<String> = filtered
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => match &a.message {
                LlmMessage::Assistant { content, .. } => {
                    let ids: Vec<String> = content
                        .iter()
                        .filter_map(|c| match c {
                            coco_types::AssistantContent::ToolCall(tc) => {
                                Some(tc.tool_call_id.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    Some(ids)
                }
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .collect();

    // Remove orphaned tool results
    filtered.retain(|m| match m {
        Message::ToolResult(tr) => tool_use_ids.contains(&tr.tool_use_id),
        _ => true,
    });

    // Step 7: Strip empty assistant messages
    filtered.retain(|m| match m {
        Message::Assistant(a) => match &a.message {
            LlmMessage::Assistant { content, .. } => !content.is_empty(),
            _ => true,
        },
        _ => true,
    });

    // Step 10: Extract LlmMessage from each surviving message
    let mut result: Vec<LlmMessage> = Vec::with_capacity(filtered.len());
    for msg in &filtered {
        if let Some(llm_msg) = extract_llm_message(msg) {
            result.push(llm_msg);
        }
    }

    // Step 8: Merge consecutive same-role messages
    result = merge_consecutive_same_role(result);

    // Step 9: Ensure starts with user
    if let Some(first) = result.first()
        && !matches!(first, LlmMessage::User { .. })
    {
        // Prepend empty user message
        result.insert(0, LlmMessage::user_text(""));
    }

    result
}

/// Extract the LlmMessage from an internal Message.
fn extract_llm_message(msg: &Message) -> Option<LlmMessage> {
    match msg {
        Message::User(m) => Some(m.message.clone()),
        Message::Assistant(m) => Some(m.message.clone()),
        Message::Attachment(m) => m.as_api_message().cloned(),
        Message::ToolResult(m) => Some(m.message.clone()),
        Message::System(_) => {
            // System messages are sent as user messages with is_meta=true
            // They become LlmMessage::User with system-reminder wrapping
            None // handled by system-reminder injection, not normalization
        }
        Message::Progress(_) | Message::Tombstone(_) | Message::ToolUseSummary(_) => None,
    }
}

/// Merge consecutive messages with the same role.
/// TS: mergeConsecutiveMessages()
fn merge_consecutive_same_role(messages: Vec<LlmMessage>) -> Vec<LlmMessage> {
    if messages.len() <= 1 {
        return messages;
    }

    let mut result: Vec<LlmMessage> = Vec::with_capacity(messages.len());

    for msg in messages {
        if let Some(last) = result.last_mut()
            && can_merge(last, &msg)
        {
            merge_into(last, msg);
            continue;
        }
        result.push(msg);
    }

    result
}

/// Whether two messages can be merged (same role).
fn can_merge(a: &LlmMessage, b: &LlmMessage) -> bool {
    matches!(
        (a, b),
        (LlmMessage::User { .. }, LlmMessage::User { .. })
            | (LlmMessage::Assistant { .. }, LlmMessage::Assistant { .. })
    )
}

/// Merge b into a (append content parts).
fn merge_into(a: &mut LlmMessage, b: LlmMessage) {
    match (a, b) {
        (
            LlmMessage::User {
                content: a_content, ..
            },
            LlmMessage::User {
                content: b_content, ..
            },
        ) => {
            a_content.extend(b_content);
        }
        (
            LlmMessage::Assistant {
                content: a_content, ..
            },
            LlmMessage::Assistant {
                content: b_content, ..
            },
        ) => {
            a_content.extend(b_content);
        }
        _ => {}
    }
}

/// Merge consecutive User messages in-place.
///
/// When system reminders are injected, two User messages may appear back-to-back.
/// This merges the second's content into the first.
pub fn merge_consecutive_user_messages(messages: &mut Vec<Message>) {
    if messages.len() <= 1 {
        return;
    }

    let mut write = 0;
    for read in 1..messages.len() {
        let both_user = matches!(
            (&messages[write], &messages[read]),
            (Message::User(_), Message::User(_))
        );
        if both_user {
            // Take the read message, merge its LlmMessage user content into write.
            let taken = std::mem::replace(&mut messages[read], placeholder_tombstone());
            if let (Message::User(dest), Message::User(src)) = (&mut messages[write], taken)
                && let (
                    LlmMessage::User {
                        content: dest_content,
                        ..
                    },
                    LlmMessage::User {
                        content: src_content,
                        ..
                    },
                ) = (&mut dest.message, src.message)
            {
                dest_content.extend(src_content);
            }
        } else {
            write += 1;
            if write != read {
                messages.swap(write, read);
            }
        }
    }
    messages.truncate(write + 1);
}

/// Merge consecutive Assistant messages in-place.
///
/// Appends content parts from the second into the first.
pub fn merge_consecutive_assistant_messages(messages: &mut Vec<Message>) {
    if messages.len() <= 1 {
        return;
    }

    let mut write = 0;
    for read in 1..messages.len() {
        let both_assistant = matches!(
            (&messages[write], &messages[read]),
            (Message::Assistant(_), Message::Assistant(_))
        );
        if both_assistant {
            let taken = std::mem::replace(&mut messages[read], placeholder_tombstone());
            if let (Message::Assistant(dest), Message::Assistant(src)) =
                (&mut messages[write], taken)
                && let (
                    LlmMessage::Assistant {
                        content: dest_content,
                        ..
                    },
                    LlmMessage::Assistant {
                        content: src_content,
                        ..
                    },
                ) = (&mut dest.message, src.message)
            {
                dest_content.extend(src_content);
            }
        } else {
            write += 1;
            if write != read {
                messages.swap(write, read);
            }
        }
    }
    messages.truncate(write + 1);
}

/// Remove image/file content parts from User messages.
///
/// Useful for models that do not support vision. Text parts are preserved.
/// If a User message becomes empty after stripping, it is removed entirely.
pub fn strip_images_from_messages(messages: &mut Vec<Message>) {
    for msg in messages.iter_mut() {
        if let Message::User(user) = msg
            && let LlmMessage::User { content, .. } = &mut user.message
        {
            content.retain(|part| matches!(part, coco_types::UserContent::Text(_)));
        }
    }
    // Remove User messages that have become empty.
    messages.retain(|msg| {
        if let Message::User(user) = msg
            && let LlmMessage::User { content, .. } = &user.message
        {
            return !content.is_empty();
        }
        true
    });
}

/// Remove email-style signature blocks from User messages.
///
/// Strips content after a line matching `"-- "` (RFC 3676 sig delimiter)
/// at the end of text content parts.
pub fn strip_signature_blocks(messages: &mut Vec<Message>) {
    for msg in messages.iter_mut() {
        if let Message::User(user) = msg
            && let LlmMessage::User { content, .. } = &mut user.message
        {
            for part in content.iter_mut() {
                if let coco_types::UserContent::Text(text_part) = part {
                    if let Some(pos) = text_part.text.find("\n-- \n") {
                        text_part.text.truncate(pos);
                    } else if text_part.text.starts_with("-- \n") {
                        text_part.text.clear();
                    }
                }
            }
        }
    }
}

/// Ensure the first message is a User message.
///
/// If the first message is not User, prepend a placeholder user message.
/// Required by some provider APIs.
pub fn ensure_user_first(messages: &mut Vec<Message>) {
    if messages.is_empty() {
        return;
    }
    if !matches!(messages.first(), Some(Message::User(_))) {
        messages.insert(
            0,
            Message::User(coco_types::UserMessage {
                message: LlmMessage::user_text(""),
                uuid: uuid::Uuid::new_v4(),
                timestamp: String::new(),
                is_visible_in_transcript_only: false,
                is_virtual: false,
                is_compact_summary: false,
                permission_mode: None,
                origin: None,
                parent_tool_use_id: None,
            }),
        );
    }
}

/// Convert internal Messages to LlmMessages for API calls.
///
/// Extracts the `.message` field from User, Assistant, Attachment, and ToolResult
/// variants. System, Progress, Tombstone, and ToolUseSummary messages are skipped.
pub fn to_llm_prompt(messages: &[Message]) -> Vec<LlmMessage> {
    messages.iter().filter_map(extract_llm_message).collect()
}

/// Placeholder tombstone used during in-place merge algorithms.
fn placeholder_tombstone() -> Message {
    Message::Tombstone(coco_types::TombstoneMessage {
        uuid: uuid::Uuid::nil(),
        original_kind: coco_types::MessageKind::Tombstone,
    })
}

#[cfg(test)]
#[path = "normalize.test.rs"]
mod tests;
