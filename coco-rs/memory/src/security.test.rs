use super::*;

#[test]
fn test_valid_paths() {
    assert!(validate_memory_path("user_role.md").is_ok());
    assert!(validate_memory_path("feedback_testing.md").is_ok());
    assert!(validate_memory_path("sub/nested.md").is_ok());
}

#[test]
fn test_traversal_rejected() {
    assert_eq!(
        validate_memory_path("../escape.md"),
        Err(PathValidationError::Traversal)
    );
    assert_eq!(
        validate_memory_path("foo/../../bar.md"),
        Err(PathValidationError::Traversal)
    );
}

#[test]
fn test_null_byte_rejected() {
    assert_eq!(
        validate_memory_path("foo\0bar.md"),
        Err(PathValidationError::NullByte)
    );
}

#[test]
fn test_absolute_path_rejected() {
    assert_eq!(
        validate_memory_path("/etc/passwd"),
        Err(PathValidationError::AbsolutePath)
    );
}

#[test]
fn test_unc_path_rejected() {
    assert_eq!(
        validate_memory_path("\\\\server\\share"),
        Err(PathValidationError::UncPath)
    );
}

#[test]
fn test_unicode_traversal_rejected() {
    // Fullwidth dot: U+FF0E
    assert_eq!(
        validate_memory_path("\u{FF0E}\u{FF0E}/escape"),
        Err(PathValidationError::UnicodeTraversal)
    );
    // Fullwidth slash: U+FF0F
    assert_eq!(
        validate_memory_path("foo\u{FF0F}bar"),
        Err(PathValidationError::UnicodeTraversal)
    );
}

#[test]
fn test_empty_path_rejected() {
    assert_eq!(validate_memory_path(""), Err(PathValidationError::Empty));
}

#[test]
fn test_url_encoded_traversal_rejected() {
    assert_eq!(
        validate_memory_path("%2e%2e/escape"),
        Err(PathValidationError::Traversal)
    );
}

#[test]
fn test_resolved_path_within_dir() {
    let mem_dir = std::path::Path::new("/home/user/.claude/memory");
    let result = validate_resolved_path(std::path::Path::new("user_role.md"), mem_dir);
    assert!(result.is_ok());
}

#[test]
fn test_resolved_path_escape() {
    let mem_dir = std::path::Path::new("/home/user/.claude/memory");
    let result = validate_resolved_path(std::path::Path::new("../../etc/passwd"), mem_dir);
    assert_eq!(result, Err(PathValidationError::Escape));
}

#[test]
fn test_is_within_memory_dir() {
    let mem_dir = std::path::Path::new("/home/user/.claude/memory");
    assert!(is_within_memory_dir(
        std::path::Path::new("/home/user/.claude/memory/foo.md"),
        mem_dir,
    ));
    assert!(!is_within_memory_dir(
        std::path::Path::new("/home/user/.claude/other.md"),
        mem_dir,
    ));
}

#[test]
fn test_sanitize_path_key() {
    assert_eq!(sanitize_path_key("User Role"), "user_role");
    assert_eq!(sanitize_path_key("foo/bar"), "foobar");
    assert_eq!(sanitize_path_key("test-key.md"), "test-key.md");
}
