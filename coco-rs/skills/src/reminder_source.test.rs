use super::*;
use crate::SkillDefinition;
use crate::SkillSource;
use coco_system_reminder::SkillsSource;

fn skill(name: &str, desc: &str) -> SkillDefinition {
    SkillDefinition {
        name: name.to_string(),
        description: desc.to_string(),
        prompt: String::new(),
        source: SkillSource::Bundled,
        aliases: Vec::new(),
        allowed_tools: None,
        model: None,
        when_to_use: None,
        argument_names: Vec::new(),
        paths: Vec::new(),
        effort: None,
        context: Default::default(),
        agent: None,
        version: None,
        disabled: false,
        hooks: None,
        argument_hint: None,
        user_invocable: true,
        disable_model_invocation: false,
        shell: None,
        content_length: 0,
        is_hidden: false,
    }
}

#[tokio::test]
async fn listing_returns_none_when_no_skills() {
    let mgr = SkillManager::new();
    assert!(mgr.listing(None).await.is_none());
}

#[tokio::test]
async fn listing_renders_sorted_bullet_list() {
    let mut mgr = SkillManager::new();
    mgr.register(skill("zeta", "last alphabetically"));
    mgr.register(skill("alpha", "first"));
    mgr.register(skill("bravo", ""));
    let body = mgr.listing(None).await.unwrap();
    let lines: Vec<&str> = body.split('\n').collect();
    assert_eq!(lines[0], "- alpha: first");
    assert_eq!(lines[1], "- bravo");
    assert_eq!(lines[2], "- zeta: last alphabetically");
}

#[tokio::test]
async fn invoked_is_empty_by_default() {
    let mgr = SkillManager::new();
    assert!(mgr.invoked(None).await.is_empty());
}
