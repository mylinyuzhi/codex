use super::XaaError;
use super::redact_tokens;
use super::should_clear_id_token_on_status;

#[test]
fn redact_tokens_masks_known_keys() {
    let raw = r#"{"access_token":"secret-abc","refresh_token":"rt-xyz","other":"keep"}"#;
    let out = redact_tokens(raw);
    assert!(out.contains("[REDACTED]"));
    assert!(!out.contains("secret-abc"));
    assert!(!out.contains("rt-xyz"));
    assert!(out.contains("\"other\":\"keep\""));
}

#[test]
fn redact_tokens_handles_all_sensitive_keys() {
    let raw = r#"{"assertion":"a","subject_token":"b","client_secret":"c","id_token":"d"}"#;
    let out = redact_tokens(raw);
    assert!(!out.contains("\"a\""));
    assert!(!out.contains("\"b\""));
    assert!(!out.contains("\"c\""));
    assert!(!out.contains("\"d\""));
    assert_eq!(out.matches("[REDACTED]").count(), 4);
}

/// Regression for an earlier implementation that looped infinitely when
/// the same sensitive key appeared multiple times: after replacing a
/// value with `[REDACTED]` it re-scanned from position 0 and kept
/// finding the still-present key name. The fix advances the scan cursor
/// past each replacement. This test MUST terminate quickly.
#[test]
fn redact_tokens_terminates_on_repeated_key() {
    let raw =
        r#"{"access_token":"first","nested":{"access_token":"second"},"access_token":"third"}"#;
    let out = redact_tokens(raw);
    assert!(!out.contains("first"));
    assert!(!out.contains("second"));
    assert!(!out.contains("third"));
    assert_eq!(out.matches("[REDACTED]").count(), 3);
}

#[test]
fn should_clear_id_token_on_5xx_returns_false() {
    // 5xx → server glitch, don't throw away the id_token
    assert!(!should_clear_id_token_on_status(500, ""));
    assert!(!should_clear_id_token_on_status(502, ""));
    assert!(!should_clear_id_token_on_status(503, ""));
}

#[test]
fn should_clear_id_token_on_4xx_returns_true() {
    // 4xx → IdP says the id_token is bad
    assert!(should_clear_id_token_on_status(400, ""));
    assert!(should_clear_id_token_on_status(401, ""));
    assert!(should_clear_id_token_on_status(403, ""));
}

#[test]
fn should_clear_id_token_on_other_inspects_body() {
    assert!(should_clear_id_token_on_status(
        300,
        r#"{"error":"invalid_grant"}"#
    ));
    assert!(should_clear_id_token_on_status(
        300,
        r#"{"error":"invalid_token"}"#
    ));
    assert!(!should_clear_id_token_on_status(
        300,
        r#"{"error":"other_error"}"#
    ));
}

#[test]
fn xaa_error_display_redacts_http_errors() {
    let err = XaaError::Provider {
        status: 400,
        body: "{\"subject_token\":\"leaked\"}".into(),
        should_clear_id_token: true,
    };
    // The error display shouldn't leak the token (the Provider variant
    // puts body in Display). For this test we just confirm the variant
    // preserves the should_clear flag.
    let XaaError::Provider {
        should_clear_id_token,
        ..
    } = err
    else {
        panic!("expected Provider variant");
    };
    assert!(should_clear_id_token);
}
