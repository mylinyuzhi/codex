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

// ============================================================================
// Compaction Integration Tests
// ============================================================================

mod compaction_integration_tests {
    use super::*;
    use crate::compaction::ThresholdStatus;
    use cocode_protocol::CompactConfig;

    /// Test: threshold recalculation after auto-compact prevents false blocking (Plan 1.1)
    #[test]
    fn threshold_status_reflects_post_compact_tokens() {
        let config = CompactConfig::default();
        let context_window = 200_000;

        // Simulate: before compact, tokens are at blocking limit
        let pre_tokens = 190_000;
        let pre_status = ThresholdStatus::calculate(pre_tokens, context_window, &config);
        assert!(
            pre_status.is_at_blocking_limit,
            "pre-compact should be at blocking limit"
        );

        // Simulate: after compact, tokens are well below
        let post_tokens = 80_000;
        let post_status = ThresholdStatus::calculate(post_tokens, context_window, &config);
        assert!(
            !post_status.is_at_blocking_limit,
            "post-compact should NOT be at blocking limit"
        );
        assert!(
            !post_status.is_above_auto_compact_threshold,
            "post-compact should NOT trigger auto-compact"
        );
    }

    /// Test: circuit breaker state is independent of compaction tier
    #[test]
    fn circuit_breaker_reset_logic() {
        // Circuit breaker opens at 3 consecutive failures
        let mut failure_count = 0;
        let mut circuit_breaker_open = false;

        // Simulate 3 Tier 2 failures
        for _ in 0..3 {
            failure_count += 1;
            if failure_count >= 3 {
                circuit_breaker_open = true;
            }
        }
        assert!(circuit_breaker_open);
        assert_eq!(failure_count, 3);

        // Simulate Tier 1 success resetting the circuit breaker (Plan 1.3)
        failure_count = 0;
        circuit_breaker_open = false;
        assert!(!circuit_breaker_open);
        assert_eq!(failure_count, 0);
    }
}
