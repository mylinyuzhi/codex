use super::*;

#[test]
fn test_bundled_skills_exist() {
    let skills = get_bundled_skills();
    assert!(skills.len() >= 14);
    assert!(skills.iter().any(|s| s.name == "commit"));
    assert!(skills.iter().any(|s| s.name == "review-pr"));
    assert!(skills.iter().any(|s| s.name == "claude-api"));
    assert!(skills.iter().any(|s| s.name == "schedule"));
}

#[test]
fn test_register_bundled() {
    let mut manager = crate::SkillManager::new();
    register_bundled(&mut manager);
    assert!(manager.get("commit").is_some());
    assert!(manager.get("claude-api").is_some());
    assert!(manager.get("schedule").is_some());
}

#[test]
fn test_debug_disables_model_invocation() {
    let skills = get_bundled_skills();
    let debug = skills.iter().find(|s| s.name == "debug").unwrap();
    assert!(debug.disable_model_invocation);
}

#[test]
fn test_batch_disables_model_invocation() {
    let skills = get_bundled_skills();
    let batch = skills.iter().find(|s| s.name == "batch").unwrap();
    assert!(batch.disable_model_invocation);
}

#[test]
fn test_keybindings_not_user_invocable() {
    let skills = get_bundled_skills();
    let kb = skills
        .iter()
        .find(|s| s.name == "keybindings-help")
        .unwrap();
    assert!(!kb.user_invocable);
}

#[test]
fn test_skills_with_when_to_use() {
    let skills = get_bundled_skills();
    let with_when: Vec<&str> = skills
        .iter()
        .filter(|s| s.when_to_use.is_some())
        .map(|s| s.name.as_str())
        .collect();
    assert!(with_when.contains(&"keybindings-help"));
    assert!(with_when.contains(&"update-config"));
    assert!(with_when.contains(&"batch"));
    assert!(with_when.contains(&"remember"));
    assert!(with_when.contains(&"skillify"));
    assert!(with_when.contains(&"claude-api"));
    assert!(with_when.contains(&"schedule"));
}

#[test]
fn test_all_bundled_are_bundled_source() {
    let skills = get_bundled_skills();
    for skill in &skills {
        assert!(
            matches!(skill.source, crate::SkillSource::Bundled),
            "skill {} should be Bundled source",
            skill.name
        );
    }
}
