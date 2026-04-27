use super::*;
use crate::types::ClearToolInputs;
use crate::types::ContextEditStrategy;

#[test]
fn test_no_options_returns_empty() {
    let opts = ApiContextOptions::default();
    let strategies = get_api_context_management(&opts);
    assert!(strategies.is_empty());
}

#[test]
fn test_thinking_only_when_enabled_and_not_redacted() {
    let opts = ApiContextOptions {
        has_thinking: true,
        is_redact_thinking_active: false,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    assert_eq!(strategies.len(), 1);
    assert!(matches!(
        strategies[0],
        ContextEditStrategy::ClearThinking { .. }
    ));
}

#[test]
fn test_thinking_skipped_when_redacted() {
    let opts = ApiContextOptions {
        has_thinking: true,
        is_redact_thinking_active: true,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    assert!(strategies.is_empty());
}

#[test]
fn test_clear_all_thinking_keeps_one_turn() {
    let opts = ApiContextOptions {
        has_thinking: true,
        clear_all_thinking: true,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    let ContextEditStrategy::ClearThinking { keep } = &strategies[0] else {
        panic!("expected ClearThinking");
    };
    assert!(matches!(
        keep,
        crate::types::ThinkingKeep::Recent { turns: 1 }
    ));
}

#[test]
fn test_thinking_default_is_keep_all() {
    let opts = ApiContextOptions {
        has_thinking: true,
        clear_all_thinking: false,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    let ContextEditStrategy::ClearThinking { keep } = &strategies[0] else {
        panic!("expected ClearThinking");
    };
    assert!(matches!(keep, crate::types::ThinkingKeep::All));
}

#[test]
fn test_clear_tool_results_emits_strategy() {
    let opts = ApiContextOptions {
        clear_tool_results: true,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    assert_eq!(strategies.len(), 1);
    let ContextEditStrategy::ClearToolUses {
        trigger,
        clear_inputs,
        exclude_tools,
        ..
    } = &strategies[0]
    else {
        panic!("expected ClearToolUses");
    };
    assert_eq!(*trigger, Some(DEFAULT_API_MAX_INPUT_TOKENS));
    assert!(matches!(clear_inputs, ClearToolInputs::SpecificTools(_)));
    assert!(exclude_tools.is_empty());
}

#[test]
fn test_clear_tool_uses_excludes_write_edit() {
    let opts = ApiContextOptions {
        clear_tool_uses: true,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    assert_eq!(strategies.len(), 1);
    let ContextEditStrategy::ClearToolUses {
        clear_inputs,
        exclude_tools,
        ..
    } = &strategies[0]
    else {
        panic!("expected ClearToolUses");
    };
    assert!(matches!(clear_inputs, ClearToolInputs::None));
    // Edit, Write, NotebookEdit must be excluded — checked via typed enum.
    assert!(exclude_tools.contains(&coco_types::ToolName::Edit));
    assert!(exclude_tools.contains(&coco_types::ToolName::Write));
    assert!(exclude_tools.contains(&coco_types::ToolName::NotebookEdit));
}

#[test]
fn test_combined_strategies_ordering() {
    let opts = ApiContextOptions {
        has_thinking: true,
        clear_tool_results: true,
        clear_tool_uses: true,
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    assert_eq!(strategies.len(), 3);
    // Ordering: thinking first, then results, then uses.
    assert!(matches!(
        strategies[0],
        ContextEditStrategy::ClearThinking { .. }
    ));
    assert!(matches!(
        strategies[1],
        ContextEditStrategy::ClearToolUses {
            clear_inputs: ClearToolInputs::SpecificTools(_),
            ..
        }
    ));
    assert!(matches!(
        strategies[2],
        ContextEditStrategy::ClearToolUses {
            clear_inputs: ClearToolInputs::None,
            ..
        }
    ));
}

#[test]
fn test_custom_thresholds_passed_through() {
    let opts = ApiContextOptions {
        clear_tool_results: true,
        trigger_threshold: Some(150_000),
        keep_target: Some(30_000),
        ..Default::default()
    };
    let strategies = get_api_context_management(&opts);
    let ContextEditStrategy::ClearToolUses { trigger, .. } = &strategies[0] else {
        panic!("expected ClearToolUses");
    };
    assert_eq!(*trigger, Some(150_000));
}
