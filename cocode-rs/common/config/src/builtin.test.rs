use super::*;
use cocode_protocol::ReasoningEffort;

#[test]
fn test_get_model_defaults() {
    ensure_initialized();

    let gpt5 = get_model_defaults("gpt-5").unwrap();
    assert_eq!(gpt5.display_name, Some("GPT-5".to_string()));
    assert_eq!(gpt5.context_window, Some(272000));

    let gemini = get_model_defaults("gemini-3-pro").unwrap();
    assert_eq!(gemini.display_name, Some("Gemini 3 Pro".to_string()));

    let unknown = get_model_defaults("unknown-model");
    assert!(unknown.is_none());
}

#[test]
fn test_get_provider_defaults() {
    ensure_initialized();

    let openai = get_provider_defaults("openai").unwrap();
    assert_eq!(openai.name, "openai");
    assert_eq!(openai.env_key, Some("OPENAI_API_KEY".to_string()));

    let gemini = get_provider_defaults("gemini").unwrap();
    assert_eq!(gemini.name, "gemini");
    assert_eq!(gemini.provider_type, ProviderType::Gemini);

    let unknown = get_provider_defaults("unknown-provider");
    assert!(unknown.is_none());
}

#[test]
fn test_list_builtin_models() {
    ensure_initialized();

    let models = list_builtin_models();
    assert!(models.contains(&"gpt-5"));
    assert!(models.contains(&"gpt-5.2"));
    assert!(models.contains(&"gpt-5.2-codex"));
    assert!(models.contains(&"gemini-3-pro"));
    assert!(models.contains(&"gemini-3-flash"));
}

#[test]
fn test_list_builtin_providers() {
    ensure_initialized();

    let providers = list_builtin_providers();
    assert!(providers.contains(&"openai"));
    assert!(providers.contains(&"gemini"));
}

#[test]
fn test_model_capabilities() {
    ensure_initialized();

    let gpt5 = get_model_defaults("gpt-5").unwrap();
    let caps = gpt5.capabilities.unwrap();
    assert!(caps.contains(&Capability::TextGeneration));
    assert!(caps.contains(&Capability::Vision));
    assert!(caps.contains(&Capability::ToolCalling));
    assert!(caps.contains(&Capability::ParallelToolCalls));

    let gpt52 = get_model_defaults("gpt-5.2").unwrap();
    let caps = gpt52.capabilities.unwrap();
    assert!(caps.contains(&Capability::ExtendedThinking));
    assert!(caps.contains(&Capability::ReasoningSummaries));
}

#[test]
fn test_thinking_models() {
    ensure_initialized();

    let gpt5 = get_model_defaults("gpt-5").unwrap();
    assert!(gpt5.default_thinking_level.is_some());
    assert!(gpt5.supported_thinking_levels.is_some());

    let default_level = gpt5.default_thinking_level.unwrap();
    assert_eq!(default_level.effort, ReasoningEffort::Medium);

    let levels = gpt5.supported_thinking_levels.unwrap();
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::Low));
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::Medium));
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::High));

    let gpt52 = get_model_defaults("gpt-5.2").unwrap();
    let levels = gpt52.supported_thinking_levels.unwrap();
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::XHigh));
}

#[test]
fn test_apply_patch_tool_type() {
    ensure_initialized();

    let gpt5 = get_model_defaults("gpt-5").unwrap();
    assert_eq!(gpt5.apply_patch_tool_type, Some(ApplyPatchToolType::Shell));

    let gpt52 = get_model_defaults("gpt-5.2").unwrap();
    assert_eq!(
        gpt52.apply_patch_tool_type,
        Some(ApplyPatchToolType::Freeform)
    );

    let codex = get_model_defaults("gpt-5.2-codex").unwrap();
    assert_eq!(
        codex.apply_patch_tool_type,
        Some(ApplyPatchToolType::Freeform)
    );

    let gemini = get_model_defaults("gemini-3-pro").unwrap();
    assert_eq!(gemini.apply_patch_tool_type, None);

    let gemini_flash = get_model_defaults("gemini-3-flash").unwrap();
    assert_eq!(gemini_flash.apply_patch_tool_type, None);
}

#[test]
fn test_shell_type() {
    ensure_initialized();

    let gpt52 = get_model_defaults("gpt-5.2").unwrap();
    assert_eq!(gpt52.shell_type, Some(ConfigShellToolType::ShellCommand));

    let gpt5 = get_model_defaults("gpt-5").unwrap();
    assert_eq!(gpt5.shell_type, None); // Default
}

#[test]
fn test_gpt52_codex() {
    ensure_initialized();

    let codex = get_model_defaults("gpt-5.2-codex").unwrap();
    assert_eq!(codex.display_name, Some("GPT-5.2 Codex".to_string()));
    assert_eq!(codex.context_window, Some(272000));
    assert_eq!(codex.max_output_tokens, Some(64000));
    assert_eq!(codex.shell_type, Some(ConfigShellToolType::ShellCommand));

    let caps = codex.capabilities.unwrap();
    assert!(caps.contains(&Capability::ExtendedThinking));
    assert!(caps.contains(&Capability::ReasoningSummaries));
    assert!(caps.contains(&Capability::ParallelToolCalls));

    let levels = codex.supported_thinking_levels.unwrap();
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::Low));
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::Medium));
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::High));
    assert!(levels.iter().any(|l| l.effort == ReasoningEffort::XHigh));
}

#[test]
fn test_excluded_tools() {
    ensure_initialized();

    let expected = &[
        "Edit",
        "Write",
        "ReadManyFiles",
        "NotebookEdit",
        "SmartEdit",
    ];

    // All GPT models must have excluded_tools set
    let gpt_slugs: Vec<_> = list_builtin_models()
        .into_iter()
        .filter(|s| s.starts_with("gpt"))
        .collect();
    assert!(!gpt_slugs.is_empty(), "should have at least one GPT model");

    for slug in &gpt_slugs {
        let model = get_model_defaults(slug).unwrap();
        let excluded = model.excluded_tools.as_ref().unwrap_or_else(|| {
            panic!("{slug} should have excluded_tools set");
        });
        for tool in expected {
            assert!(
                excluded.contains(&tool.to_string()),
                "{slug} should exclude {tool}"
            );
        }
    }

    // Non-GPT models should not have excluded_tools
    for slug in list_builtin_models() {
        if !slug.starts_with("gpt") {
            let model = get_model_defaults(slug).unwrap();
            assert_eq!(
                model.excluded_tools, None,
                "{slug} should not have excluded_tools"
            );
        }
    }
}

#[test]
fn test_builtin_models_have_instructions() {
    ensure_initialized();

    // All built-in models should have base_instructions
    for slug in list_builtin_models() {
        let model = get_model_defaults(slug).unwrap();
        assert!(
            model.base_instructions.is_some(),
            "Model {slug} should have base_instructions"
        );
        // Verify instructions are non-empty
        let instructions = model.base_instructions.as_ref().unwrap();
        assert!(
            !instructions.is_empty(),
            "Model {slug} should have non-empty base_instructions"
        );
    }
}

#[test]
fn test_get_output_style_explanatory() {
    let style = get_output_style("explanatory").unwrap();
    assert!(style.contains("Explanatory Style Active"));
    assert!(style.contains("Insight Format"));

    // Test case-insensitive variants
    assert_eq!(style, get_output_style("Explanatory").unwrap());
    assert_eq!(style, get_output_style("EXPLANATORY").unwrap());
    assert_eq!(style, get_output_style("ExPlAnAtOrY").unwrap());
}

#[test]
fn test_get_output_style_learning() {
    let style = get_output_style("learning").unwrap();
    assert!(style.contains("Learning Style Active"));
    assert!(style.contains("TODO(human)"));

    // Test case-insensitive variants
    assert_eq!(style, get_output_style("Learning").unwrap());
    assert_eq!(style, get_output_style("LEARNING").unwrap());
    assert_eq!(style, get_output_style("LeArNiNg").unwrap());
}

#[test]
fn test_get_output_style_unknown() {
    let style = get_output_style("unknown");
    assert!(style.is_none());
}

#[test]
fn test_list_builtin_output_styles() {
    let styles = list_builtin_output_styles();
    assert!(styles.contains(&"explanatory"));
    assert!(styles.contains(&"learning"));
    assert_eq!(styles.len(), 2);
}

#[test]
fn test_parse_frontmatter_empty() {
    let (fm, body) = parse_frontmatter("Hello world");
    assert!(fm.name.is_none());
    assert!(fm.description.is_none());
    assert_eq!(body, "Hello world");
}

#[test]
fn test_parse_frontmatter_simple() {
    let content = r#"---
name: concise
description: Short responses
---
Body content here."#;

    let (fm, body) = parse_frontmatter(content);
    assert_eq!(fm.name, Some("concise".to_string()));
    assert_eq!(fm.description, Some("Short responses".to_string()));
    assert!(body.contains("Body content here"));
}

#[test]
fn test_parse_frontmatter_quoted_values() {
    let content = r#"---
name: "my-style"
description: 'A quoted description'
---
Content"#;

    let (fm, _body) = parse_frontmatter(content);
    assert_eq!(fm.name, Some("my-style".to_string()));
    assert_eq!(fm.description, Some("A quoted description".to_string()));
}

#[test]
fn test_parse_frontmatter_keep_coding_instructions() {
    let content = r#"---
name: test
keep-coding-instructions: true
---
Content"#;

    let (fm, _body) = parse_frontmatter(content);
    assert_eq!(fm.keep_coding_instructions, Some(true));
}

#[test]
fn test_load_custom_output_styles_empty_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let styles = load_custom_output_styles(tmp.path());
    assert!(styles.is_empty());
}

#[test]
fn test_load_custom_output_styles_with_files() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Create a simple style file
    std::fs::write(tmp.path().join("concise.md"), "Be concise and direct.")
        .expect("write file");

    // Create a style with frontmatter
    std::fs::write(
        tmp.path().join("verbose.md"),
        r#"---
name: verbose
description: Detailed explanations
---
Provide detailed explanations for every action."#,
    )
    .expect("write file");

    let styles = load_custom_output_styles(tmp.path());
    assert_eq!(styles.len(), 2);

    // Check concise style (no frontmatter)
    let concise = styles.iter().find(|s| s.name == "concise").unwrap();
    assert_eq!(concise.content, "Be concise and direct.");

    // Check verbose style (with frontmatter)
    let verbose = styles.iter().find(|s| s.name == "verbose").unwrap();
    assert_eq!(
        verbose.description,
        Some("Detailed explanations".to_string())
    );
    assert!(verbose.content.contains("detailed explanations"));
}

#[test]
fn test_load_custom_output_styles_nonexistent_dir() {
    let styles = load_custom_output_styles(Path::new("/nonexistent/xyz"));
    assert!(styles.is_empty());
}

#[test]
fn test_load_custom_output_styles_ignores_non_md() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Create various files
    std::fs::write(tmp.path().join("style.md"), "Valid style").expect("write");
    std::fs::write(tmp.path().join("notes.txt"), "Not a style").expect("write");
    std::fs::write(tmp.path().join("config.json"), "{}").expect("write");

    let styles = load_custom_output_styles(tmp.path());
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0].name, "style");
}

#[test]
fn test_find_output_style_builtin() {
    let style = find_output_style("explanatory").unwrap();
    assert_eq!(style.name, "explanatory");
    assert!(style.source.is_builtin());
    assert!(style.content.contains("Explanatory Style Active"));
}

#[test]
fn test_find_output_style_case_insensitive() {
    let style1 = find_output_style("EXPLANATORY").unwrap();
    let style2 = find_output_style("Explanatory").unwrap();
    let style3 = find_output_style("explanatory").unwrap();

    assert_eq!(style1.content, style2.content);
    assert_eq!(style2.content, style3.content);
}

#[test]
fn test_find_output_style_not_found() {
    let style = find_output_style("nonexistent-style");
    assert!(style.is_none());
}

#[test]
fn test_output_style_source() {
    let builtin = OutputStyleSource::Builtin;
    assert!(builtin.is_builtin());
    assert!(!builtin.is_custom());

    let custom = OutputStyleSource::Custom(PathBuf::from("/test/style.md"));
    assert!(!custom.is_builtin());
    assert!(custom.is_custom());
}

#[test]
fn test_load_all_output_styles() {
    let styles = load_all_output_styles();

    // Should have at least the built-in styles
    assert!(styles.len() >= 2);
    assert!(styles.iter().any(|s| s.name == "explanatory"));
    assert!(styles.iter().any(|s| s.name == "learning"));
}
