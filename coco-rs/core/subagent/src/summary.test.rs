use super::*;

#[test]
fn read_only_agents_skip_summary() {
    assert!(!should_summarize("Explore", 5));
    assert!(!should_summarize("Plan", 5));
    assert!(!should_summarize("claude-code-guide", 5));
}

#[test]
fn trivial_transcripts_skip() {
    assert!(!should_summarize("general-purpose", 0));
    assert!(!should_summarize("general-purpose", 1));
}

#[test]
fn meaningful_transcripts_summarize() {
    assert!(should_summarize("general-purpose", 2));
    assert!(should_summarize("verification", 10));
}

#[test]
fn prompt_includes_previous_when_provided() {
    let (sys, user) = build_summary_prompts("general-purpose", Some("Reading foo.ts"));
    assert!(sys.is_empty());
    assert!(user.contains("Previous: \"Reading foo.ts\""));
    assert!(user.contains("say something NEW"));
}

#[test]
fn prompt_omits_previous_when_none() {
    let (_sys, user) = build_summary_prompts("general-purpose", None);
    assert!(!user.contains("Previous:"));
    assert!(user.contains("Describe your most recent action"));
}

#[test]
fn sanitize_strips_quotes_and_whitespace() {
    assert_eq!(
        sanitize_summary("  \"Reading runAgent.ts\"  "),
        Some("Reading runAgent.ts".to_string())
    );
}

#[test]
fn sanitize_rejects_empty() {
    assert!(sanitize_summary("").is_none());
    assert!(sanitize_summary("   ").is_none());
    assert!(sanitize_summary("\"\"").is_none());
}

#[test]
fn sanitize_rejects_none_marker() {
    assert!(sanitize_summary("NONE").is_none());
    assert!(sanitize_summary("none").is_none());
    assert!(sanitize_summary("None").is_none());
}

#[test]
fn sanitize_rejects_overlong() {
    let long = "a".repeat(81);
    assert!(sanitize_summary(&long).is_none());
    let exact = "a".repeat(80);
    assert_eq!(sanitize_summary(&exact), Some(exact));
}
