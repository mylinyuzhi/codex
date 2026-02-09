use super::*;
use crate::response::FinishReason;
use crate::response::TokenUsage;

#[test]
fn test_tool_call_manager_lifecycle() {
    let mut manager = ToolCallManager::new();

    // Start a tool call
    manager.start(0, "call_1".to_string(), "get_weather".to_string());
    assert_eq!(manager.as_slice().len(), 1);
    assert_eq!(manager.as_slice()[0].name, "get_weather");

    // Add deltas
    manager.append_delta(0, "{\"city\":");
    manager.append_delta(0, "\"NYC\"}");
    assert_eq!(manager.as_slice()[0].arguments, "{\"city\":\"NYC\"}");

    // Complete
    manager.complete(0, "call_1", "get_weather", "{\"city\":\"NYC\"}".to_string());
    assert!(manager.as_slice()[0].is_complete);
}

#[test]
fn test_tool_call_manager_done_without_start() {
    let mut manager = ToolCallManager::new();

    // Directly complete without start
    manager.complete(0, "call_direct", "direct_tool", "{}".to_string());

    assert_eq!(manager.as_slice().len(), 1);
    assert_eq!(manager.as_slice()[0].name, "direct_tool");
    assert!(manager.as_slice()[0].is_complete);
}

#[test]
fn test_processor_state_text_accumulation() {
    let mut state = ProcessorState::new();

    state.update(&StreamEvent::text_delta(0, "Hello "));
    state.update(&StreamEvent::text_delta(0, "world!"));

    assert_eq!(state.snapshot.text, "Hello world!");
}

#[test]
fn test_processor_state_thinking_accumulation() {
    let mut state = ProcessorState::new();

    state.update(&StreamEvent::thinking_delta(0, "First "));
    state.update(&StreamEvent::thinking_delta(0, "thought"));
    state.update(&StreamEvent::ThinkingDone {
        index: 0,
        content: "Ignored content".to_string(),
        signature: Some("sig_123".to_string()),
    });

    let thinking = state.snapshot.thinking.as_ref().unwrap();
    assert_eq!(thinking.content, "First thought");
    assert_eq!(thinking.signature, Some("sig_123".to_string()));
    assert!(thinking.is_complete);
}

#[test]
fn test_processor_state_thinking_done_without_deltas() {
    let mut state = ProcessorState::new();

    state.update(&StreamEvent::ThinkingDone {
        index: 0,
        content: "Direct content".to_string(),
        signature: Some("sig_456".to_string()),
    });

    let thinking = state.snapshot.thinking.as_ref().unwrap();
    assert_eq!(thinking.content, "Direct content");
    assert!(thinking.is_complete);
}

#[test]
fn test_processor_state_response_done() {
    let mut state = ProcessorState::new();

    state.update(&StreamEvent::response_created("resp_1"));
    state.update(&StreamEvent::response_done_full(
        "resp_1",
        "test-model",
        Some(TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }),
        FinishReason::Stop,
    ));

    assert_eq!(state.snapshot.id, Some("resp_1".to_string()));
    assert_eq!(state.snapshot.model, "test-model");
    assert!(state.snapshot.is_complete);
    assert_eq!(state.snapshot.finish_reason, Some(FinishReason::Stop));
    assert!(state.snapshot.usage.is_some());
}

#[test]
fn test_processor_state_tool_call_sync() {
    let mut state = ProcessorState::new();

    state.update(&StreamEvent::ToolCallStart {
        index: 0,
        id: "call_1".to_string(),
        name: "test_tool".to_string(),
    });

    assert_eq!(state.snapshot.tool_calls.len(), 1);
    assert_eq!(state.snapshot.tool_calls[0].name, "test_tool");
}
