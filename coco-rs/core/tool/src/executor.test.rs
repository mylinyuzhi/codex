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
    // Results returned in submission order
    assert_eq!(results[0].tool_use_id, "read1");
    assert_eq!(results[1].tool_use_id, "read2");
    assert_eq!(results[2].tool_use_id, "read3");
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

// ---------------------------------------------------------------------------
// Hook pipeline integration (B1.3)
// ---------------------------------------------------------------------------

use crate::hook_handle::HookHandle;
use crate::hook_handle::HookPermission;
use crate::hook_handle::PostToolUseOutcome;
use crate::hook_handle::PreToolUseOutcome;
use std::sync::atomic::AtomicUsize;

/// Hook handle that records each invocation into a shared counter so tests
/// can verify that both PreToolUse and PostToolUse fire exactly once per
/// tool call. Configurable to return specific outcomes for each stage.
struct RecordingHookHandle {
    pre_calls: Arc<AtomicUsize>,
    post_ok_calls: Arc<AtomicUsize>,
    post_err_calls: Arc<AtomicUsize>,
    pre_outcome: PreToolUseOutcome,
    post_ok_outcome: PostToolUseOutcome,
    post_err_outcome: PostToolUseOutcome,
}

impl RecordingHookHandle {
    fn default_recorder() -> (Self, Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let pre = Arc::new(AtomicUsize::new(0));
        let post_ok = Arc::new(AtomicUsize::new(0));
        let post_err = Arc::new(AtomicUsize::new(0));
        (
            Self {
                pre_calls: pre.clone(),
                post_ok_calls: post_ok.clone(),
                post_err_calls: post_err.clone(),
                pre_outcome: PreToolUseOutcome::default(),
                post_ok_outcome: PostToolUseOutcome::default(),
                post_err_outcome: PostToolUseOutcome::default(),
            },
            pre,
            post_ok,
            post_err,
        )
    }
}

#[async_trait::async_trait]
impl HookHandle for RecordingHookHandle {
    async fn run_pre_tool_use(
        &self,
        _tool_name: &str,
        _tool_use_id: &str,
        _tool_input: &Value,
    ) -> PreToolUseOutcome {
        self.pre_calls.fetch_add(1, Ordering::SeqCst);
        self.pre_outcome.clone()
    }

    async fn run_post_tool_use(
        &self,
        _tool_name: &str,
        _tool_use_id: &str,
        _tool_input: &Value,
        _tool_response: &Value,
    ) -> PostToolUseOutcome {
        self.post_ok_calls.fetch_add(1, Ordering::SeqCst);
        self.post_ok_outcome.clone()
    }

    async fn run_post_tool_use_failure(
        &self,
        _tool_name: &str,
        _tool_use_id: &str,
        _tool_input: &Value,
        _error_message: &str,
    ) -> PostToolUseOutcome {
        self.post_err_calls.fetch_add(1, Ordering::SeqCst);
        self.post_err_outcome.clone()
    }
}

/// PreToolUse + PostToolUse fire exactly once per single-tool execution
/// when a hook handle is attached.
#[tokio::test]
async fn test_hook_pipeline_single_tool_invokes_pre_and_post() {
    let (handle, pre, post_ok, post_err) = RecordingHookHandle::default_recorder();
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({"x": 1}),
    };
    let result = exec.execute_single(call, &ctx).await;
    assert!(result.result.is_ok());
    assert_eq!(pre.load(Ordering::SeqCst), 1, "PreToolUse ran once");
    assert_eq!(post_ok.load(Ordering::SeqCst), 1, "PostToolUse ran once");
    assert_eq!(
        post_err.load(Ordering::SeqCst),
        0,
        "failure hook did not run"
    );
}

/// A PreToolUse hook that sets `blocking_reason` must prevent the tool
/// from executing and the error bubbles up as PermissionDenied.
#[tokio::test]
async fn test_hook_pipeline_pre_reject_blocks_execution() {
    let (mut handle, pre, post_ok, post_err) = RecordingHookHandle::default_recorder();
    handle.pre_outcome.blocking_reason = Some("denied by test hook".into());
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({"x": 1}),
    };
    let result = exec.execute_single(call, &ctx).await;

    assert!(result.result.is_err());
    let err = result.result.unwrap_err().to_string();
    assert!(err.contains("denied by test hook"), "got: {err}");
    assert_eq!(pre.load(Ordering::SeqCst), 1, "pre hook ran");
    assert_eq!(
        post_ok.load(Ordering::SeqCst),
        0,
        "tool did not run so no post"
    );
    assert_eq!(post_err.load(Ordering::SeqCst), 0);
}

/// PreToolUse `PermissionOverride::Deny` must also hard-block.
#[tokio::test]
async fn test_hook_pipeline_pre_deny_override_blocks() {
    let (mut handle, _pre, post_ok, _post_err) = RecordingHookHandle::default_recorder();
    handle.pre_outcome.permission_override = Some(HookPermission::Deny);
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({}),
    };
    let result = exec.execute_single(call, &ctx).await;
    assert!(result.result.is_err());
    assert_eq!(post_ok.load(Ordering::SeqCst), 0);
}

/// PreToolUse `updated_input` must be forwarded to tool.execute() — the
/// tool echoes input back as the result, so we can verify the rewrite.
#[tokio::test]
async fn test_hook_pipeline_pre_modify_input_is_applied() {
    let (mut handle, _pre, _post_ok, _post_err) = RecordingHookHandle::default_recorder();
    handle.pre_outcome.updated_input = Some(json!({"x": 99, "rewritten": true}));
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({"x": 1}),
    };
    let result = exec.execute_single(call, &ctx).await;
    let data = result.result.unwrap().data;
    assert_eq!(data["x"], 99);
    assert_eq!(data["rewritten"], true);
}

/// PostToolUse `updated_output` must replace the tool's result data.
#[tokio::test]
async fn test_hook_pipeline_post_modify_output_is_applied() {
    let (mut handle, _pre, _post_ok, _post_err) = RecordingHookHandle::default_recorder();
    handle.post_ok_outcome.updated_output = Some(json!("replaced by hook"));
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({"x": 1}),
    };
    let result = exec.execute_single(call, &ctx).await;
    let data = result.result.unwrap().data;
    assert_eq!(data, json!("replaced by hook"));
}

/// When the tool errors, PostToolUseFailure fires (not PostToolUse).
#[tokio::test]
async fn test_hook_pipeline_post_failure_on_tool_error() {
    struct FailingTool;
    #[async_trait::async_trait]
    impl crate::traits::Tool for FailingTool {
        fn id(&self) -> ToolId {
            ToolId::Custom("failing".into())
        }
        fn name(&self) -> &str {
            "failing"
        }
        fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
            "".into()
        }
        fn input_schema(&self) -> ToolInputSchema {
            ToolInputSchema {
                properties: HashMap::new(),
            }
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &crate::context::ToolUseContext,
        ) -> Result<ToolResult<Value>, crate::error::ToolError> {
            Err(crate::error::ToolError::ExecutionFailed {
                message: "boom".into(),
                source: None,
            })
        }
    }

    let (handle, pre, post_ok, post_err) = RecordingHookHandle::default_recorder();
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(FailingTool),
        input: json!({}),
    };
    let _ = exec.execute_single(call, &ctx).await;

    assert_eq!(pre.load(Ordering::SeqCst), 1);
    assert_eq!(
        post_ok.load(Ordering::SeqCst),
        0,
        "success hook did not run"
    );
    assert_eq!(post_err.load(Ordering::SeqCst), 1, "failure hook ran");
}

/// Concurrent path must also invoke hooks (same contract as single path).
#[tokio::test]
async fn test_hook_pipeline_concurrent_tools_invoke_hooks() {
    let (handle, pre, post_ok, _post_err) = RecordingHookHandle::default_recorder();
    let mut ctx = crate::context::ToolUseContext::test_default();
    ctx.hook_handle = Some(Arc::new(handle));

    let exec = StreamingToolExecutor::new();
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
    assert_eq!(pre.load(Ordering::SeqCst), 3, "pre hook per tool");
    assert_eq!(post_ok.load(Ordering::SeqCst), 3, "post hook per tool");
}

/// Missing hook handle (None) falls through unchanged — existing tests
/// already cover this but we assert it explicitly to nail the contract.
#[tokio::test]
async fn test_hook_pipeline_absent_handle_is_noop() {
    let ctx = crate::context::ToolUseContext::test_default();
    // Default ctx has hook_handle: None.
    assert!(ctx.hook_handle.is_none());

    let exec = StreamingToolExecutor::new();
    let call = PendingToolCall {
        tool_use_id: "t1".into(),
        tool: Arc::new(UnsafeTool { name: "u".into() }),
        input: json!({"x": 42}),
    };
    let result = exec.execute_single(call, &ctx).await;
    assert!(result.result.is_ok());
    assert_eq!(result.result.unwrap().data, json!({"x": 42}));
}
