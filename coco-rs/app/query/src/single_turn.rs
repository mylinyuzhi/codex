//! Single-turn query execution.
//!
//! TS: query.ts (1.7K LOC) — one-shot query without multi-turn loop.
//! Used for compaction summaries, memory extraction, etc.

use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_types::TokenUsage;
use std::sync::Arc;
use vercel_ai_provider::LanguageModelV4Prompt;

/// Result of a single-turn query.
#[derive(Debug, Clone)]
pub struct SingleTurnResult {
    pub text: String,
    pub usage: TokenUsage,
    pub model: String,
    pub duration_ms: i64,
}

/// Execute a single-turn query (no tool calls, no loop).
///
/// Used for side-queries like compaction summaries, memory extraction,
/// and classifier calls.
pub async fn single_turn_query(
    client: &Arc<ApiClient>,
    system_prompt: &str,
    user_message: &str,
    max_tokens: Option<i64>,
) -> anyhow::Result<SingleTurnResult> {
    let start = std::time::Instant::now();

    let prompt: LanguageModelV4Prompt = vec![
        vercel_ai_provider::LanguageModelV4Message::system(system_prompt),
        vercel_ai_provider::LanguageModelV4Message::user_text(user_message),
    ];

    let params = QueryParams {
        prompt,
        max_tokens,
        thinking_level: None,
        fast_mode: false,
        tools: None,
    };

    let result = client
        .query(&params)
        .await
        .map_err(|e| anyhow::anyhow!("single-turn query failed: {e}"))?;

    // Extract text from response
    let text = result
        .content
        .iter()
        .filter_map(|part| {
            if let vercel_ai_provider::AssistantContentPart::Text(t) = part {
                Some(t.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");

    let duration_ms = start.elapsed().as_millis() as i64;

    Ok(SingleTurnResult {
        text,
        usage: result.usage,
        model: result.model,
        duration_ms,
    })
}

/// Execute a side query with a fast model (for classifiers, summaries).
///
/// TS: sideQuery.ts — uses a smaller/faster model for non-critical queries.
pub async fn side_query(
    client: &Arc<ApiClient>,
    system_prompt: &str,
    user_message: &str,
) -> anyhow::Result<String> {
    let result = single_turn_query(client, system_prompt, user_message, Some(4096)).await?;
    Ok(result.text)
}
