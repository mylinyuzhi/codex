use super::*;

#[test]
fn test_stop_reason_variants() {
    let reasons = vec![
        StopReason::MaxTurnsReached,
        StopReason::ModelStopSignal,
        StopReason::UserInterrupted,
        StopReason::Error {
            message: "timeout".to_string(),
        },
        StopReason::PlanModeExit { approved: true },
        StopReason::PlanModeExit { approved: false },
        StopReason::HookStopped,
    ];
    // Verify all variants can be cloned and debug-printed.
    for reason in &reasons {
        let _cloned = reason.clone();
        let _debug = format!("{reason:?}");
    }
}

#[test]
fn test_loop_result_completed() {
    let result = LoopResult::completed(
        5,
        1000,
        500,
        "Hello".to_string(),
        vec![hyper_sdk::ContentBlock::text("Hello")],
    );
    assert_eq!(result.turns_completed, 5);
    assert_eq!(result.total_input_tokens, 1000);
    assert_eq!(result.total_output_tokens, 500);
    assert_eq!(result.final_text, "Hello");
    assert_eq!(result.last_response_content.len(), 1);
}

#[test]
fn test_loop_result_max_turns() {
    let result = LoopResult::max_turns_reached(10, 2000, 1000);
    assert_eq!(result.turns_completed, 10);
    assert!(result.final_text.is_empty());
    assert!(matches!(result.stop_reason, StopReason::MaxTurnsReached));
}

#[test]
fn test_loop_result_error() {
    let result = LoopResult::error(3, 500, 200, "timeout".to_string());
    match &result.stop_reason {
        StopReason::Error { message } => assert_eq!(message, "timeout"),
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn test_stop_reason_serde_roundtrip() {
    let reason = StopReason::Error {
        message: "provider unavailable".to_string(),
    };
    let json = serde_json::to_string(&reason).expect("serialize");
    let back: StopReason = serde_json::from_str(&json).expect("deserialize");
    match back {
        StopReason::Error { message } => assert_eq!(message, "provider unavailable"),
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn test_plan_mode_exit_serde() {
    let reason = StopReason::PlanModeExit { approved: true };
    let json = serde_json::to_string(&reason).expect("serialize");
    let back: StopReason = serde_json::from_str(&json).expect("deserialize");
    match back {
        StopReason::PlanModeExit { approved } => assert!(approved),
        other => panic!("unexpected variant: {other:?}"),
    }
}
