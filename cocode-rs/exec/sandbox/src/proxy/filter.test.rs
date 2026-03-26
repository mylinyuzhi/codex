use super::*;

#[test]
fn test_is_allowed_empty_lists_allows_all() {
    let filter = DomainFilter::new(vec![], vec![]);
    assert!(filter.is_allowed("example.com"));
    assert!(filter.is_allowed("anything.org"));
}

#[test]
fn test_is_allowed_deny_list_blocks() {
    let filter = DomainFilter::new(vec![], vec!["evil.com".to_string()]);
    assert!(!filter.is_allowed("evil.com"));
    assert!(filter.is_allowed("good.com"));
}

#[test]
fn test_is_allowed_deny_takes_precedence_over_allow() {
    let filter = DomainFilter::new(
        vec!["example.com".to_string()],
        vec!["example.com".to_string()],
    );
    assert!(!filter.is_allowed("example.com"));
}

#[test]
fn test_is_allowed_allow_list_restricts() {
    let filter = DomainFilter::new(
        vec!["allowed.com".to_string(), "also-ok.org".to_string()],
        vec![],
    );
    assert!(filter.is_allowed("allowed.com"));
    assert!(filter.is_allowed("also-ok.org"));
    assert!(!filter.is_allowed("blocked.net"));
}

#[test]
fn test_is_allowed_wildcard_deny() {
    let filter = DomainFilter::new(vec![], vec!["*.evil.com".to_string()]);
    assert!(!filter.is_allowed("sub.evil.com"));
    assert!(!filter.is_allowed("deep.sub.evil.com"));
    // Exact domain does not match wildcard pattern.
    assert!(filter.is_allowed("evil.com"));
}

#[test]
fn test_is_allowed_wildcard_allow() {
    let filter = DomainFilter::new(vec!["*.example.com".to_string()], vec![]);
    assert!(filter.is_allowed("api.example.com"));
    assert!(filter.is_allowed("deep.sub.example.com"));
    // Exact domain does not match wildcard pattern.
    assert!(!filter.is_allowed("example.com"));
}

#[test]
fn test_is_allowed_case_insensitive() {
    let filter = DomainFilter::new(
        vec!["Example.COM".to_string()],
        vec!["EVIL.org".to_string()],
    );
    assert!(filter.is_allowed("example.com"));
    assert!(filter.is_allowed("EXAMPLE.COM"));
    assert!(!filter.is_allowed("evil.org"));
    assert!(!filter.is_allowed("Evil.Org"));
}

#[test]
fn test_is_allowed_wildcard_case_insensitive() {
    let filter = DomainFilter::new(vec!["*.Example.COM".to_string()], vec![]);
    assert!(filter.is_allowed("sub.example.com"));
    assert!(filter.is_allowed("SUB.EXAMPLE.COM"));
}

#[test]
fn test_is_allowed_deny_wildcard_overrides_allow_exact() {
    let filter = DomainFilter::new(
        vec!["api.example.com".to_string()],
        vec!["*.example.com".to_string()],
    );
    assert!(!filter.is_allowed("api.example.com"));
}

#[test]
fn test_is_allowed_allow_exact_and_wildcard() {
    let filter = DomainFilter::new(
        vec!["example.com".to_string(), "*.example.com".to_string()],
        vec![],
    );
    assert!(filter.is_allowed("example.com"));
    assert!(filter.is_allowed("api.example.com"));
    assert!(!filter.is_allowed("other.com"));
}
