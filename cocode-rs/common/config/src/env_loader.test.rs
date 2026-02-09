use super::*;
use std::env;

// Helper to set and cleanup env vars in tests
struct EnvGuard {
    keys: Vec<String>,
}

impl EnvGuard {
    fn new() -> Self {
        Self { keys: Vec::new() }
    }

    fn set(&mut self, key: &str, value: &str) {
        self.keys.push(key.to_string());
        // SAFETY: These are test-only operations with controlled keys
        unsafe { env::set_var(key, value) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for key in &self.keys {
            // SAFETY: Cleaning up test environment variables
            unsafe { env::remove_var(key) };
        }
    }
}

#[test]
fn test_load_tool_config_default() {
    let loader = EnvLoader::new();
    let config = loader.load_tool_config();
    assert_eq!(
        config.max_tool_concurrency,
        cocode_protocol::DEFAULT_MAX_TOOL_CONCURRENCY
    );
    assert!(config.mcp_tool_timeout.is_none());
}

#[test]
fn test_load_tool_config_from_env() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_MAX_TOOL_CONCURRENCY, "5");
    guard.set(ENV_MCP_TOOL_TIMEOUT, "30000");

    let loader = EnvLoader::new();
    let config = loader.load_tool_config();
    assert_eq!(config.max_tool_concurrency, 5);
    assert_eq!(config.mcp_tool_timeout, Some(30000));
}

#[test]
fn test_load_compact_config_from_env() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_DISABLE_COMPACT, "true");
    guard.set(ENV_AUTOCOMPACT_PCT, "80");
    guard.set(ENV_SESSION_MEMORY_MIN, "20000");

    let loader = EnvLoader::new();
    let config = loader.load_compact_config();
    assert!(config.disable_compact);
    assert_eq!(config.auto_compact_pct, Some(80));
    assert_eq!(config.session_memory_min_tokens, 20000);
}

#[test]
fn test_load_compact_config_extended_fields() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_MIN_TOKENS_TO_PRESERVE, "15000");
    guard.set(ENV_MICRO_COMPACT_MIN_SAVINGS, "25000");
    guard.set(ENV_MAX_SUMMARY_RETRIES, "3");
    guard.set(ENV_TOKEN_SAFETY_MARGIN, "1.5");
    guard.set(ENV_RECENT_TOOL_RESULTS_TO_KEEP, "5");

    let loader = EnvLoader::new();
    let config = loader.load_compact_config();
    assert_eq!(config.min_tokens_to_preserve, 15000);
    assert_eq!(config.micro_compact_min_savings, 25000);
    assert_eq!(config.max_summary_retries, 3);
    assert!((config.token_safety_margin - 1.5).abs() < f64::EPSILON);
    assert_eq!(config.recent_tool_results_to_keep, 5);
}

#[test]
fn test_load_plan_config_from_env() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_PLAN_AGENT_COUNT, "3");
    guard.set(ENV_PLAN_EXPLORE_AGENT_COUNT, "4");

    let loader = EnvLoader::new();
    let config = loader.load_plan_config();
    assert_eq!(config.agent_count, 3);
    assert_eq!(config.explore_agent_count, 4);
}

#[test]
fn test_load_plan_config_clamps_values() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_PLAN_AGENT_COUNT, "100"); // Too high
    guard.set(ENV_PLAN_EXPLORE_AGENT_COUNT, "-5"); // Too low

    let loader = EnvLoader::new();
    let config = loader.load_plan_config();
    assert_eq!(config.agent_count, cocode_protocol::MAX_AGENT_COUNT);
    assert_eq!(config.explore_agent_count, cocode_protocol::MIN_AGENT_COUNT);
}

#[test]
fn test_load_attachment_config_from_env() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_DISABLE_ATTACHMENTS, "yes");
    guard.set(ENV_ENABLE_TOKEN_USAGE, "true");

    let loader = EnvLoader::new();
    let config = loader.load_attachment_config();
    assert!(config.disable_attachments);
    assert!(config.enable_token_usage_attachment);
}

#[test]
fn test_load_path_config_from_env() {
    let mut guard = EnvGuard::new();
    guard.set(ENV_PROJECT_DIR, "/project");
    guard.set(ENV_PLUGIN_ROOT, "/plugins");
    guard.set(ENV_ENV_FILE, "/.env");

    let loader = EnvLoader::new();
    let config = loader.load_path_config();
    assert_eq!(config.project_dir, Some(PathBuf::from("/project")));
    assert_eq!(config.plugin_root, Some(PathBuf::from("/plugins")));
    assert_eq!(config.env_file, Some(PathBuf::from("/.env")));
}

#[test]
fn test_bool_parsing() {
    let loader = EnvLoader::new();

    let mut guard = EnvGuard::new();
    guard.set("TEST_BOOL_1", "1");
    guard.set("TEST_BOOL_TRUE", "true");
    guard.set("TEST_BOOL_YES", "YES");
    guard.set("TEST_BOOL_FALSE", "false");
    guard.set("TEST_BOOL_NO", "no");

    assert!(loader.get_bool("TEST_BOOL_1"));
    assert!(loader.get_bool("TEST_BOOL_TRUE"));
    assert!(loader.get_bool("TEST_BOOL_YES"));
    assert!(!loader.get_bool("TEST_BOOL_FALSE"));
    assert!(!loader.get_bool("TEST_BOOL_NO"));
    assert!(!loader.get_bool("TEST_BOOL_NONEXISTENT"));
}
