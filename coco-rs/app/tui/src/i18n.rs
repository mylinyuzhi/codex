//! Internationalization support.
//!
//! Uses `rust-i18n` with the `t!()` macro for all user-facing strings.
//! Locale files in `locales/{en.yaml, zh-CN.yaml}`.
//!
//! Usage:
//! ```ignore
//! use crate::i18n::t;
//! let text = t!("dialog.approve");
//! ```

rust_i18n::i18n!("locales", fallback = "en");

/// Re-export the `t!` macro for use across the crate.
///
/// All user-facing strings should use `t!("key")` instead of hardcoded English.
pub use rust_i18n::t;
