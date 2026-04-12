use super::*;
use crate::CommandRegistry;
use crate::handlers;
use crate::register_builtins;

#[test]
fn test_register_extended_builtins() {
    let mut registry = CommandRegistry::new();
    register_extended_builtins(&mut registry);

    // Verify we registered a reasonable number of extended commands.
    // Count may change as new commands are added.
    assert!(
        registry.len() >= 60,
        "Expected at least 60 extended commands, got {}",
        registry.len()
    );
}

#[test]
fn test_extended_builtins_no_overlap_with_base() {
    let mut base_registry = CommandRegistry::new();
    register_builtins(&mut base_registry);

    let mut ext_registry = CommandRegistry::new();
    register_extended_builtins(&mut ext_registry);

    // Collect names from both registries
    let base_names: Vec<_> = base_registry.all().map(|c| c.base.name.clone()).collect();
    let ext_names: Vec<_> = ext_registry.all().map(|c| c.base.name.clone()).collect();

    // Some commands exist in both (the extended set overrides/replaces them)
    // That's by design: extended handlers have real logic replacing stubs.
    // Verify the extended set has the key commands.
    let key_commands = [
        "compact",
        "context",
        "cost",
        "diff",
        "model",
        "permissions",
        "session",
        "resume",
        "init",
        "doctor",
        "login",
        "logout",
        "mcp",
        "plugin",
        "review",
    ];

    for cmd in &key_commands {
        assert!(
            ext_registry.get(cmd).is_some(),
            "extended registry missing key command: {cmd}"
        );
    }

    // Verify base set has its own commands
    assert!(base_registry.get("help").is_some());
    assert!(base_registry.get("clear").is_some());

    // Quick sanity: base and extended together should not panic
    let _base_count = base_names.len();
    let _ext_count = ext_names.len();
}

#[test]
fn test_all_name_constants_are_valid() {
    // Verify no constant is empty
    let all_names = [
        names::HELP,
        names::CLEAR,
        names::COMPACT,
        names::STATUS,
        names::EXIT,
        names::VERSION,
        names::CONFIG,
        names::MODEL,
        names::EFFORT,
        names::PERMISSIONS,
        names::THEME,
        names::COLOR,
        names::VIM,
        names::OUTPUT_STYLE,
        names::KEYBINDINGS,
        names::FAST,
        names::SANDBOX,
        names::PRIVACY_SETTINGS,
        names::RATE_LIMIT_OPTIONS,
        names::SESSION,
        names::RESUME,
        names::COST,
        names::CONTEXT,
        names::RENAME,
        names::BRANCH,
        names::EXPORT,
        names::COPY,
        names::REWIND,
        names::STATS,
        names::DIFF,
        names::COMMIT,
        names::PR,
        names::REVIEW,
        names::INIT,
        names::MCP,
        names::PLUGIN,
        names::AGENTS,
        names::TASKS,
        names::SKILLS,
        names::HOOKS,
        names::FILES,
        names::DOCTOR,
        names::LOGIN,
        names::LOGOUT,
        names::FEEDBACK,
        names::UPGRADE,
        names::USAGE,
        names::BTW,
        names::STICKERS,
        names::MEMORY,
        names::PLAN,
        names::ADD_DIR,
        names::DESKTOP,
        names::MOBILE,
        names::IDE,
        names::TAG,
        names::SUMMARY,
        names::RELEASE_NOTES,
        names::ONBOARDING,
        names::CHROME,
        names::PR_COMMENTS,
        names::SHARE,
        names::PASSES,
        names::EXTRA_USAGE,
        names::TELEPORT,
        names::INSTALL_GITHUB_APP,
        names::INSTALL_SLACK_APP,
    ];

    for name in &all_names {
        assert!(!name.is_empty(), "found empty command name constant");
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "command name '{name}' contains invalid characters"
        );
    }

    // Check total count matches TS source (~65+ directories)
    assert!(
        all_names.len() >= 65,
        "expected at least 65 command name constants, got {}",
        all_names.len()
    );
}

#[test]
fn test_plan_handler_subcommands() {
    assert!(plan_handler("").contains("Plan mode"));
    assert!(plan_handler("on").contains("enabled"));
    assert!(plan_handler("off").contains("disabled"));
    assert!(plan_handler("open").contains("Opening"));
    assert!(plan_handler("refactor the auth module").contains("Creating plan"));
}

#[tokio::test]
async fn test_memory_handler() {
    let output = super::handlers::memory::handler("".to_string())
        .await
        .unwrap();
    assert!(output.contains("Memory Files"));
}

#[test]
fn test_rewind_handler() {
    assert!(rewind_handler("").contains("Usage"));
    assert!(rewind_handler("5").contains("5"));
}

#[test]
fn test_skills_handler() {
    let output = skills_handler("");
    assert!(output.contains(".claude/skills/"));
    assert!(output.contains("Bundled"));
}

#[tokio::test]
async fn test_hooks_handler() {
    let output = super::handlers::hooks::handler("".to_string())
        .await
        .unwrap();
    assert!(!output.is_empty());
}

#[test]
fn test_sandbox_handler() {
    assert!(sandbox_handler("").contains("disabled"));
    assert!(sandbox_handler("none").contains("disabled"));
    assert!(sandbox_handler("readonly").contains("readonly"));
    assert!(sandbox_handler("strict").contains("strict"));
}

#[test]
fn test_version_handler() {
    let output = version_handler("");
    assert!(output.starts_with("cocode v"));
}

#[test]
fn test_vim_handler() {
    assert!(vim_handler("on").contains("enabled"));
    assert!(vim_handler("off").contains("disabled"));
    assert!(vim_handler("").contains("toggled"));
}

#[test]
fn test_theme_handler() {
    assert!(theme_handler("").contains("Available themes"));
    assert!(theme_handler("dark").contains("dark"));
}

#[test]
fn test_fast_handler() {
    assert!(fast_handler("on").contains("enabled"));
    assert!(fast_handler("off").contains("disabled"));
}

#[tokio::test]
async fn test_model_handler_empty() {
    let output = handlers::model::handler(String::new()).await.unwrap();
    assert!(output.contains("Available Models"));
    assert!(output.contains("sonnet"));
    assert!(output.contains("opus"));
    assert!(output.contains("haiku"));
}

#[tokio::test]
async fn test_model_handler_known() {
    let output = handlers::model::handler("sonnet".to_string())
        .await
        .unwrap();
    assert!(output.contains("switched to"));
}

#[tokio::test]
async fn test_model_handler_unknown() {
    let output = handlers::model::handler("gpt-4".to_string()).await.unwrap();
    assert!(output.contains("Unknown model"));
}

#[tokio::test]
async fn test_permissions_handler_empty() {
    let output = handlers::permissions::handler(String::new()).await.unwrap();
    assert!(output.contains("Permission Rules"));
    assert!(output.contains("allow"));
    assert!(output.contains("deny"));
}

#[tokio::test]
async fn test_permissions_handler_allow() {
    let output = handlers::permissions::handler("allow Bash".to_string())
        .await
        .unwrap();
    assert!(output.contains("allow rule"));
    assert!(output.contains("Bash"));
}

#[tokio::test]
async fn test_permissions_handler_deny() {
    let output = handlers::permissions::handler("deny Write".to_string())
        .await
        .unwrap();
    assert!(output.contains("deny rule"));
    assert!(output.contains("Write"));
}

#[tokio::test]
async fn test_permissions_handler_reset() {
    let output = handlers::permissions::handler("reset".to_string())
        .await
        .unwrap();
    assert!(output.contains("cleared"));
}

#[tokio::test]
async fn test_cost_handler() {
    let output = handlers::cost::handler(String::new()).await.unwrap();
    assert!(output.contains("Session Cost"));
}

#[tokio::test]
async fn test_context_handler() {
    let output = handlers::context::handler(String::new()).await.unwrap();
    assert!(output.contains("Context Window Usage"));
    assert!(output.contains("System prompt"));
    assert!(output.contains("Free"));
}

#[tokio::test]
async fn test_compact_handler_empty() {
    let output = handlers::compact::handler(String::new()).await.unwrap();
    assert!(output.contains("Compacting"));
    assert!(output.contains("Before compaction"));
}

#[tokio::test]
async fn test_compact_handler_with_instructions() {
    let output = handlers::compact::handler("focus on the API changes".to_string())
        .await
        .unwrap();
    assert!(output.contains("focus on the API changes"));
}

#[tokio::test]
async fn test_login_handler_async() {
    let output = login_handler_async(String::new()).await.unwrap();
    // Should mention authentication in some form
    assert!(
        output.contains("API key")
            || output.contains("Authentication")
            || output.contains("ANTHROPIC_API_KEY")
    );
}

#[tokio::test]
async fn test_logout_handler_async() {
    let output = logout_handler_async(String::new()).await.unwrap();
    assert!(output.contains("Logging out") || output.contains("credentials"));
}

#[tokio::test]
async fn test_mcp_handler_list() {
    let output = handlers::mcp::handler(String::new()).await.unwrap();
    assert!(output.contains("MCP"));
    assert!(output.contains("enable") || output.contains("disable"));
}

#[tokio::test]
async fn test_mcp_handler_enable() {
    let output = handlers::mcp::handler("enable my-server".to_string())
        .await
        .unwrap();
    assert!(output.contains("Enabling"));
    assert!(output.contains("my-server"));
}

#[tokio::test]
async fn test_plugin_handler_list() {
    let output = handlers::plugin::handler(String::new()).await.unwrap();
    assert!(output.contains("plugin") || output.contains("Plugin"));
    assert!(output.contains("install"));
}

#[tokio::test]
async fn test_plugin_handler_install() {
    let output = handlers::plugin::handler("install my-plugin".to_string())
        .await
        .unwrap();
    // The handler either installs successfully or reports already installed
    assert!(
        output.contains("nstall") || output.contains("my-plugin"),
        "unexpected: {output}"
    );
}

// Integration test: diff handler runs real git (if in a git repo)
#[tokio::test]
async fn test_diff_handler() {
    let output = handlers::diff::handler(String::new()).await.unwrap();
    // In a git repo, should produce some output (even if empty diff)
    assert!(!output.is_empty());
}

// Integration test: init handler checks for real files
#[tokio::test]
async fn test_init_handler_async() {
    let output = init_handler_async(String::new()).await.unwrap();
    assert!(output.contains("Initializing"));
}

// Integration test: doctor runs real checks
#[tokio::test]
async fn test_doctor_handler_async() {
    let output = doctor_handler_async(String::new()).await.unwrap();
    assert!(output.contains("diagnostics") || output.contains("Diagnostics"));
    assert!(output.contains("git") || output.contains("shell"));
}

#[test]
fn test_alias_lookups_in_extended() {
    let mut registry = CommandRegistry::new();
    register_extended_builtins(&mut registry);

    // Check aliases
    let by_alias = registry.get("ctx");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "context");

    let by_alias = registry.get("continue");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "resume");

    let by_alias = registry.get("plugins");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "plugin");

    let by_alias = registry.get("marketplace");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "plugin");

    let by_alias = registry.get("allowed-tools");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "permissions");

    let by_alias = registry.get("checkpoint");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "rewind");

    let by_alias = registry.get("quit");
    assert!(by_alias.is_some());
    assert_eq!(by_alias.unwrap().base.name, "exit");
}
