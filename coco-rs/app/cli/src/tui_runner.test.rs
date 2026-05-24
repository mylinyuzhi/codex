//! Unit tests for the TUI driver's pure helpers.
//!
//! `run_agent_driver` itself is an integration point (talks to an
//! `ApiClient`, spawns tokio tasks, etc.) so we exercise only the
//! decomposed pure logic here.

use super::PermissionsMutation;
use super::SentinelTrigger;
use super::classify_sentinel_trigger;
use super::parse_editor_command;
use super::parse_permissions_mutation;
use super::parse_slash_command;
use super::session_plan_file_path;
use super::should_trigger_title_gen;

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

#[test]
fn session_plan_file_path_uses_runtime_plan_directory_setting() {
    let config_home = tempfile::tempdir().expect("config home");
    let project = tempfile::tempdir().expect("project");
    let path = session_plan_file_path(
        config_home.path(),
        Some(project.path()),
        Some("plans"),
        "session-1",
    );

    assert!(path.starts_with(project.path().canonicalize().unwrap().join("plans")));
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("md"));
}

#[test]
fn parse_editor_command_splits_quoted_args() {
    let (program, args) =
        parse_editor_command("code --wait --reuse-window 'memory file.md'").expect("parsed");
    assert_eq!(program, "code");
    assert_eq!(args, vec!["--wait", "--reuse-window", "memory file.md"]);
}

#[test]
fn parse_editor_command_rejects_unbalanced_quotes() {
    let err = parse_editor_command("code 'unterminated").expect_err("should reject");
    assert!(err.contains("failed to parse editor command"));
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

// ── /plan dispatch parity (G5.2) ──
//
// `dispatch_plan` itself talks to a `SessionRuntime` and is exercised
// by integration tests. The TS-parity rule "fire a query if and only
// if `args` is non-empty AND not 'open'" is encoded in
// `plan_command_query_after_flip` as a pure helper so we cover that
// regression-prone branch without spinning up the runtime.

use super::plan_command_query_after_flip;

#[test]
fn plan_query_after_flip_fires_for_real_description() {
    assert_eq!(
        plan_command_query_after_flip("refactor the auth flow"),
        Some("refactor the auth flow")
    );
}

#[test]
fn plan_query_after_flip_trims_whitespace() {
    assert_eq!(
        plan_command_query_after_flip("   refactor   "),
        Some("refactor")
    );
}

#[test]
fn plan_query_after_flip_skips_bare_plan() {
    // TS `commands/plan/plan.tsx:84-89`: bare `/plan` (empty args)
    // calls `onDone('Enabled plan mode')` WITHOUT `shouldQuery`.
    assert_eq!(plan_command_query_after_flip(""), None);
    assert_eq!(plan_command_query_after_flip("   "), None);
}

#[test]
fn plan_query_after_flip_skips_open_subcommand() {
    // TS `commands/plan/plan.tsx:84`: `description !== 'open'` filter
    // — `/plan open` opens an editor, never fires a query.
    assert_eq!(plan_command_query_after_flip("open"), None);
    assert_eq!(plan_command_query_after_flip("  open  "), None);
}

#[test]
fn plan_query_after_flip_open_substring_still_queries() {
    // Only the bare token "open" suppresses the query — descriptions
    // that happen to contain it must still query.
    assert_eq!(
        plan_command_query_after_flip("open the door"),
        Some("open the door")
    );
}

mod truncate_output_tests {
    use super::super::truncate_output;
    use pretty_assertions::assert_eq;

    #[test]
    fn short_text_passes_through() {
        assert_eq!(truncate_output("hello".into(), 100, 10), "hello");
    }

    #[test]
    fn caps_at_line_count_with_marker() {
        let text = (0..15)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = truncate_output(text, 10_000, 5);
        let lines = out.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], "line 0");
        assert_eq!(lines[4], "line 4");
        assert_eq!(lines[5], "… (truncated)");
    }

    #[test]
    fn caps_at_byte_budget() {
        let long = "x".repeat(500);
        let out = truncate_output(long, 50, 1000);
        assert!(out.starts_with(&"x".repeat(50)));
        assert!(out.ends_with("(truncated)"));
    }

    #[test]
    fn preserves_utf8_boundaries_when_cut() {
        // Each 🚀 is 4 bytes; 60 chars × 4 = 240 bytes. The byte cut
        // must land on a 4-byte boundary so the string stays valid
        // UTF-8 (`.chars().count()` panics on a malformed slice).
        let rocket_run: String = "🚀".repeat(60);
        let out = truncate_output(rocket_run, 100, 1000);
        let _ = out.chars().count();
        assert!(out.ends_with("(truncated)"));
    }
}

mod turn_done_guard_tests {
    use super::super::TurnDoneGuard;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn fires_on_normal_scope_exit() {
        let (tx, mut rx) = mpsc::channel::<uuid::Uuid>(4);
        let id = uuid::Uuid::new_v4();
        {
            let _guard = TurnDoneGuard {
                turn_id: id,
                tx: tx.clone(),
            };
        }
        assert_eq!(rx.recv().await, Some(id));
    }

    #[tokio::test]
    async fn fires_on_panic_unwind_inside_spawn() {
        // The bug we're guarding against: a spawned turn task panics
        // before the original tail `turn_done_tx.send(...)` runs, so
        // the completion signal never fires and `active_turn` stays
        // locked. Drop runs during unwind, so the guard must signal
        // even on panic.
        let (tx, mut rx) = mpsc::channel::<uuid::Uuid>(4);
        let id = uuid::Uuid::new_v4();
        let tx_t = tx.clone();
        let prior_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let handle = tokio::spawn(async move {
            let _guard = TurnDoneGuard {
                turn_id: id,
                tx: tx_t,
            };
            panic!("intentional turn-task panic for test");
        });
        let res = handle.await;
        std::panic::set_hook(prior_hook);
        assert!(res.is_err(), "spawned task should have surfaced JoinError");
        assert_eq!(rx.recv().await, Some(id));
    }
}
