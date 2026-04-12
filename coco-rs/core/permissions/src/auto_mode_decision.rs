//! Auto-mode classifier decision integration.
//!
//! TS: utils/permissions/classifierDecision.ts (98 LOC)
//!
//! Entry point for auto-mode permission checks. Called by the query engine
//! after `PermissionEvaluator::evaluate()` returns `Ask` and the mode is
//! `Auto`. Chains: safe-tool check → heuristic → LLM classifier → denial
//! tracking.

use coco_types::Message;
use coco_types::PermissionDecision;
use coco_types::PermissionDecisionReason;
use serde_json::Value;
use std::future::Future;

use crate::auto_mode::AutoModeDecision;
use crate::auto_mode::classify_for_auto_mode;
use crate::auto_mode_state::AutoModeState;
use crate::classifier::AutoModeRules;
use crate::classifier::ClassifyRequest;
use crate::classifier::classify_yolo_action;
use crate::classifier::is_safe_tool;
use crate::denial_tracking::DenialTracker;

/// Attempt auto-mode classification for a tool call.
///
/// Returns `None` when auto-mode is inactive or the circuit breaker tripped
/// (caller should fall through to the interactive permission dialog).
/// Returns `Some(decision)` when auto-mode handled the request.
///
/// TS: `canUseToolInAutoMode()` in classifierDecision.ts
pub async fn can_use_tool_in_auto_mode<F, Fut>(
    tool_name: &str,
    input: &Value,
    is_read_only: bool,
    auto_state: &AutoModeState,
    denial_tracker: &mut DenialTracker,
    messages: &[Message],
    rules: &AutoModeRules,
    classify_fn: F,
) -> Option<PermissionDecision>
where
    F: Fn(ClassifyRequest) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    // 1. Not active → None (fallthrough to interactive)
    if !auto_state.is_active() {
        return None;
    }

    // 2. Circuit breaker tripped → None (fallthrough)
    if denial_tracker.is_circuit_breaker_tripped() {
        tracing::debug!("auto-mode: circuit breaker tripped, falling back to interactive");
        return None;
    }

    // 3. Safe tool allowlist → Allow immediately (skip classifier)
    if is_safe_tool(tool_name) {
        return Some(PermissionDecision::Allow {
            updated_input: None,
            feedback: None,
        });
    }

    // 4. Heuristic classifier (fast path for read-only tools, Bash read-only, etc.)
    let heuristic = classify_for_auto_mode(tool_name, input, is_read_only);
    if heuristic == AutoModeDecision::Allow {
        return Some(PermissionDecision::Allow {
            updated_input: None,
            feedback: None,
        });
    }

    // 5. LLM classifier (two-stage XML)
    let result = classify_yolo_action(messages, tool_name, input, rules, classify_fn).await;

    // 6. Interpret result
    if result.should_block {
        denial_tracker.record_denial(tool_name);
        Some(PermissionDecision::Deny {
            message: result.reason,
            reason: PermissionDecisionReason::Classifier {
                classifier: "auto_mode".into(),
                reason: format!("stage={} model={}", result.stage.unwrap_or(0), result.model),
            },
        })
    } else {
        denial_tracker.reset_consecutive();
        Some(PermissionDecision::Allow {
            updated_input: None,
            feedback: None,
        })
    }
}

#[cfg(test)]
#[path = "auto_mode_decision.test.rs"]
mod tests;
