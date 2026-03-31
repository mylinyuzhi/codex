use super::select_tools_for_model;
use cocode_inference::LanguageModelTool;
use cocode_protocol::ApplyPatchToolType;
use cocode_protocol::ModelInfo;
use cocode_protocol::ToolName;
use cocode_tools::builtin::ApplyPatchTool;
use cocode_tools_api::ToolDefinition;

fn sample_defs() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(ToolName::Read.as_str(), serde_json::json!({})),
        ToolDefinition::new(ToolName::Edit.as_str(), serde_json::json!({})),
        ToolDefinition::new("apply_patch", serde_json::json!({})),
    ]
}

/// Helper to unwrap a LanguageModelTool::Function variant.
fn unwrap_function(tool: &LanguageModelTool) -> &ToolDefinition {
    match tool {
        LanguageModelTool::Function(f) => f,
        other => panic!("expected Function variant, got {other:?}"),
    }
}

#[test]
fn function_variant_replaces_registry_default() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Function),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    let ap = result.iter().find(|d| d.name() == "apply_patch").unwrap();
    let ap = unwrap_function(ap);
    assert_eq!(ap.input_schema["type"], "object");
    assert!(ap.input_schema["properties"]["input"].is_object());
}

#[test]
fn freeform_variant_uses_custom_tool() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    let ap = result.iter().find(|d| d.name() == "apply_patch").unwrap();
    let ap = unwrap_function(ap);
    assert!(ap.provider_options.is_some());
    let opts = ap.provider_options.as_ref().unwrap();
    let openai = opts.get("openai").expect("openai provider options");
    let custom_format = openai.get("custom_format").expect("custom_format key");
    assert_eq!(custom_format["type"], "grammar");
}

#[test]
fn shell_variant_excludes_apply_patch() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Shell),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    assert!(result.iter().all(|d| d.name() != "apply_patch"));
    assert_eq!(result.len(), 2); // Read, Edit
}

#[test]
fn none_excludes_apply_patch() {
    let model_info = ModelInfo {
        apply_patch_tool_type: None,
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    assert!(result.iter().all(|d| d.name() != "apply_patch"));
    assert_eq!(result.len(), 2);
}

#[test]
fn experimental_supported_tools_whitelist() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Function),
        experimental_supported_tools: Some(vec![
            ToolName::Read.as_str().to_string(),
            "apply_patch".to_string(),
        ]),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|d| d.name() == ToolName::Read.as_str()));
    assert!(result.iter().any(|d| d.name() == "apply_patch"));
    assert!(result.iter().all(|d| d.name() != ToolName::Edit.as_str()));
}

#[test]
fn empty_supported_tools_does_not_filter() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Function),
        experimental_supported_tools: Some(vec![]),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    // Empty whitelist = no filtering
    assert_eq!(result.len(), 3);
}

#[test]
fn excluded_tools_removes_named() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Function),
        excluded_tools: Some(vec![ToolName::Edit.as_str().to_string()]),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|d| d.name() == ToolName::Read.as_str()));
    assert!(result.iter().any(|d| d.name() == "apply_patch"));
    assert!(result.iter().all(|d| d.name() != ToolName::Edit.as_str()));
}

#[test]
fn empty_excluded_tools_does_not_filter() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Function),
        excluded_tools: Some(vec![]),
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    assert_eq!(result.len(), 3);
}

#[test]
fn none_excluded_tools_does_not_filter() {
    let model_info = ModelInfo {
        apply_patch_tool_type: Some(ApplyPatchToolType::Function),
        excluded_tools: None,
        ..Default::default()
    };
    let result = select_tools_for_model(sample_defs(), &model_info);
    assert_eq!(result.len(), 3);
}

#[test]
fn static_definitions_match_expected() {
    let func_def = ApplyPatchTool::function_definition();
    assert_eq!(func_def.name, "apply_patch");
    assert_eq!(func_def.input_schema["type"], "object");

    let free_def = ApplyPatchTool::freeform_definition();
    assert_eq!(free_def.name, "apply_patch");
    assert!(free_def.provider_options.is_some());
    let opts = free_def.provider_options.as_ref().unwrap();
    let openai = opts.get("openai").expect("openai provider options");
    let custom_format = openai.get("custom_format").expect("custom_format key");
    assert_eq!(custom_format["type"], "grammar");
}
