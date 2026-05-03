//! Full compaction: summarize conversation via LLM.
//!
//! TS: compact.ts — builds summary prompt, calls LLM, replaces old messages.
//!
//! Key features:
//! - Image/document stripping before summarization
//! - Prompt-too-long retry with head group truncation (MAX_PTL_RETRIES=3)
//! - Post-compact attachment creation (file restore, plan, skills)
//! - Pre/post compact hook points

use coco_messages::Message;
use coco_messages::PartialCompactDirection;
use coco_messages::PreservedSegment;
use coco_messages::SystemCompactBoundaryMessage;
use coco_messages::SystemMessage;
use coco_messages::UserContent;
use coco_messages::UserMessage;
use coco_types::CompactTrigger;
use uuid::Uuid;

use crate::grouping::group_messages_by_api_round;
use crate::tokens;
use crate::types::CompactError;
use crate::types::CompactResult;
use crate::types::MAX_COMPACT_STREAMING_RETRIES;
use crate::types::MAX_OUTPUT_TOKENS_FOR_SUMMARY;
use crate::types::MAX_PTL_RETRIES;
use crate::types::PTL_RETRY_MARKER;
use crate::types::extract_discovered_tool_names;

/// Per-invocation parameters for [`compact_conversation`].
///
/// Distinct from `coco_config::CompactConfig` (the global resolved
/// settings struct) — this carries only the knobs that vary per call:
/// summary token budget, what to keep, and the trigger label that ends
/// up on the boundary marker.
pub struct CompactRunOptions {
    /// Maximum tokens for the summary output.
    pub max_summary_tokens: i64,
    /// Context window size of the model running the summarizer.
    pub context_window: i64,
    /// Number of recent rounds to preserve (not compacted).
    pub keep_recent_rounds: usize,
    /// Custom compact prompt override (merged from PreCompact hooks +
    /// `/compact <instructions>`).
    pub custom_prompt: Option<String>,
    /// Whether to suppress follow-up questions in the summary.
    pub suppress_follow_up: bool,
    /// How this compaction was triggered.
    pub trigger: CompactTrigger,
    /// Recompaction tracking — populated when this compaction follows a
    /// previous one in the same conversation. TS:
    /// `compact.ts:317 RecompactionInfo`. Drives `tengu_compact` analytics
    /// (H1/H2/H3/H5 chain disambiguation). When `Some`, sets
    /// `CompactResult.is_recompaction` to the embedded flag so consumers
    /// downstream see the chain state.
    pub recompaction_info: Option<crate::types::RecompactionInfo>,
}

impl Default for CompactRunOptions {
    fn default() -> Self {
        Self {
            max_summary_tokens: MAX_OUTPUT_TOKENS_FOR_SUMMARY,
            context_window: 200_000,
            keep_recent_rounds: 2,
            custom_prompt: None,
            suppress_follow_up: true,
            trigger: CompactTrigger::Auto,
            recompaction_info: None,
        }
    }
}

/// Callback type for post-compact attachment generation.
///
/// Called after summarization to produce file/skill/plan attachments.
/// Returns attachment messages to include in the CompactResult.
pub type PostCompactAttachmentFn =
    Box<dyn FnOnce(&CompactResult) -> Vec<coco_messages::AttachmentMessage> + Send>;

/// Perform full compaction on a conversation.
///
/// 1. Strip images/documents from messages
/// 2. Group messages into API rounds
/// 3. Select old rounds for compaction
/// 4. Build summary prompt from old rounds
/// 5. Call LLM to generate summary (via callback), with PTL retry
/// 6. Create boundary marker + summary messages
/// 7. Optionally generate post-compact attachments
///
/// The `summarize_fn` callback avoids depending on coco-inference:
/// the caller provides an async function that takes a prompt and returns a summary.
pub async fn compact_conversation<F, Fut>(
    messages: &[Message],
    config: &CompactRunOptions,
    summarize_fn: F,
    attachment_fn: Option<PostCompactAttachmentFn>,
) -> Result<CompactResult, CompactError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    // Step 1: Strip images/documents to avoid prompt-too-long on media-heavy conversations
    let stripped = strip_images_from_messages(messages);
    let working_messages = strip_reinjected_attachments(&stripped);

    // Step 2-3: Group and split
    let rounds = group_messages_by_api_round(&working_messages);

    if rounds.len() <= config.keep_recent_rounds {
        let boundary = create_boundary_marker(config.trigger, 0, 0, None);
        return Ok(CompactResult {
            boundary_marker: boundary,
            summary_messages: vec![],
            attachments: vec![],
            messages_to_keep: messages.to_vec(),
            hook_results: vec![],
            user_display_message: None,
            pre_compact_tokens: 0,
            post_compact_tokens: 0,
            true_post_compact_tokens: 0,
            is_recompaction: config
                .recompaction_info
                .as_ref()
                .is_some_and(|i| i.is_recompaction),
            trigger: config.trigger,
        });
    }

    let split_point = rounds.len() - config.keep_recent_rounds;
    let old_rounds = &rounds[..split_point];
    let recent_rounds = &rounds[split_point..];

    let pre_tokens = tokens::estimate_tokens(messages);

    // Step 4: Build summary prompt
    let summary_prompt = build_summary_prompt(old_rounds, config);

    // Step 5: Call LLM with retry on prompt-too-long
    let summary_text =
        call_with_ptl_retry(old_rounds, config, &summarize_fn, summary_prompt).await?;

    // Format the summary
    let formatted = crate::prompt::format_compact_summary(&summary_text);

    // Step 6: Build result messages
    let messages_to_keep: Vec<Message> = recent_rounds
        .iter()
        .flat_map(|round| round.iter().copied().cloned())
        .collect();

    let summary_user_msg = crate::prompt::get_compact_user_summary_message(
        &formatted,
        config.suppress_follow_up,
        /*transcript_path*/ None,
        /*recent_messages_preserved*/ false,
    );

    let summary_message = Message::User(coco_messages::UserMessage {
        message: coco_messages::LlmMessage::user_text(&summary_user_msg),
        uuid: uuid::Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: true,
        is_virtual: false,
        is_compact_summary: true,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    });

    let post_tokens = tokens::estimate_text_tokens(&summary_user_msg)
        + tokens::estimate_tokens(&messages_to_keep);

    let messages_summarized = old_rounds.iter().map(Vec::len).sum::<usize>() as i32;
    let mut boundary = create_boundary_marker(
        config.trigger,
        pre_tokens,
        post_tokens,
        Some(messages_summarized),
    );

    // Persist the discovered-tool set so post-compact ToolSearch state survives.
    // TS: compact.ts:606-611 (`extractDiscoveredToolNames(messages)`).
    let discovered = extract_discovered_tool_names(messages);
    if !discovered.is_empty()
        && let Message::System(SystemMessage::CompactBoundary(ref mut b)) = boundary
    {
        b.pre_compact_discovered_tools = discovered.into_iter().collect();
    }

    // Annotate the boundary with the preserved-segment chain. The anchor
    // for full compaction is the boundary marker itself (TS:
    // `annotateBoundaryWithPreservedSegment(boundary, boundary.uuid, keep)`
    // at compact.ts:1083 for the prefix-preserving case).
    if !messages_to_keep.is_empty()
        && let Message::System(SystemMessage::CompactBoundary(b)) = &boundary
    {
        let anchor = b.uuid;
        if let Message::System(SystemMessage::CompactBoundary(ref mut bm)) = boundary {
            bm.preserved_segment = build_preserved_segment(anchor, &messages_to_keep);
        }
    }

    let mut result = CompactResult {
        boundary_marker: boundary,
        summary_messages: vec![summary_message],
        attachments: vec![],
        messages_to_keep,
        hook_results: vec![],
        user_display_message: None,
        pre_compact_tokens: pre_tokens,
        post_compact_tokens: post_tokens,
        true_post_compact_tokens: post_tokens,
        is_recompaction: config
            .recompaction_info
            .as_ref()
            .is_some_and(|i| i.is_recompaction),
        trigger: config.trigger,
    };

    // Step 7: Generate post-compact attachments if callback provided
    if let Some(gen_fn) = attachment_fn {
        result.attachments = gen_fn(&result);
    }

    Ok(result)
}

/// Partial compaction: summarize half of the conversation, keep the other.
///
/// TS: `partialCompactConversation` (compact.ts:772-1106). Direction:
/// - `Newest` (TS `'from'`): pivot+ summarized, prefix kept.
///   Anchor = boundary; cache for the kept prefix is preserved.
/// - `Oldest` (TS `'up_to'`): prefix summarized, pivot+ kept.
///   Anchor = last summary message; cache invalidated.
///
/// Tool-pair safety: `messages_to_keep` is filtered against
/// `is_compact_boundary_message` to avoid re-introducing stale
/// boundaries after a re-compact (TS:798).
pub async fn partial_compact_conversation<F, Fut>(
    all_messages: &[Message],
    pivot_index: usize,
    direction: PartialCompactDirection,
    user_feedback: Option<&str>,
    custom_instructions: Option<&str>,
    summarize_fn: F,
    attachment_fn: Option<PostCompactAttachmentFn>,
) -> Result<CompactResult, CompactError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    if pivot_index > all_messages.len() {
        return Err(CompactError::LlmCallFailed {
            message: "partial compact pivot out of range".into(),
        });
    }

    let (to_summarize, to_keep_raw): (Vec<Message>, Vec<Message>) = match direction {
        PartialCompactDirection::Oldest => (
            all_messages[..pivot_index].to_vec(),
            all_messages[pivot_index..].to_vec(),
        ),
        PartialCompactDirection::Newest => (
            all_messages[pivot_index..].to_vec(),
            all_messages[..pivot_index].to_vec(),
        ),
    };

    if to_summarize.is_empty() {
        return Err(CompactError::LlmCallFailed {
            message: match direction {
                PartialCompactDirection::Oldest => {
                    "Nothing to summarize before the selected message.".into()
                }
                PartialCompactDirection::Newest => {
                    "Nothing to summarize after the selected message.".into()
                }
            },
        });
    }

    // Filter progress + (for Oldest) old compact boundaries / summary
    // messages from `to_keep_raw` so a stale boundary doesn't shadow the
    // new one. TS: compact.ts:790-800.
    let to_keep: Vec<Message> = to_keep_raw
        .into_iter()
        .filter(|m| match m {
            Message::Progress(_) => false,
            Message::System(SystemMessage::CompactBoundary(_))
                if direction == PartialCompactDirection::Oldest =>
            {
                false
            }
            Message::User(u)
                if direction == PartialCompactDirection::Oldest && u.is_compact_summary =>
            {
                false
            }
            _ => true,
        })
        .collect();

    let pre_tokens = tokens::estimate_tokens(all_messages);

    // Merge user feedback with custom instructions.
    let merged = match (custom_instructions, user_feedback) {
        (Some(ci), Some(uf)) if !ci.is_empty() && !uf.is_empty() => {
            Some(format!("{ci}\n\nUser context: {uf}"))
        }
        (Some(ci), _) if !ci.is_empty() => Some(ci.to_string()),
        (_, Some(uf)) if !uf.is_empty() => Some(format!("User context: {uf}")),
        _ => None,
    };

    let prompt = crate::prompt::get_partial_compact_prompt(merged.as_deref(), direction);

    // Strip media + attachments before summarizing.
    let working = strip_reinjected_attachments(&strip_images_from_messages(&to_summarize));
    let rounds = group_messages_by_api_round(&working);

    let summary_text = call_with_ptl_retry(
        &rounds,
        &CompactRunOptions {
            custom_prompt: merged.clone(),
            ..Default::default()
        },
        &summarize_fn,
        prompt,
    )
    .await?;

    let formatted = crate::prompt::format_compact_summary(&summary_text);
    let summary_user_msg = crate::prompt::get_compact_user_summary_message(
        &formatted, /*suppress_follow_up*/ false, /*transcript_path*/ None,
        /*recent_messages_preserved*/ true,
    );

    let summary_message = Message::User(UserMessage {
        message: coco_messages::LlmMessage::user_text(&summary_user_msg),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: true,
        is_virtual: false,
        is_compact_summary: true,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    });

    let post_tokens =
        tokens::estimate_text_tokens(&summary_user_msg) + tokens::estimate_tokens(&to_keep);

    let mut boundary_struct = SystemCompactBoundaryMessage {
        uuid: Uuid::new_v4(),
        tokens_before: pre_tokens,
        tokens_after: post_tokens,
        trigger: CompactTrigger::Manual,
        user_context: user_feedback.map(str::to_string),
        messages_summarized: Some(to_summarize.len() as i32),
        pre_compact_discovered_tools: extract_discovered_tool_names(all_messages)
            .into_iter()
            .collect(),
        preserved_segment: None,
    };

    // Anchor differs by direction (TS compact.ts:1078-1082):
    //   Newest ('from')   → anchor = boundary
    //   Oldest ('up_to') → anchor = last summary
    let anchor = match direction {
        PartialCompactDirection::Newest => boundary_struct.uuid,
        PartialCompactDirection::Oldest => {
            summary_message.uuid().copied().unwrap_or_else(Uuid::nil)
        }
    };
    annotate_boundary_with_preserved_segment(&mut boundary_struct, anchor, &to_keep);

    let mut result = CompactResult {
        boundary_marker: Message::System(SystemMessage::CompactBoundary(boundary_struct)),
        summary_messages: vec![summary_message],
        attachments: vec![],
        messages_to_keep: to_keep,
        hook_results: vec![],
        user_display_message: None,
        pre_compact_tokens: pre_tokens,
        post_compact_tokens: post_tokens,
        true_post_compact_tokens: post_tokens,
        is_recompaction: false,
        trigger: CompactTrigger::Manual,
    };

    if let Some(gen_fn) = attachment_fn {
        result.attachments = gen_fn(&result);
    }
    Ok(result)
}

/// Annotate a compact boundary with `preserved_segment` metadata.
///
/// TS: `annotateBoundaryWithPreservedSegment(boundary, anchorUuid, kept)`.
/// `anchor_uuid` is the message that sits immediately before `kept[0]` in
/// the desired chain — for prefix-preserving compactions (full / partial
/// `Newest`), this is the boundary itself; for suffix-preserving
/// (partial `Oldest` / session-memory), it is the last summary message.
pub fn annotate_boundary_with_preserved_segment(
    boundary: &mut SystemCompactBoundaryMessage,
    anchor_uuid: Uuid,
    messages_to_keep: &[Message],
) {
    boundary.preserved_segment = build_preserved_segment(anchor_uuid, messages_to_keep);
}

fn build_preserved_segment(anchor_uuid: Uuid, kept: &[Message]) -> Option<PreservedSegment> {
    let head_uuid = *kept.first().and_then(Message::uuid)?;
    let tail_uuid = *kept.last().and_then(Message::uuid)?;
    Some(PreservedSegment {
        head_uuid,
        anchor_uuid,
        tail_uuid,
    })
}

/// Assemble the post-compact message chain in TS-canonical order.
///
/// TS: `buildPostCompactMessages(result)` in compact.ts:330. Order:
/// boundary → summaries → kept → attachments → hook results. Caller wires
/// this into the conversation history.
pub fn build_post_compact_messages(result: &CompactResult) -> Vec<Message> {
    let mut out =
        Vec::with_capacity(2 + result.summary_messages.len() + result.messages_to_keep.len());
    out.push(result.boundary_marker.clone());
    out.extend(result.summary_messages.clone());
    out.extend(result.messages_to_keep.clone());
    out.extend(result.attachments.iter().cloned().map(Message::Attachment));
    out.extend(result.hook_results.clone());
    out
}

/// Merge user-supplied compact instructions with hook-provided ones.
///
/// TS: `mergeHookInstructions`. User text comes first; hook output is
/// appended after a blank line. Empty inputs collapse to `None`.
#[must_use]
pub fn merge_hook_instructions(
    user_instructions: Option<&str>,
    hook_instructions: Option<&str>,
) -> Option<String> {
    let user = user_instructions
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let hook = hook_instructions
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    match (user, hook) {
        (None, None) => None,
        (Some(u), None) => Some(u),
        (None, Some(h)) => Some(h),
        (Some(u), Some(h)) => Some(format!("{u}\n\n{h}")),
    }
}

/// Strip images and documents from messages to prevent prompt-too-long.
///
/// TS: `stripImagesFromMessages()` (`compact.ts:145-200`) — replaces
/// image/document content blocks with `[image]` / `[document]` text
/// placeholders. **Includes images nested inside tool_result content arrays**
/// (per TS lines 166-184) — Bash/MCP tool_results that carry image data
/// (e.g. `cat image.png`) must be stripped before the compact summarizer
/// runs or the summarization request itself trips prompt-too-long on the
/// re-encoded base64.
pub fn strip_images_from_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|msg| match msg {
            Message::User(u) => {
                if let coco_messages::LlmMessage::User {
                    content,
                    provider_options,
                } = &u.message
                {
                    let stripped: Vec<UserContent> = content
                        .iter()
                        .map(|part| match part {
                            UserContent::File(f) => {
                                let placeholder = if is_image_media_type(f) {
                                    "[image]"
                                } else {
                                    "[document]"
                                };
                                UserContent::text(placeholder)
                            }
                            other => other.clone(),
                        })
                        .collect();
                    let mut new_u = u.clone();
                    new_u.message = coco_messages::LlmMessage::User {
                        content: stripped,
                        provider_options: provider_options.clone(),
                    };
                    Message::User(new_u)
                } else {
                    msg.clone()
                }
            }
            // TS-parity: tool_result content arrays may carry FileData
            // (image/document) parts — those are common from BashTool when
            // stdout is detected as binary image bytes (`bash.rs:isLikely
            // ImageBytes` → `structuredContent`). Walk the inner
            // `ToolResultContent::Content` and replace FileData parts with
            // `[image]` / `[document]` Text parts.
            Message::ToolResult(tr) => {
                let coco_messages::LlmMessage::Tool {
                    content,
                    provider_options,
                } = &tr.message
                else {
                    return msg.clone();
                };
                let stripped: Vec<coco_messages::ToolContent> = content
                    .iter()
                    .map(|part| match part {
                        coco_messages::ToolContent::ToolResult(rp) => {
                            let new_output = strip_images_from_tool_result_content(&rp.output);
                            coco_messages::ToolContent::ToolResult(
                                coco_messages::ToolResultContent {
                                    output: new_output,
                                    ..rp.clone()
                                },
                            )
                        }
                        // ToolApprovalResponse carries no media — pass through.
                        other => other.clone(),
                    })
                    .collect();
                let mut new_tr = tr.clone();
                new_tr.message = coco_messages::LlmMessage::Tool {
                    content: stripped,
                    provider_options: provider_options.clone(),
                };
                Message::ToolResult(new_tr)
            }
            _ => msg.clone(),
        })
        .collect()
}

/// Replace `FileData` parts inside `ToolResultContent::Content` with
/// `[image]` / `[document]` text parts. Other variants pass through.
fn strip_images_from_tool_result_content(
    output: &coco_inference::ToolResultContent,
) -> coco_inference::ToolResultContent {
    let coco_inference::ToolResultContent::Content {
        value,
        provider_options,
    } = output
    else {
        return output.clone();
    };
    let new_value: Vec<coco_inference::ToolResultContentPart> = value
        .iter()
        .map(|p| match p {
            coco_inference::ToolResultContentPart::FileData { media_type, .. } => {
                let placeholder = if media_type.starts_with("image/") {
                    "[image]"
                } else {
                    "[document]"
                };
                coco_inference::ToolResultContentPart::Text {
                    text: placeholder.to_string(),
                    provider_options: None,
                }
            }
            other => other.clone(),
        })
        .collect();
    coco_inference::ToolResultContent::Content {
        value: new_value,
        provider_options: provider_options.clone(),
    }
}

/// Strip re-injectable attachment messages (skills, agents, etc.).
///
/// Attachments whose `AttachmentKind::survives_compaction()` returns true
/// are preserved (audit trail, UI-visible silent events, post-compact
/// file references). The rest are stripped — reminders regenerate per-turn,
/// silent dedup markers are ephemeral, and file content re-injection is
/// handled separately by [`create_post_compact_file_attachments`].
///
/// **Intentional divergence from TS** (compact.ts:211-223). TS only
/// filters `skill_discovery` / `skill_listing` and only when
/// `feature('EXPERIMENTAL_SKILL_SEARCH')` is on (no-op otherwise). Rust
/// uses the broader `AttachmentKind::survives_compaction()` predicate
/// because the Rust `AttachmentKind` taxonomy (60 variants) is a superset
/// of TS's, including reminders that didn't exist in TS at the time TS
/// wrote its narrow filter. The predicate keeps the safe ones (audit /
/// UI-visible) and drops the regenerable ones — equivalent intent, wider
/// coverage. Tracked in audit-gaps.md Round 10 as P2 (intentional, no
/// fix required).
pub fn strip_reinjected_attachments(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|msg| match msg {
            Message::Attachment(a) => a.kind.survives_compaction(),
            _ => true,
        })
        .cloned()
        .collect()
}

/// Truncate oldest message groups when prompt-too-long error occurs.
///
/// TS: `truncateHeadForPTLRetry()` (compact.ts:243-291) — drops oldest
/// API-round groups until enough tokens are freed. When `token_gap` is
/// provided, drops groups whose accumulated estimated tokens cover the
/// gap. Falls back to dropping `drop_fraction` (default 20%) of groups.
/// Keeps at least one group so there's something to summarize.
///
/// **Strips a leading [`PTL_RETRY_MARKER`] user message** before grouping —
/// otherwise the marker becomes its own group 0 on retries 2+, and the
/// 20% fallback would drop only the marker, re-add it, and stall.
///
/// On success, prepends a fresh `PTL_RETRY_MARKER` user message so the API
/// sees a `role=user` first message even if dropping group 0 left an
/// assistant-leading sequence.
pub fn truncate_head_for_ptl_retry(
    messages: &[Message],
    token_gap: Option<i64>,
    drop_fraction: f64,
) -> Option<Vec<Message>> {
    // Strip our own marker from a previous retry so it doesn't become its
    // own group 0 — TS: compact.ts:250-255.
    let input: &[Message] = match messages.first() {
        Some(Message::User(u)) if user_message_text_equals(u, PTL_RETRY_MARKER) => &messages[1..],
        _ => messages,
    };

    let group_refs = group_messages_by_api_round(input);
    if group_refs.len() < 2 {
        return None;
    }

    let drop_count = if let Some(gap) = token_gap {
        let mut acc: i64 = 0;
        let mut count = 0;
        for g in &group_refs {
            let group_msgs: Vec<Message> = g.iter().map(|m| (*m).clone()).collect();
            acc += tokens::estimate_tokens(&group_msgs);
            count += 1;
            if acc >= gap {
                break;
            }
        }
        count
    } else {
        ((group_refs.len() as f64 * drop_fraction).ceil() as usize).max(1)
    };

    // Always keep at least one group.
    let drop_count = drop_count.min(group_refs.len() - 1);
    if drop_count < 1 {
        return None;
    }

    let kept: Vec<Message> = group_refs[drop_count..]
        .iter()
        .flat_map(|g| g.iter().map(|m| (*m).clone()))
        .collect();

    // Group 0 always starts with a user-ish preamble; subsequent groups
    // start with assistant messages. Dropping group 0 leaves assistant-
    // first, which the API rejects. Prepend a synthetic user marker.
    let needs_marker = matches!(kept.first(), Some(Message::Assistant(_)));
    let mut out = Vec::with_capacity(kept.len() + usize::from(needs_marker));
    if needs_marker {
        out.push(make_ptl_marker_message());
    }
    out.extend(kept);
    Some(out)
}

fn user_message_text_equals(u: &UserMessage, needle: &str) -> bool {
    let coco_messages::LlmMessage::User { content, .. } = &u.message else {
        return false;
    };
    content
        .iter()
        .any(|p| matches!(p, UserContent::Text(t) if t.text == needle))
}

fn make_ptl_marker_message() -> Message {
    Message::User(UserMessage {
        message: coco_messages::LlmMessage::user_text(PTL_RETRY_MARKER),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: true,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

// ── Internal helpers ────────────────────────────────────────────────

/// Call the summarize function with prompt-too-long retry logic.
async fn call_with_ptl_retry<F, Fut>(
    old_rounds: &[Vec<&Message>],
    config: &CompactRunOptions,
    summarize_fn: &F,
    initial_prompt: String,
) -> Result<String, CompactError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut prompt = initial_prompt;
    // Track how many groups to skip from the front on PTL retry
    let mut head_skip: usize = 0;

    for attempt in 0..=MAX_PTL_RETRIES {
        for stream_retry in 0..=MAX_COMPACT_STREAMING_RETRIES {
            match summarize_fn(prompt.clone()).await {
                Ok(summary) => {
                    if summary.trim().is_empty() {
                        return Err(CompactError::LlmCallFailed {
                            message: "empty summary returned".into(),
                        });
                    }
                    return Ok(summary);
                }
                Err(e) if e.contains("prompt_too_long") || e.contains("context_length") => {
                    // PTL: truncate head and retry. Use the token gap if the
                    // error message exposes it (Anthropic format:
                    // "input length and `max_tokens` exceed context limit:
                    // X + Y > Z, decrease input length…"). Fallback to 20%.
                    if attempt >= MAX_PTL_RETRIES {
                        return Err(CompactError::PromptTooLong { message: e });
                    }
                    let token_gap = parse_prompt_too_long_token_gap(&e);
                    let total = old_rounds.len() - head_skip;
                    let groups_to_drop = match token_gap {
                        Some(gap) => {
                            let mut acc: i64 = 0;
                            let mut count = 0;
                            for g in &old_rounds[head_skip..] {
                                let group_msgs: Vec<Message> =
                                    g.iter().map(|m| (*m).clone()).collect();
                                acc += tokens::estimate_tokens(&group_msgs);
                                count += 1;
                                if acc >= gap {
                                    break;
                                }
                            }
                            count.max(1)
                        }
                        None => ((total as f64 * 0.2).ceil() as usize).max(1),
                    };
                    head_skip += groups_to_drop;

                    if head_skip >= old_rounds.len() {
                        return Err(CompactError::PromptTooLong { message: e });
                    }
                    tracing::warn!(
                        "prompt too long on compact attempt {attempt}, dropping {groups_to_drop} groups (gap={token_gap:?})"
                    );
                    let remaining = &old_rounds[head_skip..];
                    prompt = build_summary_prompt_from_refs(remaining, config);
                    break; // break stream_retry loop, continue PTL loop
                }
                Err(e) => {
                    // Transient error: retry stream
                    if stream_retry >= MAX_COMPACT_STREAMING_RETRIES {
                        return Err(CompactError::StreamRetryExhausted {
                            attempts: MAX_COMPACT_STREAMING_RETRIES + 1,
                        });
                    }
                    tracing::warn!("compact stream error (retry {stream_retry}): {e}");
                    continue;
                }
            }
        }
    }

    Err(CompactError::StreamRetryExhausted {
        attempts: MAX_PTL_RETRIES + 1,
    })
}

fn create_boundary_marker(
    trigger: CompactTrigger,
    pre_tokens: i64,
    post_tokens: i64,
    messages_summarized: Option<i32>,
) -> Message {
    Message::System(coco_messages::SystemMessage::CompactBoundary(
        coco_messages::SystemCompactBoundaryMessage {
            uuid: uuid::Uuid::new_v4(),
            tokens_before: pre_tokens,
            tokens_after: post_tokens,
            trigger,
            user_context: None,
            messages_summarized,
            pre_compact_discovered_tools: vec![],
            preserved_segment: None,
        },
    ))
}

/// Build a prompt asking the LLM to summarize the conversation.
fn build_summary_prompt(rounds: &[Vec<&Message>], config: &CompactRunOptions) -> String {
    build_summary_prompt_from_refs(rounds, config)
}

fn build_summary_prompt_from_refs(rounds: &[Vec<&Message>], config: &CompactRunOptions) -> String {
    let base_prompt = crate::prompt::get_compact_prompt(config.custom_prompt.as_deref());

    let mut conversation = String::with_capacity(base_prompt.len() + 4096);
    conversation.push_str(&base_prompt);
    conversation.push_str("\n\n--- Conversation to summarize ---\n\n");

    for (i, round) in rounds.iter().enumerate() {
        conversation.push_str(&format!("--- Round {} ---\n", i + 1));
        for msg in round {
            let role = match msg {
                Message::User(_) => "User",
                Message::Assistant(_) => "Assistant",
                Message::ToolResult(_) => "ToolResult",
                _ => "System",
            };
            if let Some(text) = tokens::extract_message_text(msg) {
                // Truncate very large messages to keep prompt manageable
                let max_chars = 4000;
                let truncated = if text.len() > max_chars {
                    format!("{}...(truncated {} chars)", &text[..max_chars], text.len())
                } else {
                    text
                };
                conversation.push_str(&format!("{role}: {truncated}\n"));
            }
        }
        conversation.push('\n');
    }

    conversation
}

fn is_image_media_type(file: &coco_inference::FilePart) -> bool {
    file.media_type.starts_with("image/")
}

/// Parse the token gap from an Anthropic prompt-too-long error message.
///
/// TS: `getPromptTooLongTokenGap` in `services/api/errors.ts` — extracts the
/// numeric overflow from messages of the form
/// `"input length and \`max_tokens\` exceed context limit: 250000 + 8192 > 256000, decrease …"`.
/// Returns `lhs - rhs` (the gap), or `None` if unparseable.
fn parse_prompt_too_long_token_gap(message: &str) -> Option<i64> {
    // Find "X + Y > Z" anywhere in the message.
    let parts: Vec<&str> = message.split_whitespace().collect();
    for window in parts.windows(5) {
        let [a, plus, b, gt, c] = window else {
            continue;
        };
        if *plus != "+" || *gt != ">" {
            continue;
        }
        // Strip trailing commas / punctuation.
        let lhs = a.trim_end_matches(|c: char| !c.is_ascii_digit());
        let mid = b.trim_end_matches(|c: char| !c.is_ascii_digit());
        let rhs = c.trim_end_matches(|c: char| !c.is_ascii_digit());
        let (Ok(av), Ok(bv), Ok(cv)) = (lhs.parse::<i64>(), mid.parse::<i64>(), rhs.parse::<i64>())
        else {
            continue;
        };
        let total = av + bv;
        if total > cv {
            return Some(total - cv);
        }
    }
    None
}
