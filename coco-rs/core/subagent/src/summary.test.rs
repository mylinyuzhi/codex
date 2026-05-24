use super::*;

#[test]
fn read_only_agents_skip_summary() {
    assert!(!should_summarize("Explore", 5));
    assert!(!should_summarize("Plan", 5));
    assert!(!should_summarize("coco-guide", 5));
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
fn prompt_body_has_no_leading_indent() {
    // TS `agentSummary.ts::buildSummaryPrompt` is a flat template
    // literal — every line starts at column 0. Byte parity matters
    // because the user prompt feeds into the parent's prompt cache
    // identity; a stray indent on each line shifts every byte and
    // busts the cache. The previous Rust impl used `\` continuations
    // with rustfmt indentation, which produced lines like
    // `         Good: "Reading runAgent.ts"` — wrong.
    let (_, user) = build_summary_prompts("general-purpose", None);
    for line in user.lines() {
        assert!(
            !line.starts_with(' '),
            "summary prompt line must not have leading whitespace: {line:?}"
        );
    }
}

#[test]
fn prompt_uses_tsfaithful_em_dash_in_previous_marker() {
    // TS source uses U+2014 EM DASH between `…"` and `say something
    // NEW.`. Verify the exact codepoint round-trips so cache keys
    // match across the JS and Rust runtimes.
    let (_, user) = build_summary_prompts("general-purpose", Some("Reading foo.ts"));
    assert!(user.contains("Previous: \"Reading foo.ts\" \u{2014} say something NEW."));
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
