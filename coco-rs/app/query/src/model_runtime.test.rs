//! Tests for [`ModelRuntime`].
//!
//! Covers multi-slot chain degradation, the I13 hook invariant, and
//! the half-open probe state machine (backoff ramp, attempts cap,
//! deep-chain revert, owned probe state).

use std::sync::Arc;
use std::time::Duration;

use coco_config::FallbackRecoveryPolicy;
use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use pretty_assertions::assert_eq;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

use super::*;

struct StubModel {
    id: &'static str,
}

#[async_trait::async_trait]
impl LanguageModelV4 for StubModel {
    fn provider(&self) -> &str {
        "stub"
    }
    fn model_id(&self) -> &str {
        self.id
    }
    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, vercel_ai_provider::AISdkError> {
        Ok(LanguageModelV4GenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: "stub".into(),
                provider_metadata: None,
            })],
            usage: Usage::new(0, 0),
            finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
    }
    async fn do_stream(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, vercel_ai_provider::AISdkError> {
        Err(vercel_ai_provider::AISdkError::new("no stream"))
    }
}

fn stub_client(id: &'static str) -> Arc<ApiClient> {
    Arc::new(ApiClient::new(
        Arc::new(StubModel { id }),
        RetryConfig::default(),
    ))
}

/// Test-only convenience builder: equivalent to the deleted
/// `primary_only` / `with_fallback` shims.
fn with_single_fallback(primary: &'static str, fallback: &'static str) -> ModelRuntime {
    ModelRuntime::new(stub_client(primary), vec![stub_client(fallback)])
}

fn primary_only(primary: &'static str) -> ModelRuntime {
    ModelRuntime::new(stub_client(primary), Vec::new())
}

fn fast_policy() -> FallbackRecoveryPolicy {
    // 1s initial → 4s cap, 3 attempts — keeps test doubling
    // observable in just a few iterations.
    FallbackRecoveryPolicy {
        initial_backoff_secs: 1,
        max_backoff_secs: 4,
        max_attempts: 3,
    }
}

// ─── Basic slot-state tests ─────────────────────────────────────────────────

#[test]
fn test_primary_only_reports_primary_model_name() {
    let rt = primary_only("primary");
    assert_eq!(rt.current_model_name(), "primary");
    assert_eq!(rt.active_index(), 0);
    assert!(!rt.has_fallback());
}

#[test]
fn test_advance_without_fallback_reports_exhausted() {
    let mut rt = primary_only("primary");
    assert_eq!(rt.advance(), AdvanceOutcome::Exhausted);
    assert_eq!(rt.active_index(), 0, "exhausted must not mutate active");
}

#[test]
fn test_advance_single_fallback_then_exhausted() {
    let mut rt = with_single_fallback("primary", "fallback");
    assert_eq!(rt.current_model_name(), "primary");
    assert_eq!(rt.advance(), AdvanceOutcome::Switched("fallback".into()));
    assert_eq!(rt.active_index(), 1);
    assert_eq!(rt.current_model_name(), "fallback");
    assert_eq!(
        rt.advance(),
        AdvanceOutcome::Exhausted,
        "second advance runs out of slots"
    );
    assert_eq!(rt.active_index(), 1, "exhausted must not over-advance");
}

#[test]
fn test_new_walks_every_slot_in_order() {
    let mut rt = ModelRuntime::new(
        stub_client("primary"),
        vec![stub_client("fb1"), stub_client("fb2"), stub_client("fb3")],
    );
    assert_eq!(rt.slot_count(), 4);
    assert_eq!(rt.current_model_name(), "primary");
    assert_eq!(rt.advance(), AdvanceOutcome::Switched("fb1".into()));
    assert_eq!(rt.advance(), AdvanceOutcome::Switched("fb2".into()));
    assert_eq!(rt.advance(), AdvanceOutcome::Switched("fb3".into()));
    assert_eq!(rt.advance(), AdvanceOutcome::Exhausted);
}

#[test]
fn test_new_with_empty_fallbacks_is_primary_only() {
    let rt = ModelRuntime::new(stub_client("primary"), Vec::new());
    assert_eq!(rt.slot_count(), 1);
    assert!(!rt.has_fallback());
}

#[test]
fn test_current_client_returns_active_slot() {
    let mut rt = ModelRuntime::new(
        stub_client("primary"),
        vec![stub_client("fb1"), stub_client("fb2")],
    );
    assert_eq!(rt.current_client().model_id(), "primary");
    rt.advance();
    assert_eq!(rt.current_client().model_id(), "fb1");
    rt.advance();
    assert_eq!(rt.current_client().model_id(), "fb2");
}

// ─── ModelFallbackReason serde/wire contract ───────────────────────────────

#[test]
fn test_model_fallback_reason_serializes_snake_case() {
    let r = ModelFallbackReason::CapacityDegrade {
        consecutive_errors: 3,
    };
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        serde_json::json!({ "capacity_degrade": { "consecutive_errors": 3 } })
    );
    let r2 = ModelFallbackReason::ProbeRecovery;
    assert_eq!(
        serde_json::to_value(&r2).unwrap(),
        serde_json::json!("probe_recovery")
    );
    let r3 = ModelFallbackReason::ChainExhausted;
    assert_eq!(
        serde_json::to_value(&r3).unwrap(),
        serde_json::json!("chain_exhausted")
    );
}

// ─── Probe: gate conditions ─────────────────────────────────────────────────

#[test]
fn test_probe_without_policy_always_skips() {
    let mut rt = with_single_fallback("primary", "fb");
    rt.advance();
    assert_eq!(rt.attempt_probe_if_due(Instant::now()), ProbeDecision::Skip);
    assert_eq!(rt.active_index(), 1);
}

#[test]
fn test_probe_while_on_primary_skips() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    assert_eq!(rt.attempt_probe_if_due(Instant::now()), ProbeDecision::Skip);
    assert_eq!(rt.active_index(), 0);
}

#[test]
fn test_probe_before_backoff_elapses_skips() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    assert_eq!(rt.attempt_probe_if_due(t0), ProbeDecision::Skip);
    assert_eq!(
        rt.attempt_probe_if_due(t0 + Duration::from_millis(500)),
        ProbeDecision::Skip,
    );
    assert_eq!(rt.active_index(), 1);
}

#[test]
fn test_probe_after_backoff_swaps_to_primary() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    let probe_time = t0 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe_time), ProbeDecision::Probe);
    assert_eq!(rt.active_index(), 0, "probe pre-swaps to primary");
    assert!(rt.probe_in_flight());
}

#[test]
fn test_probe_while_already_probing_skips() {
    // Guard: double-calling `attempt_probe_if_due` without
    // finalizing must not mutate state a second time.
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    let probe_time = t0 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe_time), ProbeDecision::Probe);
    assert_eq!(
        rt.attempt_probe_if_due(probe_time + Duration::from_secs(100)),
        ProbeDecision::Skip,
        "in-flight probe blocks further probe decisions",
    );
    assert!(rt.probe_in_flight());
    assert_eq!(rt.active_index(), 0);
}

// ─── Probe: outcomes ────────────────────────────────────────────────────────

#[test]
fn test_probe_success_clears_recovery_state() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    let probe_at = t0 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe_at), ProbeDecision::Probe);
    rt.finalize_probe(ProbeOutcome::Success, probe_at);
    assert_eq!(rt.active_index(), 0);
    assert_eq!(rt.recovery_attempts(), None, "state cleared on recovery");
    assert!(!rt.probe_in_flight());
}

#[test]
fn test_probe_failure_reverts_and_doubles_backoff() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);

    // Attempt 1 — initial backoff = 1s.
    let probe1 = t0 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe1), ProbeDecision::Probe);
    rt.finalize_probe(ProbeOutcome::Failure, probe1);
    assert_eq!(rt.active_index(), 1, "failure reverts to fallback");
    assert_eq!(rt.recovery_attempts(), Some(1));
    assert!(!rt.probe_in_flight());

    // Backoff is now 2s — 1s elapsed is still < 2s.
    assert_eq!(rt.attempt_probe_if_due(probe1), ProbeDecision::Skip);
    assert_eq!(
        rt.attempt_probe_if_due(probe1 + Duration::from_secs(1)),
        ProbeDecision::Skip,
    );

    // After 2s elapsed, probe is due again.
    let probe2 = probe1 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe2), ProbeDecision::Probe);
}

#[test]
fn test_probe_attempts_cap_halts_further_probes() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let mut clock = Instant::now();
    rt.advance_at(clock);

    // Walk through 3 failed probes — max_attempts=3.
    for _ in 0..3 {
        clock += Duration::from_secs(10); // past any backoff
        assert_eq!(rt.attempt_probe_if_due(clock), ProbeDecision::Probe);
        rt.finalize_probe(ProbeOutcome::Failure, clock);
    }
    // Fourth attempt is blocked.
    clock += Duration::from_secs(100);
    assert_eq!(rt.attempt_probe_if_due(clock), ProbeDecision::Skip);
    assert_eq!(rt.recovery_attempts(), Some(3));
}

#[test]
fn test_probe_failure_reverts_to_captured_slot_in_deep_chain() {
    // Runtime on slot 2 when probe fires — revert must restore
    // slot 2, not slot 1.
    let mut rt = ModelRuntime::new(
        stub_client("primary"),
        vec![stub_client("fb1"), stub_client("fb2")],
    )
    .with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    rt.advance_at(t0 + Duration::from_millis(100));
    assert_eq!(rt.active_index(), 2);

    let probe_at = t0 + Duration::from_secs(3);
    assert_eq!(rt.attempt_probe_if_due(probe_at), ProbeDecision::Probe);
    rt.finalize_probe(ProbeOutcome::Failure, probe_at);
    assert_eq!(
        rt.active_index(),
        2,
        "must revert to captured slot 2, not some intermediate"
    );
}

#[test]
fn test_finalize_probe_is_noop_without_in_flight_probe() {
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    // No probe started — finalize is a safe no-op.
    rt.finalize_probe(ProbeOutcome::Success, Instant::now());
    assert_eq!(rt.active_index(), 0);
    assert!(!rt.probe_in_flight());
}

// ─── Backoff ramp is monotonic across forward hops ──────────────────────────

#[test]
fn test_forward_advance_preserves_backoff_ramp() {
    // Critical contract: chain degradation MUST NOT reset the
    // probe backoff ramp. If we've been probing for a while and
    // fallback 1 then degrades, probing fallback 2 continues the
    // same backoff schedule instead of restarting at `initial_backoff`.
    let mut rt = ModelRuntime::new(
        stub_client("primary"),
        vec![stub_client("fb1"), stub_client("fb2")],
    )
    .with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);

    // One failed probe — backoff becomes 2s, attempts=1.
    let probe1 = t0 + Duration::from_secs(2);
    rt.attempt_probe_if_due(probe1);
    rt.finalize_probe(ProbeOutcome::Failure, probe1);
    assert_eq!(rt.recovery_attempts(), Some(1));

    // Forward hop to fb2 — the backoff ramp MUST preserve,
    // not reset to initial_backoff=1s.
    let hop_time = probe1 + Duration::from_secs(10);
    rt.advance_at(hop_time);
    assert_eq!(rt.active_index(), 2);
    assert_eq!(
        rt.recovery_attempts(),
        Some(1),
        "attempts preserved across forward hop",
    );

    // Immediate probe is still blocked — backoff is 2s from
    // switched_at=hop_time.
    assert_eq!(rt.attempt_probe_if_due(hop_time), ProbeDecision::Skip);
    assert_eq!(
        rt.attempt_probe_if_due(hop_time + Duration::from_secs(1)),
        ProbeDecision::Skip,
        "1s < preserved 2s backoff",
    );
    // But after 2s, due again.
    assert_eq!(
        rt.attempt_probe_if_due(hop_time + Duration::from_secs(2)),
        ProbeDecision::Probe,
    );
}

#[test]
fn test_advance_while_probe_in_flight_finalizes_as_failure() {
    // Edge case: the probe's underlying turn hit a capacity
    // error so the engine advances the chain. The runtime must
    // cleanly finalize the probe as failure before stepping.
    let mut rt = with_single_fallback("primary", "fb").with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    let probe_at = t0 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe_at), ProbeDecision::Probe);
    assert!(rt.probe_in_flight());

    // `advance_at` implicitly finalizes probe-as-failure — but
    // with one-fallback chain, the next slot is exhausted. So
    // advance reverts to fb, then tries to step forward →
    // Exhausted (already on last slot).
    //
    // For a multi-slot chain, advance would cleanly move to the
    // next tier. Single-fallback variant tests the defensive
    // finalize-then-check path.
    let outcome = rt.advance_at(probe_at);
    assert!(!rt.probe_in_flight(), "advance must finalize probe first");
    assert_eq!(outcome, AdvanceOutcome::Exhausted);
    assert_eq!(rt.recovery_attempts(), Some(1), "probe counted as failure");
}

#[test]
fn test_advance_while_probe_in_flight_deep_chain_finalizes_then_hops() {
    // Multi-slot chain: advance during probe → probe finalized
    // as failure (reverts to fb1), THEN advance to fb2.
    let mut rt = ModelRuntime::new(
        stub_client("primary"),
        vec![stub_client("fb1"), stub_client("fb2")],
    )
    .with_recovery_policy(fast_policy());
    let t0 = Instant::now();
    rt.advance_at(t0);
    assert_eq!(rt.active_index(), 1);

    let probe_at = t0 + Duration::from_secs(2);
    assert_eq!(rt.attempt_probe_if_due(probe_at), ProbeDecision::Probe);
    assert_eq!(rt.active_index(), 0, "probe pre-swap");

    let outcome = rt.advance_at(probe_at);
    assert_eq!(outcome, AdvanceOutcome::Switched("fb2".into()));
    assert_eq!(rt.active_index(), 2);
    assert!(!rt.probe_in_flight());
    assert_eq!(rt.recovery_attempts(), Some(1));
}

// ─── doubled_backoff unit test ─────────────────────────────────────────────

#[test]
fn test_doubled_backoff_saturates_at_policy_max() {
    let policy = fast_policy(); // max = 4s
    assert_eq!(
        super::doubled_backoff(Duration::from_secs(1), policy),
        Duration::from_secs(2),
    );
    assert_eq!(
        super::doubled_backoff(Duration::from_secs(2), policy),
        Duration::from_secs(4),
    );
    assert_eq!(
        super::doubled_backoff(Duration::from_secs(4), policy),
        Duration::from_secs(4),
        "at cap",
    );
    assert_eq!(
        super::doubled_backoff(Duration::from_secs(100), policy),
        Duration::from_secs(4),
        "over-cap inputs still cap",
    );
}
