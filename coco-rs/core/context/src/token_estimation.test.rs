use super::*;

#[test]
fn test_estimate_tokens() {
    // 100 chars ≈ 25 tokens (at 4 chars/token)
    assert_eq!(estimate_tokens("a".repeat(100).as_str()), 25);
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn test_is_over_threshold() {
    assert!(is_over_threshold(95_000, 100_000, 90));
    assert!(!is_over_threshold(85_000, 100_000, 90));
    assert!(!is_over_threshold(100, 0, 90)); // zero context window
}
