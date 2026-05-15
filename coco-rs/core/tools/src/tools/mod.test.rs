use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::PermissionMode;
use coco_types::ToolName;
use std::collections::HashSet;

#[test]
fn test_register_all_tools_count() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);
    // 43 = 42 baseline + ApplyPatchTool (gated to gpt-5 family via
    // ToolOverrides; registered universally so the layer-2 filter
    // can surface it when the model declares it as extra).
    assert_eq!(registry.len(), 43, "expected 43 tools registered");
}

#[test]
fn test_register_core_tools_count() {
    let registry = ToolRegistry::new();
    crate::register_core_tools(&registry);
    assert_eq!(registry.len(), 6, "expected 6 core tools");
}

#[test]
fn test_all_tools_have_unique_names() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);

    let names: Vec<String> = registry
        .all()
        .into_iter()
        .map(|t| t.name().to_string())
        .collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(names.len(), unique.len(), "duplicate tool names found");
}

#[test]
fn test_lookup_by_name() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);

    // Verify key tools can be found
    for name in [
        "Bash",
        "Read",
        "Write",
        "Edit",
        "Glob",
        "Grep",
        "Agent",
        "WebFetch",
        "LSP",
        "Config",
        "TaskCreate",
        "EnterPlanMode",
        "VerifyPlanExecution",
    ] {
        assert!(
            registry.get_by_name(name).is_some(),
            "tool {name} not found"
        );
    }
}

#[test]
fn test_loaded_tool_list_includes_verify_plan_execution() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);

    let visible: HashSet<String> = registry
        .loaded_tools(&ToolUseContext::test_default())
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect();

    assert!(
        visible.contains(ToolName::VerifyPlanExecution.as_str()),
        "VerifyPlanExecution must be present in the current model tool list"
    );
}

#[test]
fn test_plan_mode_tool_list_includes_verify_plan_execution() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);
    let mut ctx = ToolUseContext::test_default();
    ctx.permission_context.mode = PermissionMode::Plan;

    let visible: HashSet<String> = registry
        .loaded_tools(&ctx)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect();

    assert!(
        visible.contains(ToolName::VerifyPlanExecution.as_str()),
        "VerifyPlanExecution is read-only and must stay visible in Plan mode"
    );
}
