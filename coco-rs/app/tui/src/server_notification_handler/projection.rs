//! Shared projections from transient UI state into transcript messages.

use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;

pub(super) fn flush_streaming_to_messages(state: &mut AppState) {
    let Some(streaming) = state.ui.streaming.take() else {
        return;
    };
    if !streaming.thinking.is_empty() {
        let duration_ms = streaming.started_at.elapsed().as_millis().try_into().ok();
        let reasoning_tokens =
            crate::presentation::thinking::estimate_reasoning_tokens(&streaming.thinking);
        state.session.add_message(ChatMessage {
            id: format!(
                "thinking-{}-{}",
                state.session.turn_count,
                state.session.messages.len()
            ),
            role: crate::state::ChatRole::Assistant,
            content: MessageContent::Thinking {
                content: streaming.thinking,
                duration_ms,
                reasoning_tokens: Some(reasoning_tokens),
            },
            is_meta: false,
            created_at_ms: crate::state::session::now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
            permission_mode: None,
        });
    }
    if !streaming.content.is_empty() {
        state.session.record_agent_markdown(&streaming.content);
        state.session.add_message(ChatMessage::assistant_text(
            format!(
                "turn-{}-{}",
                state.session.turn_count,
                state.session.messages.len()
            ),
            streaming.content,
        ));
    }
}
