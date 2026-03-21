use std::sync::Mutex;

use cocode_protocol::AutoMemoryConfig;

use super::*;

/// Mutex to serialize env var tests. `set_var`/`remove_var` are
/// process-global, so parallel tests that modify them would race.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Helper: run a closure with env vars set, then restore originals.
///
/// SAFETY: `set_var`/`remove_var` are unsafe in Rust 2024 due to thread safety.
/// These tests must run with `--test-threads=1` to avoid data races.
fn with_env_vars<F, R>(vars: &[(&str, Option<&str>)], f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Save originals
    let originals: Vec<_> = vars
        .iter()
        .map(|(k, _)| (*k, std::env::var(k).ok()))
        .collect();

    // Set/remove — safe in single-threaded test context
    for (k, v) in vars {
        // SAFETY: Tests are run with --test-threads=1 so no concurrent access.
        match v {
            Some(val) => unsafe { std::env::set_var(k, val) },
            None => unsafe { std::env::remove_var(k) },
        }
    }

    let result = f();

    // Restore
    for (k, original) in originals {
        // SAFETY: Same as above — single-threaded test context.
        match original {
            Some(val) => unsafe { std::env::set_var(k, val) },
            None => unsafe { std::env::remove_var(k) },
        }
    }

    result
}

/// All env vars that `resolve_enabled` reads. Cleared before each test
/// to ensure a clean environment regardless of parallel execution order.
const CLEAN_ENV: &[(&str, Option<&str>)] = &[
    ("COCODE_DISABLE_AUTO_MEMORY", None),
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
    ("COCODE_REMOTE", None),
    ("CLAUDE_CODE_REMOTE", None),
    ("COCODE_REMOTE_MEMORY_DIR", None),
    ("CLAUDE_CODE_REMOTE_MEMORY_DIR", None),
];

#[test]
fn test_feature_disabled_returns_false() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig::default();
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            false,
            false,
            false,
        );
        assert!(!resolved.enabled);
        assert_eq!(
            resolved.disable_reason,
            Some(DisableReason::FeatureDisabled)
        );
    });
}

#[test]
fn test_feature_enabled_returns_true() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig::default();
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            true,
            false,
            false,
        );
        assert!(resolved.enabled);
        assert_eq!(resolved.disable_reason, None);
    });
}

#[test]
fn test_user_setting_overrides_feature_flag() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig {
            enabled: Some(true),
            ..Default::default()
        };
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            false,
            false,
            false,
        );
        assert!(resolved.enabled);
    });
}

#[test]
fn test_user_setting_disable_overrides_feature_flag() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig {
            enabled: Some(false),
            ..Default::default()
        };
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            true,
            false,
            false,
        );
        assert!(!resolved.enabled);
        assert_eq!(resolved.disable_reason, Some(DisableReason::UserSetting));
    });
}

#[test]
fn test_default_limits() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig::default();
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            true,
            false,
            false,
        );
        assert_eq!(resolved.max_lines, 200);
        assert_eq!(resolved.max_relevant_files, 5);
        assert_eq!(resolved.max_lines_per_file, 200);
    });
}

#[test]
fn test_relevant_memories_flag_propagated() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig::default();
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            true,
            true,
            false,
        );
        assert!(resolved.enabled);
        assert!(resolved.relevant_memories_enabled);
        assert!(!resolved.memory_extraction_enabled);
    });
}

#[test]
fn test_memory_extraction_flag_propagated() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig::default();
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            true,
            false,
            true,
        );
        assert!(resolved.enabled);
        assert!(!resolved.relevant_memories_enabled);
        assert!(resolved.memory_extraction_enabled);
    });
}

#[test]
fn test_sub_flags_disabled_when_auto_memory_disabled() {
    with_env_vars(CLEAN_ENV, || {
        let config = AutoMemoryConfig::default();
        let resolved = resolve_auto_memory_config(
            std::path::Path::new("/tmp/test"),
            &config,
            false,
            true,
            true,
        );
        assert!(!resolved.enabled);
        assert!(!resolved.relevant_memories_enabled);
        assert!(!resolved.memory_extraction_enabled);
    });
}

// === Environment variable priority chain tests ===
// These tests must not run in parallel since env vars are process-global.

#[test]
fn test_env_var_disable_overrides_everything() {
    with_env_vars(
        &[
            ("COCODE_DISABLE_AUTO_MEMORY", Some("1")),
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
            ("COCODE_REMOTE", None),
        ],
        || {
            let config = AutoMemoryConfig {
                enabled: Some(true), // user says enable
                ..Default::default()
            };
            let resolved = resolve_auto_memory_config(
                std::path::Path::new("/tmp/test"),
                &config,
                true, // feature flag says enable
                false,
                false,
            );
            assert!(
                !resolved.enabled,
                "Env var disable should override user setting and feature flag"
            );
            assert_eq!(resolved.disable_reason, Some(DisableReason::EnvVar));
        },
    );
}

#[test]
fn test_env_var_enable_overrides_feature_flag() {
    with_env_vars(
        &[
            ("COCODE_DISABLE_AUTO_MEMORY", Some("0")),
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
            ("COCODE_REMOTE", None),
        ],
        || {
            let config = AutoMemoryConfig::default(); // no user setting
            let resolved = resolve_auto_memory_config(
                std::path::Path::new("/tmp/test"),
                &config,
                false, // feature flag says disable
                false,
                false,
            );
            assert!(
                resolved.enabled,
                "Env var =0 should enable even when feature flag is off"
            );
        },
    );
}

#[test]
fn test_env_var_truthy_variants() {
    for val in &["1", "true", "True", "TRUE", "yes", "Yes", "YES"] {
        with_env_vars(
            &[
                ("COCODE_DISABLE_AUTO_MEMORY", Some(val)),
                ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
                ("COCODE_REMOTE", None),
            ],
            || {
                let config = AutoMemoryConfig::default();
                let resolved = resolve_auto_memory_config(
                    std::path::Path::new("/tmp/test"),
                    &config,
                    true,
                    false,
                    false,
                );
                assert!(
                    !resolved.enabled,
                    "Env var '{val}' should be truthy (disable)"
                );
            },
        );
    }
}

#[test]
fn test_env_var_falsy_variants() {
    for val in &["0", "false", "False", "FALSE", "no", "No", "NO"] {
        with_env_vars(
            &[
                ("COCODE_DISABLE_AUTO_MEMORY", Some(val)),
                ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
                ("COCODE_REMOTE", None),
            ],
            || {
                let config = AutoMemoryConfig::default();
                let resolved = resolve_auto_memory_config(
                    std::path::Path::new("/tmp/test"),
                    &config,
                    false,
                    false,
                    false,
                );
                assert!(resolved.enabled, "Env var '{val}' should be falsy (enable)");
            },
        );
    }
}

#[test]
fn test_compat_prefix_disable() {
    with_env_vars(
        &[
            ("COCODE_DISABLE_AUTO_MEMORY", None),
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", Some("true")),
            ("COCODE_REMOTE", None),
        ],
        || {
            let config = AutoMemoryConfig::default();
            let resolved = resolve_auto_memory_config(
                std::path::Path::new("/tmp/test"),
                &config,
                true,
                false,
                false,
            );
            assert!(
                !resolved.enabled,
                "CLAUDE_CODE_ compat prefix should also disable"
            );
            assert_eq!(resolved.disable_reason, Some(DisableReason::EnvVar));
        },
    );
}

#[test]
fn test_remote_mode_without_memory_dir_disables() {
    with_env_vars(
        &[
            ("COCODE_DISABLE_AUTO_MEMORY", None),
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
            ("COCODE_REMOTE", Some("1")),
            ("CLAUDE_CODE_REMOTE", None),
            ("COCODE_REMOTE_MEMORY_DIR", None),
            ("CLAUDE_CODE_REMOTE_MEMORY_DIR", None),
        ],
        || {
            let config = AutoMemoryConfig::default();
            let resolved = resolve_auto_memory_config(
                std::path::Path::new("/tmp/test"),
                &config,
                true,
                false,
                false,
            );
            assert!(
                !resolved.enabled,
                "Remote mode without memory dir should disable"
            );
            assert_eq!(resolved.disable_reason, Some(DisableReason::RemoteNoDir));
        },
    );
}

#[test]
fn test_remote_mode_with_memory_dir_allows() {
    with_env_vars(
        &[
            ("COCODE_DISABLE_AUTO_MEMORY", None),
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None),
            ("COCODE_REMOTE", Some("1")),
            ("CLAUDE_CODE_REMOTE", None),
            ("COCODE_REMOTE_MEMORY_DIR", Some("/remote/memory")),
            ("CLAUDE_CODE_REMOTE_MEMORY_DIR", None),
        ],
        || {
            let config = AutoMemoryConfig::default();
            let resolved = resolve_auto_memory_config(
                std::path::Path::new("/tmp/test"),
                &config,
                true,
                false,
                false,
            );
            assert!(resolved.enabled, "Remote mode with memory dir should allow");
        },
    );
}
