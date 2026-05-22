use coco_types::ErrorParams;
use coco_types::TurnFailedParams;

use super::error_body;
use super::format_error_body;
use super::turn_failed_body;

#[test]
fn format_includes_category_and_retryable_hint() {
    let body = format_error_body("boom", Some("network"), true);
    assert_eq!(
        body,
        "boom\n\nCategory: network\nRetryable — coco will retry automatically where possible.\n\nPress Esc to dismiss."
    );
}

#[test]
fn format_without_category_drops_category_line() {
    let body = format_error_body("boom", None, false);
    assert!(!body.contains("Category:"));
    assert!(body.contains("Non-retryable"));
}

#[test]
fn format_drops_empty_category() {
    let body = format_error_body("boom", Some(""), false);
    assert!(!body.contains("Category:"));
}

#[test]
fn turn_failed_body_uses_turn_category_and_non_retryable() {
    let body = turn_failed_body(&TurnFailedParams {
        error: "upstream 503".into(),
    });
    assert!(body.starts_with("upstream 503"));
    assert!(body.contains("Category: turn"));
    assert!(body.contains("Non-retryable"));
}

#[test]
fn error_body_respects_event_retryable_flag() {
    let retryable = error_body(&ErrorParams {
        message: "x".into(),
        category: Some("rate".into()),
        retryable: true,
    });
    assert!(retryable.contains("Retryable"));

    let non_retryable = error_body(&ErrorParams {
        message: "y".into(),
        category: Some("fatal".into()),
        retryable: false,
    });
    assert!(non_retryable.contains("Non-retryable"));
}
