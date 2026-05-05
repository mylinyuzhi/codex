//! Unit tests for the TUI driver's pure helpers.
//!
//! `run_agent_driver` itself is an integration point (talks to an
//! `ApiClient`, spawns tokio tasks, etc.) so we exercise only the
//! decomposed pure logic here.

use super::PermissionsMutation;
use super::SentinelTrigger;
use super::classify_sentinel_trigger;
use super::parse_clear_scope;
use super::parse_permissions_mutation;
use super::parse_slash_command;
use super::should_trigger_title_gen;
use coco_tui::ClearScope;

#[test]
fn title_gen_fires_when_all_conditions_met() {
    assert!(should_trigger_title_gen(
        /*auto_title_enabled*/ true, /*already_attempted*/ false,
        /*fast_spec_present*/ true, /*plan_has_exited*/ true,
        /*plan_text_non_empty*/ true,
    ));
}

#[test]
fn title_gen_gated_off_by_setting() {
    // User hasn't opted in.
    assert!(!should_trigger_title_gen(false, false, true, true, true));
}

#[test]
fn title_gen_does_not_retry_after_first_attempt() {
    // Latch: once we've attempted, don't re-fire even if conditions still hold.
    assert!(!should_trigger_title_gen(true, true, true, true, true));
}

#[test]
fn title_gen_skipped_without_fast_model() {
    // User enabled auto_title but hasn't wired up a Fast role / the
    // `ANTHROPIC_API_KEY` fallback isn't available. Silent skip.
    assert!(!should_trigger_title_gen(true, false, false, true, true));
}

#[test]
fn title_gen_skipped_before_plan_exited() {
    // Model hasn't successfully exited plan mode yet this session.
    assert!(!should_trigger_title_gen(true, false, true, false, true));
}

#[test]
fn title_gen_skipped_with_empty_plan() {
    // ExitPlanMode ran against an empty plan file (e.g. model called
    // Exit before writing anything). No useful context to summarize.
    assert!(!should_trigger_title_gen(true, false, true, true, false));
}

#[test]
fn parse_slash_extracts_name_only() {
    assert_eq!(parse_slash_command("/help"), Some(("help", "")));
}

#[test]
fn parse_slash_splits_args() {
    assert_eq!(
        parse_slash_command("/commit focus on auth changes"),
        Some(("commit", "focus on auth changes"))
    );
}

#[test]
fn parse_slash_collapses_extra_whitespace() {
    // Single space after the name is the conventional separator;
    // additional whitespace is preserved as part of args (the
    // handlers themselves trim again).
    assert_eq!(
        parse_slash_command("/commit   spaced"),
        Some(("commit", "spaced"))
    );
}

#[test]
fn parse_slash_trims_outer_whitespace() {
    assert_eq!(parse_slash_command("   /diff   "), Some(("diff", "")));
}

#[test]
fn parse_slash_rejects_non_slash() {
    assert_eq!(parse_slash_command("hello world"), None);
}

#[test]
fn parse_slash_rejects_bare_slash() {
    assert_eq!(parse_slash_command("/"), None);
    assert_eq!(parse_slash_command("   /   "), None);
}

// `classify_sentinel_trigger` — decides whether a registry handler's
// Text output is actually a request to fire a real feature (compact /
// dream / summary). Wrong classification means the user's `/compact`
// would silently print sentinel garbage instead of triggering compaction.

#[test]
fn classify_sentinel_compact_no_args() {
    use coco_commands::handlers::compact::COMPACT_SENTINEL;
    let text = format!("{COMPACT_SENTINEL} \nCompacting conversation…\n");
    assert_eq!(
        classify_sentinel_trigger(&text),
        Some(SentinelTrigger::Compact {
            custom_instructions: None
        })
    );
}

#[test]
fn classify_sentinel_compact_with_instructions() {
    use coco_commands::handlers::compact::COMPACT_SENTINEL;
    let text = format!("{COMPACT_SENTINEL} focus on auth\nCompacting…\n");
    assert_eq!(
        classify_sentinel_trigger(&text),
        Some(SentinelTrigger::Compact {
            custom_instructions: Some("focus on auth".to_string()),
        })
    );
}

#[test]
fn classify_sentinel_compact_whitespace_only_args_treated_as_none() {
    use coco_commands::handlers::compact::COMPACT_SENTINEL;
    // The handler emits "{SENTINEL}  \n" when args is whitespace; trim
    // should fold that back to None so the engine doesn't see an empty
    // custom_instructions string.
    let text = format!("{COMPACT_SENTINEL}    \nCompacting…\n");
    assert_eq!(
        classify_sentinel_trigger(&text),
        Some(SentinelTrigger::Compact {
            custom_instructions: None
        })
    );
}

#[test]
fn classify_sentinel_dream() {
    use coco_commands::handlers::dream::DREAM_SENTINEL;
    let text = format!("{DREAM_SENTINEL} \nKAIROS dream consolidation…\n");
    assert_eq!(
        classify_sentinel_trigger(&text),
        Some(SentinelTrigger::Dream)
    );
}

#[test]
fn classify_sentinel_summary() {
    use coco_commands::handlers::summary::SUMMARY_SENTINEL;
    let text = format!("{SUMMARY_SENTINEL} \nWriting session memory…\n");
    assert_eq!(
        classify_sentinel_trigger(&text),
        Some(SentinelTrigger::Summary)
    );
}

#[test]
fn classify_sentinel_plain_text_returns_none() {
    // The vast majority of handler outputs — anything not starting with
    // a sentinel — must classify as None so the dispatcher renders them
    // verbatim in the transcript.
    assert_eq!(classify_sentinel_trigger(""), None);
    assert_eq!(classify_sentinel_trigger("Hello, world"), None);
    assert_eq!(
        classify_sentinel_trigger("## Permission Rules\n\nNo rules"),
        None
    );
}

#[test]
fn classify_sentinel_does_not_match_substring() {
    // Sentinels must be at the *start*; a sentinel embedded in body text
    // (e.g. echoed inside an explanation) must not trigger.
    use coco_commands::handlers::compact::COMPACT_SENTINEL;
    let text = format!("Here is the sentinel: {COMPACT_SENTINEL}");
    assert_eq!(classify_sentinel_trigger(&text), None);
}

// `parse_clear_scope` — maps the typed-text args to a structured
// `ClearScope`. None means "unknown subcommand" and the dispatcher
// surfaces a usage hint instead of resetting the wrong scope.

#[test]
fn parse_clear_scope_default_is_conversation() {
    // `/clear` with no args = full TS-parity reset.
    assert_eq!(parse_clear_scope(""), Some(ClearScope::Conversation));
    assert_eq!(parse_clear_scope("   "), Some(ClearScope::Conversation));
}

#[test]
fn parse_clear_scope_all_alias() {
    // `/clear all` is a TS-era alias users still type; resolved to the
    // same Conversation scope (not the `All` variant — that variant
    // exists only for documentation symmetry).
    assert_eq!(parse_clear_scope("all"), Some(ClearScope::Conversation));
    assert_eq!(parse_clear_scope(" all "), Some(ClearScope::Conversation));
}

#[test]
fn parse_clear_scope_history_is_lighter() {
    // Rust-only lighter scope — must NOT collapse to Conversation, or
    // we'd silently invalidate file caches the user wanted preserved.
    assert_eq!(parse_clear_scope("history"), Some(ClearScope::History));
}

#[test]
fn parse_clear_scope_unknown_returns_none() {
    // Unknown subcommand → usage hint, not a silent default.
    assert_eq!(parse_clear_scope("foo"), None);
    assert_eq!(parse_clear_scope("everything"), None);
    assert_eq!(parse_clear_scope("ALL"), None); // case-sensitive
}

// `parse_permissions_mutation` — distinguishes the read-only / list
// path (None, falls through to registry) from the three mutating
// subcommands the TUI dispatcher actually applies to engine_config.

#[test]
fn parse_permissions_reset() {
    assert_eq!(
        parse_permissions_mutation("reset"),
        Some(PermissionsMutation::Reset)
    );
    assert_eq!(
        parse_permissions_mutation("  reset  "),
        Some(PermissionsMutation::Reset)
    );
}

#[test]
fn parse_permissions_allow() {
    assert_eq!(
        parse_permissions_mutation("allow Bash"),
        Some(PermissionsMutation::Allow("Bash".to_string()))
    );
    assert_eq!(
        parse_permissions_mutation("allow mcp__server__tool"),
        Some(PermissionsMutation::Allow("mcp__server__tool".to_string()))
    );
}

#[test]
fn parse_permissions_deny() {
    assert_eq!(
        parse_permissions_mutation("deny Write"),
        Some(PermissionsMutation::Deny("Write".to_string()))
    );
}

#[test]
fn parse_permissions_list_falls_through_to_registry() {
    // The read-only paths return None so the dispatcher hands off to
    // the registry handler (which reads settings.json and renders).
    assert_eq!(parse_permissions_mutation(""), None);
    assert_eq!(parse_permissions_mutation("list"), None);
    assert_eq!(parse_permissions_mutation("  "), None);
}

#[test]
fn parse_permissions_allow_without_tool_is_none() {
    // `allow ` with no tool name must fall through (the dispatcher then
    // emits a usage hint) — never let an empty-string tool reach
    // engine_config.allow_rules.
    assert_eq!(parse_permissions_mutation("allow"), None);
    assert_eq!(parse_permissions_mutation("allow "), None);
    assert_eq!(parse_permissions_mutation("allow   "), None);
}

#[test]
fn parse_permissions_deny_without_tool_is_none() {
    assert_eq!(parse_permissions_mutation("deny"), None);
    assert_eq!(parse_permissions_mutation("deny "), None);
}

#[test]
fn parse_permissions_unknown_subcommand_is_none() {
    // Unknown words pass through to the registry handler, which renders
    // its own "Unknown permissions subcommand" error.
    assert_eq!(parse_permissions_mutation("foobar"), None);
    assert_eq!(parse_permissions_mutation("revoke Bash"), None);
}

#[test]
fn title_gen_exhaustive_truth_table() {
    // Exhaustive check: the gate returns true for EXACTLY the all-true
    // combination and false for every other combination. Catches any
    // future refactor that accidentally makes one condition optional.
    for auto in [false, true] {
        for already in [false, true] {
            for spec in [false, true] {
                for exited in [false, true] {
                    for plan in [false, true] {
                        let result = should_trigger_title_gen(auto, already, spec, exited, plan);
                        let expected = auto && !already && spec && exited && plan;
                        assert_eq!(
                            result, expected,
                            "auto={auto} already={already} spec={spec} exited={exited} plan={plan}"
                        );
                    }
                }
            }
        }
    }
}
