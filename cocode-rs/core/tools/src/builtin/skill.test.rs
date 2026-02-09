use super::*;
use cocode_skill::LoadedFrom;
use cocode_skill::SkillContext;
use cocode_skill::SkillManager;
use cocode_skill::SkillPromptCommand;
use cocode_skill::SkillSource;
use std::path::PathBuf;
use std::sync::Arc;

fn make_test_skill(name: &str, prompt: &str) -> SkillPromptCommand {
    SkillPromptCommand {
        name: name.to_string(),
        description: format!("{name} description"),
        prompt: prompt.to_string(),
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
    }
}

fn make_skill_manager() -> Arc<SkillManager> {
    let mut manager = SkillManager::new();
    manager.register(make_test_skill(
        "commit",
        "Analyze the changes and generate a commit message",
    ));
    let mut review = make_test_skill("review-pr", "Review PR #$ARGUMENTS");
    review.aliases = vec!["rp".to_string()];
    manager.register(review);
    Arc::new(manager)
}

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
        .with_skill_manager(make_skill_manager())
}

#[tokio::test]
async fn test_skill_tool() {
    let tool = SkillTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "skill": "commit"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("commit"));
    assert!(text.contains("<skill-invoked"));
}

#[tokio::test]
async fn test_skill_tool_with_args() {
    let tool = SkillTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "skill": "review-pr",
        "args": "123"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    // $ARGUMENTS should be replaced with "123"
    assert!(text.contains("Review PR #123"));
    assert!(text.contains("<skill-invoked"));
}

#[tokio::test]
async fn test_skill_tool_by_alias() {
    let tool = SkillTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "skill": "rp",
        "args": "456"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("Review PR #456"));
}

#[tokio::test]
async fn test_skill_not_found() {
    let tool = SkillTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "skill": "nonexistent"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_skill_manager_not_configured() {
    let tool = SkillTool::new();
    // Context without skill manager
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({
        "skill": "commit"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_skill_disable_model_invocation() {
    let mut manager = SkillManager::new();
    let mut skill = make_test_skill("internal", "Internal only");
    skill.disable_model_invocation = true;
    manager.register(skill);

    let tool = SkillTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
        .with_skill_manager(Arc::new(manager));

    let input = serde_json::json!({
        "skill": "internal"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("cannot be invoked by the model"));
}

#[tokio::test]
async fn test_skill_base_dir_injection() {
    let mut manager = SkillManager::new();
    let mut skill = make_test_skill("deploy", "Deploy the app");
    skill.base_dir = Some(PathBuf::from("/project/skills/deploy"));
    manager.register(skill);

    let tool = SkillTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
        .with_skill_manager(Arc::new(manager));

    let input = serde_json::json!({
        "skill": "deploy"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("Base directory for this skill: /project/skills/deploy"));
    assert!(text.contains("Deploy the app"));
}

#[test]
fn test_tool_properties() {
    let tool = SkillTool::new();
    assert_eq!(tool.name(), "Skill");
    assert!(!tool.is_concurrent_safe());
    assert!(!tool.is_read_only());
}
