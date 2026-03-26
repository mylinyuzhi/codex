use pretty_assertions::assert_eq;

use super::*;
use crate::mcp_bridge::DiagnosticPosition;
use crate::mcp_bridge::DiagnosticRange;
use crate::mcp_bridge::IdeDiagnosticRaw;

fn make_raw_diagnostic(message: &str, severity: i32, line: i32) -> IdeDiagnosticRaw {
    IdeDiagnosticRaw {
        message: message.to_string(),
        severity,
        source: Some("test".to_string()),
        code: Some(serde_json::Value::String("E001".to_string())),
        range: DiagnosticRange {
            start: DiagnosticPosition { line, character: 0 },
            end: DiagnosticPosition {
                line,
                character: 10,
            },
        },
    }
}

#[test]
fn test_diagnostic_severity_from_lsp() {
    assert_eq!(DiagnosticSeverity::from_lsp(1), DiagnosticSeverity::Error);
    assert_eq!(DiagnosticSeverity::from_lsp(2), DiagnosticSeverity::Warning);
    assert_eq!(
        DiagnosticSeverity::from_lsp(3),
        DiagnosticSeverity::Information
    );
    assert_eq!(DiagnosticSeverity::from_lsp(4), DiagnosticSeverity::Hint);
    assert_eq!(DiagnosticSeverity::from_lsp(99), DiagnosticSeverity::Hint);
}

#[test]
fn test_diagnostic_severity_symbols() {
    assert_eq!(DiagnosticSeverity::Error.symbol(), "\u{2717}");
    assert_eq!(DiagnosticSeverity::Warning.symbol(), "\u{26A0}");
    assert_eq!(DiagnosticSeverity::Information.symbol(), "\u{2139}");
    assert_eq!(DiagnosticSeverity::Hint.symbol(), "\u{2605}");
}

#[test]
fn test_ide_diagnostic_from_raw() {
    let raw = make_raw_diagnostic("unused variable", 2, 10);
    let diag = IdeDiagnostic::from_raw(&raw);

    assert_eq!(diag.message, "unused variable");
    assert_eq!(diag.severity, DiagnosticSeverity::Warning);
    assert_eq!(diag.source.as_deref(), Some("test"));
    assert_eq!(diag.code.as_deref(), Some("E001"));
    assert_eq!(diag.range_start_line, 10);
    assert_eq!(diag.range_start_char, 0);
    assert_eq!(diag.range_end_line, 10);
    assert_eq!(diag.range_end_char, 10);
}

#[test]
fn test_ide_diagnostic_equality() {
    let raw = make_raw_diagnostic("error", 1, 5);
    let d1 = IdeDiagnostic::from_raw(&raw);
    let d2 = IdeDiagnostic::from_raw(&raw);
    assert_eq!(d1, d2);

    // Different message -> not equal
    let raw2 = make_raw_diagnostic("different error", 1, 5);
    let d3 = IdeDiagnostic::from_raw(&raw2);
    assert_ne!(d1, d3);
}

#[tokio::test]
async fn test_diagnostics_manager_has_baseline() {
    let manager = IdeDiagnosticsManager::new();
    let path = Path::new("/src/main.rs");
    assert!(!manager.has_baseline(path).await);

    // Manually insert baseline
    {
        let mut baseline = manager.baseline.write().await;
        baseline.insert(path.to_path_buf(), vec![]);
    }
    assert!(manager.has_baseline(path).await);
}

#[tokio::test]
async fn test_diagnostics_manager_clear() {
    let manager = IdeDiagnosticsManager::new();
    let path = Path::new("/src/main.rs");

    {
        let mut baseline = manager.baseline.write().await;
        baseline.insert(path.to_path_buf(), vec![]);
    }
    assert!(manager.has_baseline(path).await);

    manager.clear_baseline().await;
    assert!(!manager.has_baseline(path).await);
}
