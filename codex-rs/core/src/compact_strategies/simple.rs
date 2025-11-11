use crate::compact::SUMMARIZATION_PROMPT;
use crate::compact::build_compacted_history;
use crate::compact_strategy::CompactContext;
use crate::compact_strategy::CompactStrategy;
use codex_protocol::models::ResponseItem;

/// Simple compaction strategy that uses the existing implementation
///
/// This strategy:
/// - Uses the original compact prompt (handoff-focused)
/// - Preserves recent user messages (up to 20k tokens)
/// - Does NOT recover files automatically
pub struct SimpleStrategy;

impl CompactStrategy for SimpleStrategy {
    fn name(&self) -> &str {
        "simple"
    }

    fn generate_prompt(&self) -> &str {
        SUMMARIZATION_PROMPT
    }

    fn build_compacted_history(
        &self,
        initial_context: Vec<ResponseItem>,
        user_messages: &[String],
        summary_text: &str,
        _context: &CompactContext,
    ) -> Vec<ResponseItem> {
        // Delegate to existing implementation
        build_compacted_history(initial_context, user_messages, summary_text)
    }
}
