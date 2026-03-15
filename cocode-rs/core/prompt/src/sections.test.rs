use cocode_context::ContextInjection;
use cocode_context::EnvironmentInfo;
use cocode_context::MemoryFile;

use super::*;

fn test_ctx() -> ConversationContext {
    let env = EnvironmentInfo::builder()
        .platform("darwin")
        .os_version("Darwin 24.0.0")
        .cwd("/home/user/project")
        .is_git_repo(true)
        .git_branch("main")
        .date("2025-01-29")
        .context_window(200000)
        .max_output_tokens(16384)
        .build()
        .unwrap();

    ConversationContext::builder()
        .environment(env)
        .build()
        .unwrap()
}

#[test]
fn test_assemble_sections_order() {
    let sections = vec![
        (PromptSection::Identity, "First section".to_string()),
        (PromptSection::Security, "Second section".to_string()),
        (PromptSection::Environment, "Third section".to_string()),
    ];

    let result = assemble_sections(&sections);
    assert!(result.starts_with("First section"));
    assert!(result.contains("Second section"));
    assert!(result.ends_with("Third section"));
}

#[test]
fn test_assemble_sections_skips_empty() {
    let sections = vec![
        (PromptSection::Identity, "Content".to_string()),
        (PromptSection::Security, "".to_string()),
        (PromptSection::Environment, "   ".to_string()),
        (PromptSection::Permission, "More content".to_string()),
    ];

    let result = assemble_sections(&sections);
    assert_eq!(result, "Content\n\nMore content");
}

#[test]
fn test_render_environment() {
    let ctx = test_ctx();
    let rendered = render_environment(&ctx);

    assert!(rendered.contains("darwin"));
    assert!(rendered.contains("/home/user/project"));
    assert!(rendered.contains("2025-01-29"));
    assert!(rendered.contains("main"));
    assert!(rendered.contains("true"));
    assert!(rendered.contains("OS Version: Darwin 24.0.0"));
    assert!(!rendered.contains("{{"));
}

#[test]
fn test_permission_section() {
    assert!(permission_section(&PermissionMode::Default).contains("Default"));
    assert!(permission_section(&PermissionMode::Plan).contains("Plan"));
    assert!(permission_section(&PermissionMode::AcceptEdits).contains("Accept Edits"));
    assert!(permission_section(&PermissionMode::Bypass).contains("Bypass"));
}

#[test]
fn test_render_memory_files() {
    let env = EnvironmentInfo::builder().cwd("/tmp").build().unwrap();

    let ctx = ConversationContext::builder()
        .environment(env)
        .memory_files(vec![
            MemoryFile {
                path: "CLAUDE.md".to_string(),
                content: "Project rules here".to_string(),
                priority: 0,
            },
            MemoryFile {
                path: "README.md".to_string(),
                content: "Readme content".to_string(),
                priority: 1,
            },
        ])
        .build()
        .unwrap();

    let rendered = render_memory_files(&ctx);
    assert!(rendered.contains("CLAUDE.md"));
    assert!(rendered.contains("Project rules here"));
    assert!(rendered.contains("README.md"));
    // CLAUDE.md should come first (lower priority value)
    let claude_pos = rendered.find("CLAUDE.md").unwrap();
    let readme_pos = rendered.find("README.md").unwrap();
    assert!(claude_pos < readme_pos);
}

#[test]
fn test_render_memory_files_empty() {
    let ctx = test_ctx();
    let rendered = render_memory_files(&ctx);
    assert!(rendered.is_empty());
}

#[test]
fn test_render_injections() {
    let env = EnvironmentInfo::builder().cwd("/tmp").build().unwrap();

    let ctx = ConversationContext::builder()
        .environment(env)
        .injections(vec![
            ContextInjection {
                label: "hook-output".to_string(),
                content: "Hook says hello".to_string(),
                position: InjectionPosition::EndOfPrompt,
            },
            ContextInjection {
                label: "pre-tool".to_string(),
                content: "Before tools".to_string(),
                position: InjectionPosition::BeforeTools,
            },
        ])
        .build()
        .unwrap();

    let end_injections = render_injections(&ctx, InjectionPosition::EndOfPrompt);
    assert!(end_injections.contains("Hook says hello"));
    assert!(!end_injections.contains("Before tools"));

    let before_injections = render_injections(&ctx, InjectionPosition::BeforeTools);
    assert!(before_injections.contains("Before tools"));
}

#[test]
fn test_render_environment_without_language_preference() {
    let ctx = test_ctx();
    let rendered = render_environment(&ctx);

    // Should not contain language preference section
    assert!(!rendered.contains("# Language Preference"));
}

#[test]
fn test_render_environment_with_language_preference() {
    let env = EnvironmentInfo::builder()
        .platform("darwin")
        .os_version("Darwin 24.0.0")
        .cwd("/home/user/project")
        .is_git_repo(true)
        .git_branch("main")
        .date("2025-01-29")
        .language_preference("中文")
        .build()
        .unwrap();

    let ctx = ConversationContext::builder()
        .environment(env)
        .build()
        .unwrap();

    let rendered = render_environment(&ctx);

    // Should contain language preference section
    assert!(rendered.contains("# Language Preference"));
    assert!(rendered.contains("中文"));
    assert!(rendered.contains("MUST respond in"));
}

#[test]
fn test_generate_tool_policy_lines_with_ls() {
    let tool_names = vec!["Read".to_string(), "Edit".to_string(), "LS".to_string()];
    let result = generate_tool_policy_lines(&tool_names);
    assert!(result.contains("Use Read for reading files"));
    assert!(result.contains("Use Edit for modifying files"));
    assert!(result.contains("Use LS for directory listing"));
    assert!(!result.contains("Use Grep"));
}

#[test]
fn test_generate_tool_policy_lines_without_ls() {
    let tool_names = vec!["Read".to_string(), "Edit".to_string(), "Grep".to_string()];
    let result = generate_tool_policy_lines(&tool_names);
    assert!(result.contains("Use Read for reading files"));
    assert!(result.contains("Use Edit for modifying files"));
    assert!(result.contains("Use Grep for searching file contents"));
    assert!(!result.contains("Use LS"));
}

#[test]
fn test_generate_tool_policy_lines_empty() {
    let tool_names: Vec<String> = vec![];
    let result = generate_tool_policy_lines(&tool_names);
    assert!(result.is_empty());
}

#[test]
fn test_generate_tool_policy_lines_all_tools() {
    let tool_names = vec![
        "Read".to_string(),
        "Edit".to_string(),
        "Write".to_string(),
        "Grep".to_string(),
        "Glob".to_string(),
        "LS".to_string(),
    ];
    let result = generate_tool_policy_lines(&tool_names);
    assert!(result.contains("Use Read"));
    assert!(result.contains("Use Edit"));
    assert!(result.contains("Use Write"));
    assert!(result.contains("Use Grep"));
    assert!(result.contains("Use Glob"));
    assert!(result.contains("Use LS"));
}
