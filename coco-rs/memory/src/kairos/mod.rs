//! KAIROS assistant-mode features.
//!
//! TS: `memdir/paths.ts:246-251` (`getAutoMemDailyLogPath`) + the
//! daily-log append protocol from `memdir.ts:327-348`. The runtime
//! gate / `/dream` skill trigger live in
//! `services/autoDream/autoDream.ts`; the lock itself is already
//! ported under [`crate::lock`] with full PID+mtime CAS parity.
//!
//! Scope of this module: pure path resolution + append helpers for
//! the daily log file. Auto-trigger / scheduler logic lives in
//! [`crate::service::dream`].

pub mod daily_log;
pub mod rollover;

pub use daily_log::DailyLogStore;
pub use daily_log::daily_log_path;
pub use rollover::KairosRolloverWatcher;
