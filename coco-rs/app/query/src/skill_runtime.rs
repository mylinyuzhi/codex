//! `QuerySkillRuntime` — real implementation of
//! [`coco_tool_runtime::SkillHandle`] backed by [`coco_skills::SkillManager`].
//!
//! TS: `src/tools/SkillTool/SkillTool.ts` — skill resolution +
//! inline vs forked routing; `src/tools/SkillTool/prompt.ts` —
//! dynamic skill listing.
//!
//! Phase 7-β completion: prior slice scaffolded the `SkillHandle`
//! trait with a `NoOpSkillHandle` that returned `Unavailable`; this
//! module lands the real implementation.
//!
//! ## Resolution order
//!
//! 1. Normalize name (strip leading `/`).
//! 2. Lookup in `SkillManager` (canonical name + aliases).
//! 3. Reject disabled skills (`SkillDefinition.disabled`).
//! 4. Reject model-hidden skills when `SkillTool` invokes them
//!    (`SkillDefinition.disable_model_invocation`).
//! 5. Branch on `SkillContext::{Inline, Fork}`:
//!    - Inline: expand arguments via `skill_advanced::expand_skill_prompt`
//!      and return `SkillInvocationResult::Inline { summary, new_messages }`.
//!    - Fork: route the expanded prompt through the installed
//!      `AgentQueryEngine` as a sub-agent query, return
//!      `SkillInvocationResult::Forked { agent_id, output }`.
//!
//! ## Parent tool-use id tagging
//!
//! Per plan I5 and TS `SkillTool.ts:728` `tagMessagesWithToolUseID`,
//! inline-expansion messages are tagged with
//! `parent_tool_use_id = <the SkillTool call id>` so they group
//! with the parent tool_result in transcripts. The runtime doesn't
//! know the SkillTool's `tool_use_id` because `SkillHandle::invoke_skill`
//! doesn't surface it — `SkillTool` downstream is responsible for
//! tagging before pushing to history. This module returns
//! untagged `UserMessage` values; the caller tags.

use std::sync::Arc;

use async_trait::async_trait;
use coco_skills::SkillContext;
use coco_skills::SkillManager;
use coco_tool_runtime::AgentQueryConfig;
use coco_tool_runtime::AgentQueryEngineRef;
use coco_tool_runtime::SkillHandle;
use coco_tool_runtime::SkillInvocationError;
use coco_tool_runtime::SkillInvocationResult;
use tokio::sync::RwLock;

/// Real skill-runtime implementation.
///
/// Holds an `Arc<RwLock<SkillManager>>` so the skill set can be
/// hot-reloaded (via file-watcher callbacks) without rebuilding the
/// handle. Fork routing goes through the same
/// [`AgentQueryEngineRef`] that `SwarmAgentHandle::spawn_subagent`
/// uses, keeping one subagent execution path.
pub struct QuerySkillRuntime {
    manager: Arc<RwLock<SkillManager>>,
    /// Optional agent query engine for fork-mode skills. `None`
    /// returns `SkillInvocationError::Forked` for any fork
    /// invocation — fine for sessions without a subagent runtime,
    /// loud-failing instead of silently ignoring.
    agent_engine: Option<AgentQueryEngineRef>,
}

impl QuerySkillRuntime {
    pub fn new(manager: Arc<RwLock<SkillManager>>) -> Self {
        Self {
            manager,
            agent_engine: None,
        }
    }

    /// Install the agent-query engine used for fork-mode skills.
    /// Without this, fork invocations fail with `Forked { reason }`.
    pub fn with_agent_engine(mut self, engine: AgentQueryEngineRef) -> Self {
        self.agent_engine = Some(engine);
        self
    }
}

#[async_trait]
impl SkillHandle for QuerySkillRuntime {
    async fn invoke_skill(
        &self,
        name: &str,
        args: &str,
    ) -> Result<SkillInvocationResult, SkillInvocationError> {
        let name = coco_tools::tools::skill_advanced::normalize_skill_name(name);
        let manager = self.manager.read().await;
        let skill = manager
            .get(name)
            .cloned()
            .ok_or_else(|| SkillInvocationError::NotFound {
                name: name.to_string(),
            })?;
        drop(manager);

        if skill.disabled {
            return Err(SkillInvocationError::Disabled {
                name: skill.name.clone(),
            });
        }
        if skill.disable_model_invocation {
            return Err(SkillInvocationError::HiddenFromModel {
                name: skill.name.clone(),
            });
        }

        // Expand argument substitutions. TS parity:
        // `SkillTool.ts:565-597` runs the expander before either
        // inline injection or fork spawn. We reuse the existing
        // `coco-tools::skill_advanced::expand_skill_prompt_simple`
        // which handles `$ARGS`, numeric positionals, and named
        // argument substitution.
        let expanded_prompt =
            coco_tools::tools::skill_advanced::expand_skill_prompt_simple(&skill.prompt, args);

        match skill.context {
            SkillContext::Inline => {
                // Inline: surface the expanded prompt as a new user
                // message the next turn will see. The SkillTool
                // caller is responsible for tagging
                // `parent_tool_use_id` before pushing to history.
                let expanded_message = coco_messages::create_user_message(&expanded_prompt);
                let summary = format!(
                    "Inline skill '{}' expanded ({} chars)",
                    skill.name,
                    expanded_prompt.len()
                );
                let new_messages = vec![
                    serde_json::to_value(&expanded_message).unwrap_or(serde_json::Value::Null),
                ];
                Ok(SkillInvocationResult::Inline {
                    summary,
                    new_messages,
                })
            }
            SkillContext::Fork => {
                // Fork: route the expanded prompt through the
                // subagent engine. TS parity: `SkillTool.ts:636-692`
                // spawns a fork agent with the skill's
                // allowed_tools + model override.
                let engine =
                    self.agent_engine
                        .clone()
                        .ok_or_else(|| SkillInvocationError::Forked {
                            reason: format!(
                                "Skill '{}' is fork-mode but no AgentQueryEngine is installed. \
                             Use QuerySkillRuntime::with_agent_engine at session bootstrap.",
                                skill.name
                            ),
                        })?;

                let agent_id = format!(
                    "skill-{}-{}",
                    skill.name,
                    &uuid::Uuid::new_v4().simple().to_string()[..8]
                );
                let config = AgentQueryConfig {
                    system_prompt: String::new(),
                    model: skill.model.clone().unwrap_or_default(),
                    max_turns: None,
                    context_window: None,
                    max_output_tokens: None,
                    allowed_tools: skill.allowed_tools.clone().unwrap_or_default(),
                    preserve_tool_use_results: false,
                    permission_mode: None,
                    agent_id: Some(agent_id.clone()),
                    is_teammate: false,
                    plan_mode_required: false,
                    session_id: None,
                    bypass_permissions_available: false,
                    cwd_override: None,
                    // Skill fork: inherits the parent session's Main
                    // role by deferring to the factory default.
                    model_role: None,
                    fork_context_messages: Vec::new(),
                };

                let query_result = engine
                    .execute_query(&expanded_prompt, config)
                    .await
                    .map_err(|e| SkillInvocationError::Forked {
                        reason: e.to_string(),
                    })?;
                let output = query_result
                    .response_text
                    .unwrap_or_else(|| "(no output)".into());
                Ok(SkillInvocationResult::Forked { agent_id, output })
            }
        }
    }
}

#[cfg(test)]
#[path = "skill_runtime.test.rs"]
mod tests;
