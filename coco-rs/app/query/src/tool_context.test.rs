use std::sync::Arc;

use coco_tool_runtime::ToolRegistry;
use coco_types::PermissionMode;
use coco_types::ThinkingLevel;
use coco_types::ToolAppState;
use pretty_assertions::assert_eq;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::config::QueryEngineConfig;

fn test_config() -> QueryEngineConfig {
    QueryEngineConfig {
        model_id: "claude-test".into(),
        permission_mode: PermissionMode::Default,
        context_window: 200_000,
        max_output_tokens: 8_192,
        session_id: "session-abc".into(),
        ..Default::default()
    }
}

fn factory(config: QueryEngineConfig) -> ToolContextFactory {
    factory_with_live_rules(config, Arc::new(RwLock::new(Vec::new())))
}

fn factory_with_live_rules(
    config: QueryEngineConfig,
    live_command_rules: Arc<RwLock<Vec<coco_types::PermissionRule>>>,
) -> ToolContextFactory {
    ToolContextFactory {
        config,
        tools: Arc::new(ToolRegistry::new()),
        cancel: CancellationToken::new(),
        mailbox: None,
        task_list: None,
        todo_list: None,
        task_handle: None,
        permission_bridge: None,
        app_state: None,
        file_read_state: None,
        file_history: None,
        config_home: None,
        hook_handle: None,
        agent_handle: None,
        skill_handle: None,
        tool_schema_validator: None,
        agent_catalog: None,
        live_command_rules,
    }
}

#[tokio::test]
async fn test_factory_main_loop_model_defaults_to_config_model_id() {
    // Pre-A contract: when no current_model_id override is supplied,
    // main_loop_model mirrors the static config.model_id. This path
    // is used by tests and legacy single-client constructions that
    // don't have a ModelRuntime.
    let config = test_config();
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(ctx.main_loop_model, "claude-test");
}

#[tokio::test]
async fn test_factory_honors_current_model_id_override() {
    // Pre-A fix: after a fallback switch, the engine passes the
    // active-slot model id via ToolContextOverrides so tools and
    // subagents see post-fallback state instead of the static config
    // value (which was set at session bootstrap and never updated).
    let config = test_config();
    let ctx = factory(config)
        .build(ToolContextOverrides {
            current_model_id: Some("fallback-model".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(ctx.main_loop_model, "fallback-model");
}

#[tokio::test]
async fn test_factory_threads_tool_reference_capability() {
    // The engine derives `current_model_supports_tool_reference` from
    // the active client's `ModelInfo` and passes it through overrides.
    // The factory must surface it on the built `ToolUseContext` so
    // `ToolSearchTool::execute` can branch into the cache-friendly
    // path on capable models.
    let config = test_config();
    let ctx_capable = factory(config.clone())
        .build(ToolContextOverrides {
            current_model_supports_tool_reference: true,
            ..Default::default()
        })
        .await;
    assert!(ctx_capable.model_supports_tool_reference);

    let ctx_incapable = factory(config)
        .build(ToolContextOverrides {
            current_model_supports_tool_reference: false,
            ..Default::default()
        })
        .await;
    assert!(!ctx_incapable.model_supports_tool_reference);
}

#[tokio::test]
async fn test_factory_threads_client_side_tool_search_capability() {
    // Same plumbing as `tool_reference` — the client-side capability
    // is the universal cousin (no Anthropic beta dependency). When
    // both capabilities are absent, `ctx.tool_search_active()` is
    // false and `ToolSearch` hides from the model.
    let config = test_config();

    let ctx_neither = factory(config.clone()).build(Default::default()).await;
    assert!(!ctx_neither.model_supports_client_side_tool_search);
    assert!(
        !ctx_neither.tool_search_active(),
        "no capability → tool_search inactive even if feature on"
    );

    let ctx_client_only = factory(config.clone())
        .build(ToolContextOverrides {
            current_model_supports_client_side_tool_search: true,
            ..Default::default()
        })
        .await;
    assert!(ctx_client_only.model_supports_client_side_tool_search);
    assert!(!ctx_client_only.model_supports_tool_reference);
    assert!(
        ctx_client_only.tool_search_active(),
        "client-side cap alone is sufficient when feature on"
    );

    let ctx_server_only = factory(config)
        .build(ToolContextOverrides {
            current_model_supports_tool_reference: true,
            ..Default::default()
        })
        .await;
    assert!(
        ctx_server_only.tool_search_active(),
        "server-side cap alone is sufficient when feature on"
    );
}

#[tokio::test]
async fn test_factory_honors_is_non_interactive() {
    let mut config = test_config();
    config.is_non_interactive = true;
    let ctx = factory(config).build(Default::default()).await;
    assert!(ctx.is_non_interactive);
}

#[tokio::test]
async fn test_factory_honors_max_budget_usd() {
    let mut config = test_config();
    config.max_budget_usd = Some(12.5);
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(ctx.max_budget_usd, Some(12.5));
}

#[tokio::test]
async fn test_factory_maps_system_prompt_to_custom() {
    let mut config = test_config();
    config.system_prompt = Some("custom prompt body".into());
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(
        ctx.custom_system_prompt.as_deref(),
        Some("custom prompt body")
    );
}

#[tokio::test]
async fn test_factory_honors_append_system_prompt() {
    let mut config = test_config();
    config.append_system_prompt = Some("extra rules".into());
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(ctx.append_system_prompt.as_deref(), Some("extra rules"));
}

#[tokio::test]
async fn test_factory_honors_thinking_level() {
    let mut config = test_config();
    config.thinking_level = Some(ThinkingLevel::medium());
    let ctx = factory(config).build(Default::default()).await;
    let level = ctx.thinking_level.expect("thinking level must propagate");
    assert_eq!(level.effort, ThinkingLevel::medium().effort);
}

#[tokio::test]
async fn test_factory_uses_live_permission_mode_from_app_state() {
    let mut config = test_config();
    config.permission_mode = PermissionMode::Default;
    let state = Arc::new(RwLock::new(ToolAppState::default()));
    state.write().await.permission_mode = Some(PermissionMode::Plan);
    let f = ToolContextFactory {
        app_state: Some(state),
        ..factory(config)
    };
    let ctx = f.build(Default::default()).await;
    assert_eq!(ctx.permission_context.mode, PermissionMode::Plan);
}

#[tokio::test]
async fn test_factory_falls_back_to_config_permission_mode_without_app_state() {
    let mut config = test_config();
    config.permission_mode = PermissionMode::AcceptEdits;
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(ctx.permission_context.mode, PermissionMode::AcceptEdits);
}

#[tokio::test]
async fn test_factory_passes_user_message_id_override() {
    let ctx = factory(test_config())
        .build(ToolContextOverrides {
            user_message_id: Some("u-123".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(ctx.user_message_id.as_deref(), Some("u-123"));
}

#[tokio::test]
async fn test_factory_threads_progress_tx_override_into_context() {
    // Phase 9 — progress forwarding. The engine builds one mpsc
    // channel per session, clones the tx into every `ToolUseContext`
    // built for that session, and drains the rx to `TuiOnlyEvent::
    // ToolProgress`. The factory is the one-place that wires the tx
    // into the context; a test-level override verifies the plumbing
    // without standing up a full engine.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<coco_tool_runtime::ToolProgress>();
    let ctx = factory(test_config())
        .build(ToolContextOverrides {
            progress_tx: Some(tx),
            ..Default::default()
        })
        .await;
    let ctx_tx = ctx
        .progress_tx
        .clone()
        .expect("progress_tx must propagate from overrides");
    ctx_tx
        .send(coco_tool_runtime::ToolProgress {
            tool_use_id: "abc".into(),
            parent_tool_use_id: None,
            data: serde_json::json!({"status": "running"}),
        })
        .unwrap();
    let got = rx
        .recv()
        .await
        .expect("drain side must receive tool progress");
    assert_eq!(got.tool_use_id, "abc");
}

#[tokio::test]
async fn test_factory_defaults_agent_handle_to_noop() {
    // Without `with_agent_handle`, the factory must hand out the
    // NoOp fallback so AgentTool invocations fail with a clean
    // "not available" error rather than panicking.
    let ctx = factory(test_config()).build(Default::default()).await;
    // Call a NoOp method — the NoOp impl returns Err, but the key
    // point is that the handle is installed (not a null pointer).
    let res = ctx.agent.send_message("any", "ping").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_factory_installs_custom_agent_handle() {
    use async_trait::async_trait;
    use coco_tool_runtime::AgentHandle;
    use coco_tool_runtime::AgentSpawnRequest;
    use coco_tool_runtime::AgentSpawnResponse;

    struct MarkerHandle;
    #[async_trait]
    impl AgentHandle for MarkerHandle {
        async fn spawn_agent(
            &self,
            _request: AgentSpawnRequest,
        ) -> Result<AgentSpawnResponse, String> {
            Err("marker".into())
        }
        async fn send_message(&self, _to: &str, _content: &str) -> Result<String, String> {
            Ok("marker".into())
        }
        async fn create_team(&self, _name: &str) -> Result<String, String> {
            Err("marker".into())
        }
        async fn delete_team(&self) -> Result<String, String> {
            Err("marker".into())
        }
        // resume_agent uses the trait default impl.
        async fn query_agent_status(&self, _agent_id: &str) -> Result<AgentSpawnResponse, String> {
            Err("marker".into())
        }
        async fn get_agent_output(&self, _agent_id: &str) -> Result<String, String> {
            Err("marker".into())
        }
        async fn background_agent(&self, _agent_id: &str) -> Result<(), String> {
            Err("marker".into())
        }
    }

    let f = ToolContextFactory {
        agent_handle: Some(Arc::new(MarkerHandle)),
        ..factory(test_config())
    };
    let ctx = f.build(Default::default()).await;
    // `send_message` on the marker returns Ok("marker") — proves
    // the factory installed our handle, not the NoOp fallback.
    let res = ctx.agent.send_message("any", "ping").await;
    assert_eq!(res.as_deref().ok(), Some("marker"));
}

#[tokio::test]
async fn test_factory_propagates_cwd_override_from_config() {
    // Phase 6 Workstream C: QueryEngineConfig.cwd_override must
    // reach every ToolUseContext built by the factory so worktree-
    // isolated subagents see their worktree path on every tool call.
    use std::path::PathBuf;
    let override_path = PathBuf::from("/tmp/worktree-test-XYZ");
    let mut config = test_config();
    config.cwd_override = Some(override_path.clone());
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(
        ctx.cwd_override.as_ref(),
        Some(&override_path),
        "factory must install cwd_override on every ToolUseContext"
    );
}

#[tokio::test]
async fn test_factory_threads_allow_rules_from_config() {
    use coco_types::PermissionBehavior;
    use coco_types::PermissionRule;
    use coco_types::PermissionRuleSource;
    use coco_types::PermissionRuleValue;
    let mut config = test_config();
    let mut rules = std::collections::HashMap::new();
    rules.insert(
        PermissionRuleSource::UserSettings,
        vec![PermissionRule {
            source: PermissionRuleSource::UserSettings,
            behavior: PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Read".into(),
                rule_content: None,
            },
        }],
    );
    config.allow_rules = rules.clone();
    let ctx = factory(config).build(Default::default()).await;
    // PermissionRule doesn't impl PartialEq (foreign-crate type);
    // compare via JSON serialization for stable structural equality.
    assert_eq!(
        serde_json::to_string(&ctx.permission_context.allow_rules).unwrap(),
        serde_json::to_string(&rules).unwrap(),
        "factory must install allow_rules from config"
    );
}

#[tokio::test]
async fn test_factory_threads_deny_rules_from_config() {
    use coco_types::PermissionBehavior;
    use coco_types::PermissionRule;
    use coco_types::PermissionRuleSource;
    use coco_types::PermissionRuleValue;
    let mut config = test_config();
    let mut rules = std::collections::HashMap::new();
    rules.insert(
        PermissionRuleSource::PolicySettings,
        vec![PermissionRule {
            source: PermissionRuleSource::PolicySettings,
            behavior: PermissionBehavior::Deny,
            value: PermissionRuleValue {
                tool_pattern: "Bash".into(),
                rule_content: None,
            },
        }],
    );
    config.deny_rules = rules.clone();
    let ctx = factory(config).build(Default::default()).await;
    assert_eq!(
        serde_json::to_string(&ctx.permission_context.deny_rules).unwrap(),
        serde_json::to_string(&rules).unwrap(),
    );
}

#[tokio::test]
async fn test_factory_cwd_override_none_when_config_unset() {
    // Baseline: no override in config → no override on context.
    // Guards against a stray default slipping in.
    let ctx = factory(test_config()).build(Default::default()).await;
    assert!(
        ctx.cwd_override.is_none(),
        "factory must not synthesize a cwd_override when config has none"
    );
}

#[tokio::test]
async fn test_factory_defaults_skill_handle_to_noop_unavailable() {
    // NoOpSkillHandle returns `Unavailable` — verifies the factory
    // installs it when no real runtime is wired.
    let ctx = factory(test_config()).build(Default::default()).await;
    let err = ctx
        .skill
        .invoke_skill("any", "", coco_tool_runtime::SubagentInheritance::default())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        coco_tool_runtime::SkillInvocationError::Unavailable { .. }
    ));
}

// ── Live Command-source rule merge tests ──
//
// Verifies the `engine.live_command_rules` Arc threaded into the
// factory is read at every `build()` and folded into
// `permission_context.allow_rules[Command]`. TS parity:
// `getAppState().alwaysAllowRules.command` is read each permission
// check; per-engine = per-user-msg scoping comes from the engine's
// fresh-per-turn lifecycle (see `engine_live_rules` module docs).

fn skill_cmd_rule(tool_pattern: &str) -> coco_types::PermissionRule {
    coco_types::PermissionRule {
        source: coco_types::PermissionRuleSource::Command,
        behavior: coco_types::PermissionBehavior::Allow,
        value: coco_types::PermissionRuleValue {
            tool_pattern: tool_pattern.into(),
            rule_content: None,
        },
    }
}

#[tokio::test]
async fn test_factory_returns_base_allow_rules_when_live_rules_empty() {
    // Zero-clone fast path: when the live store is empty, the
    // factory must hand back the config's base allow_rules verbatim
    // (no Command entry inserted). This is the common case — the
    // overwhelming majority of turns have no skill-emitted rules.
    let config = test_config();
    let base_clone = config.allow_rules.clone();
    let ctx = factory(config).build(Default::default()).await;
    // PermissionRule doesn't impl PartialEq; compare via JSON roundtrip.
    assert_eq!(
        serde_json::to_string(&ctx.permission_context.allow_rules).unwrap(),
        serde_json::to_string(&base_clone).unwrap(),
        "fast path must hand back base allow_rules verbatim"
    );
    // No Command source ever materialised because we never inserted one.
    assert!(
        !ctx.permission_context
            .allow_rules
            .contains_key(&coco_types::PermissionRuleSource::Command)
    );
}

#[tokio::test]
async fn test_factory_merges_live_rules_into_command_source() {
    // Skill emitted a rule earlier this user message — factory.build
    // for the next batch must surface it under the Command source so
    // the evaluator sees it.
    let config = test_config();
    let store: Arc<RwLock<Vec<coco_types::PermissionRule>>> =
        Arc::new(RwLock::new(vec![skill_cmd_rule("Read")]));
    let ctx = factory_with_live_rules(config, store.clone())
        .build(Default::default())
        .await;
    let cmd_rules = ctx
        .permission_context
        .allow_rules
        .get(&coco_types::PermissionRuleSource::Command)
        .expect("Command source should be populated from live rules");
    assert_eq!(cmd_rules.len(), 1);
    assert_eq!(cmd_rules[0].value.tool_pattern, "Read");
}

#[tokio::test]
async fn test_factory_cross_batch_propagation_within_same_arc() {
    // Same Arc shared with the (hypothetical) engine + handle:
    // batch 1's `build()` sees `[Read]`, then a "tool emission"
    // appends `[Edit]`, batch 2's `build()` sees both. This is the
    // cross-turn-within-user-msg path that TS gets via the
    // closure-captured appState.
    let config = test_config();
    let store: Arc<RwLock<Vec<coco_types::PermissionRule>>> =
        Arc::new(RwLock::new(vec![skill_cmd_rule("Read")]));
    let factory = factory_with_live_rules(config, store.clone());

    let ctx1 = factory.build(Default::default()).await;
    let cmd1 = ctx1
        .permission_context
        .allow_rules
        .get(&coco_types::PermissionRuleSource::Command)
        .expect("batch 1 should see Read");
    assert_eq!(cmd1.len(), 1);

    // Simulate a tool emission between batches.
    store.write().await.push(skill_cmd_rule("Edit"));

    let ctx2 = factory.build(Default::default()).await;
    let cmd2 = ctx2
        .permission_context
        .allow_rules
        .get(&coco_types::PermissionRuleSource::Command)
        .expect("batch 2 should see both rules");
    let patterns: Vec<&str> = cmd2.iter().map(|r| r.value.tool_pattern.as_str()).collect();
    assert_eq!(patterns, vec!["Read", "Edit"]);
}

#[tokio::test]
async fn test_factory_isolates_per_arc_engines() {
    // Two factories with two independent Arc-stores ≡ two engines
    // (= two user messages, or main + subagent). A write into one
    // must NOT be visible through the other.
    let config_a = test_config();
    let config_b = test_config();
    let store_a: Arc<RwLock<Vec<coco_types::PermissionRule>>> = Arc::new(RwLock::new(Vec::new()));
    let store_b: Arc<RwLock<Vec<coco_types::PermissionRule>>> = Arc::new(RwLock::new(Vec::new()));

    store_a.write().await.push(skill_cmd_rule("Read"));

    let ctx_a = factory_with_live_rules(config_a, store_a)
        .build(Default::default())
        .await;
    let ctx_b = factory_with_live_rules(config_b, store_b)
        .build(Default::default())
        .await;

    assert!(
        ctx_a
            .permission_context
            .allow_rules
            .contains_key(&coco_types::PermissionRuleSource::Command)
    );
    assert!(
        !ctx_b
            .permission_context
            .allow_rules
            .contains_key(&coco_types::PermissionRuleSource::Command)
    );
}

#[tokio::test]
async fn test_factory_preserves_base_command_rules_when_merging() {
    // If the user has set Command-source rules at the config layer
    // (e.g. via CLI `--allow Command:Read`, though uncommon), the
    // factory must append live rules to those rather than replace
    // the base.
    let mut config = test_config();
    config
        .allow_rules
        .entry(coco_types::PermissionRuleSource::Command)
        .or_default()
        .push(skill_cmd_rule("Glob"));

    let store: Arc<RwLock<Vec<coco_types::PermissionRule>>> =
        Arc::new(RwLock::new(vec![skill_cmd_rule("Read")]));
    let ctx = factory_with_live_rules(config, store)
        .build(Default::default())
        .await;
    let cmd_rules = ctx
        .permission_context
        .allow_rules
        .get(&coco_types::PermissionRuleSource::Command)
        .expect("Command source should retain base + live entries");
    let patterns: Vec<&str> = cmd_rules
        .iter()
        .map(|r| r.value.tool_pattern.as_str())
        .collect();
    assert_eq!(patterns, vec!["Glob", "Read"]);
}
