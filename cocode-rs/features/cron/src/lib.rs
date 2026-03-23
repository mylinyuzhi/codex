//! Cron scheduling system for cocode-rs.
//!
//! This crate provides the core scheduling layer for recurring and one-shot
//! task execution, aligned with Claude Code's loop/cron system. It includes:
//!
//! - **Types**: CronJob, CronJobStatus, CronFireEvent
//! - **Store**: Thread-safe job store with formatting helpers
//! - **Schedule**: Cron expression parsing and validation
//! - **Matcher**: Cron time matching engine
//! - **Jitter**: Thundering-herd prevention with configurable jitter
//! - **Persistence**: Durable job storage with missed one-shot detection
//! - **Lock**: Inter-process lock for multi-session coordination
//! - **Watcher**: File watcher for external task file changes
//! - **Scheduler**: Background 1-second tick scheduler with circuit breaker
//! - **Config**: Configurable limits, intervals, and jitter parameters
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                       cocode-cron                             │
//! ├──────────────────────────────────────────────────────────────┤
//! │  types    │ store   │ schedule │ matcher  │ jitter  │ config │
//! │  CronJob  │ Store   │ parse    │ matches  │ jitter  │ Cron   │
//! │  Status   │ (Arc+   │ validate │ fields   │ period  │ Config │
//! │  Event    │  Mutex) │          │          │         │        │
//! ├──────────────────────────────────────────────────────────────┤
//! │  persistence │ lock       │ watcher      │ scheduler         │
//! │  save/load   │ InterProc  │ TaskFile     │ CronScheduler     │
//! │  missed      │ Lock (PID) │ Watcher      │ (1s tick + CB)    │
//! └──────────────────────────────────────────────────────────────┘
//! ```

pub mod config;
pub mod error;
pub mod jitter;
pub mod lock;
pub mod matcher;
pub mod persistence;
pub mod schedule;
pub mod scheduler;
pub mod store;
pub mod types;
pub mod watcher;

// Re-export primary types for convenience.
pub use config::CronConfig;
pub use config::DEFAULT_MAX_JOBS;
pub use config::DEFAULT_RECURRING_EXPIRY_SECS;
pub use config::JitterConfig;
pub use error::CronError;
pub use error::Result;
pub use lock::InterProcessLock;
pub use matcher::matches_cron;
pub use persistence::MissedTask;
pub use persistence::detect_missed_oneshots;
pub use persistence::load_durable_jobs;
pub use persistence::save_durable_jobs;
pub use schedule::parse_schedule;
pub use schedule::validate_cron_expression;
pub use scheduler::CronFireEvent;
pub use scheduler::CronScheduler;
pub use store::CronJobStore;
pub use store::format_cron_summary;
pub use store::jobs_to_value;
pub use store::new_cron_store;
pub use types::CronJob;
pub use types::CronJobStatus;
pub use types::generate_cron_id;
pub use watcher::TaskFileWatcher;
