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

use crate::LlmMessage;
use crate::Message;
use crate::Visibility;

use crate::predicates;

/// Fields the query layer injects into `ExitPlanMode` tool input so that
/// hooks, SDK consumers, and the persisted transcript observe the plan
/// the tool reads from disk. Produced by
/// `app/query::tool_input_normalizer::normalize_observable_tool_input`
/// and stripped back out by [`strip_observable_tool_input_for_api`]
/// before the assistant message is re-sent to the model.
///
/// TS parity: `normalizeToolInput` injects these, `normalizeToolInputForAPI`
/// strips them (`utils/api.ts`).
pub const EXIT_PLAN_MODE_INJECTED_PLAN_FIELD: &str = "plan";
/// See [`EXIT_PLAN_MODE_INJECTED_PLAN_FIELD`].
pub const EXIT_PLAN_MODE_INJECTED_PLAN_FILE_PATH_FIELD: &str = "planFilePath";

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
pub fn filter_by_options(messages: &[Message], opts: NormalizationOptions) -> Vec<&Message> {
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
/// Pipeline (TS-aligned order from `utils/messages.ts:2255-2343`):
/// 1. Filter out virtual messages (not sent to API)
/// 2. Filter out tombstoned messages
/// 3. Filter out progress messages (UI-only)
/// 4. Filter out tool use summaries (UI-only)
/// 5. Filter out whitespace-only user messages
/// 6. Ensure tool result pairing (orphaned results → remove)
/// 7. Strip empty assistant messages
/// 8. `filter_orphaned_thinking_only_messages` (TS-parity, P1).
/// 9. `filter_trailing_thinking_from_last_assistant` (TS-parity, P1) —
///    must run BEFORE the whitespace filter; reverse order has a known bug
///    (TS comment line 2313).
/// 10. `filter_whitespace_only_assistant_messages` (TS-parity, P1).
/// 11. `ensure_non_empty_assistant_content` (TS-parity, P1).
/// 12. Merge consecutive Users (unconditional) and consecutive Assistants
///     **with matching `request_id`** (TS `messages.ts:2257-2261`).
/// 13. Extract LlmMessage from each surviving message.
/// 14. `smoosh_system_reminder_into_tool_result` (TS-parity, P0) — runs
///     after merge so SR-only User messages can be folded into prior Tool.
/// 15. `sanitize_error_tool_result_content` (TS-parity, P0) — runs AFTER
///     smoosh per TS so any text smooshed into is_error tool_results gets
///     the final text-only normalization.
/// 16. Ensure conversation starts with user message.
///
/// Still missing (P3, gated and no current feature in coco-rs):
///   - `relocateToolReferenceSiblings` — Tool Reference feature isn't
///     ported, no callers can produce the offending pattern today.
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
                            crate::AssistantContent::ToolCall(tc) => Some(tc.tool_call_id.clone()),
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

    // Steps 8-12: TS-parity assistant-content + tool_result fixups.
    // These need to mutate `Message` (not `LlmMessage`) because the source
    // structs carry `request_id` / `is_error` flags consulted by the passes.
    let mut owned: Vec<Message> = filtered.iter().map(|m| (*m).clone()).collect();
    filter_orphaned_thinking_only_messages(&mut owned);
    // Order matters (TS messages.ts:2313): trailing-thinking BEFORE whitespace.
    filter_trailing_thinking_from_last_assistant(&mut owned);
    filter_whitespace_only_assistant_messages(&mut owned);
    ensure_non_empty_assistant_content(&mut owned);

    // Step 13a: merge consecutive Users (unconditional — TS `mergeUserMessages`)
    // and consecutive Assistants WITH matching request_id (TS `messages.ts:2257-2261`
    // — different message.id stays separate). Must happen at `Message` level so
    // request_id is still readable; LlmMessage doesn't carry it.
    merge_consecutive_user_messages(&mut owned);
    merge_consecutive_assistants_by_request_id(&mut owned);

    // Step 13a': strip the query-layer-injected `ExitPlanMode` observable
    // fields (`plan` / `planFilePath`) before they reach the wire. TS
    // parity: `normalizeToolInputForAPI` in `normalizeMessagesForAPI`'s
    // assistant branch — the `ExitPlanMode` schema is an empty object;
    // the injected fields exist only for hooks / SDK / transcript.
    strip_observable_tool_input_for_api(&mut owned);

    // Step 13b: Extract LlmMessage from each surviving message
    let mut result: Vec<LlmMessage> = Vec::with_capacity(owned.len());
    for msg in &owned {
        if let Some(llm_msg) = extract_llm_message(msg) {
            result.push(llm_msg);
        }
    }

    // Step 14: smoosh `<system-reminder>` text into preceding tool_result.
    // Runs AFTER request_id-aware merge so SR-only User messages are isolated
    // and can be folded into the prior Tool message before the wire-level
    // `group_into_blocks` (vercel-ai-anthropic) merges User+Tool.
    smoosh_system_reminder_into_tool_result(&mut result);

    // Step 15: sanitize is_error tool_results — runs AFTER smoosh per TS
    // (smoosh may have appended text into a Content array; sanitize ensures
    // the final form is text-only when `is_error=true`). Operates at the
    // LlmMessage level since by this point the Message envelope is gone.
    sanitize_error_tool_result_in_llm_messages(&mut result);

    // Step 15b: forward-direction `ensureToolResultPairing` (TS
    // `messages.ts:5301-5326`). Synthesize an `is_error: true` placeholder
    // tool_result for every assistant tool_use that lacks a matching
    // tool_result. Without this, a single race / panic / discard miss
    // produces the provider error `unexpected tool_use_id` and the next
    // turn fails. Coco-rs's existing cancel/discard paths
    // (`permission_controller.rs:237-258`, `executor.rs:286-306`) cover
    // the common cases; this is the fail-safe last line.
    synthesize_missing_tool_results(&mut result);

    // Step 16: Ensure starts with user
    if let Some(first) = result.first()
        && !matches!(first, LlmMessage::User { .. })
    {
        // Prepend empty user message
        result.insert(0, LlmMessage::user_text(""));
    }

    result
}

/// Strip the observable-input fields the query layer injects into
/// `ExitPlanMode` tool calls ([`EXIT_PLAN_MODE_INJECTED_PLAN_FIELD`] /
/// [`EXIT_PLAN_MODE_INJECTED_PLAN_FILE_PATH_FIELD`]) before the assistant
/// message is sent to the model.
///
/// TS parity: `normalizeToolInputForAPI` (`utils/api.ts`). The
/// `ExitPlanMode` wire schema is an empty object — the injected fields
/// exist only so hooks / SDK / transcript consumers can observe the plan.
/// Re-sending them would bloat every subsequent turn with a duplicate of
/// the plan that already appears in the `ExitPlanMode` tool_result.
fn strip_observable_tool_input_for_api(messages: &mut [Message]) {
    for msg in messages.iter_mut() {
        let Message::Assistant(assistant) = msg else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &mut assistant.message else {
            continue;
        };
        for part in content.iter_mut() {
            let crate::AssistantContent::ToolCall(tc) = part else {
                continue;
            };
            if tc.tool_name != coco_types::ToolName::ExitPlanMode.as_str() {
                continue;
            }
            if let serde_json::Value::Object(map) = &mut tc.input {
                map.remove(EXIT_PLAN_MODE_INJECTED_PLAN_FIELD);
                map.remove(EXIT_PLAN_MODE_INJECTED_PLAN_FILE_PATH_FIELD);
            }
        }
    }
}

/// Placeholder text shipped in the synthetic tool_result body. Literal
/// match to TS `claude-code/src/utils/messages.ts:246-247` — exact wire
/// format so transcripts produced by either runtime are interchangeable
/// and so HFI / strict-pairing detectors that key off this exact string
/// keep working.
const SYNTHETIC_TOOL_RESULT_PLACEHOLDER: &str = "[Tool result missing due to internal error]";

/// TS-parity forward synthesis of missing tool_results
/// (`utils/messages.ts::ensureToolResultPairing`, lines 5301-5326).
///
/// Walks `messages` and, for each `Assistant` whose `ToolCall` parts have
/// no matching `ToolResult` anywhere in the transcript, inserts an
/// `is_error: true` placeholder tool_result. If a `Tool` message already
/// follows the orphan-bearing `Assistant`, the synthetic parts are
/// appended to its existing content (no extra message) so the wire-level
/// role-merge stays clean. Otherwise a fresh `Tool` message is inserted
/// immediately after the `Assistant`.
///
/// **Index advance asymmetry** — when we *append* to an existing Tool at
/// `i+1`, the next loop iteration's natural `i += 1` lands on that Tool
/// (correct: it's not an Assistant, so the loop walks past). When we
/// *insert* a new Tool at `i+1`, we add an extra `i += 1` to skip the
/// newly-inserted Tool before the natural increment fires.
///
/// **Idempotency** — `resolved` is collected from existing Tool messages
/// up front; once a synthetic carries the orphan id, a subsequent call
/// re-collects `resolved` (now including the synthetic id) and finds no
/// orphans. Verified by `normalize_synthesis_is_idempotent`.
///
/// `pub(crate)` so the test module can call it twice in a row to verify
/// the idempotency invariant directly.
pub(crate) fn synthesize_missing_tool_results(messages: &mut Vec<LlmMessage>) {
    use coco_llm_types::ToolContentPart;
    use coco_llm_types::ToolResultContent;
    use coco_llm_types::ToolResultPart;

    let resolved: std::collections::HashSet<String> = messages
        .iter()
        .filter_map(|m| match m {
            LlmMessage::Tool { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|p| match p {
                        ToolContentPart::ToolResult(r) => Some(r.tool_call_id.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();

    let mut i = 0;
    while i < messages.len() {
        if let LlmMessage::Assistant { content, .. } = &messages[i] {
            let orphans: Vec<(String, String)> = content
                .iter()
                .filter_map(|c| match c {
                    crate::AssistantContent::ToolCall(tc)
                        if !resolved.contains(&tc.tool_call_id) =>
                    {
                        Some((tc.tool_call_id.clone(), tc.tool_name.clone()))
                    }
                    _ => None,
                })
                .collect();

            if !orphans.is_empty() {
                let mut synthetic_parts: Vec<ToolContentPart> = orphans
                    .into_iter()
                    .map(|(id, name)| {
                        ToolContentPart::ToolResult(ToolResultPart {
                            tool_call_id: id,
                            tool_name: name,
                            output: ToolResultContent::text(SYNTHETIC_TOOL_RESULT_PLACEHOLDER),
                            is_error: true,
                            provider_metadata: None,
                        })
                    })
                    .collect();

                if let Some(LlmMessage::Tool {
                    content: existing, ..
                }) = messages.get_mut(i + 1)
                {
                    existing.append(&mut synthetic_parts);
                } else {
                    let synthetic = LlmMessage::Tool {
                        content: synthetic_parts,
                        provider_options: None,
                    };
                    messages.insert(i + 1, synthetic);
                    i += 1;
                }
            }
        }
        i += 1;
    }
}

/// LlmMessage-level variant of `sanitize_error_tool_result_content` that
/// runs AFTER the same-role merge, since at that point the message envelope
/// has been extracted and the wire-level `is_error` flag is the only source
/// of truth.
fn sanitize_error_tool_result_in_llm_messages(messages: &mut [LlmMessage]) {
    for msg in messages.iter_mut() {
        let LlmMessage::Tool { content, .. } = msg else {
            continue;
        };
        for part in content.iter_mut() {
            let coco_llm_types::ToolContentPart::ToolResult(rp) = part else {
                continue;
            };
            if !rp.is_error {
                continue;
            }
            let coco_llm_types::ToolResultContent::Content {
                value,
                provider_options,
            } = &rp.output
            else {
                continue;
            };
            if value
                .iter()
                .all(|p| matches!(p, coco_llm_types::ToolResultContentPart::Text { .. }))
            {
                continue;
            }
            let texts: Vec<String> = value
                .iter()
                .filter_map(|p| match p {
                    coco_llm_types::ToolResultContentPart::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            let new_value = if texts.is_empty() {
                Vec::new()
            } else {
                vec![coco_llm_types::ToolResultContentPart::Text {
                    text: texts.join("\n\n"),
                    provider_options: None,
                }]
            };
            rp.output = coco_llm_types::ToolResultContent::Content {
                value: new_value,
                provider_options: provider_options.clone(),
            };
        }
    }
}

/// Merge consecutive Assistant messages **only when their `request_id`
/// matches**. TS: `messages.ts:2257-2261` — chunks with the same
/// `message.id` get merged (typical for streaming), chunks with different
/// `message.id` stay separate (typical for retry-after-partial-stream
/// failure on resume). Without this guard, two distinct API responses
/// landing back-to-back in the transcript get incorrectly stitched into
/// one, producing an assistant message with mismatched thinking-block
/// signatures that the API rejects with 400.
///
/// Messages with `request_id == None` never merge with anything (matches
/// TS `msg.message.id === normalizedMessage.message.id` — `undefined`
/// never equals `undefined` in this comparison because TS uses strict
/// equality on the field).
pub fn merge_consecutive_assistants_by_request_id(messages: &mut Vec<Message>) {
    if messages.len() <= 1 {
        return;
    }
    let mut write = 0;
    for read in 1..messages.len() {
        let can_merge = matches!(
            (&messages[write], &messages[read]),
            (Message::Assistant(a), Message::Assistant(b))
                if a.request_id.is_some()
                    && a.request_id == b.request_id
        );
        if can_merge {
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
    // Any tombstones inserted above are filtered by the next pass — for
    // safety, drop them here too in case no caller filters tombstones.
    messages.retain(|m| !matches!(m, Message::Tombstone(t) if t.uuid.is_nil()));
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

// `merge_consecutive_same_role` (LlmMessage-level) was removed because it
// merged consecutive assistant chunks unconditionally — incorrect per TS
// (`messages.ts:2257-2261` only merges when message.id matches). The
// request_id-aware Message-level merge runs in step 12 of
// `normalize_messages_for_api`; this avoids losing the id through the
// LlmMessage extraction. User+User merging is handled by
// `merge_consecutive_user_messages` (Message-level, unconditional).

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
            content.retain(|part| matches!(part, crate::UserContent::Text(_)));
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
pub fn strip_signature_blocks(messages: &mut [Message]) {
    for msg in messages.iter_mut() {
        if let Message::User(user) = msg
            && let LlmMessage::User { content, .. } = &mut user.message
        {
            for part in content.iter_mut() {
                if let crate::UserContent::Text(text_part) = part {
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
            Message::User(crate::UserMessage {
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
    Message::Tombstone(crate::TombstoneMessage {
        uuid: uuid::Uuid::nil(),
        original_kind: crate::MessageKind::Tombstone,
    })
}

// ─────────────────────────────────────────────────────────────────────────
// TS-parity normalization passes (audit-gaps.md Round 10)
// ─────────────────────────────────────────────────────────────────────────

/// API-rejection guard: when `is_error=true`, all `tool_result.content` parts
/// must be Text. The Anthropic API returns 400 "all content must be type
/// text if is_error is true" when an image/document part is mixed with the
/// error flag.
///
/// TS: `messages.ts:1884 sanitizeErrorToolResultContent`. Read-side guard
/// for transcripts persisted before the error-aware smoosh learned to
/// filter on `is_error`. Without this, a resumed session containing an
/// image-in-error tool_result 400s on every call and cannot be recovered.
///
/// Walk every `Message::ToolResult`. If `is_error=true` AND output is
/// `ToolResultContent::Content { value }`, drop non-Text parts and
/// concatenate surviving Text parts with `\n\n` between them. Idempotent.
pub fn sanitize_error_tool_result_content(messages: &mut [Message]) {
    for msg in messages.iter_mut() {
        let Message::ToolResult(tr) = msg else {
            continue;
        };
        // NOTE: do NOT gate on `tr.is_error` — that mirror flag can fall out
        // of sync with the wire-level `rp.is_error` flag (engine sets both
        // together but transcripts persisted before that became invariant
        // can carry just one). The wire serializer reads `rp.is_error`, so
        // the wire-level flag is the source of truth for what the API sees.
        let LlmMessage::Tool { content, .. } = &mut tr.message else {
            continue;
        };
        for part in content.iter_mut() {
            let crate::ToolContent::ToolResult(rp) = part else {
                continue;
            };
            if !rp.is_error {
                continue;
            }
            let coco_llm_types::ToolResultContent::Content {
                value,
                provider_options,
            } = &rp.output
            else {
                continue;
            };
            if value
                .iter()
                .all(|p| matches!(p, coco_llm_types::ToolResultContentPart::Text { .. }))
            {
                continue;
            }
            let texts: Vec<String> = value
                .iter()
                .filter_map(|p| match p {
                    coco_llm_types::ToolResultContentPart::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            let new_value: Vec<coco_llm_types::ToolResultContentPart> = if texts.is_empty() {
                Vec::new()
            } else {
                vec![coco_llm_types::ToolResultContentPart::Text {
                    text: texts.join("\n\n"),
                    provider_options: None,
                }]
            };
            rp.output = coco_llm_types::ToolResultContent::Content {
                value: new_value,
                provider_options: provider_options.clone(),
            };
        }
    }
}

/// API-rejection guard: smoosh `<system-reminder>`-prefixed user text into
/// the tool_result content of the immediately preceding tool message.
///
/// TS: `messages.ts:1835-1873 smooshSystemReminderSiblings`. The Anthropic
/// provider groups consecutive User+Tool messages into a single
/// `role: "user"` block on the wire (see `vercel-ai-anthropic`'s
/// `convert_to_anthropic_messages::group_into_blocks` lines 46-89). When a
/// `<system-reminder>`-wrapped attachment lands as a sibling **after** a
/// tool_result inside that block, the wire renders as
/// `</function_results>\n\nHuman:<sr>...` — capybara/older-Anthropic models
/// learn to emit `\n\nHuman:` after tool results, leading to 3-token
/// empty `end_turn` responses (TS issue #21049).
///
/// This pass scans `Vec<LlmMessage>` AFTER `merge_consecutive_same_role`
/// and folds qualifying SR-text into the prior `LlmMessage::Tool`'s
/// last `ToolResultPart`'s output. It bails (leaves the messages alone)
/// when:
/// - the tool_result output is not Text or Content (Json/ErrorJson/
///   ErrorText/ExecutionDenied — risky to mutate),
/// - the tool_result has `is_error=true` (per TS `smooshIntoToolResult`,
///   error tool_results must remain text-only and we'd be inserting
///   text-only anyway, but the original SR text may itself be non-trivial
///   to merge with prior text — keep behavior surgical for now),
/// - any block in the User content is a non-Text part (File etc. — those
///   cannot fold into tool_result.content).
pub fn smoosh_system_reminder_into_tool_result(messages: &mut Vec<LlmMessage>) {
    if messages.len() < 2 {
        return;
    }
    let mut i = 0;
    while i + 1 < messages.len() {
        let next_is_user_with_sr = matches!(
            &messages[i + 1],
            LlmMessage::User { content, .. }
                if !content.is_empty()
                    && content.iter().all(|p| matches!(p, coco_llm_types::UserContentPart::Text(_)))
                    && matches!(content.first(), Some(coco_llm_types::UserContentPart::Text(t)) if t.text.starts_with("<system-reminder>"))
        );
        let prev_is_tool = matches!(&messages[i], LlmMessage::Tool { .. });
        if !(prev_is_tool && next_is_user_with_sr) {
            i += 1;
            continue;
        }
        // Extract the SR-text payload from the User message.
        let sr_texts: Vec<String> = match &messages[i + 1] {
            LlmMessage::User { content, .. } => content
                .iter()
                .filter_map(|p| match p {
                    coco_llm_types::UserContentPart::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        // Try to fold into the last tool_result of the Tool message.
        let folded = fold_text_into_last_tool_result(&mut messages[i], &sr_texts);
        if folded {
            messages.remove(i + 1);
            // Don't advance i — there may be another SR-User chain after the merge.
        } else {
            i += 1;
        }
    }
}

/// Try to append `sr_texts` as text content to the last `ToolResultPart` of
/// the given Tool LlmMessage. Returns `true` on successful fold.
///
/// `is_error=true` tool_results: TS `smooshIntoToolResult` (`messages.ts:2545-2553`)
/// proceeds with the smoosh but filters incoming blocks to text-only so the
/// API's "is_error must be text-only" invariant is preserved. SR text is
/// already text-only, so the smoosh is safe — bailing instead would leave
/// the dangerous `text-after-tool_result` pattern intact, which is the
/// exact `\n\nHuman:` issue smoosh exists to prevent.
fn fold_text_into_last_tool_result(tool: &mut LlmMessage, sr_texts: &[String]) -> bool {
    let LlmMessage::Tool { content, .. } = tool else {
        return false;
    };
    // Walk back to find the last ToolResultPart.
    let Some(last_idx) = content
        .iter()
        .rposition(|p| matches!(p, coco_llm_types::ToolContentPart::ToolResult(_)))
    else {
        return false;
    };
    let coco_llm_types::ToolContentPart::ToolResult(rp) = &mut content[last_idx] else {
        return false;
    };
    let joined = sr_texts.join("\n\n");
    if joined.is_empty() {
        return false;
    }
    match &mut rp.output {
        coco_llm_types::ToolResultContent::Text { value, .. } => {
            if value.is_empty() {
                *value = joined;
            } else {
                value.push_str("\n\n");
                value.push_str(&joined);
            }
            true
        }
        coco_llm_types::ToolResultContent::Content { value, .. } => {
            if let Some(coco_llm_types::ToolResultContentPart::Text {
                text: last_text, ..
            }) = value.last_mut()
            {
                last_text.push_str("\n\n");
                last_text.push_str(&joined);
            } else {
                value.push(coco_llm_types::ToolResultContentPart::Text {
                    text: joined,
                    provider_options: None,
                });
            }
            true
        }
        // Json / ErrorText / ErrorJson / ExecutionDenied: bailing is safer
        // than potentially corrupting structured payloads.
        _ => false,
    }
}

/// Strip trailing `Reasoning` parts from the last assistant message.
///
/// TS: `messages.ts:4781 filterTrailingThinkingFromLastAssistant`. Trailing
/// thinking-only blocks at the very end of the conversation (typical when
/// a stream is cancelled mid-thinking) cause API "thinking blocks cannot
/// be modified" errors on the next turn. Replace with a `[No message
/// content]` placeholder when ALL blocks were thinking, otherwise just
/// truncate the trailing run.
pub fn filter_trailing_thinking_from_last_assistant(messages: &mut [Message]) {
    let Some(last) = messages.last_mut() else {
        return;
    };
    let Message::Assistant(asst) = last else {
        return;
    };
    let LlmMessage::Assistant { content, .. } = &mut asst.message else {
        return;
    };
    if content.is_empty() {
        return;
    }
    let last_is_reasoning = matches!(
        content.last(),
        Some(coco_llm_types::AssistantContentPart::Reasoning(_))
            | Some(coco_llm_types::AssistantContentPart::ReasoningFile(_))
    );
    if !last_is_reasoning {
        return;
    }
    // Find last non-thinking index.
    let last_valid = content.iter().rposition(|p| {
        !matches!(
            p,
            coco_llm_types::AssistantContentPart::Reasoning(_)
                | coco_llm_types::AssistantContentPart::ReasoningFile(_)
        )
    });
    match last_valid {
        Some(idx) => {
            content.truncate(idx + 1);
        }
        None => {
            *content = vec![coco_llm_types::AssistantContentPart::Text(
                coco_llm_types::TextPart::new("[No message content]"),
            )];
        }
    }
}

/// Drop assistant messages whose content is only whitespace-only Text parts.
///
/// TS: `messages.ts:4869 filterWhitespaceOnlyAssistantMessages`. The API
/// rejects "text content blocks must contain non-whitespace text". Happens
/// when the model emits `\n\n` before a thinking block but the user
/// cancels mid-stream, leaving only whitespace text.
pub fn filter_whitespace_only_assistant_messages(messages: &mut Vec<Message>) {
    let original_len = messages.len();
    messages.retain(|m| {
        let Message::Assistant(asst) = m else {
            return true;
        };
        let LlmMessage::Assistant { content, .. } = &asst.message else {
            return true;
        };
        if content.is_empty() {
            return true;
        }
        // Pure whitespace-only Text parts? drop. Any non-Text or any
        // non-whitespace Text? keep.
        let only_whitespace = content.iter().all(|p| match p {
            coco_llm_types::AssistantContentPart::Text(t) => t.text.trim().is_empty(),
            _ => false,
        });
        !only_whitespace
    });
    if messages.len() != original_len {
        // Removing assistants may leave adjacent users that need merging.
        merge_consecutive_user_messages(messages);
    }
}

/// Replace empty content arrays in non-final assistant messages with a
/// `[No message content]` placeholder.
///
/// TS: `messages.ts:4933 ensureNonEmptyAssistantContent`. The API requires
/// "all messages must have non-empty content except for the optional final
/// assistant message". The final message is left as-is so prefill paths
/// keep working.
pub fn ensure_non_empty_assistant_content(messages: &mut [Message]) {
    if messages.is_empty() {
        return;
    }
    let last_idx = messages.len() - 1;
    for (idx, msg) in messages.iter_mut().enumerate() {
        if idx == last_idx {
            continue;
        }
        let Message::Assistant(asst) = msg else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &mut asst.message else {
            continue;
        };
        if content.is_empty() {
            *content = vec![coco_llm_types::AssistantContentPart::Text(
                coco_llm_types::TextPart::new("[No message content]"),
            )];
        }
    }
}

/// Filter assistant messages whose content is only `Reasoning` parts and
/// whose `request_id` is NOT shared by another assistant message that
/// carries non-thinking content.
///
/// TS: `messages.ts:4991 filterOrphanedThinkingOnlyMessages`. Streaming
/// emits one assistant message per content_block_stop with a stable
/// `message.id`; loaders that fail to merge those chunks (compaction
/// slicing, resume) leave orphaned thinking-only chunks behind. Sending
/// them produces "thinking blocks cannot be modified" 400s. Rust uses
/// `request_id` as the equivalent of TS `message.id`.
pub fn filter_orphaned_thinking_only_messages(messages: &mut Vec<Message>) {
    use std::collections::HashSet;
    let mut ids_with_non_thinking: HashSet<String> = HashSet::new();
    for m in messages.iter() {
        let Message::Assistant(asst) = m else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &asst.message else {
            continue;
        };
        let has_non_thinking = content.iter().any(|p| {
            !matches!(
                p,
                coco_llm_types::AssistantContentPart::Reasoning(_)
                    | coco_llm_types::AssistantContentPart::ReasoningFile(_)
            )
        });
        if has_non_thinking && let Some(id) = asst.request_id.as_ref() {
            ids_with_non_thinking.insert(id.clone());
        }
    }
    messages.retain(|m| {
        let Message::Assistant(asst) = m else {
            return true;
        };
        let LlmMessage::Assistant { content, .. } = &asst.message else {
            return true;
        };
        if content.is_empty() {
            return true;
        }
        let all_thinking = content.iter().all(|p| {
            matches!(
                p,
                coco_llm_types::AssistantContentPart::Reasoning(_)
                    | coco_llm_types::AssistantContentPart::ReasoningFile(_)
            )
        });
        if !all_thinking {
            return true;
        }
        // All thinking; keep only if a sibling with same request_id has
        // non-thinking content (merge will heal it later).
        match asst.request_id.as_ref() {
            Some(id) => ids_with_non_thinking.contains(id),
            None => false, // No id → cannot ever merge → orphaned.
        }
    });
}

#[cfg(test)]
#[path = "normalize.test.rs"]
mod tests;
