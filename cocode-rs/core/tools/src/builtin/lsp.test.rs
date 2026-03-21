use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[test]
fn test_tool_properties() {
    let tool = LspTool::new();
    assert_eq!(tool.name(), "Lsp");
    assert!(tool.is_concurrent_safe());
    assert!(tool.is_read_only());
}

#[test]
fn test_feature_gate() {
    let tool = LspTool::new();
    assert_eq!(tool.feature_gate(), Some(cocode_protocol::Feature::Lsp));
}

#[tokio::test]
async fn test_execute_without_manager() {
    let tool = LspTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "operation": "goToDefinition",
        "filePath": "/test/file.rs",
        "symbolName": "Config"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("LSP feature not enabled"));
}

#[tokio::test]
async fn test_validation_missing_operation() {
    let tool = LspTool::new();

    let input = serde_json::json!({
        "filePath": "/test/file.rs"
    });

    let result = tool.validate(&input).await;
    assert!(matches!(
        result,
        cocode_protocol::ValidationResult::Invalid { .. }
    ));
}

#[test]
fn test_format_locations_empty() {
    let result = format_locations(&[]);
    assert_eq!(result, "No results found");
}

#[test]
fn test_format_document_symbols_empty() {
    let result = format_document_symbols(&[]);
    assert_eq!(result, "No symbols found in this file");
}
