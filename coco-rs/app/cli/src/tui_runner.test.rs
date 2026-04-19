//! Unit tests for the TUI driver's pure helpers.
//!
//! `run_agent_driver` itself is an integration point (talks to an
//! `ApiClient`, spawns tokio tasks, etc.) so we exercise only the
//! decomposed pure logic here.

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
