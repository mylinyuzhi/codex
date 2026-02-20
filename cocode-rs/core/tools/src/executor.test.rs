use super::*;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use serde_json::Value;

struct SafeTool;

#[async_trait]
impl Tool for SafeTool {
    fn name(&self) -> &str {
        "safe_tool"
    }
    fn description(&self) -> &str {
        "A safe tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::text("safe result"))
    }
}

struct UnsafeTool;

#[async_trait]
impl Tool for UnsafeTool {
    fn name(&self) -> &str {
        "unsafe_tool"
    }
    fn description(&self) -> &str {
        "An unsafe tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::text("unsafe result"))
    }
}

#[tokio::test]
async fn test_executor_safe_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(SafeTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    let tool_call = ToolCall::new("call-1", "safe_tool", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;

    // Safe tool should start immediately
    assert_eq!(executor.active_count().await, 1);
    assert_eq!(executor.pending_count().await, 0);

    let results = executor.drain().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].result.is_ok());
}

#[tokio::test]
async fn test_executor_unsafe_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(UnsafeTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    let tool_call = ToolCall::new("call-1", "unsafe_tool", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;

    // Unsafe tool should be queued
    assert_eq!(executor.active_count().await, 0);
    assert_eq!(executor.pending_count().await, 1);

    // Execute pending
    executor.execute_pending_unsafe().await;

    let results = executor.drain().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].result.is_ok());
}

/// A tool gated on a feature flag.
struct FeatureGatedTool;

#[async_trait]
impl Tool for FeatureGatedTool {
    fn name(&self) -> &str {
        "gated_tool"
    }
    fn description(&self) -> &str {
        "A feature-gated tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Ls)
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::text("gated result"))
    }
}

#[tokio::test]
async fn test_feature_gated_tool_rejected_when_disabled() {
    let mut registry = ToolRegistry::new();
    registry.register(FeatureGatedTool);

    // Disable the Ls feature
    let mut features = cocode_protocol::Features::with_defaults();
    features.disable(cocode_protocol::Feature::Ls);

    let config = ExecutorConfig {
        features,
        ..ExecutorConfig::default()
    };
    let executor = StreamingToolExecutor::new(Arc::new(registry), config, None);

    let tool_call = ToolCall::new("call-1", "gated_tool", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;
    executor.execute_pending_unsafe().await;

    let results = executor.drain().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].result.is_err());
    let err = results[0].result.as_ref().unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("NotFound"),
        "Expected NotFound error, got: {err}"
    );
}

#[tokio::test]
async fn test_executor_not_found() {
    let registry = ToolRegistry::new();
    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    let tool_call = ToolCall::new("call-1", "nonexistent", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;

    // Should be queued since tool not found
    assert_eq!(executor.pending_count().await, 1);

    // Execute pending - should fail
    executor.execute_pending_unsafe().await;

    let results = executor.drain().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].result.is_err());
}

#[tokio::test]
async fn test_allowed_tool_names_rejects_unlisted_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(SafeTool);
    registry.register(UnsafeTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    // Only allow safe_tool — unsafe_tool is registered but not in the allowlist
    executor.set_allowed_tool_names(vec!["safe_tool".to_string()].into_iter().collect());

    // safe_tool → should succeed
    let tool_call = ToolCall::new("call-1", "safe_tool", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;

    // unsafe_tool → should be rejected immediately by allowlist
    let tool_call = ToolCall::new("call-2", "unsafe_tool", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;

    executor.execute_pending_unsafe().await;
    let results = executor.drain().await;

    assert_eq!(results.len(), 2);

    let safe_result = results.iter().find(|r| r.call_id == "call-1").unwrap();
    assert!(safe_result.result.is_ok(), "safe_tool should succeed");

    let unsafe_result = results.iter().find(|r| r.call_id == "call-2").unwrap();
    assert!(
        unsafe_result.result.is_err(),
        "unsafe_tool should be rejected"
    );
    let err = unsafe_result.result.as_ref().unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("NotFound"),
        "Expected NotFound error, got: {err}"
    );
}

#[tokio::test]
async fn test_no_allowlist_allows_all_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(SafeTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    // No allowlist set → all registered tools should work
    let tool_call = ToolCall::new("call-1", "safe_tool", serde_json::json!({}));
    executor.on_tool_complete(tool_call).await;

    let results = executor.drain().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].result.is_ok());
}

/// A tool with per-input concurrency safety (like Bash).
struct PerInputTool;

#[async_trait]
impl Tool for PerInputTool {
    fn name(&self) -> &str {
        "per_input_tool"
    }
    fn description(&self) -> &str {
        "A tool with per-input concurrency"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe // Default unsafe
    }
    fn is_concurrency_safe_for(&self, input: &Value) -> bool {
        // Safe only when "safe" key is true
        input["safe"].as_bool().unwrap_or(false)
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::text("per-input result"))
    }
}

/// A slow safe tool for concurrency testing.
struct SlowSafeTool;

#[async_trait]
impl Tool for SlowSafeTool {
    fn name(&self) -> &str {
        "slow_safe"
    }
    fn description(&self) -> &str {
        "A slow safe tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(ToolOutput::text("slow safe result"))
    }
}

#[tokio::test]
async fn test_interleaved_safe_unsafe_scheduling() {
    // Queue: [safe, unsafe, safe] — the unsafe tool should act as a barrier
    let mut registry = ToolRegistry::new();
    registry.register(SafeTool);
    registry.register(UnsafeTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    // Submit all three as pending (via on_tool_complete)
    let safe1 = ToolCall::new("safe-1", "safe_tool", serde_json::json!({}));
    let unsafe1 = ToolCall::new("unsafe-1", "unsafe_tool", serde_json::json!({}));
    let safe2 = ToolCall::new("safe-2", "safe_tool", serde_json::json!({}));

    // safe_tool starts immediately
    executor.on_tool_complete(safe1).await;
    assert_eq!(executor.active_count().await, 1);

    // unsafe_tool gets queued
    executor.on_tool_complete(unsafe1).await;
    assert_eq!(executor.pending_count().await, 1);

    // another safe_tool also gets queued (since an unsafe is already pending)
    executor.on_tool_complete(safe2).await;

    // Execute pending: should drain safe-1 first, then run unsafe-1, then safe-2
    executor.execute_pending_unsafe().await;

    let results = executor.drain().await;
    // All three should complete successfully
    assert_eq!(results.len(), 3);
    for result in &results {
        assert!(result.result.is_ok(), "Tool {} failed", result.call_id);
    }
}

#[tokio::test]
async fn test_per_input_concurrency_safe_runs_concurrently() {
    let mut registry = ToolRegistry::new();
    registry.register(PerInputTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    // Safe input should start immediately
    let safe_call = ToolCall::new(
        "call-safe",
        "per_input_tool",
        serde_json::json!({"safe": true}),
    );
    executor.on_tool_complete(safe_call).await;
    assert_eq!(executor.active_count().await, 1, "Safe input should start");
    assert_eq!(executor.pending_count().await, 0);

    // Unsafe input should be queued
    let unsafe_call = ToolCall::new(
        "call-unsafe",
        "per_input_tool",
        serde_json::json!({"safe": false}),
    );
    executor.on_tool_complete(unsafe_call).await;
    assert_eq!(
        executor.pending_count().await,
        1,
        "Unsafe input should queue"
    );

    executor.execute_pending_unsafe().await;
    let results = executor.drain().await;
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.result.is_ok());
    }
}

#[tokio::test]
async fn test_max_concurrency_overflow_queues() {
    let mut registry = ToolRegistry::new();
    registry.register(SlowSafeTool);

    // Set max_concurrency to 2
    let config = ExecutorConfig {
        max_concurrency: 2,
        ..ExecutorConfig::default()
    };
    let executor = StreamingToolExecutor::new(Arc::new(registry), config, None);

    // Submit 3 safe tools — first 2 start, third should queue
    executor
        .on_tool_complete(ToolCall::new("c1", "slow_safe", serde_json::json!({})))
        .await;
    executor
        .on_tool_complete(ToolCall::new("c2", "slow_safe", serde_json::json!({})))
        .await;
    executor
        .on_tool_complete(ToolCall::new("c3", "slow_safe", serde_json::json!({})))
        .await;

    assert_eq!(executor.active_count().await, 2, "Only 2 should be active");
    assert_eq!(executor.pending_count().await, 1, "Third should be queued");

    // Drain pending — should process the third after one active completes
    executor.execute_pending_unsafe().await;
    let results = executor.drain().await;
    assert_eq!(results.len(), 3, "All three should complete");
    for r in &results {
        assert!(r.result.is_ok());
    }
}

#[tokio::test]
async fn test_multiple_safe_tools_concurrent() {
    let mut registry = ToolRegistry::new();
    registry.register(SafeTool);

    let executor = StreamingToolExecutor::new(Arc::new(registry), ExecutorConfig::default(), None);

    // Submit multiple safe tools — all should start concurrently
    for i in 0..5 {
        let call = ToolCall::new(format!("call-{i}"), "safe_tool", serde_json::json!({}));
        executor.on_tool_complete(call).await;
    }

    // All should be active (within max_concurrency=10)
    assert_eq!(executor.active_count().await, 5);
    assert_eq!(executor.pending_count().await, 0);

    let results = executor.drain().await;
    assert_eq!(results.len(), 5);
    for r in &results {
        assert!(r.result.is_ok());
    }
}

#[test]
fn test_extract_prefix_pattern_bash_command() {
    let input = serde_json::json!({"command": "git push origin main"});
    assert_eq!(
        extract_prefix_pattern("Bash", &input),
        Some("git *".to_string())
    );
}

#[test]
fn test_extract_prefix_pattern_bash_single_word() {
    let input = serde_json::json!({"command": "ls"});
    assert_eq!(
        extract_prefix_pattern("Bash", &input),
        Some("ls *".to_string())
    );
}

#[test]
fn test_extract_prefix_pattern_non_bash_tool() {
    let input = serde_json::json!({"command": "git push"});
    assert_eq!(extract_prefix_pattern("Read", &input), None);
    assert_eq!(extract_prefix_pattern("Edit", &input), None);
    assert_eq!(extract_prefix_pattern("Write", &input), None);
}

#[test]
fn test_extract_prefix_pattern_missing_command() {
    let input = serde_json::json!({"file_path": "/tmp/test"});
    assert_eq!(extract_prefix_pattern("Bash", &input), None);
}

#[test]
fn test_extract_prefix_pattern_empty_command() {
    let input = serde_json::json!({"command": ""});
    assert_eq!(extract_prefix_pattern("Bash", &input), None);
}

#[test]
fn test_extract_prefix_pattern_whitespace_only() {
    let input = serde_json::json!({"command": "   "});
    assert_eq!(extract_prefix_pattern("Bash", &input), None);
}

#[test]
fn test_extract_prefix_pattern_complex_command() {
    let input = serde_json::json!({"command": "cargo test --no-fail-fast -- -q"});
    assert_eq!(
        extract_prefix_pattern("Bash", &input),
        Some("cargo *".to_string())
    );
}
