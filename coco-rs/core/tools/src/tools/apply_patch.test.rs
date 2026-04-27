use super::*;
use coco_tool_runtime::ToolUseContext;
use coco_types::Features;
use coco_types::ToolOverrides;
use std::sync::Arc;

#[test]
fn is_enabled_only_when_model_adds_apply_patch() {
    let tool = ApplyPatchTool;

    // Default overrides — model does NOT add apply_patch as extra.
    let mut ctx = ToolUseContext::test_default();
    ctx.features = Arc::new(Features::with_defaults());
    ctx.tool_overrides = Arc::new(ToolOverrides::none());
    assert!(
        !tool.is_enabled(&ctx),
        "apply_patch must be hidden when the active model didn't add it"
    );

    // gpt-5-style overrides — extra: apply_patch.
    ctx.tool_overrides =
        Arc::new(ToolOverrides::default().with_extra(ToolId::Builtin(ToolName::ApplyPatch)));
    assert!(tool.is_enabled(&ctx));
}
