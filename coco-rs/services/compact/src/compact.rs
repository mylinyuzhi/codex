//! Full compaction: summarize conversation via LLM.
//!
//! TS: compact.ts — builds summary prompt, calls LLM, replaces old messages.
//!
//! Key features:
//! - Image/document stripping before summarization
//! - Prompt-too-long retry with head group truncation (MAX_PTL_RETRIES=3)
//! - Post-compact attachment creation (file restore, plan, skills)
//! - Pre/post compact hook points

use coco_types::CompactTrigger;
use coco_types::Message;
use coco_types::UserContent;

use crate::grouping::group_messages_by_api_round;
use crate::tokens;
use crate::types::CompactError;
use crate::types::CompactResult;
use crate::types::MAX_COMPACT_STREAMING_RETRIES;
use crate::types::MAX_OUTPUT_TOKENS_FOR_SUMMARY;
use crate::types::MAX_PTL_RETRIES;

/// Configuration for full compaction.
pub struct CompactConfig {
    /// Maximum tokens for the summary output.
    pub max_summary_tokens: i64,
    /// Context window size.
    pub context_window: i64,
    /// Number of recent rounds to preserve (not compacted).
    pub keep_recent_rounds: usize,
    /// Custom compact prompt override.
    pub custom_prompt: Option<String>,
    /// Whether to suppress follow-up questions in the summary.
    pub suppress_follow_up: bool,
    /// How this compaction was triggered.
    pub trigger: CompactTrigger,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            max_summary_tokens: MAX_OUTPUT_TOKENS_FOR_SUMMARY,
            context_window: 200_000,
            keep_recent_rounds: 2,
            custom_prompt: None,
            suppress_follow_up: true,
            trigger: CompactTrigger::Auto,
        }
    }
}

/// Callback type for post-compact attachment generation.
///
/// Called after summarization to produce file/skill/plan attachments.
/// Returns attachment messages to include in the CompactResult.
pub type PostCompactAttachmentFn =
    Box<dyn FnOnce(&CompactResult) -> Vec<coco_types::AttachmentMessage> + Send>;

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
    config: &CompactConfig,
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
            is_recompaction: false,
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
    );

    let summary_message = Message::User(coco_types::UserMessage {
        message: coco_types::LlmMessage::user_text(&summary_user_msg),
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
    let boundary = create_boundary_marker(
        config.trigger,
        pre_tokens,
        post_tokens,
        Some(messages_summarized),
    );

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
        is_recompaction: false,
        trigger: config.trigger,
    };

    // Step 7: Generate post-compact attachments if callback provided
    if let Some(gen_fn) = attachment_fn {
        result.attachments = gen_fn(&result);
    }

    Ok(result)
}

/// Strip images and documents from messages to prevent prompt-too-long.
///
/// TS: `stripImagesFromMessages()` — replaces image/document content blocks
/// with `[image]` / `[document]` text placeholders.
pub fn strip_images_from_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|msg| match msg {
            Message::User(u) => {
                if let coco_types::LlmMessage::User {
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
                    new_u.message = coco_types::LlmMessage::User {
                        content: stripped,
                        provider_options: provider_options.clone(),
                    };
                    Message::User(new_u)
                } else {
                    msg.clone()
                }
            }
            _ => msg.clone(),
        })
        .collect()
}

/// Strip re-injectable attachment messages (skills, agents, etc.).
///
/// Attachments whose `AttachmentKind::survives_compaction()` returns true
/// are preserved (audit trail, UI-visible silent events, post-compact
/// file references). The rest are stripped — reminders regenerate per-turn,
/// silent dedup markers are ephemeral, and file content re-injection is
/// handled separately by [`create_post_compact_file_attachments`].
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
/// TS: `truncateHeadForPTLRetry()` — drops oldest groups until the prompt fits,
/// keeping at least 1 group. Returns None if only 1 group (nothing to drop).
pub fn truncate_head_for_ptl_retry<'a>(
    rounds: &'a [Vec<&'a Message>],
    drop_fraction: f64,
) -> Option<Vec<Vec<&'a Message>>> {
    if rounds.len() <= 1 {
        return None;
    }

    // Drop a fraction of groups from the front (default 20% if no gap info)
    let groups_to_drop = ((rounds.len() as f64 * drop_fraction).ceil() as usize).max(1);
    let remaining = &rounds[groups_to_drop..];

    if remaining.is_empty() {
        return None;
    }

    Some(remaining.to_vec())
}

// ── Internal helpers ────────────────────────────────────────────────

/// Call the summarize function with prompt-too-long retry logic.
async fn call_with_ptl_retry<F, Fut>(
    old_rounds: &[Vec<&Message>],
    config: &CompactConfig,
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
                    // PTL: truncate head and retry
                    if attempt >= MAX_PTL_RETRIES {
                        return Err(CompactError::PromptTooLong { message: e });
                    }
                    let total = old_rounds.len() - head_skip;
                    let groups_to_drop = ((total as f64 * 0.2).ceil() as usize).max(1);
                    head_skip += groups_to_drop;

                    if head_skip >= old_rounds.len() {
                        return Err(CompactError::PromptTooLong { message: e });
                    }
                    tracing::warn!(
                        "prompt too long on compact attempt {attempt}, dropping {groups_to_drop} groups"
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
    Message::System(coco_types::SystemMessage::CompactBoundary(
        coco_types::SystemCompactBoundaryMessage {
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
fn build_summary_prompt(rounds: &[Vec<&Message>], config: &CompactConfig) -> String {
    build_summary_prompt_from_refs(rounds, config)
}

fn build_summary_prompt_from_refs(rounds: &[Vec<&Message>], config: &CompactConfig) -> String {
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

fn is_image_media_type(file: &vercel_ai_provider::FilePart) -> bool {
    file.media_type.starts_with("image/")
}
