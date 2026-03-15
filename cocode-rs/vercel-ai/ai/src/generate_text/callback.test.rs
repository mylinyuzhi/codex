//! Tests for callback.rs

use super::*;

#[test]
fn test_on_start_event() {
    let event = OnStartEvent::new("gpt-4");
    assert_eq!(event.model_id, "gpt-4");
}

#[test]
fn test_on_finish_event() {
    let event = OnFinishEvent::new(FinishReason::stop(), Usage::default(), "Hello".to_string());
    assert_eq!(event.finish_reason, FinishReason::stop());
    assert_eq!(event.output, "Hello");
}

#[test]
fn test_on_step_start_event() {
    let event = OnStepStartEvent::new(0);
    assert_eq!(event.step, 0);
    assert!(event.tool_call.is_none());
}

#[test]
fn test_callbacks_builder() {
    let callbacks = GenerateTextCallbacks::new()
        .with_on_start(|e| println!("Started: {}", e.model_id))
        .with_on_finish(|_e| println!("Finished"));

    assert!(callbacks.on_start.is_some());
    assert!(callbacks.on_finish.is_some());
}
