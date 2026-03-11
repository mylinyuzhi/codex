use super::*;

#[test]
fn test_api_key_debug_redacted() {
    let key = ApiKey::new("secret-key".to_string());
    let debug_output = format!("{key:?}");
    assert_eq!(debug_output, "ApiKey([REDACTED])");
    assert!(!debug_output.contains("secret-key"));
}

#[test]
fn test_api_key_expose() {
    let key = ApiKey::new("my-secret-key".to_string());
    assert_eq!(key.expose(), "my-secret-key");
}

#[test]
fn test_api_key_from_string() {
    let key = ApiKey::from("test-key".to_string());
    assert_eq!(key.expose(), "test-key");
}

#[test]
fn test_api_key_into_inner() {
    let key = ApiKey::new("inner-key".to_string());
    let inner = key.into_inner();
    assert_eq!(inner, "inner-key");
}

#[test]
fn test_api_key_equality() {
    let key1 = ApiKey::new("same-key".to_string());
    let key2 = ApiKey::new("same-key".to_string());
    let key3 = ApiKey::new("different-key".to_string());

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
}

#[test]
fn test_api_key_as_ref() {
    let key = ApiKey::new("test-key".to_string());
    assert_eq!(<ApiKey as AsRef<str>>::as_ref(&key), "test-key");
}
