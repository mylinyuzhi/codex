use super::*;

#[test]
fn test_basic_parse() {
    let content = "---\nname: commit\ndescription: test\n---\nThis is the prompt body.\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert_eq!(yaml, "name: commit\ndescription: test\n");
    assert_eq!(body, "This is the prompt body.\n");
}

#[test]
fn test_no_frontmatter_error() {
    let content = "This is just markdown without frontmatter.";
    let result = parse_frontmatter(content);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing opening"));
}

#[test]
fn test_missing_closing_delimiter() {
    let content = "---\nname: test\nno closing delimiter here\n";
    let result = parse_frontmatter(content);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing closing"));
}

#[test]
fn test_empty_frontmatter() {
    let content = "---\n---\nBody content here.\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert_eq!(yaml, "");
    assert_eq!(body, "Body content here.\n");
}

#[test]
fn test_body_with_triple_dashes() {
    // Triple dashes in the body (after frontmatter) should not interfere
    let content = "---\nname: test\n---\nSome content.\n\n---\n\nMore content.\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert_eq!(yaml, "name: test\n");
    assert_eq!(body, "Some content.\n\n---\n\nMore content.\n");
}

#[test]
fn test_multiline_yaml() {
    let content = r#"---
name: commit
description: Generate a commit message
allowed-tools:
  - Bash
  - Read
model: sonnet
---
Look at staged changes and generate a commit message.

$ARGUMENTS
"#;
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert!(yaml.contains("name: commit"));
    assert!(yaml.contains("allowed-tools:"));
    assert!(yaml.contains("  - Bash"));
    assert!(body.contains("Look at staged changes"));
    assert!(body.contains("$ARGUMENTS"));
}

#[test]
fn test_bom_stripped() {
    let content = "\u{feff}---\nname: test\n---\nBody.\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert_eq!(yaml, "name: test\n");
    assert_eq!(body, "Body.\n");
}

#[test]
fn test_empty_body() {
    let content = "---\nname: test\n---\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert_eq!(yaml, "name: test\n");
    assert_eq!(body, "");
}

#[test]
fn test_closing_delimiter_with_trailing_whitespace() {
    let content = "---\nname: test\n---  \nBody.\n";
    let (yaml, body) = parse_frontmatter(content).unwrap();
    assert_eq!(yaml, "name: test\n");
    assert_eq!(body, "Body.\n");
}
