use serde_json::json;

use super::*;

#[test]
fn test_parse_diff_resolution_file_saved() {
    let texts = vec!["FILE_SAVED".into(), "new file content".into()];
    let result = parse_diff_resolution(&texts).expect("should parse");
    match result {
        DiffResolution::FileSaved { content } => {
            assert_eq!(content, "new file content");
        }
        other => panic!("expected FileSaved, got {other:?}"),
    }
}

#[test]
fn test_parse_diff_resolution_file_saved_no_content() {
    let texts = vec!["FILE_SAVED".into()];
    let result = parse_diff_resolution(&texts).expect("should parse");
    match result {
        DiffResolution::FileSaved { content } => {
            assert_eq!(content, "");
        }
        other => panic!("expected FileSaved, got {other:?}"),
    }
}

#[test]
fn test_parse_diff_resolution_tab_closed() {
    let texts = vec!["TAB_CLOSED".into()];
    let result = parse_diff_resolution(&texts).expect("should parse");
    assert!(matches!(result, DiffResolution::TabClosed));
}

#[test]
fn test_parse_diff_resolution_rejected() {
    let texts = vec!["DIFF_REJECTED".into()];
    let result = parse_diff_resolution(&texts).expect("should parse");
    assert!(matches!(result, DiffResolution::DiffRejected));
}

#[test]
fn test_parse_diff_resolution_empty() {
    let texts: Vec<String> = vec![];
    let result = parse_diff_resolution(&texts);
    assert!(result.is_err());
}

#[test]
fn test_parse_diff_resolution_unknown() {
    let texts = vec!["UNKNOWN".into()];
    let result = parse_diff_resolution(&texts);
    assert!(result.is_err());
}

#[test]
fn test_ide_diagnostic_raw_deserialize() {
    let json = json!({
        "message": "unused variable",
        "severity": 2,
        "source": "rust-analyzer",
        "code": "unused_variables",
        "range": {
            "start": {"line": 10, "character": 5},
            "end": {"line": 10, "character": 15}
        }
    });

    let diag: IdeDiagnosticRaw = serde_json::from_value(json).expect("should parse");
    assert_eq!(diag.message, "unused variable");
    assert_eq!(diag.severity, 2);
    assert_eq!(diag.source.as_deref(), Some("rust-analyzer"));
    assert_eq!(diag.range.start.line, 10);
    assert_eq!(diag.range.start.character, 5);
}

#[test]
fn test_ide_diagnostic_raw_minimal() {
    let json = json!({"message": "error"});
    let diag: IdeDiagnosticRaw = serde_json::from_value(json).expect("should parse");
    assert_eq!(diag.message, "error");
    assert_eq!(diag.severity, 1); // default
    assert!(diag.source.is_none());
}
