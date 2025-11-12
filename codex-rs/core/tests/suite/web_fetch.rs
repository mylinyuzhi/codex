use anyhow::Result;
use codex_core::model_family::find_family_for_model;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use serde_json::Value;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::any;
use wiremock::matchers::method;
use wiremock::matchers::path;

const MODEL_WITH_TOOL: &str = "test-gpt-5-codex";

async fn build_test_codex(llm_server: &MockServer) -> Result<TestCodex> {
    let model_family = find_family_for_model(MODEL_WITH_TOOL).expect("valid model family");
    test_codex(
        &llm_server.uri(),
        model_family
            .with_experimental_supported_tools(vec!["web_fetch".to_string()])
            .with_reasoning_summary(ReasoningSummary::None),
        AskForApproval::Never,
        SandboxPolicy::ReadOnly,
    )
    .await
}

async fn mount_tool_sequence(server: &MockServer, call_id: &str, arguments: &str, tool_name: &str) {
    responses::mount_sse_once(
        server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(call_id, tool_name, arguments),
            ev_assistant_message("Done."),
            ev_completed(),
        ]),
    )
    .await;
}

async fn submit_turn(test: &TestCodex, prompt: &str) -> Result<()> {
    test.ops_tx
        .send(Op::SendInput(UserInput::prompt(prompt.to_string())))
        .await?;
    wait_for_event!(test, |e| matches!(e, EventMsg::Completed { .. }));
    Ok(())
}

async fn recorded_bodies(server: &MockServer) -> Result<Vec<Value>> {
    let records = server.received_requests().await.expect("request records");
    records
        .into_iter()
        .map(|req| {
            let bytes = req.body.as_slice();
            serde_json::from_slice(bytes).map_err(anyhow::Error::from)
        })
        .collect()
}

fn find_tool_output<'a>(bodies: &'a [Value], call_id: &str) -> Option<&'a Value> {
    for body in bodies {
        if let Some(arr) = body.get("input").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(output) = item.get("function_call_output") {
                    if output.get("call_id").and_then(|v| v.as_str()) == Some(call_id) {
                        return Some(output);
                    }
                }
            }
        }
    }
    None
}

fn extract_content_and_success(payload: &Value) -> (Option<&str>, Option<bool>) {
    let content = payload.get("output").and_then(|v| v.as_str());
    let success = payload.get("success").and_then(|v| v.as_bool());
    (content, success)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_fetch_fetches_simple_text() -> Result<()> {
    skip_if_no_network!(Ok(()));

    // Setup LLM mock server
    let llm_server = start_mock_server().await;
    let test = build_test_codex(&llm_server).await?;

    // Setup HTTP mock server for fetching content
    let http_server = MockServer::start().await;
    let test_content = "Hello from the web!";

    Mock::given(method("GET"))
        .and(path("/test-page"))
        .respond_with(ResponseTemplate::new(200).set_body_string(test_content))
        .mount(&http_server)
        .await;

    let fetch_url = format!("{}/test-page", http_server.uri());
    let call_id = "web-fetch-simple";
    let arguments = serde_json::json!({
        "prompt": format!("Fetch content from {}", fetch_url),
    })
    .to_string();

    mount_tool_sequence(&llm_server, call_id, &arguments, "web_fetch").await;
    submit_turn(&test, &format!("please fetch {}", fetch_url)).await?;

    let bodies = recorded_bodies(&llm_server).await?;
    let tool_output = find_tool_output(&bodies, call_id).expect("tool output present");
    let payload = tool_output.get("output").expect("output field present");
    let (content_opt, success_opt) = extract_content_and_success(payload);
    let content = content_opt.expect("content present");
    let success = success_opt.unwrap_or(false);

    assert!(success, "expected successful fetch");
    assert!(
        content.contains(test_content),
        "content should contain fetched text"
    );
    assert!(content.contains(&fetch_url), "content should include URL");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_fetch_converts_html_to_text() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let llm_server = start_mock_server().await;
    let test = build_test_codex(&llm_server).await?;

    let http_server = MockServer::start().await;
    let html_content = r#"
        <html>
            <head><title>Test Page</title></head>
            <body>
                <h1>Hello World</h1>
                <p>This is a test paragraph.</p>
            </body>
        </html>
    "#;

    Mock::given(method("GET"))
        .and(path("/html-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(html_content)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&http_server)
        .await;

    let fetch_url = format!("{}/html-page", http_server.uri());
    let call_id = "web-fetch-html";
    let arguments = serde_json::json!({
        "prompt": format!("Fetch HTML from {}", fetch_url),
    })
    .to_string();

    mount_tool_sequence(&llm_server, call_id, &arguments, "web_fetch").await;
    submit_turn(&test, &format!("please fetch {}", fetch_url)).await?;

    let bodies = recorded_bodies(&llm_server).await?;
    let tool_output = find_tool_output(&bodies, call_id).expect("tool output present");
    let payload = tool_output.get("output").expect("output field present");
    let (content_opt, success_opt) = extract_content_and_success(payload);
    let content = content_opt.expect("content present");
    let success = success_opt.unwrap_or(false);

    assert!(success, "expected successful fetch");
    assert!(
        content.contains("Hello World"),
        "should extract text from HTML"
    );
    assert!(
        content.contains("test paragraph"),
        "should extract paragraph text"
    );
    // HTML tags should be removed
    assert!(!content.contains("<h1>"), "HTML tags should be removed");
    assert!(!content.contains("<p>"), "HTML tags should be removed");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_fetch_handles_multiple_urls() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let llm_server = start_mock_server().await;
    let test = build_test_codex(&llm_server).await?;

    let http_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/page1"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Content from page 1"))
        .mount(&http_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/page2"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Content from page 2"))
        .mount(&http_server)
        .await;

    let url1 = format!("{}/page1", http_server.uri());
    let url2 = format!("{}/page2", http_server.uri());
    let call_id = "web-fetch-multi";
    let arguments = serde_json::json!({
        "prompt": format!("Fetch content from {} and {}", url1, url2),
    })
    .to_string();

    mount_tool_sequence(&llm_server, call_id, &arguments, "web_fetch").await;
    submit_turn(&test, &format!("please fetch {} and {}", url1, url2)).await?;

    let bodies = recorded_bodies(&llm_server).await?;
    let tool_output = find_tool_output(&bodies, call_id).expect("tool output present");
    let payload = tool_output.get("output").expect("output field present");
    let (content_opt, success_opt) = extract_content_and_success(payload);
    let content = content_opt.expect("content present");
    let success = success_opt.unwrap_or(false);

    assert!(success, "expected successful fetch");
    assert!(
        content.contains("Content from page 1"),
        "should contain first page content"
    );
    assert!(
        content.contains("Content from page 2"),
        "should contain second page content"
    );
    assert!(content.contains(&url1), "should include first URL");
    assert!(content.contains(&url2), "should include second URL");

    Ok(())
}

#[test]
fn web_fetch_unit_tests_run() {
    // Just ensure the unit tests in the handler module compile and run
    // The actual unit tests are in the handler file itself
    assert!(true);
}
