//! Layer-1 tool-input parsing helper for the streaming / non-streaming
//! engine paths.
//!
//! Wraps [`coco_utils_json_repair::parse_with_repair`] with the
//! `parse_failure → Value::Object({})` fallback that Layer 2 schema
//! validation depends on. Mirrors TS Claude Code's `parsed ?? {}` in
//! `utils/messages.ts:2694` — by handing the validator an empty
//! object instead of a `Null` sentinel, the LLM receives a precise
//! "missing required field foo" reply on the next turn instead of a
//! generic "JSON broken".
//!
//! Parallel to `vercel_ai_provider_utils::parse_tool_arguments_or_empty`
//! (used inside provider adapters). Both delegate to `llm_json` so
//! behavioral drift is bounded; the duplication exists because the
//! layering rule forbids provider-utils from depending on
//! `coco-utils-*`.

use coco_utils_json_repair::RepairOutcome;
use coco_utils_json_repair::parse_with_repair;
use serde_json::Value;

/// Parse tool-call arguments, falling back to `Value::Object({})`
/// when even repair fails. Emits a `warn!` on both repair-assisted
/// parses and total failures so ops can monitor real-world repair
/// frequency without sampling code.
pub fn parse_tool_arguments_or_empty(raw: &str, tool_name: &str) -> Value {
    match parse_with_repair(raw) {
        Ok((v, RepairOutcome::Clean)) => v,
        Ok((v, RepairOutcome::Repaired)) => {
            tracing::warn!(
                target: "coco_query::tool_input",
                tool_name,
                args_bytes = raw.len(),
                "tool-call arguments JSON required repair before parse"
            );
            v
        }
        Err(err) => {
            tracing::warn!(
                target: "coco_query::tool_input",
                tool_name,
                args_bytes = raw.len(),
                error = %err,
                "tool-call arguments parse failed; falling back to empty object"
            );
            Value::Object(Default::default())
        }
    }
}
