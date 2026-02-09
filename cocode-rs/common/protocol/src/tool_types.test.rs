use super::*;

#[test]
fn test_concurrency_safety_default() {
    assert_eq!(ConcurrencySafety::default(), ConcurrencySafety::Safe);
    assert!(ConcurrencySafety::Safe.is_safe());
    assert!(!ConcurrencySafety::Unsafe.is_safe());
}

#[test]
fn test_tool_output_constructors() {
    let text = ToolOutput::text("Hello");
    assert!(!text.is_error);
    assert!(text.modifiers.is_empty());

    let error = ToolOutput::error("Something went wrong");
    assert!(error.is_error);

    let structured = ToolOutput::structured(serde_json::json!({"key": "value"}));
    assert!(!structured.is_error);
}

#[test]
fn test_tool_output_with_modifiers() {
    let output = ToolOutput::text("Read file")
        .with_modifier(ContextModifier::FileRead {
            path: PathBuf::from("/tmp/test.txt"),
            content: "file content".to_string(),
        })
        .with_modifier(ContextModifier::PermissionGranted {
            tool: "Read".to_string(),
            pattern: "/tmp/*".to_string(),
        });

    assert_eq!(output.modifiers.len(), 2);
}

#[test]
fn test_validation_result() {
    assert!(ValidationResult::valid().is_valid());
    assert!(!ValidationResult::error("invalid").is_valid());

    let result = ValidationResult::invalid([
        ValidationError::new("field required"),
        ValidationError::with_path("must be positive", "count"),
    ]);

    if let ValidationResult::Invalid { errors } = result {
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[1].path.as_deref(), Some("count"));
    } else {
        panic!("Expected Invalid result");
    }
}

#[test]
fn test_validation_error_display() {
    let error = ValidationError::new("something wrong");
    assert_eq!(format!("{error}"), "something wrong");

    let error_with_path = ValidationError::with_path("must be positive", "count");
    assert_eq!(format!("{error_with_path}"), "count: must be positive");
}

#[test]
fn test_serde_roundtrip() {
    let output = ToolOutput::text("test").with_modifier(ContextModifier::FileRead {
        path: PathBuf::from("/test"),
        content: "content".to_string(),
    });

    let json = serde_json::to_string(&output).unwrap();
    let parsed: ToolOutput = serde_json::from_str(&json).unwrap();
    assert!(!parsed.is_error);
    assert_eq!(parsed.modifiers.len(), 1);
}

#[test]
fn test_truncate_to_no_op_when_within_limit() {
    let mut output = ToolOutput::text("short text");
    output.truncate_to(100);
    assert!(matches!(&output.content, ToolResultContent::Text(s) if s == "short text"));
}

#[test]
fn test_truncate_to_truncates_long_text() {
    let long = "a".repeat(1000);
    let mut output = ToolOutput::text(long);
    output.truncate_to(100);
    if let ToolResultContent::Text(ref s) = output.content {
        assert!(s.len() < 1000);
        assert!(s.contains("output truncated"));
        assert!(s.contains("characters omitted"));
    } else {
        panic!("Expected Text content");
    }
}

#[test]
fn test_truncate_to_preserves_start_and_end() {
    let text = format!("START{}END", "x".repeat(1000));
    let mut output = ToolOutput::text(text);
    output.truncate_to(100);
    if let ToolResultContent::Text(ref s) = output.content {
        assert!(s.starts_with("START"));
        assert!(s.ends_with("END"));
    } else {
        panic!("Expected Text content");
    }
}

#[test]
fn test_truncate_to_ignores_structured() {
    let mut output = ToolOutput::structured(serde_json::json!({"key": "value"}));
    output.truncate_to(1); // Should not panic or change
    assert!(matches!(&output.content, ToolResultContent::Structured(_)));
}

#[test]
fn test_truncate_to_utf8_safe() {
    // Use multibyte characters to verify UTF-8 safety
    let text = "你好世界".repeat(100); // 4 chars × 3 bytes each × 100 = 1200 bytes
    let mut output = ToolOutput::text(text);
    output.truncate_to(100);
    if let ToolResultContent::Text(ref s) = output.content {
        // Should not panic and result should be valid UTF-8
        assert!(s.is_char_boundary(0));
        assert!(s.contains("output truncated"));
    } else {
        panic!("Expected Text content");
    }
}
