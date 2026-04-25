use coco_tool_runtime::ToolRegistry;

#[test]
fn test_register_all_tools_count() {
    let mut registry = ToolRegistry::new();
    crate::register_all_tools(&mut registry);
    assert_eq!(registry.len(), 41, "expected 41 tools registered");
}

#[test]
fn test_register_core_tools_count() {
    let mut registry = ToolRegistry::new();
    crate::register_core_tools(&mut registry);
    assert_eq!(registry.len(), 6, "expected 6 core tools");
}

#[test]
fn test_all_tools_have_unique_names() {
    let mut registry = ToolRegistry::new();
    crate::register_all_tools(&mut registry);

    let names: Vec<String> = registry.all().map(|t| t.name().to_string()).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(names.len(), unique.len(), "duplicate tool names found");
}

#[test]
fn test_lookup_by_name() {
    let mut registry = ToolRegistry::new();
    crate::register_all_tools(&mut registry);

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
