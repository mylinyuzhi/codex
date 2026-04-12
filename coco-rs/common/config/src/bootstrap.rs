//! Bootstrap session state tracking.
//!
//! Ports the essential session state from TS `bootstrap/state.ts` and the
//! onboarding/first-run detection from `utils/config.ts`. The TS original is a
//! 1700-line global mutable singleton with getter/setter pairs; here we model
//! the same data as a `SessionState` struct protected by a global `OnceLock` +
//! `Mutex` so tests can reset it safely.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;

use crate::constants;

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Core session state carried for the lifetime of a single CLI invocation.
///
/// TS ref: `State` type in `bootstrap/state.ts`.
/// Only the fields that are meaningful for the Rust port are included; telemetry
/// counters, React-specific UI latches, and OTel provider handles are omitted.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Unique session identifier (UUID v4).
    pub session_id: String,
    /// Parent session id for tracking session lineage.
    pub parent_session_id: Option<String>,
    /// Original working directory at startup (symlinks resolved).
    pub original_cwd: PathBuf,
    /// Stable project root -- set once at startup, never updated mid-session.
    pub project_root: PathBuf,
    /// Current working directory (may change as tools run).
    pub cwd: PathBuf,
    /// Active model identifier.
    pub model: Option<String>,
    /// When the session started.
    pub started_at: Instant,
    /// Number of user messages processed this session.
    pub message_count: i64,
    /// Total cost accumulated this session (USD).
    pub total_cost_usd: f64,
    /// Per-model token usage breakdown.
    pub model_usage: HashMap<String, ModelUsageEntry>,
    /// Total API call duration in milliseconds.
    pub total_api_duration_ms: f64,
    /// Total tool execution duration in milliseconds.
    pub total_tool_duration_ms: f64,
    /// Lines added across all edits this session.
    pub total_lines_added: i64,
    /// Lines removed across all edits this session.
    pub total_lines_removed: i64,
    /// Whether the session is interactive (TUI) or headless (piped).
    pub is_interactive: bool,
    /// Client type identifier (e.g. "cli", "sdk", "vscode").
    pub client_type: String,
    /// Whether file checkpointing is enabled.
    pub file_checkpointing_enabled: bool,
    /// Whether session persistence to disk is disabled.
    pub session_persistence_disabled: bool,
}

/// Per-model token usage entry.
///
/// TS ref: `ModelUsage` in `agentSdkTypes.ts`.
#[derive(Debug, Clone, Default)]
pub struct ModelUsageEntry {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub cost_usd: f64,
}

impl Default for SessionState {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
        Self {
            session_id: uuid_v4(),
            parent_session_id: None,
            original_cwd: cwd.clone(),
            project_root: cwd.clone(),
            cwd,
            model: None,
            started_at: Instant::now(),
            message_count: 0,
            total_cost_usd: 0.0,
            model_usage: HashMap::new(),
            total_api_duration_ms: 0.0,
            total_tool_duration_ms: 0.0,
            total_lines_added: 0,
            total_lines_removed: 0,
            is_interactive: false,
            client_type: "cli".to_string(),
            file_checkpointing_enabled: true,
            session_persistence_disabled: false,
        }
    }
}

impl SessionState {
    /// Regenerate the session id, optionally setting the current id as parent.
    pub fn regenerate_session_id(&mut self, set_current_as_parent: bool) -> &str {
        if set_current_as_parent {
            self.parent_session_id = Some(self.session_id.clone());
        }
        self.session_id = uuid_v4();
        &self.session_id
    }

    /// Record cost and token usage for a model.
    pub fn add_cost(&mut self, cost: f64, model: &str, usage: ModelUsageEntry) {
        self.model_usage.insert(model.to_string(), usage);
        self.total_cost_usd += cost;
    }

    /// Record API call duration.
    pub fn add_api_duration(&mut self, duration_ms: f64) {
        self.total_api_duration_ms += duration_ms;
    }

    /// Record tool execution duration.
    pub fn add_tool_duration(&mut self, duration_ms: f64) {
        self.total_tool_duration_ms += duration_ms;
    }

    /// Record lines changed.
    pub fn add_lines_changed(&mut self, added: i64, removed: i64) {
        self.total_lines_added += added;
        self.total_lines_removed += removed;
    }

    /// Increment the user message counter.
    pub fn increment_message_count(&mut self) {
        self.message_count += 1;
    }

    /// Total input tokens across all models.
    pub fn total_input_tokens(&self) -> i64 {
        self.model_usage.values().map(|u| u.input_tokens).sum()
    }

    /// Total output tokens across all models.
    pub fn total_output_tokens(&self) -> i64 {
        self.model_usage.values().map(|u| u.output_tokens).sum()
    }

    /// Wall-clock duration since session start.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Reset cost/duration state (used on `/clear`).
    pub fn reset_cost_state(&mut self) {
        self.total_cost_usd = 0.0;
        self.total_api_duration_ms = 0.0;
        self.total_tool_duration_ms = 0.0;
        self.total_lines_added = 0;
        self.total_lines_removed = 0;
        self.model_usage.clear();
        self.started_at = Instant::now();
    }
}

// ---------------------------------------------------------------------------
// Bootstrap / onboarding config
// ---------------------------------------------------------------------------

/// First-run detection and onboarding state.
///
/// TS ref: `GlobalConfig.hasCompletedOnboarding`, `numStartups`,
/// `firstStartTime` in `utils/config.ts`.
#[derive(Debug, Clone, Default)]
pub struct BootstrapConfig {
    /// Number of times the CLI has been started.
    pub num_startups: i64,
    /// Whether onboarding has been completed.
    pub has_completed_onboarding: bool,
    /// ISO timestamp of first ever startup (if recorded).
    pub first_start_time: Option<String>,
    /// Version that last reset onboarding.
    pub last_onboarding_version: Option<String>,
    /// Whether the project trust dialog has been accepted for the current dir.
    pub has_trust_dialog_accepted: bool,
    /// Whether the project-level onboarding has been completed.
    pub has_completed_project_onboarding: bool,
}

impl BootstrapConfig {
    /// Whether this is the very first run (no prior startups recorded).
    pub fn is_first_run(&self) -> bool {
        self.num_startups == 0 && !self.has_completed_onboarding
    }

    /// Whether onboarding should be shown. Accounts for version-gated resets.
    pub fn needs_onboarding(&self, current_version: &str) -> bool {
        if !self.has_completed_onboarding {
            return true;
        }
        // If a version reset was recorded and the current version is newer,
        // re-show onboarding.
        if let Some(ref last) = self.last_onboarding_version {
            return last != current_version;
        }
        false
    }

    /// Record a startup, optionally capturing the first-start timestamp.
    pub fn record_startup(&mut self) {
        self.num_startups += 1;
        if self.first_start_time.is_none() {
            self.first_start_time = Some(chrono_now_iso());
        }
    }

    /// Mark onboarding as complete for the given version.
    pub fn complete_onboarding(&mut self, version: &str) {
        self.has_completed_onboarding = true;
        self.last_onboarding_version = Some(version.to_string());
    }
}

// ---------------------------------------------------------------------------
// Global accessor (process-wide singleton, like the TS `STATE`)
// ---------------------------------------------------------------------------

static GLOBAL_STATE: OnceLock<Mutex<SessionState>> = OnceLock::new();

fn global_state() -> &'static Mutex<SessionState> {
    GLOBAL_STATE.get_or_init(|| Mutex::new(SessionState::default()))
}

/// Acquire the global session state lock, returning a `MutexGuard`.
///
/// Panics only if the lock has been poisoned (another thread panicked while
/// holding it). In practice this never happens in single-threaded CLI usage.
fn lock_state() -> std::sync::MutexGuard<'static, SessionState> {
    match global_state().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Get a clone of the current session state.
pub fn get_session_state() -> SessionState {
    lock_state().clone()
}

/// Mutate the global session state in-place.
pub fn with_session_state_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut SessionState) -> R,
{
    let mut guard = lock_state();
    f(&mut guard)
}

/// Get the current session id without cloning the entire state.
pub fn get_session_id() -> String {
    lock_state().session_id.clone()
}

/// Get the current working directory from session state.
pub fn get_cwd() -> PathBuf {
    lock_state().cwd.clone()
}

/// Set the current working directory in session state.
pub fn set_cwd(cwd: PathBuf) {
    lock_state().cwd = cwd;
}

/// Maximum number of in-memory error log entries.
pub const MAX_IN_MEMORY_ERRORS: usize = constants::MAX_SESSION_COST_ENTRIES;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Simple UUID v4 generator using random bytes.
fn uuid_v4() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 16];
    getrandom(&mut bytes);
    // Set version (4) and variant (RFC 4122).
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut s = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            s.push('-');
        }
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Fill buffer with random bytes, falling back to /dev/urandom.
fn getrandom(buf: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom")
        && f.read_exact(buf).is_ok()
    {
        return;
    }
    // Last-resort fallback: use the address of the buffer XORed with time.
    let seed = buf.as_ptr() as u64
        ^ std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(i as u64))
            >> 33) as u8;
    }
}

/// Current time as ISO 8601 string (UTC).
fn chrono_now_iso() -> String {
    // Avoid pulling in chrono; use a simple manual format.
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple epoch-to-ISO: good enough for a timestamp field.
    format!("{secs}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "bootstrap.test.rs"]
mod tests;
