use super::*;
use crate::command::SkillContext;
use crate::command::SkillPromptCommand;
use crate::source::LoadedFrom;
use crate::source::SkillSource;
use std::path::PathBuf;

fn make_success(name: &str) -> SkillLoadOutcome {
    SkillLoadOutcome::Success {
        skill: SkillPromptCommand {
            name: name.to_string(),
            description: "desc".to_string(),
            prompt: "prompt".to_string(),
            allowed_tools: None,
            user_invocable: true,
            disable_model_invocation: false,
            is_hidden: false,
            source: SkillSource::Bundled,
            loaded_from: LoadedFrom::Bundled,
            context: SkillContext::Main,
            agent: None,
            model: None,
            base_dir: None,
            when_to_use: None,
            argument_hint: None,
            aliases: Vec::new(),
            interface: None,
            command_type: crate::command::CommandType::Prompt,
        },
        source: SkillSource::Bundled,
    }
}

fn make_failed(path: &str) -> SkillLoadOutcome {
    SkillLoadOutcome::Failed {
        path: PathBuf::from(path),
        error: "error".to_string(),
    }
}

#[test]
fn test_deduplicator_basic() {
    let mut dedup = SkillDeduplicator::new();
    assert!(dedup.is_empty());
    assert!(!dedup.is_duplicate("a"));
    assert_eq!(dedup.len(), 1);
    assert!(dedup.is_duplicate("a"));
    assert_eq!(dedup.len(), 1);
    assert!(!dedup.is_duplicate("b"));
    assert_eq!(dedup.len(), 2);
}

#[test]
fn test_dedup_skills_no_duplicates() {
    let skills = vec![make_success("a"), make_success("b"), make_success("c")];
    let result = dedup_skills(skills);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_dedup_skills_removes_duplicates() {
    let skills = vec![
        make_success("a"),
        make_success("b"),
        make_success("a"), // duplicate
        make_success("c"),
        make_success("b"), // duplicate
    ];
    let result = dedup_skills(skills);
    assert_eq!(result.len(), 3);

    let names: Vec<_> = result.iter().filter_map(|o| o.skill_name()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn test_dedup_skills_keeps_first_occurrence() {
    let skills = vec![
        SkillLoadOutcome::Success {
            skill: SkillPromptCommand {
                name: "x".to_string(),
                description: "first".to_string(),
                prompt: "first prompt".to_string(),
                allowed_tools: None,
                user_invocable: true,
                disable_model_invocation: false,
                is_hidden: false,
                source: SkillSource::ProjectSettings {
                    path: PathBuf::from("/first"),
                },
                loaded_from: LoadedFrom::ProjectSettings,
                context: SkillContext::Main,
                agent: None,
                model: None,
                base_dir: None,
                when_to_use: None,
                argument_hint: None,
                aliases: Vec::new(),
                interface: None,
                command_type: crate::command::CommandType::Prompt,
            },
            source: SkillSource::ProjectSettings {
                path: PathBuf::from("/first"),
            },
        },
        SkillLoadOutcome::Success {
            skill: SkillPromptCommand {
                name: "x".to_string(),
                description: "second".to_string(),
                prompt: "second prompt".to_string(),
                allowed_tools: None,
                user_invocable: true,
                disable_model_invocation: false,
                is_hidden: false,
                source: SkillSource::UserSettings {
                    path: PathBuf::from("/second"),
                },
                loaded_from: LoadedFrom::UserSettings,
                context: SkillContext::Main,
                agent: None,
                model: None,
                base_dir: None,
                when_to_use: None,
                argument_hint: None,
                aliases: Vec::new(),
                interface: None,
                command_type: crate::command::CommandType::Prompt,
            },
            source: SkillSource::UserSettings {
                path: PathBuf::from("/second"),
            },
        },
    ];
    let result = dedup_skills(skills);
    assert_eq!(result.len(), 1);

    if let SkillLoadOutcome::Success { skill, .. } = &result[0] {
        assert_eq!(skill.description, "first");
        assert_eq!(skill.prompt, "first prompt");
    } else {
        panic!("expected Success outcome");
    }
}

#[test]
fn test_dedup_skills_keeps_failures() {
    let skills = vec![
        make_success("a"),
        make_failed("/bad1"),
        make_success("a"), // duplicate
        make_failed("/bad2"),
    ];
    let result = dedup_skills(skills);
    assert_eq!(result.len(), 3); // 1 success + 2 failures

    let successes = result.iter().filter(|o| o.is_success()).count();
    let failures = result.iter().filter(|o| !o.is_success()).count();
    assert_eq!(successes, 1);
    assert_eq!(failures, 2);
}

#[test]
fn test_dedup_skills_empty() {
    let result = dedup_skills(Vec::new());
    assert!(result.is_empty());
}
