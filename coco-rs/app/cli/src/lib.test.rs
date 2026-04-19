use clap::Parser;

use super::Cli;

/// PR-E3 flags should parse without colliding with pre-existing flags.
/// Verifies every new flag round-trips through clap parsing.
#[test]
fn parses_all_pr_e3_flags() {
    let args = [
        "coco",
        "--input-format",
        "stream-json",
        "--json-schema",
        "/tmp/schema.json",
        "--replay-user-messages",
        "--include-hook-events",
        "--include-partial-messages",
        "--thinking",
        "adaptive",
        "--max-thinking-tokens",
        "8192",
        "--append-system-prompt-file",
        "/tmp/extra.md",
        "--strict-mcp-config",
        "--setting-sources",
        "user,project",
        "--fork-session",
        "--betas",
        "prompt-caching-2024-07-31",
        "--session-id",
        "11111111-2222-3333-4444-555555555555",
        "--permission-prompt-tool",
        "mcp__approval__prompt",
    ];
    let cli = Cli::try_parse_from(args).expect("parse pr-e3 flags");

    assert_eq!(cli.input_format.as_deref(), Some("stream-json"));
    assert_eq!(cli.json_schema.as_deref(), Some("/tmp/schema.json"));
    assert!(cli.replay_user_messages);
    assert!(cli.include_hook_events);
    assert!(cli.include_partial_messages);
    assert_eq!(cli.thinking.as_deref(), Some("adaptive"));
    assert_eq!(cli.max_thinking_tokens, Some(8192));
    assert_eq!(
        cli.append_system_prompt_file.as_deref(),
        Some("/tmp/extra.md")
    );
    assert!(cli.strict_mcp_config);
    assert_eq!(cli.setting_sources.as_deref(), Some("user,project"));
    assert!(cli.fork_session);
    assert_eq!(cli.betas.as_deref(), Some("prompt-caching-2024-07-31"));
    assert_eq!(
        cli.session_id.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(
        cli.permission_prompt_tool.as_deref(),
        Some("mcp__approval__prompt")
    );
}

/// Existing flags must still parse; new additions must not change existing
/// defaults or argument parsing for pre-PR-E3 flags.
#[test]
fn pr_e3_defaults_leave_existing_flags_untouched() {
    let cli = Cli::try_parse_from(["coco"]).expect("parse no-arg");
    assert!(cli.input_format.is_none());
    assert!(cli.json_schema.is_none());
    assert!(!cli.replay_user_messages);
    assert!(!cli.include_hook_events);
    assert!(!cli.include_partial_messages);
    assert!(cli.thinking.is_none());
    assert!(cli.max_thinking_tokens.is_none());
    assert!(cli.append_system_prompt_file.is_none());
    assert!(!cli.strict_mcp_config);
    assert!(cli.setting_sources.is_none());
    assert!(!cli.fork_session);
    assert!(cli.betas.is_none());
    assert!(cli.session_id.is_none());
    assert!(cli.permission_prompt_tool.is_none());
}
