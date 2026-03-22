use super::*;

#[test]
fn test_rotator_current() {
    let rotator = ApiKeyRotator::new(vec!["key-1".into(), "key-2".into(), "key-3".into()]);
    assert_eq!(rotator.current(), "key-1");
    // current doesn't advance
    assert_eq!(rotator.current(), "key-1");
}

#[test]
fn test_rotator_rotate() {
    let rotator = ApiKeyRotator::new(vec!["key-1".into(), "key-2".into(), "key-3".into()]);
    assert_eq!(rotator.current(), "key-1");
    assert_eq!(rotator.rotate(), "key-2");
    assert_eq!(rotator.rotate(), "key-3");
    // Wraps around
    assert_eq!(rotator.rotate(), "key-1");
    assert_eq!(rotator.rotate(), "key-2");
}

#[test]
fn test_rotator_single_key() {
    let rotator = ApiKeyRotator::new(vec!["only-key".into()]);
    assert_eq!(rotator.current(), "only-key");
    assert_eq!(rotator.rotate(), "only-key");
    assert_eq!(rotator.rotate(), "only-key");
    assert!(!rotator.has_alternatives());
}

#[test]
fn test_rotator_has_alternatives() {
    let single = ApiKeyRotator::new(vec!["one".into()]);
    assert!(!single.has_alternatives());

    let multi = ApiKeyRotator::new(vec!["one".into(), "two".into()]);
    assert!(multi.has_alternatives());
}

#[test]
fn test_rotator_key_count() {
    let rotator = ApiKeyRotator::new(vec!["a".into(), "b".into(), "c".into()]);
    assert_eq!(rotator.key_count(), 3);
}

#[test]
#[should_panic(expected = "ApiKeyRotator requires at least one key")]
fn test_rotator_empty_panics() {
    ApiKeyRotator::new(vec![]);
}
