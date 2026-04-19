//! Internationalization (i18n) support for the TUI.
//!
//! Uses `rust-i18n` with the `t!()` macro for all user-facing strings.
//! Supported locales: English (`en`, default) and Simplified Chinese (`zh-CN`).
//! Locale files live in `locales/{en,zh-CN}.yaml`.
//!
//! Locale detection priority:
//! 1. `COCO_LANG` — app-specific override
//! 2. `LANG` — standard Unix locale
//! 3. `LC_ALL` — alternative Unix locale
//! 4. Fallback to `en`
//!
//! Call [`init`] once at application startup before rendering anything.

// Note: the `i18n!()` macro is invoked at the crate root (lib.rs) so the
// generated `_rust_i18n_t` symbol is visible to every module's `t!()` call.

pub use rust_i18n::t;

/// Initialize the i18n system with locale detection from env vars.
pub fn init() {
    let locale = detect_locale();
    rust_i18n::set_locale(locale);
    tracing::debug!(locale, "i18n initialized");
}

/// Return the currently active locale.
pub fn current_locale() -> String {
    rust_i18n::locale().to_string()
}

/// Set the active locale explicitly.
pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

fn detect_locale() -> &'static str {
    for var in ["COCO_LANG", "LANG", "LC_ALL"] {
        if let Ok(value) = std::env::var(var)
            && let Some(locale) = parse_locale(&value)
        {
            return locale;
        }
    }
    "en"
}

fn parse_locale(locale: &str) -> Option<&'static str> {
    let normalized = locale.to_lowercase().replace('_', "-");
    let lang_part = normalized.split('.').next().unwrap_or(&normalized);

    match lang_part {
        s if s.starts_with("zh-cn") || s.starts_with("zh-hans") => Some("zh-CN"),
        s if s.starts_with("zh") => Some("zh-CN"),
        s if s.starts_with("en") => Some("en"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
