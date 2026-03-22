use super::*;

#[test]
fn test_build_summarization_prompt() {
    let (system, user) = build_summarization_prompt("User asked to fix a bug", None);

    assert!(!system.is_empty());
    assert!(user.contains("fix a bug"));
    assert!(system.contains("Conversation Summarization"));
}

#[test]
fn test_build_summarization_prompt_with_instructions() {
    let (system, _user) =
        build_summarization_prompt("conversation text", Some("Focus on Rust code"));

    assert!(system.contains("Focus on Rust code"));
    assert!(system.contains("Additional Instructions"));
}

#[test]
fn test_build_brief_summary_prompt() {
    let (system, user) = build_brief_summary_prompt("some conversation");

    assert!(system.contains("brief"));
    assert!(user.contains("some conversation"));
}

#[test]
fn test_parse_summary_response_with_tags() {
    let response = r#"
Here is the summary:

<summary>
The user asked to implement two new crates for context management.
Files were created in core/context/ and core/prompt/.
</summary>

<analysis>
The conversation was productive. All tasks were completed.
</analysis>
"#;

    let parsed = parse_summary_response(response);
    assert!(parsed.summary.contains("two new crates"));
    assert!(parsed.analysis.is_some());
    assert!(parsed.analysis.as_deref().unwrap().contains("productive"));
}

#[test]
fn test_parse_summary_response_no_tags() {
    let response = "This is a plain summary without any tags.";
    let parsed = parse_summary_response(response);
    assert_eq!(parsed.summary, response);
    assert!(parsed.analysis.is_none());
}

#[test]
fn test_parse_summary_response_partial_tags() {
    let response = "<summary>Only summary here</summary>";
    let parsed = parse_summary_response(response);
    assert_eq!(parsed.summary, "Only summary here");
    assert!(parsed.analysis.is_none());
}

#[test]
fn test_extract_tag() {
    assert_eq!(
        extract_tag("<foo>bar</foo>", "foo"),
        Some("bar".to_string())
    );
    assert_eq!(extract_tag("no tags here", "foo"), None);
    assert_eq!(
        extract_tag("<foo>  spaced  </foo>", "foo"),
        Some("spaced".to_string())
    );
}
