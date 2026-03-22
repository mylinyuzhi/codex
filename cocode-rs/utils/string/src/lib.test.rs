use super::*;
use pretty_assertions::assert_eq;

#[test]
fn find_uuids_finds_multiple() {
    let input = "x 00112233-4455-6677-8899-aabbccddeeff-k y 12345678-90ab-cdef-0123-456789abcdef";
    assert_eq!(
        find_uuids(input),
        vec![
            "00112233-4455-6677-8899-aabbccddeeff".to_string(),
            "12345678-90ab-cdef-0123-456789abcdef".to_string(),
        ]
    );
}

#[test]
fn find_uuids_ignores_invalid() {
    let input = "not-a-uuid-1234-5678-9abc-def0-123456789abc";
    assert_eq!(find_uuids(input), Vec::<String>::new());
}

#[test]
fn find_uuids_handles_non_ascii_without_overlap() {
    let input = "\u{1f642} 55e5d6f7-8a7f-4d2a-8d88-123456789012abc";
    assert_eq!(
        find_uuids(input),
        vec!["55e5d6f7-8a7f-4d2a-8d88-123456789012".to_string()]
    );
}

#[test]
fn sanitize_metric_tag_value_trims_and_fills_unspecified() {
    let msg = "///";
    assert_eq!(sanitize_metric_tag_value(msg), "unspecified");
}

#[test]
fn sanitize_metric_tag_value_replaces_invalid_chars() {
    let msg = "bad value!";
    assert_eq!(sanitize_metric_tag_value(msg), "bad_value");
}

#[test]
fn normalize_markdown_hash_location_suffix_converts_single_location() {
    assert_eq!(
        normalize_markdown_hash_location_suffix("#L74C3"),
        Some(":74:3".to_string())
    );
}

#[test]
fn normalize_markdown_hash_location_suffix_converts_ranges() {
    assert_eq!(
        normalize_markdown_hash_location_suffix("#L74C3-L76C9"),
        Some(":74:3-76:9".to_string())
    );
}
