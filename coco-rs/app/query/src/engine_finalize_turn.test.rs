// Tests for `render_teammate_message_wrapper` lived here. The helper
// was deleted alongside the engine-side `Inbox`: teammate messages now
// flow through `CommandQueue` with `QueueOrigin::Coordinator` /
// `QueueOrigin::TaskNotification`, and the drain at
// `helpers::queued_command_to_attachment` applies origin-specific
// framing via `wrap_command_text`. TS parity:
// `getAgentPendingMessageAttachments` (`attachments.ts:1085-1100`)
// also surfaces coordinator messages as `queued_command` attachments,
// not as a separate `<teammate-message>` envelope.

// Phase 7 — Wire stub-field tests for `build_suggestion_context`.
//
// These assert the three previously-stubbed `SuggestionContext`
// fields (`pending_permission`, `elicitation_active`, `rate_limit`)
// now reflect live state on `ToolAppState`. Each test seeds the
// relevant counter / map, calls `build_suggestion_context`, and
// asserts the field flips on/off.

use super::build_suggestion_context;
use coco_types::CacheSafeParams;
use coco_types::PendingPermissionGuard;
use coco_types::ProviderApi;
use coco_types::RateLimitEntry;
use coco_types::RateLimitStatus;
use coco_types::ToolAppState;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::RwLock;

fn empty_cache(provider: &str) -> CacheSafeParams {
    CacheSafeParams {
        rendered_system_prompt: String::new(),
        model_id: "claude-opus-4-7".into(),
        provider: provider.into(),
        fork_context_messages: Vec::new(),
    }
}

#[tokio::test]
async fn build_suggestion_context_pending_permission_reflects_counter() {
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let cache = empty_cache("anthropic");

    // Counter at 0 → field is false.
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(!ctx.pending_permission, "counter == 0 should give false");

    // Acquire a guard → counter == 1 → field flips true.
    let counter = app_state.read().await.pending_permission_count.clone();
    let guard = PendingPermissionGuard::acquire(counter);
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(ctx.pending_permission, "counter > 0 should give true");

    // Drop guard → counter back to 0 → field flips false again.
    drop(guard);
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(
        !ctx.pending_permission,
        "guard drop should decrement counter"
    );
}

#[tokio::test]
async fn build_suggestion_context_elicitation_active_reflects_counter() {
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let cache = empty_cache("anthropic");

    let counter = app_state.read().await.elicitation_pending_count.clone();
    counter.fetch_add(1, Ordering::Relaxed);
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(ctx.elicitation_active);

    counter.fetch_sub(1, Ordering::Relaxed);
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(!ctx.elicitation_active);
}

#[tokio::test]
async fn build_suggestion_context_rate_limit_selective_by_provider() {
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));

    // Insert a Rejected entry for Anthropic with a future reset.
    {
        let mut snap = app_state.write().await;
        let now = chrono::Utc::now().timestamp_millis();
        snap.rate_limits.insert(
            "anthropic".to_string(),
            RateLimitEntry {
                api: ProviderApi::Anthropic,
                status: RateLimitStatus::Rejected,
                reset_at_ms: Some(now + 60_000),
                retry_after_seconds: Some(60),
                last_observed_ms: now,
            },
        );
    }

    // Cache provider "anthropic" → suppress.
    let cache = empty_cache("anthropic");
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(
        ctx.rate_limit,
        "Rejected entry on cache.provider should suppress"
    );

    // Cache provider "openai" (different) → no suppression
    // (selectivity).
    let cache = empty_cache("openai");
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(
        !ctx.rate_limit,
        "Rejected entry on a different provider must not suppress (selective policy)"
    );
}

#[tokio::test]
async fn build_suggestion_context_rate_limit_expires_with_reset_at() {
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));

    // Insert a Rejected entry with a reset time already in the past.
    {
        let mut snap = app_state.write().await;
        let now = chrono::Utc::now().timestamp_millis();
        snap.rate_limits.insert(
            "anthropic".to_string(),
            RateLimitEntry {
                api: ProviderApi::Anthropic,
                status: RateLimitStatus::Rejected,
                reset_at_ms: Some(now - 60_000), // already expired
                retry_after_seconds: Some(60),
                last_observed_ms: now - 120_000,
            },
        );
    }

    let cache = empty_cache("anthropic");
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(
        !ctx.rate_limit,
        "expired Rejected entry must not suppress (defensive read-side check)"
    );
}

#[tokio::test]
async fn build_suggestion_context_rate_limit_empty_provider_fails_open() {
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));

    {
        let mut snap = app_state.write().await;
        let now = chrono::Utc::now().timestamp_millis();
        snap.rate_limits.insert(
            "anthropic".to_string(),
            RateLimitEntry {
                api: ProviderApi::Anthropic,
                status: RateLimitStatus::Rejected,
                reset_at_ms: Some(now + 60_000),
                retry_after_seconds: Some(60),
                last_observed_ms: now,
            },
        );
    }

    // Pre-Phase-7 transcripts deserialize with `provider: ""`. We
    // can't match selectively without a key, so we fail open
    // (no suppression) rather than silencing all suggestions.
    let cache = empty_cache("");
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(
        !ctx.rate_limit,
        "empty cache.provider must fail open even when entries exist"
    );
}

#[tokio::test]
async fn prune_stale_rate_limits_removes_expired_entries() {
    use super::prune_stale_rate_limits;

    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let now = chrono::Utc::now().timestamp_millis();

    {
        let mut snap = app_state.write().await;
        // Expired (reset 60s ago).
        snap.rate_limits.insert(
            "anthropic".to_string(),
            RateLimitEntry {
                api: ProviderApi::Anthropic,
                status: RateLimitStatus::Rejected,
                reset_at_ms: Some(now - 60_000),
                retry_after_seconds: None,
                last_observed_ms: now - 120_000,
            },
        );
        // Still active (reset 60s in future).
        snap.rate_limits.insert(
            "openai".to_string(),
            RateLimitEntry {
                api: ProviderApi::Openai,
                status: RateLimitStatus::Rejected,
                reset_at_ms: Some(now + 60_000),
                retry_after_seconds: None,
                last_observed_ms: now,
            },
        );
        // None reset → retained until overwritten.
        snap.rate_limits.insert(
            "google".to_string(),
            RateLimitEntry {
                api: ProviderApi::Gemini,
                status: RateLimitStatus::Rejected,
                reset_at_ms: None,
                retry_after_seconds: None,
                last_observed_ms: now,
            },
        );
    }

    prune_stale_rate_limits(&app_state).await;

    let snap = app_state.read().await;
    assert!(
        !snap.rate_limits.contains_key("anthropic"),
        "expired anthropic entry should be pruned"
    );
    assert!(
        snap.rate_limits.contains_key("openai"),
        "still-active openai entry should be retained"
    );
    assert!(
        snap.rate_limits.contains_key("google"),
        "None-reset entry should be retained until overwritten"
    );
}

#[tokio::test]
async fn record_rate_limit_observation_writes_entry() {
    use crate::engine_helpers::record_rate_limit_observation;

    let app_state = Arc::new(RwLock::new(ToolAppState::default()));

    record_rate_limit_observation(
        &app_state,
        "anthropic",
        ProviderApi::Anthropic,
        Some(45_000), // 45s retry-after
    )
    .await;

    let snap = app_state.read().await;
    let entry = snap
        .rate_limits
        .get("anthropic")
        .expect("entry should be inserted");
    assert_eq!(entry.api, ProviderApi::Anthropic);
    assert_eq!(entry.status, RateLimitStatus::Rejected);
    assert_eq!(entry.retry_after_seconds, Some(45));
    let now = chrono::Utc::now().timestamp_millis();
    let reset = entry
        .reset_at_ms
        .expect("retry_after_ms should produce reset_at_ms");
    // Within reasonable jitter of now + 45s.
    assert!(
        (reset - (now + 45_000)).abs() < 1_000,
        "reset_at_ms should equal now + retry_after_ms (within 1s jitter); reset={reset} now={now}"
    );
}

#[tokio::test]
async fn record_rate_limit_observation_skips_empty_provider() {
    use crate::engine_helpers::record_rate_limit_observation;

    let app_state = Arc::new(RwLock::new(ToolAppState::default()));

    // Empty provider → skip silently rather than write a "" entry
    // that no selectivity check could match.
    record_rate_limit_observation(&app_state, "", ProviderApi::Anthropic, Some(1_000)).await;

    assert!(app_state.read().await.rate_limits.is_empty());
}

#[tokio::test]
async fn build_suggestion_context_rate_limit_allowed_status_does_not_suppress() {
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));

    {
        let mut snap = app_state.write().await;
        let now = chrono::Utc::now().timestamp_millis();
        snap.rate_limits.insert(
            "anthropic".to_string(),
            RateLimitEntry {
                api: ProviderApi::Anthropic,
                status: RateLimitStatus::AllowedWarning,
                reset_at_ms: Some(now + 60_000),
                retry_after_seconds: None,
                last_observed_ms: now,
            },
        );
    }

    let cache = empty_cache("anthropic");
    let ctx = build_suggestion_context(&cache, &app_state, false, false).await;
    assert!(
        !ctx.rate_limit,
        "AllowedWarning should not suppress — only Rejected does"
    );
}
