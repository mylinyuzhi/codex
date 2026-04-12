//! Tests for frontmatter parser.

use crate::FrontmatterValue;
use crate::parse;

#[test]
fn test_basic_frontmatter() {
    let input = "---\ntitle: Hello World\nauthor: Claude\n---\n# Body content";
    let fm = parse(input);
    assert_eq!(fm.data.get("title").unwrap().as_str(), Some("Hello World"));
    assert_eq!(fm.data.get("author").unwrap().as_str(), Some("Claude"));
    assert_eq!(fm.content.trim(), "# Body content");
}

#[test]
fn test_no_frontmatter() {
    let input = "# Just a heading\nSome text";
    let fm = parse(input);
    assert!(fm.data.is_empty());
    assert_eq!(fm.content, input);
}

#[test]
fn test_bool_values() {
    let input = "---\nenabled: true\ndisabled: false\n---\nbody";
    let fm = parse(input);
    assert_eq!(fm.data.get("enabled").unwrap().as_bool(), Some(true));
    assert_eq!(fm.data.get("disabled").unwrap().as_bool(), Some(false));
}

#[test]
fn test_null_value() {
    let input = "---\nkey:\n---\nbody";
    let fm = parse(input);
    assert_eq!(fm.data.get("key"), Some(&FrontmatterValue::Null));
}

#[test]
fn test_integer_value() {
    let input = "---\ncount: 42\n---\nbody";
    let fm = parse(input);
    assert_eq!(fm.data.get("count"), Some(&FrontmatterValue::Int(42)));
}

#[test]
fn test_quoted_string() {
    let input = "---\nname: \"quoted value\"\n---\nbody";
    let fm = parse(input);
    assert_eq!(fm.data.get("name").unwrap().as_str(), Some("quoted value"));
}

#[test]
fn test_list_values() {
    let input = "---\nallowed-tools:\n- Read\n- Write\n- Bash\n---\nbody";
    let fm = parse(input);
    let tools = fm
        .data
        .get("allowed-tools")
        .unwrap()
        .as_string_list()
        .unwrap();
    assert_eq!(tools, vec!["Read", "Write", "Bash"]);
}

#[test]
fn test_no_closing_delimiter() {
    let input = "---\nkey: val\nno closing delimiter";
    let fm = parse(input);
    assert!(fm.data.is_empty());
}

#[test]
fn test_empty_body() {
    let input = "---\nkey: val\n---\n";
    let fm = parse(input);
    assert_eq!(fm.data.get("key").unwrap().as_str(), Some("val"));
    assert!(fm.content.trim().is_empty());
}

#[test]
fn test_skill_frontmatter() {
    let input = r#"---
description: Review changed code
allowed-tools:
- Read
- Grep
- Glob
model: sonnet
user-invocable: true
---
Review the code changes and suggest improvements.
"#;
    let fm = parse(input);
    assert_eq!(
        fm.data.get("description").unwrap().as_str(),
        Some("Review changed code")
    );
    assert_eq!(fm.data.get("model").unwrap().as_str(), Some("sonnet"));
    assert_eq!(fm.data.get("user-invocable").unwrap().as_bool(), Some(true));
    let tools = fm
        .data
        .get("allowed-tools")
        .unwrap()
        .as_string_list()
        .unwrap();
    assert_eq!(tools, vec!["Read", "Grep", "Glob"]);
    assert!(fm.content.contains("Review the code changes"));
}
