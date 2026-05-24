//! Source-backed presentation model for the active streaming tail.

use crate::presentation::thinking::estimate_reasoning_tokens;
use crate::state::ui::StreamingState;

#[derive(Debug, Clone, Copy)]
pub(crate) struct StreamingTailInput<'a> {
    pub(crate) streaming: &'a StreamingState,
    pub(crate) show_thinking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamingTailView<'a> {
    pub(crate) blocks: Vec<StreamingTailBlock<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamingTailBlock<'a> {
    AssistantText(&'a str),
    Cursor,
    ThinkingTokens { count: i64 },
}

pub(crate) fn streaming_tail_view(input: StreamingTailInput<'_>) -> StreamingTailView<'_> {
    let mut blocks = Vec::new();
    let content = input.streaming.visible_content();
    if !content.is_empty() {
        blocks.push(StreamingTailBlock::AssistantText(content));
        blocks.push(StreamingTailBlock::Cursor);
    }

    if input.show_thinking && !input.streaming.thinking.is_empty() {
        blocks.push(StreamingTailBlock::ThinkingTokens {
            count: estimate_reasoning_tokens(&input.streaming.thinking),
        });
    }

    StreamingTailView { blocks }
}

#[cfg(test)]
#[path = "streaming.test.rs"]
mod tests;
