use super::*;
use crate::result::StopReason;

#[test]
fn test_default_config() {
    let config = LoopConfig::default();
    assert_eq!(config.max_turns, None);
    assert!(!config.enable_streaming_tools);
    assert!(!config.enable_micro_compaction);
}

#[test]
fn test_builder_defaults() {
    let builder = AgentLoopBuilder::new();
    assert!(builder.api_client.is_none());
    assert!(builder.tool_registry.is_none());
    assert!(builder.context.is_none());
    assert!(builder.event_tx.is_none());
}

#[test]
fn test_loop_result_constructors() {
    let completed = LoopResult::completed(5, 1000, 500, "text".to_string(), vec![]);
    assert_eq!(completed.turns_completed, 5);
    assert!(matches!(completed.stop_reason, StopReason::ModelStopSignal));

    let max = LoopResult::max_turns_reached(10, 2000, 1000);
    assert!(matches!(max.stop_reason, StopReason::MaxTurnsReached));

    let interrupted = LoopResult::interrupted(3, 500, 200);
    assert!(matches!(
        interrupted.stop_reason,
        StopReason::UserInterrupted
    ));

    let err = LoopResult::error(1, 100, 50, "boom".to_string());
    assert!(matches!(err.stop_reason, StopReason::Error { .. }));
}

#[test]
fn test_constants() {
    assert_eq!(cocode_protocol::DEFAULT_MIN_BLOCKING_OFFSET, 13_000);
    assert_eq!(MAX_OUTPUT_TOKEN_RECOVERY, 3);
}

#[test]
fn test_micro_compact_empty_history() {
    // Cannot construct a full AgentLoop without a model, but we can test
    // the candidate finder directly.
    let messages: Vec<serde_json::Value> = vec![];
    let candidates = crate::compaction::micro_compact_candidates(&messages);
    assert!(candidates.is_empty());
}

mod select_tools_for_model_tests {
    use super::*;
    use cocode_protocol::ApplyPatchToolType;
    use cocode_protocol::ModelInfo;
    use cocode_tools::builtin::ApplyPatchTool;

    fn sample_defs() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition::new("Read", serde_json::json!({})),
            ToolDefinition::new("Edit", serde_json::json!({})),
            ToolDefinition::new("apply_patch", serde_json::json!({})),
        ]
    }

    #[test]
    fn function_variant_replaces_registry_default() {
        let model_info = ModelInfo {
            apply_patch_tool_type: Some(ApplyPatchToolType::Function),
            ..Default::default()
        };
        let result = select_tools_for_model(sample_defs(), &model_info);
        let ap = result.iter().find(|d| d.name == "apply_patch").unwrap();
        assert_eq!(ap.parameters["type"], "object");
        assert!(ap.parameters["properties"]["input"].is_object());
    }

    #[test]
    fn freeform_variant_uses_custom_tool() {
        let model_info = ModelInfo {
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            ..Default::default()
        };
        let result = select_tools_for_model(sample_defs(), &model_info);
        let ap = result.iter().find(|d| d.name == "apply_patch").unwrap();
        assert!(ap.custom_format.is_some());
        assert_eq!(ap.custom_format.as_ref().unwrap()["type"], "grammar");
    }

    #[test]
    fn shell_variant_excludes_apply_patch() {
        let model_info = ModelInfo {
            apply_patch_tool_type: Some(ApplyPatchToolType::Shell),
            ..Default::default()
        };
        let result = select_tools_for_model(sample_defs(), &model_info);
        assert!(result.iter().all(|d| d.name != "apply_patch"));
        assert_eq!(result.len(), 2); // Read, Edit
    }

    #[test]
    fn none_excludes_apply_patch() {
        let model_info = ModelInfo {
            apply_patch_tool_type: None,
            ..Default::default()
        };
        let result = select_tools_for_model(sample_defs(), &model_info);
        assert!(result.iter().all(|d| d.name != "apply_patch"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn experimental_supported_tools_whitelist() {
        let model_info = ModelInfo {
            apply_patch_tool_type: Some(ApplyPatchToolType::Function),
            experimental_supported_tools: Some(vec!["Read".to_string(), "apply_patch".to_string()]),
            ..Default::default()
        };
        let result = select_tools_for_model(sample_defs(), &model_info);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|d| d.name == "Read"));
        assert!(result.iter().any(|d| d.name == "apply_patch"));
        assert!(result.iter().all(|d| d.name != "Edit"));
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
            excluded_tools: Some(vec!["Edit".to_string()]),
            ..Default::default()
        };
        let result = select_tools_for_model(sample_defs(), &model_info);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|d| d.name == "Read"));
        assert!(result.iter().any(|d| d.name == "apply_patch"));
        assert!(result.iter().all(|d| d.name != "Edit"));
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
        assert_eq!(func_def.parameters["type"], "object");

        let free_def = ApplyPatchTool::freeform_definition();
        assert_eq!(free_def.name, "apply_patch");
        assert!(free_def.custom_format.is_some());
        assert_eq!(free_def.custom_format.as_ref().unwrap()["type"], "grammar");
    }
}
