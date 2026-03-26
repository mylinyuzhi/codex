use super::*;
use cocode_inference::AssistantContentPart;
use cocode_inference::CollectedResponse;
use cocode_inference::FinishReason;
use cocode_inference::ToolResultContent;

fn make_response() -> CollectedResponse {
    CollectedResponse {
        content: vec![AssistantContentPart::text("Hello!")],
        usage: None,
        finish_reason: FinishReason::stop(),
    }
}

#[test]
fn test_create_user_message() {
    let msg = create_user_message("Hello", "turn-1");
    assert_eq!(msg.text(), "Hello");
    assert_eq!(msg.turn_id, "turn-1");
}

#[test]
fn test_create_assistant_message() {
    let response = make_response();
    let msg = create_assistant_message(&response, "turn-1");
    assert_eq!(msg.text(), "Hello!");
}

#[test]
fn test_create_tool_result() {
    let msg = create_tool_result_message("call-1", "Success!", "turn-1");
    assert!(matches!(
        msg.source,
        MessageSource::Tool {
            call_id: ref id
        } if id == "call-1"
    ));
}

#[test]
fn test_create_tool_error() {
    let msg = create_tool_error_message("call-1", "Something went wrong", "turn-1");
    assert!(matches!(msg.source, MessageSource::Tool { .. }));
}

#[test]
fn test_create_compaction_summary() {
    let msg = create_compaction_summary("Previous conversation summary", "turn-1");
    assert!(msg.text().contains("compaction_summary"));
    assert!(matches!(msg.source, MessageSource::CompactionSummary));
}

#[test]
fn test_message_builder() {
    let builder = MessageBuilder::for_turn("turn-1");

    let user_msg = builder.user("Hello");
    assert_eq!(user_msg.turn_id, "turn-1");

    let system_msg = builder.system("You are helpful");
    assert_eq!(system_msg.turn_id, "turn-1");

    let tool_msg = builder.tool_result("call-1", "Done");
    assert_eq!(tool_msg.turn_id, "turn-1");
}

#[test]
fn test_batch_tool_results() {
    let results = vec![
        (
            "call-1".to_string(),
            ToolResultContent::text("Result 1"),
            false,
        ),
        (
            "call-2".to_string(),
            ToolResultContent::text("Error!"),
            true,
        ),
    ];

    let messages = create_tool_results_batch(results, "turn-1");
    assert_eq!(messages.len(), 2);
}
