#![cfg(not(target_os = "windows"))]

use codex_core::protocol::{AskForApproval, EventMsg, Op, SandboxPolicy};
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::matchers::any;

/// Test that incremental input only includes FunctionCallOutput, not FunctionCall/Reasoning.
/// This verifies the fix for the bug where model outputs were redundantly sent to the server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incremental_input_excludes_model_outputs() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Setup workspace
    std::fs::write(cwd.path().join("test.txt"), "file content")?;

    // Turn 1: Model returns FunctionCall + Reasoning
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_reasoning_item("rs-1", &[], &["Need to read the file"]),
            responses::ev_function_call("call_1", "read_file", &json!({"path": "test.txt"}).to_string()),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "read test.txt".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model.clone(),
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    wait_for_event(&codex, |e| matches!(e, EventMsg::TaskComplete(_))).await;

    // Verify first request was sent
    let request1 = mock1.single_request();
    assert!(request1.body_json().get("input").is_some());

    // Turn 2: Send another message to trigger incremental input
    let mock2 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-1", "Done"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "summarize it".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    wait_for_event(&codex, |e| matches!(e, EventMsg::TaskComplete(_))).await;

    // Verify second request uses incremental input
    let request2 = mock2.single_request();
    let body = request2.body_json();

    // Should have previous_response_id
    assert_eq!(
        body.get("previous_response_id").and_then(|v| v.as_str()),
        Some("resp-1")
    );

    // Get input items
    let input = body.get("input").unwrap().as_array().unwrap();

    // CRITICAL ASSERTION: Input should only contain new user inputs, not model outputs
    // Before fix: input would have 3+ items (FunctionCall, Reasoning, FunctionCallOutput, UserMessage)
    // After fix: input should have 2 items (FunctionCallOutput, UserMessage)

    // Count items by type
    let mut function_call_count = 0;
    let mut function_call_output_count = 0;
    let mut reasoning_count = 0;
    let mut user_message_count = 0;

    for item in input {
        match item.get("type").and_then(|v| v.as_str()) {
            Some("function_call") => function_call_count += 1,
            Some("function_call_output") => function_call_output_count += 1,
            Some("reasoning") => reasoning_count += 1,
            Some("message") if item.get("role").and_then(|v| v.as_str()) == Some("user") => {
                user_message_count += 1
            }
            _ => {}
        }
    }

    // Assertions:
    // 1. Should NOT include FunctionCall (model output already on server)
    assert_eq!(
        function_call_count, 0,
        "FunctionCall should NOT be in incremental input (it's a model output)"
    );

    // 2. Should NOT include Reasoning (model output already on server)
    assert_eq!(
        reasoning_count, 0,
        "Reasoning should NOT be in incremental input (it's a model output)"
    );

    // 3. SHOULD include FunctionCallOutput (actual new user input)
    assert_eq!(
        function_call_output_count, 1,
        "FunctionCallOutput SHOULD be in incremental input (it's new user input)"
    );

    // 4. SHOULD include new UserMessage
    assert_eq!(
        user_message_count, 1,
        "New user message SHOULD be in incremental input"
    );

    // Total should be 2 items (FunctionCallOutput + UserMessage)
    assert_eq!(
        input.len(),
        2,
        "Incremental input should only have 2 items (tool output + user message), not model outputs"
    );

    Ok(())
}
