use std::sync::Arc;

use coco_llm_types::ToolCallPart;
use coco_messages::MessageHistory;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::prepare_committed_tool_call;
use crate::helpers::ToolCompletionEventMode;

fn registry_with(tool: Arc<dyn coco_tool_runtime::DynTool>) -> ToolRegistry {
    let registry = ToolRegistry::new();
    registry.register(tool);
    registry
}

/// calm-bouncing-biscuit regression: a freeform apply_patch call arrives as
/// a BARE STRING. The prepared call must carry the coerced `{patch: …}`
/// object — never the raw string — so permission carve-outs and
/// execute-time `T::Input` deserialization see the typed shape, while the
/// committed `ToolCallPart` keeps the wire shape for the provider
/// round-trip.
#[tokio::test]
async fn test_prepare_committed_freeform_raw_string_threads_coerced_input() {
    let tools = registry_with(Arc::new(coco_tools::tools::ApplyPatchTool));
    let ctx = ToolUseContext::test_default();
    let mut history = MessageHistory::new();
    let raw = "*** Begin Patch\n*** Add File: a.txt\n+hi\n*** End Patch\n";
    let tc = ToolCallPart::new("call-1", "apply_patch", json!(raw));

    let prepared = prepare_committed_tool_call(
        &None,
        &mut history,
        &tools,
        &ctx,
        &tc,
        ToolCompletionEventMode::Emit,
        None,
    )
    .await
    .expect("raw freeform input must prepare cleanly");

    assert_eq!(prepared.input.as_value(), &json!({ "patch": raw }));
    // Wire shape untouched: the assistant message round-trips the raw
    // envelope to the provider (`custom_tool_call.input` is a string).
    assert_eq!(tc.input, json!(raw));
    // No synthetic error tool_result was pushed.
    assert_eq!(history.len(), 0);
}

/// Double-encoded function arguments (`"{\"file_path\": …}"` as a JSON
/// string) take the string-recovery path; the recovered object must reach
/// the prepared call the same way freeform coercion does.
#[tokio::test]
async fn test_prepare_committed_double_encoded_json_threads_recovered_input() {
    let tools = registry_with(Arc::new(coco_tools::tools::ReadTool));
    let ctx = ToolUseContext::test_default();
    let mut history = MessageHistory::new();
    let tc = ToolCallPart::new(
        "call-2",
        "Read",
        json!("{\"file_path\": \"/tmp/recovered.txt\"}"),
    );

    let prepared = prepare_committed_tool_call(
        &None,
        &mut history,
        &tools,
        &ctx,
        &tc,
        ToolCompletionEventMode::Emit,
        None,
    )
    .await
    .expect("double-encoded input must recover and prepare");

    assert_eq!(
        prepared.input.as_value(),
        &json!({ "file_path": "/tmp/recovered.txt" })
    );
    assert_eq!(tc.input, json!("{\"file_path\": \"/tmp/recovered.txt\"}"));
}
