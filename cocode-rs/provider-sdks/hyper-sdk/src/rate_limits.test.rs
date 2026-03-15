use super::*;
use http::HeaderValue;

fn make_headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.insert(
            http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

#[test]
fn test_openai_headers() {
    let headers = make_headers(&[
        ("x-ratelimit-remaining-requests", "100"),
        ("x-ratelimit-remaining-tokens", "50000"),
        ("x-ratelimit-reset-requests", "60.5"),
    ]);

    let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
    assert_eq!(snapshot.remaining_requests, Some(100));
    assert_eq!(snapshot.remaining_tokens, Some(50000));
    assert_eq!(snapshot.reset_seconds, Some(60.5));
    assert!(!snapshot.is_approaching_limit());
    assert!(!snapshot.is_exhausted());
}

#[test]
fn test_anthropic_headers() {
    let headers = make_headers(&[
        ("anthropic-ratelimit-requests-remaining", "5"),
        ("anthropic-ratelimit-tokens-remaining", "1000"),
    ]);

    let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
    assert_eq!(snapshot.remaining_requests, Some(5));
    assert_eq!(snapshot.remaining_tokens, Some(1000));
    assert!(snapshot.is_approaching_limit());
}

#[test]
fn test_retry_after_header() {
    let headers = make_headers(&[("retry-after", "30")]);

    let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
    assert_eq!(snapshot.retry_after, Some(Duration::from_secs(30)));
    assert_eq!(snapshot.suggested_wait(), Some(Duration::from_secs(30)));
}

#[test]
fn test_exhausted_limit() {
    let headers = make_headers(&[("x-ratelimit-remaining-requests", "0")]);

    let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
    assert!(snapshot.is_exhausted());
    assert!(snapshot.is_approaching_limit());
}

#[test]
fn test_no_headers_returns_none() {
    let headers = HeaderMap::new();
    assert!(RateLimitSnapshot::from_headers(&headers).is_none());
}

#[test]
fn test_duration_string_parsing() {
    assert_eq!(
        parse_duration_string("1m30s"),
        Some(Duration::from_secs(90))
    );
    assert_eq!(parse_duration_string("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(parse_duration_string("45s"), Some(Duration::from_secs(45)));
    assert_eq!(
        parse_duration_string("1h30m"),
        Some(Duration::from_secs(5400))
    );
    assert_eq!(parse_duration_string("invalid"), None);
}

#[test]
fn test_suggested_wait_prefers_retry_after() {
    let headers = make_headers(&[("retry-after", "10"), ("x-ratelimit-reset-requests", "60")]);

    let snapshot = RateLimitSnapshot::from_headers(&headers).unwrap();
    // Should prefer retry-after over reset
    assert_eq!(snapshot.suggested_wait(), Some(Duration::from_secs(10)));
}
