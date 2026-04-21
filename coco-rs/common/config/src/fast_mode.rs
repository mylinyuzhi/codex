use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::OnceLock;

use crate::env;
use crate::env::EnvKey;

/// Fast mode state.
/// Fast mode: same model (Opus 4.6), faster output speed. NOT a model switch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FastModeState {
    Active,
    Cooldown {
        reset_at: i64,
        reason: CooldownReason,
    },
}

/// Why fast mode is in cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CooldownReason {
    RateLimit,
    Overloaded,
}

/// Why fast mode is unavailable at the org level.
///
/// TS: fastMode.ts — `DisabledReason` type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisabledReason {
    /// Free account — fast mode not available.
    Free,
    /// Org preference disabled fast mode.
    Preference,
    /// Org billing not enabled for extra usage.
    ExtraUsageDisabled,
    /// API fetch error during org check.
    NetworkError,
    /// Unknown disabled reason from API.
    Unknown,
}

impl std::fmt::Display for DisabledReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Free => write!(f, "Fast mode is not available on free accounts"),
            Self::Preference => write!(f, "Fast mode is disabled by org preference"),
            Self::ExtraUsageDisabled => write!(f, "Extra usage billing is not enabled"),
            Self::NetworkError => write!(f, "Could not check fast mode availability"),
            Self::Unknown => write!(f, "Fast mode is unavailable"),
        }
    }
}

/// Org-level fast mode availability status.
///
/// TS: `OrgStatus = Pending | Enabled | Disabled(reason)`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[derive(Default)]
pub enum OrgFastModeStatus {
    /// Not yet fetched.
    #[default]
    Pending,
    /// Org allows fast mode.
    Enabled,
    /// Org disallows fast mode.
    Disabled { reason: DisabledReason },
}

/// Default cooldown duration for rate limit (60 seconds).
const RATE_LIMIT_COOLDOWN_MS: i64 = 60_000;

/// Default cooldown duration for overloaded (120 seconds).
const OVERLOADED_COOLDOWN_MS: i64 = 120_000;

impl FastModeState {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if cooldown has expired (now >= reset_at).
    pub fn is_cooldown_expired(&self, now_ms: i64) -> bool {
        match self {
            Self::Active => false,
            Self::Cooldown { reset_at, .. } => now_ms >= *reset_at,
        }
    }

    /// Remaining cooldown time in milliseconds, or 0 if expired/active.
    pub fn remaining_cooldown_ms(&self, now_ms: i64) -> i64 {
        match self {
            Self::Active => 0,
            Self::Cooldown { reset_at, .. } => (*reset_at - now_ms).max(0),
        }
    }
}

fn fast_mode_global() -> &'static Mutex<FastModeGlobal> {
    static STATE: OnceLock<Mutex<FastModeGlobal>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(FastModeGlobal::default()))
}

/// Global fast mode state — runtime state + org availability.
#[derive(Default)]
struct FastModeGlobal {
    runtime: Option<FastModeState>,
    org_status: OrgFastModeStatus,
    /// Per-session opt-in tracking.
    session_opted_in: bool,
}

/// Trigger cooldown when 429/503 errors occur during fast mode.
/// TS: triggerCooldown() in fastMode.ts
pub fn trigger_cooldown(reason: CooldownReason, reset_at: i64) {
    let mut g = fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    g.runtime = Some(FastModeState::Cooldown { reset_at, reason });
}

/// Trigger cooldown with default durations based on reason.
pub fn trigger_cooldown_default(reason: CooldownReason, now_ms: i64) {
    let duration = match reason {
        CooldownReason::RateLimit => RATE_LIMIT_COOLDOWN_MS,
        CooldownReason::Overloaded => OVERLOADED_COOLDOWN_MS,
    };
    trigger_cooldown(reason, now_ms + duration);
}

/// Trigger cooldown from an HTTP error status code.
///
/// Returns `true` if a cooldown was triggered (429 or 503).
pub fn trigger_cooldown_from_status(status_code: i32, now_ms: i64) -> bool {
    let reason = match status_code {
        429 => CooldownReason::RateLimit,
        503 => CooldownReason::Overloaded,
        _ => return false,
    };
    trigger_cooldown_default(reason, now_ms);
    true
}

/// Get current fast mode state.
pub fn get_fast_mode_state() -> Option<FastModeState> {
    fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .runtime
        .clone()
}

/// Reset fast mode to active.
pub fn reset_fast_mode() {
    let mut g = fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    g.runtime = Some(FastModeState::Active);
}

/// Set org-level fast mode status (from prefetch).
pub fn set_org_fast_mode_status(status: OrgFastModeStatus) {
    let mut g = fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    g.org_status = status;
}

/// Get org-level fast mode status.
pub fn get_org_fast_mode_status() -> OrgFastModeStatus {
    fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .org_status
        .clone()
}

/// Set per-session opt-in.
pub fn set_session_opted_in(opted_in: bool) {
    let mut g = fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    g.session_opted_in = opted_in;
}

/// Check per-session opt-in status.
pub fn is_session_opted_in() -> bool {
    fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .session_opted_in
}

/// Full availability check chain. Returns `(available, reason_if_not)`.
///
/// TS: fastMode.ts availability checks (in order):
/// 1. Env: COCO_DISABLE_FAST_MODE
/// 2. Auth: 1P only (not Bedrock/Vertex/Foundry)
/// 3. Org status: must be Enabled (not Pending/Disabled)
/// 4. Per-session opt-in: if fastModePerSessionOptIn setting is true, check session flag
///
/// Note: this intentionally reads live env rather than going through an
/// `EnvSnapshot`. Fast mode is a runtime toggle that an operator may
/// flip mid-session (via a manual `export`); the snapshot is captured
/// at startup and would miss such flips. All other env reads in
/// `coco-config` are snapshot-based — see `RuntimeConfig`.
pub fn check_fast_mode_availability(
    is_first_party: bool,
    per_session_opt_in_setting: bool,
) -> (bool, Option<String>) {
    // 1. Environment disable
    if env::is_env_truthy(EnvKey::CocoDisableFastMode) {
        return (false, Some("Disabled by COCO_DISABLE_FAST_MODE".into()));
    }

    // 2. Auth provider check
    if !is_first_party {
        return (false, Some("Fast mode requires first-party auth".into()));
    }

    // 3. Org status
    let g = fast_mode_global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    match &g.org_status {
        OrgFastModeStatus::Pending => {
            return (false, Some("Org status not yet fetched".into()));
        }
        OrgFastModeStatus::Disabled { reason } => {
            return (false, Some(reason.to_string()));
        }
        OrgFastModeStatus::Enabled => {}
    }

    // 4. Per-session opt-in
    if per_session_opt_in_setting && !g.session_opted_in {
        return (false, Some("Per-session opt-in required".into()));
    }

    (true, None)
}

/// Prefetch fast mode availability at startup.
/// TS: prefetchFastModeStatus() — checks org settings, auth scope.
/// Returns (available, unavailable_reason).
pub async fn prefetch_fast_mode_status() -> (bool, Option<String>) {
    // TODO: Implement org check at {BASE_API_URL}/api/claude_code_penguin_mode
    // For now, assume available and set org status to Enabled.
    set_org_fast_mode_status(OrgFastModeStatus::Enabled);
    (true, None)
}

/// Get the fast mode model identifier.
pub fn get_fast_mode_model() -> &'static str {
    "claude-opus-4-6-20250514"
}

#[cfg(test)]
#[path = "fast_mode.test.rs"]
mod tests;
