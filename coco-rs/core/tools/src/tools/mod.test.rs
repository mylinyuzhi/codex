use coco_tool_runtime::ToolRegistry;

#[test]
fn test_register_all_tools_count() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);
    // 42 = 41 baseline + ApplyPatchTool (gated to gpt-5 family via
    // ToolOverrides; registered universally so the layer-2 filter
    // can surface it when the model declares it as extra).
    assert_eq!(registry.len(), 42, "expected 42 tools registered");
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
    ] {
        assert!(
            registry.get_by_name(name).is_some(),
            "tool {name} not found"
        );
    }
}
