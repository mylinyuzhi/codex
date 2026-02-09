use super::*;
use hyper_sdk::ContentBlock;

#[test]
fn test_turn_result_from_loop_result() {
    let loop_result = LoopResult::completed(
        3,
        1000,
        500,
        "Hello!".to_string(),
        vec![ContentBlock::text("Hello!")],
    );

    let turn = TurnResult::from_loop_result(&loop_result);
    assert_eq!(turn.final_text, "Hello!");
    assert_eq!(turn.turns_completed, 3);
    assert_eq!(turn.usage.input_tokens, 1000);
    assert_eq!(turn.usage.output_tokens, 500);
    assert!(turn.is_complete);
}

#[test]
fn test_turn_result_serde() {
    let turn = TurnResult {
        final_text: "test".to_string(),
        turns_completed: 5,
        usage: TokenUsage::new(100, 50),
        has_pending_tools: false,
        is_complete: true,
    };

    let json = serde_json::to_string(&turn).expect("serialize");
    let parsed: TurnResult = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.final_text, turn.final_text);
    assert_eq!(parsed.turns_completed, turn.turns_completed);
    assert_eq!(parsed.usage.input_tokens, turn.usage.input_tokens);
}
