use std::path::Path;

use coco_types::ToolName;
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ToolInputNormalizationContext<'a> {
    pub session_id: Option<&'a str>,
    pub plans_dir: Option<&'a Path>,
    pub agent_id: Option<&'a str>,
}

/// Normalize assistant-emitted tool input before it reaches transcript,
/// stream events, hooks, permission, and execution.
///
/// TS parity: `utils/api.ts::normalizeToolInput` injects `plan` and
/// `planFilePath` for `ExitPlanMode` so hooks and SDK consumers observe
/// the same plan the tool will read from disk. The mirrored strip step
/// (`normalizeToolInputForAPI`) lives in
/// [`coco_messages::normalize`]: `normalize_messages_for_api` removes
/// these same fields before the assistant message is re-sent to the
/// model, since the `ExitPlanMode` wire schema is an empty object.
pub(crate) fn normalize_observable_tool_input(
    tool_name: &str,
    input: Value,
    ctx: ToolInputNormalizationContext<'_>,
) -> Value {
    if tool_name != ToolName::ExitPlanMode.as_str() {
        return input;
    }

    let (Some(session_id), Some(plans_dir)) = (ctx.session_id, ctx.plans_dir) else {
        return input;
    };
    let Some(plan) = coco_context::get_plan(session_id, plans_dir, ctx.agent_id) else {
        return input;
    };

    let plan_file_path = coco_context::get_plan_file_path(session_id, plans_dir, ctx.agent_id)
        .to_string_lossy()
        .into_owned();
    let mut object = match input {
        Value::Object(map) => map,
        other => return other,
    };
    object.insert(
        coco_messages::EXIT_PLAN_MODE_INJECTED_PLAN_FIELD.into(),
        Value::String(plan),
    );
    object.insert(
        coco_messages::EXIT_PLAN_MODE_INJECTED_PLAN_FILE_PATH_FIELD.into(),
        Value::String(plan_file_path),
    );
    Value::Object(object)
}

#[cfg(test)]
#[path = "tool_input_normalizer.test.rs"]
mod tests;
