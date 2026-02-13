use super::*;

#[test]
fn test_input_content_text() {
    let block = InputContentBlock::text("Hello");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"input_text""#));
    assert!(json.contains(r#""text":"Hello""#));
}

#[test]
fn test_input_content_image_base64() {
    let block = InputContentBlock::image_base64("data123", ImageMediaType::Png);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"input_image""#));
    assert!(json.contains(r#""data":"data123""#));
    assert!(json.contains(r#""media_type":"image/png""#));
}

#[test]
fn test_input_content_image_url() {
    let block = InputContentBlock::image_url("https://example.com/image.png");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"input_image""#));
    assert!(json.contains(r#""url":"https://example.com/image.png""#));
}

#[test]
fn test_input_content_image_url_with_detail() {
    let block = InputContentBlock::image_url_with_detail(
        "https://example.com/image.png",
        ImageDetail::High,
    );
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""detail":"high""#));
}

#[test]
fn test_input_content_function_output() {
    let block = InputContentBlock::function_call_output("call-1", r#"{"result": 42}"#, None);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"function_call_output""#));
    assert!(json.contains(r#""call_id":"call-1""#));
}

#[test]
fn test_output_content_block_helpers() {
    let text = OutputContentBlock::OutputText {
        text: "Hello".to_string(),
        annotations: vec![],
        logprobs: None,
    };
    assert_eq!(text.as_text(), Some("Hello"));
    assert!(text.as_refusal().is_none());

    let refusal = OutputContentBlock::Refusal {
        refusal: "Cannot do that".to_string(),
    };
    assert!(refusal.as_text().is_none());
    assert_eq!(refusal.as_refusal(), Some("Cannot do that"));
}

#[test]
fn test_input_audio() {
    let block = InputContentBlock::audio("base64data", AudioFormat::Mp3);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"input_audio""#));
    assert!(json.contains(r#""data":"base64data""#));
    assert!(json.contains(r#""format":"mp3""#));
}

#[test]
fn test_input_file() {
    let block = InputContentBlock::file("file-123");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"input_file""#));
    assert!(json.contains(r#""file_id":"file-123""#));

    let block = InputContentBlock::file_with_name("file-123", "document.pdf");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""filename":"document.pdf""#));
}

#[test]
fn test_computer_call_output() {
    let block =
        InputContentBlock::computer_call_output_screenshot("call-1", Some("base64".into()), None);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"computer_call_output""#));
    assert!(json.contains(r#""call_id":"call-1""#));

    let block = InputContentBlock::computer_call_output_action("call-2", Some("success".into()));
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"computer_call_output""#));
}

#[test]
fn test_tool_call_outputs() {
    let block = InputContentBlock::file_search_call_output("call-1", Some("results".into()));
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"file_search_call_output""#));

    let block = InputContentBlock::web_search_call_output("call-2", Some("results".into()));
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"web_search_call_output""#));

    let block = InputContentBlock::code_interpreter_call_output("call-3", Some("output".into()));
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"code_interpreter_call_output""#));

    let block = InputContentBlock::local_shell_call_output("call-4", Some("output".into()));
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"local_shell_call_output""#));

    let block = InputContentBlock::mcp_call_output("call-5", Some("output".into()), None);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"mcp_call_output""#));

    let block = InputContentBlock::apply_patch_call_output("call-6", Some("patched".into()));
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"apply_patch_call_output""#));
}

#[test]
fn test_item_reference() {
    let block = InputContentBlock::item_reference("item-123");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"item_reference""#));
    assert!(json.contains(r#""id":"item-123""#));
}

#[test]
fn test_logprobs() {
    let logprobs = Logprobs {
        content: vec![
            LogprobContent {
                token: "Hello".to_string(),
                logprob: -0.5,
                bytes: None,
                top_logprobs: vec![TopLogprob {
                    token: "Hi".to_string(),
                    logprob: -1.0,
                    bytes: None,
                }],
            },
            LogprobContent {
                token: " world".to_string(),
                logprob: -0.3,
                bytes: None,
                top_logprobs: vec![],
            },
        ],
    };

    assert_eq!(logprobs.tokens(), vec!["Hello", " world"]);
    assert!((logprobs.total_logprob() - (-0.8)).abs() < 0.001);

    let json = serde_json::to_string(&logprobs).unwrap();
    assert!(json.contains(r#""token":"Hello""#));
    assert!(json.contains(r#""logprob":-0.5"#));
}

#[test]
fn test_custom_tool_call_output_serialization() {
    let block = InputContentBlock::custom_tool_call_output("call-custom-1", "patch applied");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"custom_tool_call_output""#));
    assert!(json.contains(r#""call_id":"call-custom-1""#));
    assert!(json.contains(r#""output":"patch applied""#));
    assert!(!json.contains(r#""id""#)); // id is None, should be skipped

    // Roundtrip
    let parsed: InputContentBlock = serde_json::from_str(&json).unwrap();
    if let InputContentBlock::CustomToolCallOutput {
        call_id,
        output,
        id,
    } = parsed
    {
        assert_eq!(call_id, "call-custom-1");
        assert_eq!(output, "patch applied");
        assert!(id.is_none());
    } else {
        panic!("Expected CustomToolCallOutput variant");
    }
}

#[test]
fn test_output_text_with_logprobs() {
    let block = OutputContentBlock::OutputText {
        text: "Hello".to_string(),
        annotations: vec![],
        logprobs: Some(Logprobs {
            content: vec![LogprobContent {
                token: "Hello".to_string(),
                logprob: -0.5,
                bytes: None,
                top_logprobs: vec![],
            }],
        }),
    };
    assert!(block.as_logprobs().is_some());
    assert_eq!(block.as_logprobs().unwrap().tokens(), vec!["Hello"]);
}
