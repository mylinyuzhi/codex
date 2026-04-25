//! Tests for [`QuerySkillRuntime`].
//!
//! Uses in-memory `SkillManager` fixtures + a stub
//! `AgentQueryEngine` to exercise both inline and fork routing
//! without touching the filesystem.

use std::sync::Arc;

use async_trait::async_trait;
use coco_skills::SkillContext;
use coco_skills::SkillDefinition;
use coco_skills::SkillManager;
use coco_skills::SkillSource;
use coco_tool::AgentQueryConfig;
use coco_tool::AgentQueryEngine;
use coco_tool::AgentQueryResult;
use coco_tool::SkillHandle;
use coco_tool::SkillInvocationError;
use coco_tool::SkillInvocationResult;
use pretty_assertions::assert_eq;
use tokio::sync::RwLock;

use super::*;

fn sample_skill(
    name: &str,
    prompt: &str,
    context: SkillContext,
    disabled: bool,
    disable_model: bool,
) -> SkillDefinition {
    SkillDefinition {
        name: name.into(),
        description: format!("{name} description"),
        prompt: prompt.into(),
        source: SkillSource::Bundled,
        aliases: Vec::new(),
        allowed_tools: None,
        model: None,
        when_to_use: None,
        argument_names: Vec::new(),
        paths: Vec::new(),
        effort: None,
        context,
        agent: None,
        version: None,
        disabled,
        hooks: None,
        argument_hint: None,
        user_invocable: true,
        disable_model_invocation: disable_model,
        shell: None,
        content_length: prompt.len() as i64,
        is_hidden: false,
    }
}

fn runtime_with(skills: Vec<SkillDefinition>) -> QuerySkillRuntime {
    let mut mgr = SkillManager::new();
    for s in skills {
        mgr.register(s);
    }
    QuerySkillRuntime::new(Arc::new(RwLock::new(mgr)))
}

#[tokio::test]
async fn test_not_found_returns_not_found_error() {
    let rt = runtime_with(vec![]);
    let err = rt.invoke_skill("nope", "").await.unwrap_err();
    assert!(matches!(err, SkillInvocationError::NotFound { .. }));
}

#[tokio::test]
async fn test_disabled_skill_returns_disabled_error() {
    let skill = sample_skill("foo", "body", SkillContext::Inline, true, false);
    let rt = runtime_with(vec![skill]);
    let err = rt.invoke_skill("foo", "").await.unwrap_err();
    assert!(matches!(err, SkillInvocationError::Disabled { .. }));
}

#[tokio::test]
async fn test_disable_model_invocation_returns_hidden_error() {
    let skill = sample_skill("foo", "body", SkillContext::Inline, false, true);
    let rt = runtime_with(vec![skill]);
    let err = rt.invoke_skill("foo", "").await.unwrap_err();
    assert!(matches!(err, SkillInvocationError::HiddenFromModel { .. }));
}

#[tokio::test]
async fn test_inline_skill_expands_prompt_into_new_messages() {
    let skill = sample_skill(
        "greet",
        "Hello from a skill!",
        SkillContext::Inline,
        false,
        false,
    );
    let rt = runtime_with(vec![skill]);
    let result = rt.invoke_skill("greet", "").await.expect("ok");
    match result {
        SkillInvocationResult::Inline {
            summary,
            new_messages,
        } => {
            assert!(summary.contains("greet"));
            assert_eq!(new_messages.len(), 1);
            // Serialized UserMessage should carry the expanded prompt.
            let json = &new_messages[0];
            let text = serde_json::to_string(json).unwrap();
            assert!(
                text.contains("Hello from a skill"),
                "expanded prompt must appear in new_messages; got: {text}"
            );
        }
        other => panic!("expected Inline, got {other:?}"),
    }
}

#[tokio::test]
async fn test_inline_skill_expands_arguments() {
    // `expand_skill_prompt_simple` substitutes $ARGUMENTS with the
    // raw args string. Prove the substitution happens.
    let skill = sample_skill(
        "echo",
        "You said: $ARGUMENTS",
        SkillContext::Inline,
        false,
        false,
    );
    let rt = runtime_with(vec![skill]);
    let result = rt.invoke_skill("echo", "hello world").await.expect("ok");
    let json = match result {
        SkillInvocationResult::Inline { new_messages, .. } => new_messages[0].clone(),
        _ => panic!("expected Inline"),
    };
    let text = serde_json::to_string(&json).unwrap();
    assert!(
        text.contains("hello world"),
        "args should be substituted; got: {text}"
    );
}

#[tokio::test]
async fn test_fork_skill_without_engine_fails_forked() {
    let skill = sample_skill("run", "Run something", SkillContext::Fork, false, false);
    let rt = runtime_with(vec![skill]);
    let err = rt.invoke_skill("run", "").await.unwrap_err();
    match err {
        SkillInvocationError::Forked { reason } => {
            assert!(
                reason.contains("no AgentQueryEngine"),
                "must explain why; got: {reason}"
            );
        }
        other => panic!("expected Forked, got {other:?}"),
    }
}

struct StubEngine;

#[async_trait]
impl AgentQueryEngine for StubEngine {
    async fn execute_query(
        &self,
        prompt: &str,
        _config: AgentQueryConfig,
    ) -> anyhow::Result<AgentQueryResult> {
        Ok(AgentQueryResult {
            response_text: Some(format!("child got: {prompt}")),
            messages: Vec::new(),
            turns: 1,
            input_tokens: 10,
            output_tokens: 5,
            tool_use_count: 0,
            cancelled: false,
        })
    }
}

#[tokio::test]
async fn test_fork_skill_with_engine_routes_through_agent_query() {
    let skill = sample_skill(
        "analyze",
        "Analyze the input: $ARGUMENTS",
        SkillContext::Fork,
        false,
        false,
    );
    let rt = runtime_with(vec![skill]).with_agent_engine(Arc::new(StubEngine));

    let result = rt.invoke_skill("analyze", "a signal").await.expect("ok");
    match result {
        SkillInvocationResult::Forked { agent_id, output } => {
            assert!(agent_id.starts_with("skill-analyze-"));
            assert!(
                output.contains("a signal"),
                "child prompt must reflect args; got: {output}"
            );
        }
        other => panic!("expected Forked, got {other:?}"),
    }
}

#[tokio::test]
async fn test_name_normalization_strips_leading_slash() {
    // Users may invoke `/greet` in slash-command style; the
    // runtime should strip the slash.
    let skill = sample_skill("greet", "Hi!", SkillContext::Inline, false, false);
    let rt = runtime_with(vec![skill]);
    let ok = rt.invoke_skill("/greet", "").await;
    assert!(ok.is_ok(), "leading slash should normalize away");
}
