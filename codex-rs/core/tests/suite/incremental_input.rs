#![cfg(not(target_os = "windows"))]

use codex_core::protocol::AskForApproval;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
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
            responses::ev_function_call(
                "call_1",
                "read_file",
                &json!({"path": "test.txt"}).to_string(),
            ),
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

/// Test that multiple parallel tool calls have all their outputs delivered incrementally.
/// Verifies that stateless filtering correctly handles concurrent tool executions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incremental_input_with_multiple_tool_calls() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Setup workspace
    std::fs::write(cwd.path().join("file1.txt"), "content1")?;
    std::fs::write(cwd.path().join("file2.txt"), "content2")?;

    // Turn 1: Model returns TWO parallel FunctionCalls
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_function_call(
                "call_1",
                "read_file",
                &json!({"path": "file1.txt"}).to_string(),
            ),
            responses::ev_function_call(
                "call_2",
                "read_file",
                &json!({"path": "file2.txt"}).to_string(),
            ),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "read both files".into(),
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

    let request1 = mock1.single_request();
    assert!(request1.body_json().get("input").is_some());

    // Turn 2: New message to trigger incremental input
    let mock2 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-1", "Files read"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "compare them".into(),
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

    // Verify second request includes BOTH tool outputs but NO function calls
    let request2 = mock2.single_request();
    let body2 = request2.body_json();
    let input = body2.get("input").unwrap().as_array().unwrap();

    let function_call_output_count = input
        .iter()
        .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("function_call_output"))
        .count();
    let function_call_count = input
        .iter()
        .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("function_call"))
        .count();

    assert_eq!(
        function_call_output_count, 2,
        "Should have both function call outputs"
    );
    assert_eq!(
        function_call_count, 0,
        "Should NOT have function calls (model outputs)"
    );

    Ok(())
}

/// Test that the first turn (no previous response) sends full history.
/// Verifies fallback behavior when no LLM outputs exist yet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incremental_input_first_turn_sends_full_history() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Turn 1: First interaction (no previous response)
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", "Hello!"),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "hello".into(),
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

    // Verify first request does NOT use previous_response_id
    let request1 = mock1.single_request();
    let body = request1.body_json();

    assert_eq!(
        body.get("previous_response_id"),
        None,
        "First turn should NOT have previous_response_id"
    );

    // Should send full history (just the user message)
    let input = body.get("input").unwrap().as_array().unwrap();
    assert!(
        !input.is_empty(),
        "First turn should send full history (user message)"
    );

    Ok(())
}

/// Test that error recovery with PreviousResponseNotFound still works.
/// With stateless filtering, errors should be self-correcting without explicit tracking clear.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incremental_input_error_recovery() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Turn 1: Successful response
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", "First response"),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "first message".into(),
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
    let _ = mock1.single_request();

    // Turn 2: Simulated error (server returns 404 for previous_response_id)
    // First attempt: error response
    let mock2_error = responses::mount_sse_once_match(
        &server,
        any(),
        responses::error_response(
            404,
            &json!({
                "error": {
                    "message": "Previous response not found",
                    "type": "invalid_request_error"
                }
            })
            .to_string(),
        ),
    )
    .await;

    // Second attempt: success (retry with filtering)
    let mock2_retry = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-2", "Recovered"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "second message".into(),
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

    // Verify error response was received
    let _ = mock2_error.single_request();

    // Verify retry succeeded (with stateless filtering adapting automatically)
    let request2_retry = mock2_retry.single_request();
    let body = request2_retry.body_json();

    // After error, retry should still work (filtering is stateless)
    assert!(
        body.get("input").is_some(),
        "Retry should send input (stateless filtering adapts)"
    );

    Ok(())
}

/// Test incremental input with interleaved user messages (pending input).
/// Verifies that user messages submitted during tool execution are correctly appended.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incremental_input_with_pending_user_messages() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Setup workspace
    std::fs::write(cwd.path().join("test.txt"), "content")?;

    // Turn 1: Model returns FunctionCall
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_function_call(
                "call_1",
                "read_file",
                &json!({"path": "test.txt"}).to_string(),
            ),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "read the file".into(),
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

    // While tool is executing, user submits additional input
    // (In real usage this could be via Op::UserInput, but we'll test via new turn)
    wait_for_event(&codex, |e| matches!(e, EventMsg::TaskComplete(_))).await;
    let _ = mock1.single_request();

    // Turn 2: New user input appended to incremental history
    let mock2 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-1", "Understood"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![
                UserInput::Text {
                    text: "also check this".into(),
                },
                UserInput::Text {
                    text: "and summarize".into(),
                },
            ],
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

    // Verify second request includes tool output + both new user messages
    let request2 = mock2.single_request();
    let body2 = request2.body_json();
    let input = body2.get("input").unwrap().as_array().unwrap();

    // Count item types
    let function_call_output_count = input
        .iter()
        .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("function_call_output"))
        .count();

    let user_message = input
        .iter()
        .find(|item| {
            item.get("type").and_then(|v| v.as_str()) == Some("message")
                && item.get("role").and_then(|v| v.as_str()) == Some("user")
        })
        .expect("Should have user message");

    // Verify user message has both texts (merged into single message)
    let content = user_message.get("content").unwrap().as_array().unwrap();
    assert_eq!(
        content.len(),
        2,
        "User message should have both input texts"
    );

    assert_eq!(function_call_output_count, 1, "Should have tool output");

    Ok(())
}

/// Test that compact operation clears previous_response_id tracking.
/// After compact, next turn should send full compacted history (not incremental).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_compact_clears_previous_response_id() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Setup workspace
    std::fs::write(cwd.path().join("test.txt"), "file content")?;

    // Turn 1: Initial conversation
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_function_call(
                "call_1",
                "read_file",
                &json!({"path": "test.txt"}).to_string(),
            ),
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

    drop(mock1);

    // Trigger compact operation
    let mock_compact = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-compact"),
            responses::ev_assistant_message("msg-compact", "Compacted summary"),
            responses::ev_completed("resp-compact"),
        ]),
    )
    .await;

    codex.submit(Op::Compact).await?;
    wait_for_event(&codex, |e| matches!(e, EventMsg::TaskComplete(_))).await;

    drop(mock_compact);

    // Turn 2: Next turn after compact should send full history (not incremental)
    let mock2 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-2", "Response after compact"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "After compact".into(),
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

    // Verify request had NO previous_response_id after compact
    let request2 = mock2.single_request();
    let body2 = request2.body_json();

    // After compact, should NOT use incremental mode (no previous_response_id)
    assert!(
        body2.get("previous_response_id").is_none(),
        "Should NOT have previous_response_id after compact"
    );

    // Should send full history (compacted message + new user message)
    let input = body2.get("input").unwrap().as_array().unwrap();
    assert!(
        input.len() >= 2,
        "Should send full history after compact (not incremental)"
    );

    drop(mock2);

    Ok(())
}

/// Test that error recovery clears stale previous_response_id.
/// When server returns 400 previous_response_not_found, retry should send full history.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_error_recovery_clears_stale_response_id() -> anyhow::Result<()> {
    use wiremock::Mock;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Turn 1: Initial conversation (establish previous_response_id)
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", "First response"),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "Initial message".into(),
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

    drop(mock1);

    // Turn 2: First request fails with previous_response_not_found error
    let mock_error = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "code": "previous_response_not_found",
                "message": "The specified previous_response_id was not found"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Second request (retry) should succeed with full history
    let mock_retry = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-2", "Recovered response"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "Message causing error".into(),
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

    // Verify retry request had NO previous_response_id (cleared after error)
    let retry_request = mock_retry.single_request();
    let retry_body = retry_request.body_json();

    assert!(
        retry_body.get("previous_response_id").is_none(),
        "Retry should NOT have previous_response_id after error"
    );

    drop(mock_error);
    drop(mock_retry);

    Ok(())
}

/// Test that empty incremental input falls back to full history.
/// Edge case: filtering produces no items and no pending input.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_empty_incremental_falls_back_to_full_history() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;

    let test_ctx = test_codex().build(&server).await?;
    let codex = test_ctx.codex;
    let cwd = test_ctx.cwd;
    let session_model = test_ctx.session_configured.model.clone();

    // Turn 1: Get initial response (pure assistant message, no tools)
    let mock1 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", "First response"),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "Initial message".into(),
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

    drop(mock1);

    // Turn 2: Immediately send another message (no tool outputs to filter)
    // Filtering would return empty (no user inputs after last assistant message)
    // Should fallback to full history
    let mock2 = responses::mount_sse_once_match(
        &server,
        any(),
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-2", "Second response"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "Follow-up message".into(),
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

    // Verify request was sent successfully
    let request2 = mock2.single_request();
    let body2 = request2.body_json();
    let input = body2.get("input").unwrap().as_array().unwrap();

    // Should have sent full history (not empty)
    // At minimum: initial user message + assistant response + new user message
    assert!(
        input.len() >= 3,
        "Should send full history when incremental would be empty (got {} items)",
        input.len()
    );

    // Verify new user message is present
    let has_follow_up = input.iter().any(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some("message")
            && item.get("role").and_then(|v| v.as_str()) == Some("user")
            && item
                .get("content")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.get("text"))
                .and_then(|t| t.as_str())
                .map(|text| text.contains("Follow-up"))
                .unwrap_or(false)
    });

    assert!(has_follow_up, "Should include follow-up user message");

    drop(mock2);

    Ok(())
}
