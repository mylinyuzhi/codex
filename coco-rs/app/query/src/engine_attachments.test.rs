use std::fs;
use std::sync::Arc;

use coco_inference::AISdkError;
use coco_inference::ApiClient;
use coco_inference::AssistantContentPart;
use coco_inference::FinishReason;
use coco_inference::LanguageModel;
use coco_inference::LanguageModelCallOptions;
use coco_inference::LanguageModelGenerateResult;
use coco_inference::LanguageModelStreamResult;
use coco_inference::RetryConfig;
use coco_inference::TextPart;
use coco_inference::UnifiedFinishReason;
use coco_inference::Usage;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

use crate::config::QueryEngineConfig;
use crate::engine::QueryEngine;

/// Minimal mock — drain logic doesn't drive the model, but `QueryEngine`
/// requires a non-null client to construct.
struct StubModel;

#[async_trait::async_trait]
impl LanguageModel for StubModel {
    fn provider(&self) -> &str {
        "stub"
    }
    fn model_id(&self) -> &str {
        "stub"
    }
    async fn do_generate(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, AISdkError> {
        Ok(LanguageModelGenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: "".into(),
                provider_metadata: None,
            })],
            usage: Usage::new(0, 0),
            finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
    }
    async fn do_stream(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, AISdkError> {
        let result = self.do_generate(options).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

fn make_test_engine() -> QueryEngine {
    let model = Arc::new(StubModel);
    let client = Arc::new(ApiClient::with_default_fingerprint(
        model,
        RetryConfig::default(),
    ));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();
    QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None)
}

fn make_test_ctx_with_cwd(cwd: std::path::PathBuf) -> ToolUseContext {
    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(cwd);
    ctx
}

#[tokio::test]
async fn drain_empty_set_is_noop() {
    let engine = make_test_engine();
    let dir = tempdir().unwrap();
    let ctx = make_test_ctx_with_cwd(dir.path().to_path_buf());

    engine.drain_nested_memory_triggers(&ctx).await;
    let pending = engine.take_pending_nested_memory().await;
    assert!(
        pending.is_empty(),
        "empty trigger Set must produce no pending entries"
    );
}

#[tokio::test]
async fn drain_traverses_intermediate_claude_md() {
    // CWD = /tmp/proj. Trigger file = /tmp/proj/sub/handler.rs.
    // Should pick up /tmp/proj/sub/CLAUDE.md (descendant of CWD).
    let root = tempdir().unwrap();
    let proj = root.path().join("proj");
    let sub = proj.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("CLAUDE.md"), "# sub").unwrap();
    let trigger = sub.join("handler.rs");
    fs::write(&trigger, "").unwrap();

    let engine = make_test_engine();
    let ctx = make_test_ctx_with_cwd(proj.clone());

    // Simulate a tool push.
    {
        let mut triggers = ctx.nested_memory_attachment_triggers.write().await;
        triggers.insert(trigger.canonicalize().unwrap().display().to_string());
    }

    engine.drain_nested_memory_triggers(&ctx).await;

    // Trigger Set is now empty.
    assert!(
        ctx.nested_memory_attachment_triggers
            .read()
            .await
            .is_empty(),
        "drain must clear the trigger Set in place"
    );

    let pending = engine.take_pending_nested_memory().await;
    assert_eq!(pending.len(), 1, "expected 1 entry, got {pending:?}");
    assert!(
        pending[0].path.contains("sub/CLAUDE.md") || pending[0].path.contains("sub\\CLAUDE.md"),
        "expected sub/CLAUDE.md path, got {}",
        pending[0].path
    );
    assert_eq!(pending[0].content, "# sub");
}

#[tokio::test]
async fn drain_dedupes_via_session_loaded_set() {
    // Two file reads under the same subtree should each surface
    // sub/CLAUDE.md — but only the first injection survives the
    // session-level dedup.
    let root = tempdir().unwrap();
    let proj = root.path().join("proj");
    let sub = proj.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("CLAUDE.md"), "# sub").unwrap();
    let trigger1 = sub.join("a.rs");
    let trigger2 = sub.join("b.rs");
    fs::write(&trigger1, "").unwrap();
    fs::write(&trigger2, "").unwrap();

    let engine = make_test_engine();

    // Batch 1: trigger a.rs.
    {
        let ctx = make_test_ctx_with_cwd(proj.clone());
        ctx.nested_memory_attachment_triggers
            .write()
            .await
            .insert(trigger1.canonicalize().unwrap().display().to_string());
        engine.drain_nested_memory_triggers(&ctx).await;
    }
    let first = engine.take_pending_nested_memory().await;
    assert_eq!(first.len(), 1, "first batch should surface sub/CLAUDE.md");

    // Batch 2: trigger b.rs in the same subtree.
    {
        let ctx = make_test_ctx_with_cwd(proj.clone());
        ctx.nested_memory_attachment_triggers
            .write()
            .await
            .insert(trigger2.canonicalize().unwrap().display().to_string());
        engine.drain_nested_memory_triggers(&ctx).await;
    }
    let second = engine.take_pending_nested_memory().await;
    assert!(
        second.is_empty(),
        "second batch must not re-inject already-loaded sub/CLAUDE.md, got {second:?}"
    );

    // After clearing, third batch should re-inject.
    engine.clear_loaded_nested_memory_paths().await;
    {
        let ctx = make_test_ctx_with_cwd(proj.clone());
        ctx.nested_memory_attachment_triggers
            .write()
            .await
            .insert(trigger1.canonicalize().unwrap().display().to_string());
        engine.drain_nested_memory_triggers(&ctx).await;
    }
    let third = engine.take_pending_nested_memory().await;
    assert_eq!(
        third.len(),
        1,
        "after clear_loaded_nested_memory_paths, should re-inject"
    );
}

#[tokio::test]
async fn drain_picks_up_agents_md() {
    // Coco-rs extension: AGENTS.md alongside CLAUDE.md.
    let root = tempdir().unwrap();
    let proj = root.path().join("proj");
    let sub = proj.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("AGENTS.md"), "# agents").unwrap();
    let trigger = sub.join("f.rs");
    fs::write(&trigger, "").unwrap();

    let engine = make_test_engine();
    let ctx = make_test_ctx_with_cwd(proj.clone());
    ctx.nested_memory_attachment_triggers
        .write()
        .await
        .insert(trigger.canonicalize().unwrap().display().to_string());

    engine.drain_nested_memory_triggers(&ctx).await;
    let pending = engine.take_pending_nested_memory().await;
    assert_eq!(pending.len(), 1, "expected AGENTS.md to be picked up");
    assert!(pending[0].path.contains("AGENTS.md"));
}

#[tokio::test]
async fn drain_file_outside_cwd_emits_nothing() {
    // File outside CWD → nested_dirs empty → nothing to load.
    // (Phase 1 + Phase 4 conditional rules are still stubbed.)
    let root = tempdir().unwrap();
    let proj = root.path().join("proj");
    let elsewhere = root.path().join("other");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("CLAUDE.md"), "x").unwrap();
    let trigger = elsewhere.join("file.rs");
    fs::write(&trigger, "").unwrap();

    let engine = make_test_engine();
    let ctx = make_test_ctx_with_cwd(proj.clone());
    ctx.nested_memory_attachment_triggers
        .write()
        .await
        .insert(trigger.canonicalize().unwrap().display().to_string());

    engine.drain_nested_memory_triggers(&ctx).await;
    let pending = engine.take_pending_nested_memory().await;
    assert!(
        pending.is_empty(),
        "outside-CWD trigger must not surface intermediate CLAUDE.md, got {pending:?}"
    );
}

#[tokio::test]
async fn pending_slot_is_drained_after_take() {
    let engine = make_test_engine();
    {
        let mut p = engine.pending_nested_memory.lock().await;
        p.push(coco_system_reminder::generators::memory::NestedMemoryInfo {
            path: "/x".into(),
            content: "y".into(),
        });
    }
    let first = engine.take_pending_nested_memory().await;
    assert_eq!(first.len(), 1);

    let second = engine.take_pending_nested_memory().await;
    assert!(second.is_empty(), "take must clear the slot");
}
