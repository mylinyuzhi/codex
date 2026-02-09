use super::*;

fn make_success() -> SkillLoadOutcome {
    SkillLoadOutcome::Success {
        skill: SkillPromptCommand {
            name: "test".to_string(),
            description: "A test skill".to_string(),
            prompt: "Do something".to_string(),
            allowed_tools: None,
            user_invocable: true,
            disable_model_invocation: false,
            is_hidden: false,
            source: SkillSource::Bundled,
            loaded_from: crate::source::LoadedFrom::Bundled,
            context: crate::command::SkillContext::Main,
            agent: None,
            model: None,
            base_dir: None,
            when_to_use: None,
            argument_hint: None,
            aliases: Vec::new(),
            interface: None,
        },
        source: SkillSource::Bundled,
    }
}

fn make_failed() -> SkillLoadOutcome {
    SkillLoadOutcome::Failed {
        path: PathBuf::from("/bad/skill"),
        error: "parse error".to_string(),
    }
}

#[test]
fn test_is_success() {
    assert!(make_success().is_success());
    assert!(!make_failed().is_success());
}

#[test]
fn test_skill_name() {
    assert_eq!(make_success().skill_name(), Some("test"));
    assert_eq!(make_failed().skill_name(), None);
}

#[test]
fn test_into_skill() {
    let skill = make_success().into_skill();
    assert!(skill.is_some());
    assert_eq!(skill.as_ref().map(|s| s.name.as_str()), Some("test"));

    let skill = make_failed().into_skill();
    assert!(skill.is_none());
}
