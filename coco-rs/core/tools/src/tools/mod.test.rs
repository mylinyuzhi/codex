use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::PermissionMode;
use coco_types::ToolName;
use std::collections::HashSet;

#[test]
fn test_register_all_tools_count() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);
    // 42 = 41 baseline + ApplyPatchTool (gated to gpt-5 family via
    // ToolOverrides; registered universally so the layer-2 filter
    // can surface it when the model declares it as extra).
    // `StructuredOutputTool` is intentionally **not** in the baseline:
    // it's conditionally injected via `register_structured_output_tool`
    // only when the non-interactive bootstrap parses `--json-schema`
    // (TS parity: `tools.ts` `specialTools` excludes it).
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

/// Force-initialize every registered tool's runtime validation schema. The
/// schemas are `OnceLock`-lazy, so registering a tool does NOT compile them —
/// only calling `runtime_validation_schema()` does. This is the gate the schema
/// constructors rely on: a malformed Bucket-A (`from_input_type`) or hand-built
/// (`from_static_value`) schema panics HERE in CI, not on first production use.
#[test]
fn test_all_tool_schemas_force_initialize() {
    let all = ToolRegistry::new();
    crate::register_all_tools(&all);
    let core = ToolRegistry::new();
    crate::register_core_tools(&core);
    for registry in [&all, &core] {
        for tool in registry.all() {
            assert!(
                tool.runtime_validation_schema().as_value().is_object(),
                "{} runtime schema must compile to a root object",
                tool.name()
            );
        }
    }
}

/// `tool_spec()` is the single source of truth for a tool's model-facing wire
/// shape — `engine_prompt` builds the wire `description` from it. This guards
/// the gap where a tool ships with an *empty* description (Function via the
/// default `prompt()` path, or a hand-built Freeform spec).
#[tokio::test]
async fn test_all_registered_tools_have_nonempty_spec_description() {
    let registry = ToolRegistry::new();
    crate::register_all_tools(&registry);
    let prompt_opts = coco_tool_runtime::PromptOptions::default();
    let schema_ctx = coco_tool_runtime::SchemaContext::default();
    for tool in registry.all() {
        let spec = tool.tool_spec(&schema_ctx, &prompt_opts).await;
        assert!(
            !spec.description().trim().is_empty(),
            "tool `{}` has an empty model-facing tool_spec() description",
            tool.name()
        );
    }
}
