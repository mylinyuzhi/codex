use super::*;
use coco_types::Features;

#[test]
fn human_user_gets_unconditional_only() {
    let skills = get_bundled_skills(UserType::Human);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    // Unconditional set
    assert!(names.contains(&"update-config"));
    assert!(names.contains(&"keybindings-help"));
    assert!(names.contains(&"batch"));
    // Ant-only must NOT appear for human
    assert!(!names.contains(&"verify"));
    assert!(!names.contains(&"debug"));
    assert!(!names.contains(&"skillify"));
    assert!(!names.contains(&"remember"));
    assert!(!names.contains(&"simplify"));
    assert!(!names.contains(&"stuck"));
    assert!(!names.contains(&"lorem-ipsum"));
}

#[test]
fn ant_user_gets_unconditional_plus_ant_only() {
    let skills = get_bundled_skills(UserType::Ant);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    for required in [
        "update-config",
        "keybindings-help",
        "batch",
        "verify",
        "debug",
        "skillify",
        "remember",
        "simplify",
        "stuck",
        "lorem-ipsum",
    ] {
        assert!(names.contains(&required), "ant should get {required}");
    }
}

#[test]
fn no_rust_only_extras() {
    // commit/review-pr/pdf were removed in Round 11 — TS does not ship them as bundled.
    let skills = get_bundled_skills(UserType::Ant);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(!names.contains(&"commit"));
    assert!(!names.contains(&"review-pr"));
    assert!(!names.contains(&"pdf"));
}

#[test]
fn debug_disables_model_invocation() {
    let skills = get_bundled_skills(UserType::Ant);
    let debug = skills.iter().find(|s| s.name == "debug").unwrap();
    assert!(debug.disable_model_invocation);
}

#[test]
fn batch_disables_model_invocation() {
    let skills = get_bundled_skills(UserType::Human);
    let batch = skills.iter().find(|s| s.name == "batch").unwrap();
    assert!(batch.disable_model_invocation);
}

#[test]
fn keybindings_not_user_invocable() {
    let skills = get_bundled_skills(UserType::Human);
    let kb = skills
        .iter()
        .find(|s| s.name == "keybindings-help")
        .unwrap();
    assert!(!kb.user_invocable);
    assert!(kb.is_hidden);
}

#[test]
fn loop_is_gated_by_agent_triggers() {
    let skills = get_bundled_skills(UserType::Ant);
    let l = skills.iter().find(|s| s.name == "loop").unwrap();
    assert_eq!(l.gated_by, Some(Feature::AgentTriggers));
}

#[test]
fn schedule_is_gated_by_agent_triggers_remote() {
    let skills = get_bundled_skills(UserType::Ant);
    let s = skills.iter().find(|s| s.name == "schedule").unwrap();
    assert_eq!(s.gated_by, Some(Feature::AgentTriggersRemote));
}

#[test]
fn claude_api_is_gated_by_building_claude_apps() {
    let skills = get_bundled_skills(UserType::Ant);
    let c = skills.iter().find(|s| s.name == "claude-api").unwrap();
    assert_eq!(c.gated_by, Some(Feature::BuildingClaudeApps));
}

#[test]
fn dream_hunter_chrome_runskillgen_present_and_gated() {
    let skills = get_bundled_skills(UserType::Ant);
    let dream = skills.iter().find(|s| s.name == "dream").unwrap();
    let hunter = skills.iter().find(|s| s.name == "hunter").unwrap();
    let chrome = skills
        .iter()
        .find(|s| s.name == "claude-in-chrome")
        .unwrap();
    let rsg = skills
        .iter()
        .find(|s| s.name == "run-skill-generator")
        .unwrap();
    assert_eq!(dream.gated_by, Some(Feature::KairosDream));
    assert_eq!(hunter.gated_by, Some(Feature::ReviewArtifact));
    assert_eq!(chrome.gated_by, Some(Feature::ClaudeInChrome));
    assert_eq!(rsg.gated_by, Some(Feature::RunSkillGenerator));
}

#[test]
fn visible_filters_by_features() {
    let manager = crate::SkillManager::new();
    register_bundled(&manager, UserType::Ant);

    let no_features = Features::empty();
    let visible_empty_skills = manager.visible(&no_features);
    let visible_empty: Vec<&str> = visible_empty_skills
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    // Even with no features enabled, ungated skills appear.
    assert!(visible_empty.contains(&"update-config"));
    assert!(visible_empty.contains(&"verify")); // ant + ungated
    // Gated skills should NOT appear.
    assert!(!visible_empty.contains(&"loop"));
    assert!(!visible_empty.contains(&"dream"));

    let mut features = Features::empty();
    features
        .enable(Feature::AgentTriggers)
        .enable(Feature::KairosDream);
    let visible_some_skills = manager.visible(&features);
    let visible_some: Vec<&str> = visible_some_skills
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(visible_some.contains(&"loop"));
    assert!(visible_some.contains(&"dream"));
    assert!(!visible_some.contains(&"hunter")); // not enabled
}

#[test]
fn all_bundled_are_bundled_source() {
    let skills = get_bundled_skills(UserType::Ant);
    for skill in &skills {
        assert!(
            matches!(skill.source, crate::SkillSource::Bundled),
            "skill {} should be Bundled source",
            skill.name
        );
    }
}
