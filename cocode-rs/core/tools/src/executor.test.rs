use super::*;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_hooks::HookOutcome;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolName;
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
        extract_prefix_pattern(ToolName::Bash.as_str(), &input),
        Some("git *".to_string())
    );
}

#[test]
fn test_extract_prefix_pattern_bash_single_word() {
    let input = serde_json::json!({"command": "ls"});
    assert_eq!(
        extract_prefix_pattern(ToolName::Bash.as_str(), &input),
        Some("ls *".to_string())
    );
}

#[test]
fn test_extract_prefix_pattern_non_bash_tool() {
    let input = serde_json::json!({"command": "git push"});
    assert_eq!(
        extract_prefix_pattern(ToolName::Read.as_str(), &input),
        None
    );
    assert_eq!(
        extract_prefix_pattern(ToolName::Edit.as_str(), &input),
        None
    );
    assert_eq!(
        extract_prefix_pattern(ToolName::Write.as_str(), &input),
        None
    );
}

#[test]
fn test_extract_prefix_pattern_missing_command() {
    let input = serde_json::json!({"file_path": "/tmp/test"});
    assert_eq!(
        extract_prefix_pattern(ToolName::Bash.as_str(), &input),
        None
    );
}

#[test]
fn test_extract_prefix_pattern_empty_command() {
    let input = serde_json::json!({"command": ""});
    assert_eq!(
        extract_prefix_pattern(ToolName::Bash.as_str(), &input),
        None
    );
}

#[test]
fn test_extract_prefix_pattern_whitespace_only() {
    let input = serde_json::json!({"command": "   "});
    assert_eq!(
        extract_prefix_pattern(ToolName::Bash.as_str(), &input),
        None
    );
}

#[test]
fn test_extract_prefix_pattern_complex_command() {
    let input = serde_json::json!({"command": "cargo test --no-fail-fast -- -q"});
    assert_eq!(
        extract_prefix_pattern(ToolName::Bash.as_str(), &input),
        Some("cargo *".to_string())
    );
}

// --- PostHookAction tests ---

#[test]
fn test_post_hook_action_replace_output_is_not_error() {
    let original = Ok(ToolOutput::text("original"));
    let replacement = PostHookAction::ReplaceOutput(ToolOutput::text("hook-replaced"));

    let result = finalize_tool_result(
        original,
        replacement,
        "test_tool",
        "call-1",
        std::time::Instant::now(),
        &None,
    );

    let output = result.expect("ReplaceOutput should produce Ok");
    assert!(
        !output.is_error,
        "ReplaceOutput should not be marked as error"
    );
    match &output.content {
        cocode_protocol::ToolResultContent::Text(t) => assert_eq!(t, "hook-replaced"),
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_post_hook_action_reject_produces_error() {
    let original = Ok(ToolOutput::text("original"));
    let rejection = PostHookAction::Reject("denied by hook".to_string());

    let result = finalize_tool_result(
        original,
        rejection,
        "test_tool",
        "call-1",
        std::time::Instant::now(),
        &None,
    );

    let output = result.expect("Reject wraps in Ok(error)");
    assert!(output.is_error, "Reject should produce an error output");
}

#[test]
fn test_post_hook_action_none_preserves_result() {
    let original = Ok(ToolOutput::text("original"));
    let result = finalize_tool_result(
        original,
        PostHookAction::None,
        "test_tool",
        "call-1",
        std::time::Instant::now(),
        &None,
    );

    let output = result.expect("None should preserve original");
    match &output.content {
        cocode_protocol::ToolResultContent::Text(t) => assert_eq!(t, "original"),
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_post_hook_action_stop_continuation_preserves_result() {
    let original = Ok(ToolOutput::text("original output"));
    let result = finalize_tool_result(
        original,
        PostHookAction::StopContinuation("hook requested stop".to_string()),
        "test_tool",
        "call-1",
        std::time::Instant::now(),
        &None,
    );

    let output = result.expect("StopContinuation should preserve original result");
    assert!(
        !output.is_error,
        "StopContinuation should not mark output as error"
    );
    match &output.content {
        cocode_protocol::ToolResultContent::Text(t) => assert_eq!(t, "original output"),
        _ => panic!("Expected text content"),
    }
}

// --- approval_check_value tests ---

#[test]
fn test_approval_check_value_bash_extracts_command() {
    let input = serde_json::json!({"command": "git push origin main"});
    let value = approval_check_value(
        ToolName::Bash.as_str(),
        &input,
        "Bash: git push origin main",
    );
    assert_eq!(value, "git push origin main");
}

#[test]
fn test_approval_check_value_file_tool_extracts_path() {
    let input = serde_json::json!({"file_path": "/tmp/test.rs"});
    let value = approval_check_value(ToolName::Edit.as_str(), &input, "Edit: /tmp/test.rs");
    assert_eq!(value, "/tmp/test.rs");
}

#[test]
fn test_approval_check_value_fallback_to_description() {
    let input = serde_json::json!({"query": "search term"});
    let value = approval_check_value(
        ToolName::WebSearch.as_str(),
        &input,
        "Execute tool: WebSearch",
    );
    assert_eq!(value, "Execute tool: WebSearch");
}

#[test]
fn test_approval_wildcard_matches_raw_command() {
    use cocode_policy::ApprovalStore;

    let mut store = ApprovalStore::new();
    // Simulate user approving "git *" prefix pattern
    store.approve_pattern(ToolName::Bash.as_str(), "git *");

    // The raw command value (not the prefixed description) should match
    let raw_command = "git push origin main";
    assert!(
        store.is_approved(ToolName::Bash.as_str(), raw_command),
        "Wildcard 'git *' should match raw command 'git push origin main'"
    );

    // The old buggy description should NOT match
    let description = "Bash: git push origin main";
    assert!(
        !store.is_approved(ToolName::Bash.as_str(), description),
        "Wildcard 'git *' should NOT match prefixed description"
    );
}

// --- drain_one_active tests ---

/// A tool with configurable delay for testing drain_one_active ordering.
struct DelayTool {
    name: &'static str,
}

#[async_trait]
impl Tool for DelayTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "A tool with configurable delay"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object", "properties": {"delay_ms": {"type": "integer"}}})
    }
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }
    async fn execute(&self, input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        let delay_ms = input["delay_ms"].as_u64().unwrap_or(10);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        Ok(ToolOutput::text(format!("done after {delay_ms}ms")))
    }
}

#[tokio::test]
async fn test_drain_one_active_picks_fastest() {
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool { name: "delay_tool" });

    let config = ExecutorConfig {
        max_concurrency: 10,
        ..ExecutorConfig::default()
    };
    let executor = StreamingToolExecutor::new(Arc::new(registry), config, None);

    // Start a slow (200ms) and a fast (5ms) tool
    let slow = ToolCall::new("slow", "delay_tool", serde_json::json!({"delay_ms": 200}));
    let fast = ToolCall::new("fast", "delay_tool", serde_json::json!({"delay_ms": 5}));

    executor.on_tool_complete(slow).await;
    executor.on_tool_complete(fast).await;

    assert_eq!(executor.active_count().await, 2);

    // drain_one_active should complete the fast one first
    executor.drain_one_active().await;

    // One completed, one still active
    let completed = executor.completed_results.lock().await;
    assert_eq!(completed.len(), 1, "One task should have completed");
    assert_eq!(
        completed[0].call_id, "fast",
        "The faster task should complete first"
    );
    drop(completed);

    assert_eq!(
        executor.active_count().await,
        1,
        "One should still be active"
    );

    // Drain the remaining — returns ALL completed results (fast + slow)
    let results = executor.drain().await;
    assert_eq!(
        results.len(),
        2,
        "Both tasks should be in completed results"
    );
    assert!(
        results.iter().any(|r| r.call_id == "slow"),
        "Slow task should be in results"
    );
}

// --- Permission aggregation tests (Issue 4) ---

fn make_permission_outcome(decision: &str, reason: Option<&str>) -> HookOutcome {
    HookOutcome {
        hook_name: format!("hook-{decision}"),
        result: HookResult::PermissionOverride {
            decision: decision.to_string(),
            reason: reason.map(ToString::to_string),
        },
        duration_ms: 10,
        suppress_output: false,
    }
}

fn make_continue_outcome(name: &str) -> HookOutcome {
    HookOutcome {
        hook_name: name.to_string(),
        result: HookResult::Continue,
        duration_ms: 5,
        suppress_output: false,
    }
}

#[test]
fn test_permission_aggregation_deny_beats_allow() {
    let outcomes = vec![
        make_permission_outcome("allow", None),
        make_permission_outcome("deny", Some("blocked by policy")),
    ];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Deny);
    assert_eq!(reason, Some("blocked by policy".to_string()));
}

#[test]
fn test_permission_aggregation_ask_beats_allow() {
    let outcomes = vec![
        make_permission_outcome("allow", None),
        make_permission_outcome("ask", None),
    ];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Ask);
    assert!(reason.is_none());
}

#[test]
fn test_permission_aggregation_single_allow() {
    let outcomes = vec![make_permission_outcome("allow", None)];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Allow);
    assert!(reason.is_none());
}

#[test]
fn test_permission_aggregation_no_overrides() {
    let outcomes = vec![
        make_continue_outcome("hook-1"),
        make_continue_outcome("hook-2"),
    ];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Undefined);
    assert!(reason.is_none());
}

#[test]
fn test_permission_aggregation_empty() {
    let outcomes: Vec<HookOutcome> = vec![];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Undefined);
    assert!(reason.is_none());
}

#[test]
fn test_permission_aggregation_deny_beats_ask() {
    let outcomes = vec![
        make_permission_outcome("ask", None),
        make_permission_outcome("deny", Some("denied")),
    ];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Deny);
    assert_eq!(reason, Some("denied".to_string()));
}

#[test]
fn test_permission_aggregation_unknown_decision_is_undefined() {
    let outcomes = vec![make_permission_outcome("unknown_value", None)];
    let (level, _) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Undefined);
}

#[test]
fn test_permission_aggregation_first_deny_reason_preserved() {
    // When multiple denies, only the first reason is kept
    let outcomes = vec![
        make_permission_outcome("deny", Some("first deny")),
        make_permission_outcome("deny", Some("second deny")),
    ];
    let (level, reason) = aggregate_permission_overrides(&outcomes);
    assert_eq!(level, PermissionLevel::Deny);
    assert_eq!(reason, Some("first deny".to_string()));
}

#[test]
fn test_permission_level_ordering() {
    assert!(PermissionLevel::Deny < PermissionLevel::Ask);
    assert!(PermissionLevel::Ask < PermissionLevel::Allow);
    assert!(PermissionLevel::Allow < PermissionLevel::Undefined);
}

// --- apply_permission_mode Plan mode tests ---

/// A non-read-only tool for plan mode tests.
struct WriteLikeTool;

#[async_trait]
impl Tool for WriteLikeTool {
    fn name(&self) -> &str {
        "write_like_tool"
    }
    fn description(&self) -> &str {
        "A non-read-only tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn is_read_only(&self) -> bool {
        false
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::text("write result"))
    }
}

#[test]
fn test_plan_mode_respects_tool_allowed() {
    let mut registry = ToolRegistry::new();
    registry.register(WriteLikeTool);

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::Allowed,
        PermissionMode::Plan,
        "write_like_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "Plan mode should respect tool's explicit Allowed, got: {result:?}"
    );
}

#[test]
fn test_plan_mode_denies_needs_approval() {
    let mut registry = ToolRegistry::new();
    registry.register(WriteLikeTool);

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::NeedsApproval {
            request: cocode_protocol::ApprovalRequest {
                request_id: "test".to_string(),
                tool_name: "write_like_tool".to_string(),
                description: "test".to_string(),
                risks: vec![],
                allow_remember: false,
                proposed_prefix_pattern: None,
                input: None,
            },
        },
        PermissionMode::Plan,
        "write_like_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "Plan mode should deny NeedsApproval for non-read-only tools, got: {result:?}"
    );
}

// --- apply_permission_mode Bypass mode tests ---

#[test]
fn test_bypass_mode_respects_denied() {
    let registry = ToolRegistry::new();

    // Bypass mode should NOT override explicit Denied results (deny is absolute).
    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::Denied {
            reason: "deny rule matched".to_string(),
        },
        PermissionMode::Bypass,
        "any_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "Bypass mode must respect Denied results, got: {result:?}"
    );
}

#[test]
fn test_bypass_mode_allows_needs_approval() {
    let registry = ToolRegistry::new();

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::NeedsApproval {
            request: cocode_protocol::ApprovalRequest::default(),
        },
        PermissionMode::Bypass,
        "any_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "Bypass mode should allow NeedsApproval, got: {result:?}"
    );
}

#[test]
fn test_bypass_mode_allows_passthrough() {
    let registry = ToolRegistry::new();

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::Passthrough,
        PermissionMode::Bypass,
        "any_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "Bypass mode should allow Passthrough, got: {result:?}"
    );
}

// --- apply_permission_mode Bypass mode: Allowed stays Allowed ---

#[test]
fn test_bypass_mode_preserves_allowed() {
    let registry = ToolRegistry::new();

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::Allowed,
        PermissionMode::Bypass,
        "any_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "Bypass mode should preserve Allowed, got: {result:?}"
    );
}

// --- apply_permission_mode DontAsk mode tests ---

#[test]
fn test_dont_ask_mode_denies_needs_approval() {
    let registry = ToolRegistry::new();

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::NeedsApproval {
            request: cocode_protocol::ApprovalRequest {
                request_id: "test".to_string(),
                tool_name: "some_tool".to_string(),
                description: "test op".to_string(),
                risks: vec![],
                allow_remember: false,
                proposed_prefix_pattern: None,
                input: None,
            },
        },
        PermissionMode::DontAsk,
        "some_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "DontAsk mode should convert NeedsApproval to Denied, got: {result:?}"
    );
}

#[test]
fn test_dont_ask_mode_preserves_denied() {
    let registry = ToolRegistry::new();

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::Denied {
            reason: "already denied".to_string(),
        },
        PermissionMode::DontAsk,
        "some_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "DontAsk mode should preserve Denied, got: {result:?}"
    );
}

#[test]
fn test_dont_ask_mode_preserves_allowed() {
    let registry = ToolRegistry::new();

    let result = apply_permission_mode(
        cocode_protocol::PermissionResult::Allowed,
        PermissionMode::DontAsk,
        "some_tool",
        &registry,
    );
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "DontAsk mode should preserve Allowed (pre-approved), got: {result:?}"
    );
}

// --- is_read_only_or_plan_tool tests ---

/// A read-only tool for plan mode tests.
struct ReadOnlyTool;

#[async_trait]
impl Tool for ReadOnlyTool {
    fn name(&self) -> &str {
        "read_only_tool"
    }
    fn description(&self) -> &str {
        "A read-only tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn is_read_only(&self) -> bool {
        true
    }
    async fn execute(&self, _input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::text("read result"))
    }
}

#[test]
fn test_is_read_only_or_plan_tool_plan_control_tools() {
    let registry = ToolRegistry::new();

    // Plan control tools should always return true, even without registry entries
    assert!(is_read_only_or_plan_tool(
        &registry,
        ToolName::EnterPlanMode.as_str()
    ));
    assert!(is_read_only_or_plan_tool(
        &registry,
        ToolName::ExitPlanMode.as_str()
    ));
    assert!(is_read_only_or_plan_tool(
        &registry,
        ToolName::AskUserQuestion.as_str()
    ));
    assert!(is_read_only_or_plan_tool(
        &registry,
        ToolName::TodoWrite.as_str()
    ));
    assert!(is_read_only_or_plan_tool(
        &registry,
        ToolName::TaskCreate.as_str()
    ));
    assert!(is_read_only_or_plan_tool(
        &registry,
        ToolName::TaskUpdate.as_str()
    ));
}

#[test]
fn test_is_read_only_or_plan_tool_read_only() {
    let mut registry = ToolRegistry::new();
    registry.register(ReadOnlyTool);

    assert!(is_read_only_or_plan_tool(&registry, "read_only_tool"));
}

#[test]
fn test_is_read_only_or_plan_tool_non_read_only() {
    let mut registry = ToolRegistry::new();
    registry.register(WriteLikeTool);

    assert!(!is_read_only_or_plan_tool(&registry, "write_like_tool"));
}

#[test]
fn test_is_read_only_or_plan_tool_unknown() {
    let registry = ToolRegistry::new();

    // Unknown tools are not read-only or plan tools
    assert!(!is_read_only_or_plan_tool(&registry, "nonexistent_tool"));
}

#[test]
fn test_is_read_only_or_plan_tool_mcp_tools_bypass() {
    let registry = ToolRegistry::new();

    // MCP tools always bypass plan mode filtering (CC: mcp__ prefix in Xk8)
    assert!(is_read_only_or_plan_tool(&registry, "mcp__server__tool"));
    assert!(is_read_only_or_plan_tool(
        &registry,
        "mcp__github__list_repos"
    ));
    assert!(is_read_only_or_plan_tool(
        &registry,
        "mcp__slack__send_message"
    ));

    // Non-MCP unknown tools are still denied
    assert!(!is_read_only_or_plan_tool(&registry, "some_random_tool"));
}

#[test]
fn test_permission_level_from_decision() {
    assert_eq!(
        PermissionLevel::from_decision("deny"),
        PermissionLevel::Deny
    );
    assert_eq!(PermissionLevel::from_decision("ask"), PermissionLevel::Ask);
    assert_eq!(
        PermissionLevel::from_decision("allow"),
        PermissionLevel::Allow
    );
    assert_eq!(
        PermissionLevel::from_decision(""),
        PermissionLevel::Undefined
    );
    assert_eq!(
        PermissionLevel::from_decision("invalid"),
        PermissionLevel::Undefined
    );
}
