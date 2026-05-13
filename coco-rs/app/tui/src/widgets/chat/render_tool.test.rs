use super::*;

#[test]
fn parse_location_marker_extracts_path_line_col() {
    let (prefix, path, tail) =
        parse_location_marker("Defined in src/main.rs:42:5").expect("location marker");
    assert_eq!(prefix, "Defined in ");
    assert_eq!(path, "src/main.rs");
    assert_eq!(tail, ":42:5");
}

#[test]
fn parse_location_marker_extracts_bare_path_line() {
    let (prefix, path, tail) = parse_location_marker("  src/foo.rs:11").expect("location marker");
    assert_eq!(prefix, "  ");
    assert_eq!(path, "src/foo.rs");
    assert_eq!(tail, ":11");
}

#[test]
fn parse_location_marker_ignores_prose_with_only_colon_numbers() {
    // "Hover info at 5:3" — `5:3` has no path tokens (no `/`, `\`, `.`),
    // so we don't mis-stylize the digits as a location.
    assert!(parse_location_marker("Hover info at 5:3").is_none());
}

#[test]
fn parse_location_marker_handles_windows_separator() {
    let result = parse_location_marker("src\\main.rs:1:1");
    assert!(result.is_some());
    let (_, path, tail) = result.unwrap();
    assert_eq!(path, "src\\main.rs");
    assert_eq!(tail, ":1:1");
}

#[test]
fn split_leading_space_keeps_pure_whitespace_as_no_split() {
    // Empty / pure whitespace returns None — there's nothing to style.
    assert!(split_leading_space("").is_none());
    assert!(split_leading_space("   ").is_none());
}

#[test]
fn split_leading_space_splits_indent_and_body() {
    let (lead, body) = split_leading_space("    body").expect("split");
    assert_eq!(lead, "    ");
    assert_eq!(body, "body");
}
