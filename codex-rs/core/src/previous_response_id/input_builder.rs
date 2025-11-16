use crate::codex::Session;
use crate::codex::TurnContext;
use crate::protocol::EventMsg;
use crate::protocol::IncrementalInputUsedEvent;
use codex_protocol::models::ResponseItem;
use std::sync::Arc;

/// Build input for the current turn, using incremental history if possible.
///
/// # Decision Logic
///
/// Uses incremental input when ALL conditions are met:
/// 1. Adapter supports `previous_response_id`
/// 2. SessionState has last_response tracking data (response_id + history_len)
///
/// When using incremental mode, pending_input is appended to the incremental history.
/// Otherwise, falls back to full history.
///
/// # Arguments
///
/// - `session` - Current session with state and history
/// - `turn_context` - Current turn context with ModelClient
/// - `pending_input` - User-submitted items during model execution
///
/// # Returns
///
/// Vec<ResponseItem> ready to be sent to the model
pub async fn build_turn_input(
    session: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    pending_input: &[ResponseItem],
) -> Vec<ResponseItem> {
    // Check if adapter supports previous_response_id (no lock needed)
    let adapter_supports_incremental = turn_context
        .client
        .provider()
        .adapter
        .as_ref()
        .and_then(|name| {
            crate::adapters::get_adapter(name)
                .ok()
                .map(|adapter| adapter.supports_previous_response_id())
        })
        .unwrap_or(false);

    // Atomically get both response_id and history_len, then release lock immediately
    let last_response = {
        let state = session.state.lock().await;
        state
            .get_last_response()
            .map(|(id, len)| (id.to_string(), len))
    };

    // Decision: use incremental if adapter supports AND we have tracking data
    let use_incremental = adapter_supports_incremental && last_response.is_some();

    tracing::debug!(
        "Input mode decision: adapter_supports={}, has_tracking={}, pending_items={}, mode={}",
        adapter_supports_incremental,
        last_response.is_some(),
        pending_input.len(),
        if use_incremental {
            "incremental"
        } else {
            "full"
        }
    );

    if use_incremental {
        let (response_id, history_len) = last_response.as_ref().unwrap();

        // Build incremental input (only new items since last response)
        let mut incremental_input =
            build_incremental_input(session, response_id, *history_len).await;

        // Append any pending input (tool outputs, user messages during execution)
        incremental_input.extend_from_slice(pending_input);

        let total_items = incremental_input.len();
        let pending_count = pending_input.len();

        tracing::debug!(
            "Built incremental input: {} total items ({} new + {} pending), response_id={}",
            total_items,
            total_items - pending_count,
            pending_count,
            response_id
        );

        // Notify UI that we're using incremental mode (for debugging/transparency)
        session
            .send_event(
                turn_context.as_ref(),
                EventMsg::IncrementalInputUsed(IncrementalInputUsedEvent {
                    items_count: total_items as i64,
                }),
            )
            .await;

        incremental_input
    } else {
        // Standard full history fallback
        session.clone_history().await.get_history_for_prompt()
    }
}

/// Build incremental input containing only items after the last response completed.
///
/// # Algorithm
///
/// 1. Get full history from ContextManager
/// 2. Validate that expected_history_len <= current history length
/// 3. Return all items after expected_history_len (i.e., new items since last response)
///
/// # Arguments
///
/// - `session` - Current session with history
/// - `expected_response_id` - The response_id for logging/debugging
/// - `expected_history_len` - History length when the response completed
///
/// # Returns
///
/// Items added after the response completed, or full history if validation fails
async fn build_incremental_input(
    session: &Arc<Session>,
    expected_response_id: &str,
    expected_history_len: usize,
) -> Vec<ResponseItem> {
    let history = session.clone_history().await.get_history_for_prompt();
    let current_len = history.len();

    tracing::trace!(
        "Incremental input check: expected_len={}, current_len={}, response_id={}",
        expected_history_len,
        current_len,
        expected_response_id
    );

    // Defensive check: if expected_len > current_len, history was rolled back
    // This should not happen if clear_last_response() is called correctly,
    // but we fallback to full history as a safety measure
    if expected_history_len > current_len {
        tracing::error!(
            "CRITICAL: History rollback detected but last_response not cleared! \
             expected_len={}, current_len={}. This indicates a bug in history management. \
             Falling back to full history.",
            expected_history_len,
            current_len
        );
        return history;
    }

    // Extract new items added since last response
    let incremental: Vec<_> = history.into_iter().skip(expected_history_len).collect();

    let new_items_count = incremental.len();

    tracing::debug!(
        "Incremental mode: {} new items added since response completion \
         (response_id={}, base_len={}, current_len={})",
        new_items_count,
        expected_response_id,
        expected_history_len,
        current_len
    );

    incremental
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseItem;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_incremental_input_slicing() {
        let history = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "first message".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "first response".to_string(),
                }],
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
                call_id: "call_1".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call_1".to_string(),
                output: codex_protocol::models::FunctionCallOutputPayload {
                    content: "file content".to_string(),
                    content_items: None,
                    success: Some(true),
                },
            },
        ];

        // Simulate: response completed when history had 2 items
        // New items added: FunctionCall and FunctionCallOutput (indices 2, 3)
        let expected_history_len = 2;
        let incremental: Vec<_> = history.into_iter().skip(expected_history_len).collect();

        assert_eq!(incremental.len(), 2);
        assert!(matches!(incremental[0], ResponseItem::FunctionCall { .. }));
        assert!(matches!(
            incremental[1],
            ResponseItem::FunctionCallOutput { .. }
        ));
    }

    #[test]
    fn test_no_response_history_returns_full_history() {
        let history = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "first message".to_string(),
            }],
        }];

        // Simulate: no previous response completed (history_len = 0)
        // Should return full history
        let expected_history_len = 0;
        let result: Vec<_> = history.into_iter().skip(expected_history_len).collect();

        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], ResponseItem::Message { role, .. } if role == "user"));
    }

    #[test]
    fn test_multiple_turns_uses_correct_history_len() {
        let history = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Q1".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "A1".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Q2".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "A2".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Q3".to_string(),
                }],
            },
        ];

        // Simulate: last response completed when history had 4 items (up to A2)
        // New item added: Q3 (index 4)
        let expected_history_len = 4;
        let incremental: Vec<_> = history.into_iter().skip(expected_history_len).collect();

        assert_eq!(incremental.len(), 1); // Only Q3
        assert!(matches!(
            &incremental[0],
            ResponseItem::Message { role, .. } if role == "user"
        ));
    }
}
