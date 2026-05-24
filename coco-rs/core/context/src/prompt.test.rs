use super::*;
use crate::EnvironmentInfo;
use crate::Platform;
use crate::ShellKind;

fn empty_env() -> EnvironmentInfo {
    EnvironmentInfo {
        platform: Platform::Linux,
        shell: ShellKind::Bash,
        cwd: "/tmp".to_string(),
        os_version: String::new(),
        model: String::new(),
        knowledge_cutoff: String::new(),
        is_git_repo: false,
        git_status: None,
    }
}

fn env_for_snapshot() -> EnvironmentInfo {
    EnvironmentInfo {
        platform: Platform::Darwin,
        shell: ShellKind::Zsh,
        cwd: "/repo".to_string(),
        os_version: "Darwin 24.0.0".to_string(),
        model: "claude-opus-4-7".to_string(),
        knowledge_cutoff: "January 2026".to_string(),
        is_git_repo: true,
        git_status: None,
    }
}

#[test]
fn no_output_style_yields_no_section() {
    let env = empty_env();
    let prompt = build_system_prompt("identity", &[], &env, None, None, None, None, &[]);
    let text = prompt.full_text();
    assert!(!text.contains("# Output Style"));
}

#[test]
fn output_style_block_renders_after_identity() {
    let env = empty_env();
    let style = OutputStyleSection {
        name: "Explanatory",
        prompt: "Explain choices.",
        keep_coding_instructions: true,
    };
    let prompt = build_system_prompt(
        "identity-line",
        &[],
        &env,
        None,
        None,
        None,
        Some(style),
        &[],
    );
    let text = prompt.full_text();
    let idx_identity = text.find("identity-line").unwrap();
    let idx_style = text.find("# Output Style: Explanatory").unwrap();
    let idx_env = text.find("<env>").unwrap();
    assert!(idx_identity < idx_style);
    assert!(idx_style < idx_env);
    assert!(text.contains("Explain choices."));
}

#[test]
fn output_style_uses_namespaced_plugin_name() {
    let env = empty_env();
    let style = OutputStyleSection {
        name: "alpha:concise",
        prompt: "Be very brief.",
        keep_coding_instructions: false,
    };
    let prompt = build_system_prompt("ID", &[], &env, None, None, None, Some(style), &[]);
    let text = prompt.full_text();
    assert!(text.contains("# Output Style: alpha:concise"));
}

#[test]
fn cache_breakpoint_falls_after_output_style() {
    let env = empty_env();
    let style = OutputStyleSection {
        name: "Learning",
        prompt: "Hands-on.",
        keep_coding_instructions: true,
    };
    let prompt = build_system_prompt("ID", &[], &env, None, None, None, Some(style), &[]);
    let mut prev_text: Option<&str> = None;
    let mut hit_cb = false;
    for block in &prompt.blocks {
        match block {
            SystemPromptBlock::Text { content } => prev_text = Some(content),
            SystemPromptBlock::CacheBreakpoint => {
                hit_cb = true;
                break;
            }
        }
    }
    assert!(hit_cb, "expected at least one cache breakpoint");
    assert!(prev_text.is_some());
    assert!(prev_text.unwrap().contains("# Output Style: Learning"));
}

/// G6 regression: AGENT_NOTES (passed via `notes_after_env`) must render
/// BEFORE the memory section, mirroring TS
/// `enhanceSystemPromptWithEnvDetails` (where `notes` come bundled with
/// the env block, not after memory).
#[test]
fn notes_after_env_renders_before_memory() {
    let env = empty_env();
    let prompt = build_system_prompt(
        "ID",
        &[],
        &env,
        /*skill_listing*/ None,
        /*memory_section*/ Some("MEMORY-MARKER"),
        /*notes_after_env*/ Some("NOTES-MARKER"),
        /*output_style*/ None,
        &[],
    );
    let text = prompt.full_text();
    let idx_env = text.find("<env>").expect("env block missing");
    let idx_notes = text.find("NOTES-MARKER").expect("notes missing");
    let idx_memory = text.find("MEMORY-MARKER").expect("memory missing");
    assert!(idx_env < idx_notes, "notes must come after env");
    assert!(idx_notes < idx_memory, "notes must come BEFORE memory");
}

/// Knowledge-cutoff line gracefully omits when the model is unknown
/// (delegation to `coco-model-card` is the substring-matching fix).
#[test]
fn unknown_model_omits_knowledge_cutoff_line() {
    let env = EnvironmentInfo {
        platform: Platform::Linux,
        shell: ShellKind::Bash,
        cwd: "/tmp".into(),
        os_version: "Linux".into(),
        model: "future-model-unreleased".into(),
        knowledge_cutoff: String::new(), // empty → omitted by render_env_block
        is_git_repo: false,
        git_status: None,
    };
    let prompt = build_system_prompt("ID", &[], &env, None, None, None, None, &[]);
    let text = prompt.full_text();
    assert!(
        !text.contains("knowledge cutoff"),
        "unknown model should not render a cutoff line; got: {text}"
    );
}

/// Known model renders the cutoff string verbatim.
#[test]
fn known_model_renders_knowledge_cutoff() {
    let env = env_for_snapshot();
    let prompt = build_system_prompt("ID", &[], &env, None, None, None, None, &[]);
    let text = prompt.full_text();
    assert!(text.contains("Assistant knowledge cutoff is January 2026."));
}

/// G9 snapshot test: full byte-level capture of a typical subagent
/// system prompt with all sections present. Locks in the env block
/// shape, AGENT_NOTES placement, and ordering vs TS.
#[test]
fn snapshot_subagent_full_prompt() {
    let env = env_for_snapshot();
    let prompt = build_system_prompt(
        "You are a focused exploration agent.",
        &[],
        &env,
        /*skill_listing*/ Some("- review: do a review"),
        /*memory_section*/ Some("# Persistent Agent Memory\n\nYour MEMORY.md is empty."),
        /*notes_after_env*/ Some(AGENT_NOTES),
        /*output_style*/ None,
        /*additional_working_directories*/ &["/repo/extras".to_string()],
    );
    insta::assert_snapshot!(prompt.full_text());
}

/// Main-agent prompt (no AGENT_NOTES — TS parity).
#[test]
fn snapshot_main_agent_prompt() {
    let env = env_for_snapshot();
    let prompt = build_system_prompt(
        "You are Claude Code.",
        &[],
        &env,
        None,
        None,
        /*notes_after_env*/ None,
        None,
        &[],
    );
    insta::assert_snapshot!(prompt.full_text());
}
