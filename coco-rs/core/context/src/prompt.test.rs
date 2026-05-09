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

#[test]
fn no_output_style_yields_no_section() {
    let env = empty_env();
    let prompt = build_system_prompt("identity", &[], &env, None, None, None, None);
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
    let prompt = build_system_prompt("identity-line", &[], &env, None, None, None, Some(style));
    let text = prompt.full_text();
    let idx_identity = text.find("identity-line").unwrap();
    let idx_style = text.find("# Output Style: Explanatory").unwrap();
    let idx_env = text.find("# Environment").unwrap();
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
    let prompt = build_system_prompt("ID", &[], &env, None, None, None, Some(style));
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
    let prompt = build_system_prompt("ID", &[], &env, None, None, None, Some(style));
    // Find the first cache breakpoint and assert the preceding text
    // block contains the output style header.
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
