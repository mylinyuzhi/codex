use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolResult;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

use super::*;
use crate::traits::DescriptionOptions;

/// A test tool that is concurrency-safe.
struct SafeTool {
    name: String,
}

#[async_trait::async_trait]
impl crate::traits::Tool for SafeTool {
    fn id(&self) -> ToolId {
        ToolId::Custom(self.name.clone())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "safe".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        input: Value,
        _ctx: &crate::context::ToolUseContext,
    ) -> Result<ToolResult<Value>, crate::error::ToolError> {
        Ok(ToolResult {
            data: input,
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

/// A test tool that is NOT concurrency-safe.
struct UnsafeTool {
    name: String,
}

#[async_trait::async_trait]
impl crate::traits::Tool for UnsafeTool {
    fn id(&self) -> ToolId {
        ToolId::Custom(self.name.clone())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "unsafe".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        false
    }
    async fn execute(
        &self,
        input: Value,
        _ctx: &crate::context::ToolUseContext,
    ) -> Result<ToolResult<Value>, crate::error::ToolError> {
        Ok(ToolResult {
            data: input,
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

fn make_call(name: &str, safe: bool) -> PendingToolCall {
    let tool: Arc<dyn crate::traits::Tool> = if safe {
        Arc::new(SafeTool { name: name.into() })
    } else {
        Arc::new(UnsafeTool { name: name.into() })
    };
    PendingToolCall {
        tool_use_id: name.into(),
        tool,
        input: json!({}),
    }
}

#[test]
fn test_partition_all_safe() {
    let executor = StreamingToolExecutor::new();
    let calls = vec![
        make_call("read1", /*safe*/ true),
        make_call("read2", true),
        make_call("read3", true),
    ];
    let batches = executor.partition(calls);
    assert_eq!(batches.len(), 1);
    assert!(matches!(&batches[0], ToolBatch::ConcurrentSafe(v) if v.len() == 3));
}

#[test]
fn test_partition_all_unsafe() {
    let executor = StreamingToolExecutor::new();
    let calls = vec![
        make_call("bash1", /*safe*/ false),
        make_call("bash2", false),
    ];
    let batches = executor.partition(calls);
    assert_eq!(batches.len(), 2);
    assert!(matches!(&batches[0], ToolBatch::SingleUnsafe(_)));
    assert!(matches!(&batches[1], ToolBatch::SingleUnsafe(_)));
}

#[test]
fn test_partition_mixed() {
    // [safe, safe, unsafe, safe]
    // -> batch1: ConcurrentSafe([safe, safe])
    // -> batch2: SingleUnsafe(unsafe)
    // -> batch3: ConcurrentSafe([safe])
    let executor = StreamingToolExecutor::new();
    let calls = vec![
        make_call("read1", true),
        make_call("read2", true),
        make_call("bash", false),
        make_call("read3", true),
    ];
    let batches = executor.partition(calls);
    assert_eq!(batches.len(), 3);
    assert!(matches!(&batches[0], ToolBatch::ConcurrentSafe(v) if v.len() == 2));
    assert!(matches!(&batches[1], ToolBatch::SingleUnsafe(_)));
    assert!(matches!(&batches[2], ToolBatch::ConcurrentSafe(v) if v.len() == 1));
}

#[test]
fn test_partition_empty() {
    let executor = StreamingToolExecutor::new();
    let batches = executor.partition(vec![]);
    assert!(batches.is_empty());
}

#[tokio::test]
async fn test_execute_single_tool() {
    let executor = StreamingToolExecutor::new();
    let ctx = crate::context::ToolUseContext::test_default();
    let call = make_call("test_tool", false);

    let result = executor.execute_single(call, &ctx).await;
    assert_eq!(result.tool_use_id, "test_tool");
    assert!(result.result.is_ok());
    assert!(result.duration_ms >= 0);
}

/// A slow tool that tracks concurrent execution via an atomic counter.
struct SlowSafeTool {
    name: String,
    concurrent_count: Arc<AtomicI32>,
    max_concurrent: Arc<AtomicI32>,
    sleep_ms: u64,
}

#[async_trait::async_trait]
impl crate::traits::Tool for SlowSafeTool {
    fn id(&self) -> ToolId {
        ToolId::Custom(self.name.clone())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "slow safe".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: &crate::context::ToolUseContext,
    ) -> Result<ToolResult<Value>, crate::error::ToolError> {
        let prev = self.concurrent_count.fetch_add(1, Ordering::SeqCst);
        // Track peak concurrency
        let current = prev + 1;
        self.max_concurrent.fetch_max(current, Ordering::SeqCst);

        tokio::time::sleep(tokio::time::Duration::from_millis(self.sleep_ms)).await;

        self.concurrent_count.fetch_sub(1, Ordering::SeqCst);
        Ok(ToolResult {
            data: json!({"tool": self.name}),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

struct PatchSafeTool {
    name: String,
    sleep_ms: u64,
    digit: i64,
}

#[async_trait::async_trait]
impl crate::traits::Tool for PatchSafeTool {
    fn id(&self) -> ToolId {
        ToolId::Custom(self.name.clone())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "patch safe".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: &crate::context::ToolUseContext,
    ) -> Result<ToolResult<Value>, crate::error::ToolError> {
        tokio::time::sleep(tokio::time::Duration::from_millis(self.sleep_ms)).await;
        let digit = self.digit;
        Ok(ToolResult {
            data: json!({"tool": self.name}),
            new_messages: vec![],
            app_state_patch: Some(Box::new(move |state| {
                state.plan_mode_attachment_count = state.plan_mode_attachment_count * 10 + digit;
            })),
        })
    }
}

#[tokio::test]
async fn test_execute_concurrent_tools() {
    let executor = StreamingToolExecutor::with_max_concurrency(10);
    let ctx = crate::context::ToolUseContext::test_default();

    let calls = vec![
        make_call("read1", /*safe*/ true),
        make_call("read2", true),
        make_call("read3", true),
    ];

    let results = executor.execute_concurrent(calls, &ctx).await;
    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(r.result.is_ok(), "concurrent tool should succeed");
    }
}

#[tokio::test]
async fn test_execute_concurrent_tools_returns_completion_order() {
    let concurrent_count = Arc::new(AtomicI32::new(0));
    let max_concurrent = Arc::new(AtomicI32::new(0));

    let make_slow = |name: &str, sleep_ms| -> PendingToolCall {
        PendingToolCall {
            tool_use_id: name.into(),
            tool: Arc::new(SlowSafeTool {
                name: name.into(),
                concurrent_count: concurrent_count.clone(),
                max_concurrent: max_concurrent.clone(),
                sleep_ms,
            }),
            input: json!({}),
        }
    };

    let executor = StreamingToolExecutor::with_max_concurrency(2);
    let ctx = crate::context::ToolUseContext::test_default();

    let results = executor
        .execute_concurrent(
            vec![make_slow("slow_first", 80), make_slow("fast_second", 10)],
            &ctx,
        )
        .await;

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].tool_use_id, "fast_second");
    assert_eq!(results[1].tool_use_id, "slow_first");
}

#[tokio::test]
async fn test_execute_concurrent_patches_apply_in_model_order() {
    let app_state = Arc::new(tokio::sync::RwLock::new(coco_types::ToolAppState::default()));
    let executor = StreamingToolExecutor::with_max_concurrency(2).with_app_state(app_state.clone());
    let ctx = crate::context::ToolUseContext::test_default();

    let make_patch = |name: &str, sleep_ms, digit| PendingToolCall {
        tool_use_id: name.into(),
        tool: Arc::new(PatchSafeTool {
            name: name.into(),
            sleep_ms,
            digit,
        }),
        input: json!({}),
    };

    let results = executor
        .execute_concurrent(
            vec![
                make_patch("slow_first", 80, 1),
                make_patch("fast_second", 10, 2),
            ],
            &ctx,
        )
        .await;

    assert_eq!(results[0].tool_use_id, "fast_second");
    assert_eq!(results[1].tool_use_id, "slow_first");
    assert_eq!(app_state.read().await.plan_mode_attachment_count, 12);
}

#[tokio::test]
async fn test_concurrent_tools_run_in_parallel() {
    let concurrent_count = Arc::new(AtomicI32::new(0));
    let max_concurrent = Arc::new(AtomicI32::new(0));

    let make_slow = |name: &str| -> PendingToolCall {
        PendingToolCall {
            tool_use_id: name.into(),
            tool: Arc::new(SlowSafeTool {
                name: name.into(),
                concurrent_count: concurrent_count.clone(),
                max_concurrent: max_concurrent.clone(),
                sleep_ms: 50,
            }),
            input: json!({}),
        }
    };

    let executor = StreamingToolExecutor::with_max_concurrency(5);
    let ctx = crate::context::ToolUseContext::test_default();

    let calls = vec![make_slow("slow1"), make_slow("slow2"), make_slow("slow3")];

    let results = executor.execute_concurrent(calls, &ctx).await;
    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(r.result.is_ok());
    }
    // All 3 tools should have been running concurrently
    assert!(
        max_concurrent.load(Ordering::SeqCst) >= 2,
        "expected at least 2 concurrent, got {}",
        max_concurrent.load(Ordering::SeqCst),
    );
}

#[tokio::test]
async fn test_execute_all_mixed_batches() {
    let executor = StreamingToolExecutor::with_max_concurrency(10);
    let ctx = crate::context::ToolUseContext::test_default();

    // [safe, safe, unsafe, safe] -> 3 batches
    let calls = vec![
        make_call("read1", true),
        make_call("read2", true),
        make_call("bash1", false),
        make_call("read3", true),
    ];

    let results = executor.execute_all(calls, &ctx).await;
    assert_eq!(results.len(), 4);
    assert_eq!(results[0].tool_use_id, "read1");
    assert_eq!(results[1].tool_use_id, "read2");
    assert_eq!(results[2].tool_use_id, "bash1");
    assert_eq!(results[3].tool_use_id, "read3");
    for r in &results {
        assert!(r.result.is_ok());
    }
}

#[tokio::test]
async fn test_semaphore_limits_concurrency() {
    let concurrent_count = Arc::new(AtomicI32::new(0));
    let max_concurrent = Arc::new(AtomicI32::new(0));

    let make_slow = |name: &str| -> PendingToolCall {
        PendingToolCall {
            tool_use_id: name.into(),
            tool: Arc::new(SlowSafeTool {
                name: name.into(),
                concurrent_count: concurrent_count.clone(),
                max_concurrent: max_concurrent.clone(),
                sleep_ms: 50,
            }),
            input: json!({}),
        }
    };

    // Only allow 2 concurrent
    let executor = StreamingToolExecutor::with_max_concurrency(2);
    let ctx = crate::context::ToolUseContext::test_default();

    let calls = vec![
        make_slow("t1"),
        make_slow("t2"),
        make_slow("t3"),
        make_slow("t4"),
    ];

    let results = executor.execute_concurrent(calls, &ctx).await;
    assert_eq!(results.len(), 4);
    // Max concurrency should be capped at 2
    assert!(
        max_concurrent.load(Ordering::SeqCst) <= 2,
        "expected max 2 concurrent, got {}",
        max_concurrent.load(Ordering::SeqCst),
    );
}

#[test]
fn test_streaming_add_tool() {
    let mut executor = StreamingToolExecutor::new();
    let tool: Arc<dyn crate::traits::Tool> = Arc::new(SafeTool {
        name: "test".into(),
    });
    executor.add_tool("t1".into(), tool.clone(), json!({}));
    executor.add_tool("t2".into(), tool, json!({}));
    assert!(executor.has_pending());
    assert!(!executor.is_complete());
}

#[test]
fn test_streaming_discard() {
    let mut executor = StreamingToolExecutor::new();
    let tool: Arc<dyn crate::traits::Tool> = Arc::new(SafeTool {
        name: "test".into(),
    });
    executor.add_tool("t1".into(), tool, json!({}));

    executor.discard();

    let updates = executor.drain_completed();
    assert_eq!(updates.len(), 1);
    match &updates[0] {
        StreamingToolUpdate::Result(r) => {
            assert_eq!(r.tool_use_id, "t1");
            assert!(r.result.is_err());
        }
        _ => panic!("expected Result, got Progress"),
    }
}

use crate::hook_handle::HookHandle;
use crate::hook_handle::PostToolUseOutcome;
use crate::hook_handle::PreToolUseOutcome;

struct CountingHookHandle {
    calls: Arc<AtomicI32>,
}

#[async_trait::async_trait]
impl HookHandle for CountingHookHandle {
    async fn run_pre_tool_use(
        &self,
        _tool_name: &str,
        _tool_use_id: &str,
        _tool_input: &Value,
    ) -> PreToolUseOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        PreToolUseOutcome {
            updated_input: Some(json!({"rewritten": true})),
            blocking_reason: Some("executor should not run hooks".into()),
            ..Default::default()
        }
    }

    async fn run_post_tool_use(
        &self,
        _tool_name: &str,
        _tool_use_id: &str,
        _tool_input: &Value,
        _tool_response: &Value,
    ) -> PostToolUseOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        PostToolUseOutcome {
            updated_output: Some(json!("rewritten by hook")),
            blocking_reason: Some("executor should not run hooks".into()),
            ..Default::default()
        }
    }

    async fn run_post_tool_use_failure(
        &self,
        _tool_name: &str,
        _tool_use_id: &str,
        _tool_input: &Value,
        _error_message: &str,
    ) -> PostToolUseOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        PostToolUseOutcome::default()
    }
}

#[tokio::test]
async fn test_executor_does_not_run_hooks() {
    let hook_calls = Arc::new(AtomicI32::new(0));
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(CountingHookHandle {
        calls: hook_calls.clone(),
    }));

    let exec = StreamingToolExecutor::new();
    let single = PendingToolCall {
        tool_use_id: "single".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({"x": 42}),
    };
    let single_result = exec.execute_single(single, &ctx).await;

    assert_eq!(single_result.result.unwrap().data, json!({"x": 42}));

    let calls = vec![
        PendingToolCall {
            tool_use_id: "a".into(),
            tool: Arc::new(SafeTool { name: "s".into() }),
            input: json!({"i": 1}),
        },
        PendingToolCall {
            tool_use_id: "b".into(),
            tool: Arc::new(SafeTool { name: "s".into() }),
            input: json!({"i": 2}),
        },
        PendingToolCall {
            tool_use_id: "c".into(),
            tool: Arc::new(SafeTool { name: "s".into() }),
            input: json!({"i": 3}),
        },
    ];
    let results = exec.execute_concurrent(calls, &ctx).await;

    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.result.is_ok()));
    assert_eq!(hook_calls.load(Ordering::SeqCst), 0);
}

// ══════════════════════════════════════════════════════════════════
// Phase 4d-α: execute_with scheduler tests
// ══════════════════════════════════════════════════════════════════
//
// Locks in the I12 ordering contract for the new callback-driven
// scheduler: concurrent batches surface in completion order, serial
// tools apply patches pre-next-tool, EarlyOutcome acts as a barrier,
// and concurrent-batch patches apply in model_index order
// post-batch.

use crate::call_plan::PreparedToolCall;
use crate::call_plan::ToolCallOutcome;
use crate::call_plan::ToolCallPlan;
use crate::call_plan::ToolMessagePath;
use crate::call_plan::ToolSideEffects;
use crate::call_plan::UnstampedToolCallOutcome;
use std::sync::Mutex;
use std::time::Duration;

fn prepared_from(
    tool: Arc<dyn crate::traits::Tool>,
    tool_use_id: &str,
    model_index: usize,
) -> PreparedToolCall {
    PreparedToolCall {
        tool_use_id: tool_use_id.into(),
        tool_id: tool.id(),
        tool,
        parsed_input: json!({}),
        model_index,
    }
}

fn empty_unstamped(tool_use_id: &str, model_index: usize) -> UnstampedToolCallOutcome {
    UnstampedToolCallOutcome {
        tool_use_id: tool_use_id.into(),
        tool_id: ToolId::Custom(tool_use_id.into()),
        model_index,
        ordered_messages: vec![],
        message_path: ToolMessagePath::Success,
        error_kind: None,
        permission_denial: None,
        prevent_continuation: None,
        effects: ToolSideEffects::none(),
    }
}

fn unstamped_with_patch<F>(
    tool_use_id: &str,
    model_index: usize,
    patch: F,
) -> UnstampedToolCallOutcome
where
    F: FnOnce(&mut coco_types::ToolAppState) + Send + Sync + 'static,
{
    UnstampedToolCallOutcome {
        effects: ToolSideEffects {
            app_state_patch: Some(Box::new(patch)),
        },
        ..empty_unstamped(tool_use_id, model_index)
    }
}

/// Drive `execute_with` and capture the completion-order sequence of
/// outcomes surfaced to `on_outcome`.
async fn drive_capture<F, Fut>(
    exec: &StreamingToolExecutor,
    plans: Vec<ToolCallPlan>,
    run_one: F,
) -> Vec<ToolCallOutcome>
where
    F: Fn(PreparedToolCall, crate::call_plan::RunOneRuntime) -> Fut + Sync,
    Fut: std::future::Future<Output = UnstampedToolCallOutcome> + Send,
{
    let captured = Mutex::new(Vec::<ToolCallOutcome>::new());
    exec.execute_with(plans, run_one, |o| captured.lock().unwrap().push(o))
        .await;
    captured.into_inner().unwrap()
}

#[tokio::test]
async fn test_execute_with_concurrent_batch_surfaces_in_completion_order() {
    // Three safe tools: A sleeps 80ms, B sleeps 10ms, C sleeps 40ms.
    // Completion order MUST be [B, C, A] regardless of submission.
    let a = Arc::new(SlowSafeTool {
        name: "a".into(),
        concurrent_count: Arc::new(AtomicI32::new(0)),
        max_concurrent: Arc::new(AtomicI32::new(0)),
        sleep_ms: 80,
    });
    let b = Arc::new(SlowSafeTool {
        name: "b".into(),
        concurrent_count: Arc::new(AtomicI32::new(0)),
        max_concurrent: Arc::new(AtomicI32::new(0)),
        sleep_ms: 10,
    });
    let c = Arc::new(SlowSafeTool {
        name: "c".into(),
        concurrent_count: Arc::new(AtomicI32::new(0)),
        max_concurrent: Arc::new(AtomicI32::new(0)),
        sleep_ms: 40,
    });

    let plans = vec![
        ToolCallPlan::Runnable(prepared_from(a, "A", 0)),
        ToolCallPlan::Runnable(prepared_from(b, "B", 1)),
        ToolCallPlan::Runnable(prepared_from(c, "C", 2)),
    ];

    let exec = StreamingToolExecutor::new();
    let outcomes = drive_capture(&exec, plans, |prepared, _runtime| async move {
        let tool_use_id = prepared.tool_use_id.clone();
        let model_index = prepared.model_index;
        // Simulate actual tool work via the SlowSafeTool's own sleep.
        let ctx = crate::context::ToolUseContext::test_default();
        let _ = prepared.tool.execute(prepared.parsed_input, &ctx).await;
        empty_unstamped(&tool_use_id, model_index)
    })
    .await;

    let ids: Vec<_> = outcomes
        .iter()
        .map(|o| o.tool_use_id().to_string())
        .collect();
    assert_eq!(
        ids,
        vec!["B".to_string(), "C".to_string(), "A".to_string()],
        "concurrent batch must surface in completion order (fast → slow)"
    );

    // completion_seq stamped monotonically in surface order.
    let seqs: Vec<_> = outcomes
        .iter()
        .map(ToolCallOutcome::completion_seq)
        .collect();
    assert_eq!(seqs, vec![0, 1, 2]);
}

#[tokio::test]
async fn test_execute_with_concurrent_batch_applies_patches_in_model_order() {
    // Two concurrent tools; A's patch sets permission_mode → Plan,
    // B's patch sets permission_mode → Default. After the batch the
    // state must reflect the LAST write in model order (= B, index 1),
    // regardless of which future resolved first.
    let app_state = Arc::new(RwLock::new(coco_types::ToolAppState::default()));
    let exec = StreamingToolExecutor::new().with_app_state(app_state.clone());

    let a = Arc::new(SafeTool { name: "a".into() });
    let b = Arc::new(SafeTool { name: "b".into() });

    let plans = vec![
        ToolCallPlan::Runnable(prepared_from(a, "A", 0)),
        ToolCallPlan::Runnable(prepared_from(b, "B", 1)),
    ];

    // Vary completion order: B finishes first (so completion_seq 0),
    // A finishes second (completion_seq 1). Model-order patches still
    // mean A applies before B — final state = B's write.
    drive_capture(&exec, plans, |prepared, _runtime| async move {
        let tool_use_id = prepared.tool_use_id.clone();
        let model_index = prepared.model_index;
        let sleep_ms = if tool_use_id == "A" { 40 } else { 5 };
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        let target_mode = if model_index == 0 {
            coco_types::PermissionMode::Plan
        } else {
            coco_types::PermissionMode::Default
        };
        unstamped_with_patch(&tool_use_id, model_index, move |state| {
            state.permission_mode = Some(target_mode);
        })
    })
    .await;

    let guard = app_state.read().await;
    assert_eq!(
        guard.permission_mode,
        Some(coco_types::PermissionMode::Default),
        "post-batch state must reflect B (model_index 1) applied AFTER A, \
         regardless of completion order"
    );
}

#[tokio::test]
async fn test_execute_with_serial_tool_applies_patch_before_next_context() {
    // A serial unsafe tool followed by another serial unsafe tool.
    // The first tool's patch must be visible (via shared state read)
    // by the time the second run_one is called — serial mode applies
    // between tools per TS `toolOrchestration.ts:130-141`.
    let app_state = Arc::new(RwLock::new(coco_types::ToolAppState::default()));
    let exec = StreamingToolExecutor::new().with_app_state(app_state.clone());

    let u1 = Arc::new(UnsafeTool { name: "u1".into() });
    let u2 = Arc::new(UnsafeTool { name: "u2".into() });

    let plans = vec![
        ToolCallPlan::Runnable(prepared_from(u1, "u1", 0)),
        ToolCallPlan::Runnable(prepared_from(u2, "u2", 1)),
    ];

    // u1 writes plan_mode_attachment_count = 7. When u2's run_one
    // fires, we read shared state and capture what we saw.
    let observed_by_u2: Arc<Mutex<Option<i64>>> = Arc::new(Mutex::new(None));
    let observed_by_u2_clone = observed_by_u2.clone();
    let app_state_read = app_state.clone();

    exec.execute_with(
        plans,
        move |prepared, _runtime| {
            let app_state_read = app_state_read.clone();
            let observed = observed_by_u2_clone.clone();
            async move {
                let tool_use_id = prepared.tool_use_id.clone();
                let model_index = prepared.model_index;
                if tool_use_id == "u1" {
                    unstamped_with_patch(&tool_use_id, model_index, |state| {
                        state.plan_mode_attachment_count = 7;
                    })
                } else {
                    // u2 snapshots the state it sees. Must already
                    // reflect u1's patch.
                    let snap = app_state_read.read().await.plan_mode_attachment_count;
                    *observed.lock().unwrap() = Some(snap);
                    empty_unstamped(&tool_use_id, model_index)
                }
            }
        },
        |_outcome| {},
    )
    .await;

    assert_eq!(
        *observed_by_u2.lock().unwrap(),
        Some(7),
        "u2 must observe u1's patch — serial tools apply between calls"
    );
}

#[tokio::test]
async fn test_execute_with_early_outcome_is_barrier_between_safe_batches() {
    // Partition [safe_A, early_B, safe_C]:
    //   block 0: ConcurrentSafe([safe_A])
    //   block 1: EarlyOutcome(early_B)
    //   block 2: ConcurrentSafe([safe_C])
    //
    // EarlyOutcome must NOT share a batch with safe neighbors, and
    // its completion_seq lands between the two safe outcomes'
    // completion_seqs in partition order — NOT globally before both
    // (TS `toolOrchestration.ts:91-115` + plan I12).
    let a = Arc::new(SafeTool { name: "a".into() });
    let c = Arc::new(SafeTool { name: "c".into() });

    let plans = vec![
        ToolCallPlan::Runnable(prepared_from(a, "A", 0)),
        ToolCallPlan::EarlyOutcome(UnstampedToolCallOutcome {
            tool_use_id: "B".into(),
            tool_id: ToolId::Custom("unknown".into()),
            model_index: 1,
            ordered_messages: vec![],
            message_path: ToolMessagePath::EarlyReturn,
            error_kind: Some(crate::call_plan::ToolCallErrorKind::UnknownTool),
            permission_denial: None,
            prevent_continuation: None,
            effects: ToolSideEffects::none(),
        }),
        ToolCallPlan::Runnable(prepared_from(c, "C", 2)),
    ];

    let exec = StreamingToolExecutor::new();
    let outcomes = drive_capture(&exec, plans, |prepared, _| async move {
        let tool_use_id = prepared.tool_use_id.clone();
        empty_unstamped(&tool_use_id, prepared.model_index)
    })
    .await;

    // Partition order: A, B, C — each in its own block.
    let ids: Vec<_> = outcomes
        .iter()
        .map(|o| o.tool_use_id().to_string())
        .collect();
    assert_eq!(ids, vec!["A", "B", "C"]);

    // completion_seq stamped in partition traversal order, not
    // "EarlyOutcome stamps first".
    let seqs: Vec<_> = outcomes
        .iter()
        .map(ToolCallOutcome::completion_seq)
        .collect();
    assert_eq!(seqs, vec![0, 1, 2]);

    // The middle outcome is the synthetic unknown-tool error.
    assert_eq!(
        outcomes[1].error_kind(),
        Some(&crate::call_plan::ToolCallErrorKind::UnknownTool)
    );
    assert_eq!(outcomes[1].message_path(), ToolMessagePath::EarlyReturn);
}

#[tokio::test]
async fn test_execute_with_every_plan_gets_one_outcome() {
    // I1 / per-call invariant: one ToolUseQueued → exactly one
    // ToolUseCompleted. Encoded here as: every plan submitted
    // surfaces exactly one outcome.
    let a = Arc::new(SafeTool { name: "a".into() });
    let b = Arc::new(UnsafeTool { name: "b".into() });

    let plans = vec![
        ToolCallPlan::Runnable(prepared_from(a, "A", 0)),
        ToolCallPlan::EarlyOutcome(empty_unstamped("B", 1)),
        ToolCallPlan::Runnable(prepared_from(b, "C", 2)),
    ];

    let exec = StreamingToolExecutor::new();
    let outcomes = drive_capture(&exec, plans, |prepared, _| async move {
        empty_unstamped(&prepared.tool_use_id, prepared.model_index)
    })
    .await;

    assert_eq!(outcomes.len(), 3);
    let ids: Vec<_> = outcomes
        .iter()
        .map(|o| o.tool_use_id().to_string())
        .collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}
