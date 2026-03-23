use super::shorten_path;

#[test]
fn test_shorten_path_short_input_returns_unchanged() {
    let result = shorten_path("/usr/bin", 40);
    assert_eq!(result, "/usr/bin");
}

#[test]
fn test_long_path_truncated() {
    let result = shorten_path("/very/long/deeply/nested/path/to/something", 20);
    assert_eq!(result, "/very/.../something");
}

#[test]
fn test_few_segments_not_truncated() {
    // 3 or fewer segments => no truncation even if long
    let result = shorten_path("/a/really_long_directory_name", 10);
    assert_eq!(result, "/a/really_long_directory_name");
}
