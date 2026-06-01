use super::*;

#[test]
fn test_classify_tool_error() {
    assert_eq!(classify_tool_error(&ToolError::Cancelled), "cancelled");
    assert_eq!(
        classify_tool_error(&ToolError::Timeout { timeout_ms: 1000 }),
        "timeout"
    );
    assert_eq!(
        classify_tool_error(&ToolError::PermissionDenied {
            message: "denied".into()
        }),
        "permission_denied"
    );
}

#[test]
fn test_is_code_editing_tool() {
    assert!(is_code_editing_tool("Edit"));
    assert!(is_code_editing_tool("Write"));
    assert!(!is_code_editing_tool("Read"));
    assert!(!is_code_editing_tool("Glob"));
}

#[test]
fn test_extract_file_extension() {
    assert_eq!(
        extract_file_extension("Read", &serde_json::json!({"file_path": "src/main.rs"})),
        Some("rs".to_string())
    );
    assert_eq!(
        extract_file_extension("Bash", &serde_json::json!({"command": "ls"})),
        None
    );
}

// ── R7-T19: defense-in-depth strip tests ──
//
// `strip_internal_bash_fields` removes underscore-prefixed fields from
// model-provided Bash input as a safeguard against the model trying
// to set internal-only fields like `_simulatedSedEdit`.

#[test]
fn test_strip_simulated_sed_edit_from_bash_input() {
    let input = serde_json::json!({
        "command": "echo hello",
        "_simulatedSedEdit": {
            "filePath": "/etc/passwd",
            "newContent": "malicious"
        }
    });
    let stripped = strip_internal_bash_fields("Bash", input);
    assert!(stripped.get("command").is_some());
    assert!(
        stripped.get("_simulatedSedEdit").is_none(),
        "internal field must be stripped, got: {stripped:?}"
    );
}

#[test]
fn test_strip_passes_through_normal_bash_input() {
    let input = serde_json::json!({
        "command": "ls -la",
        "timeout": 5000,
        "description": "list files"
    });
    let stripped = strip_internal_bash_fields("Bash", input.clone());
    // Normal fields pass through unchanged.
    assert_eq!(stripped, input);
}

#[test]
fn test_strip_does_not_touch_non_bash_tools() {
    // Read tool input has `file_path` but no underscore-prefixed
    // fields. Even if it DID have one, the stripping is gated on
    // `tool_name == Bash` so other tools are untouched.
    let input = serde_json::json!({
        "file_path": "/tmp/foo.txt",
        "_some_internal": "should stay because not Bash"
    });
    let stripped = strip_internal_bash_fields("Read", input.clone());
    assert_eq!(stripped, input);
}

#[test]
fn test_strip_removes_all_underscore_prefixed_bash_fields() {
    // The convention is "any underscore-prefixed key", not just
    // `_simulatedSedEdit` specifically. Future internal fields
    // following the same convention will be stripped automatically.
    let input = serde_json::json!({
        "command": "echo hi",
        "_simulatedSedEdit": { "filePath": "/x", "newContent": "y" },
        "_secretFlag": true,
        "_anotherInternal": 42
    });
    let stripped = strip_internal_bash_fields("Bash", input);
    let obj = stripped.as_object().unwrap();
    assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("command"));
}

#[test]
fn test_strip_handles_non_object_bash_input() {
    // Defensive: Bash input that's not an object (rare but possible
    // in malformed traffic) is returned unchanged.
    let input = serde_json::json!("not an object");
    let stripped = strip_internal_bash_fields("Bash", input.clone());
    assert_eq!(stripped, input);
}

struct ValidateRawExecuteStrippedBashTool;

#[async_trait::async_trait]
impl crate::traits::Tool for ValidateRawExecuteStrippedBashTool {
    fn runtime_validation_schema(&self) -> &crate::schema::ToolInputSchema {
        crate::schema::test_runtime_schema()
    } // Migration scaffold: assoc types pinned to `Value`.
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Bash)
    }

    fn name(&self) -> &str {
        ToolName::Bash.as_str()
    }

    fn description(&self, _: &serde_json::Value, _: &crate::traits::DescriptionOptions) -> String {
        String::new()
    }

    fn validate_input(
        &self,
        input: &serde_json::Value,
        _ctx: &ToolUseContext,
    ) -> crate::validation::ValidationResult {
        if input.get("_simulatedSedEdit").is_some() {
            crate::validation::ValidationResult::Valid
        } else {
            crate::validation::ValidationResult::invalid("validation expected raw input")
        }
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<serde_json::Value>, ToolError> {
        if input.get("_simulatedSedEdit").is_some() {
            return Err(ToolError::ExecutionFailed {
                message: "execution received unstripped input".into(),
                display_data: None,
                source: None,
            });
        }

        Ok(ToolResult {
            data: input,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

#[tokio::test]
async fn test_execute_tool_call_validates_raw_input_before_stripping() {
    let tools = ToolRegistry::new();
    tools.register(std::sync::Arc::new(ValidateRawExecuteStrippedBashTool));
    let ctx = ToolUseContext::test_default();

    let result = execute_tool_call(
        "toolu_1",
        ToolName::Bash.as_str(),
        serde_json::json!({
            "command": "echo ok",
            "_simulatedSedEdit": {
                "filePath": "/tmp/file",
                "newContent": "content"
            }
        }),
        &tools,
        &ctx,
    )
    .await;

    let data = result.result.unwrap().data;
    assert_eq!(data, serde_json::json!({"command": "echo ok"}));
}

// ── Step 3.5 (canUseTool callback) tests — Phase 1 fork plumbing ──
//
// Each test installs a custom `CanUseToolHandle` on the
// `ToolUseContext.can_use_tool` slot and asserts the executor honours
// the decision per TS `services/tools/toolExecution.ts:706-748`.

/// Echo tool that records the input it receives at execute time so we
/// can verify path-rewrite from `Allow{updated_input}` actually
/// reaches the tool. `check_permissions` always allows so the test
/// only observes the canUseTool decision (not the built-in opinion).
struct EchoTool {
    /// When set, the tool's built-in `check_permissions` denies. Used
    /// to verify that `Allow` from canUseTool short-circuits the
    /// built-in check (TS parity: callback is authoritative).
    deny_in_check: bool,
}

#[async_trait::async_trait]
impl crate::traits::Tool for EchoTool {
    fn runtime_validation_schema(&self) -> &crate::schema::ToolInputSchema {
        crate::schema::test_runtime_schema()
    } // Migration scaffold: assoc types pinned to `Value`.
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    fn id(&self) -> ToolId {
        ToolId::Custom("Echo".into())
    }
    fn name(&self) -> &str {
        "Echo"
    }
    fn description(&self, _: &serde_json::Value, _: &crate::traits::DescriptionOptions) -> String {
        String::new()
    }
    fn validate_input(
        &self,
        _input: &serde_json::Value,
        _ctx: &ToolUseContext,
    ) -> crate::validation::ValidationResult {
        crate::validation::ValidationResult::Valid
    }
    async fn check_permissions(
        &self,
        _input: &serde_json::Value,
        _ctx: &ToolUseContext,
    ) -> coco_types::ToolCheckResult {
        if self.deny_in_check {
            coco_types::ToolCheckResult::Deny {
                message: "built-in deny".into(),
            }
        } else {
            coco_types::ToolCheckResult::Passthrough
        }
    }
    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<serde_json::Value>, ToolError> {
        Ok(ToolResult {
            data: input,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

#[derive(Debug)]
struct AlwaysDenyHandle;

#[async_trait::async_trait]
impl crate::can_use_tool::CanUseToolHandle for AlwaysDenyHandle {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &crate::can_use_tool::CanUseToolCallContext,
    ) -> crate::can_use_tool::CanUseToolDecision {
        crate::can_use_tool::CanUseToolDecision::Deny {
            message: "fork deny".into(),
            decision_reason: crate::can_use_tool::DecisionReason::Other {
                reason: "test".into(),
            },
        }
    }
}

#[derive(Debug)]
struct AlwaysAllowRewriteHandle {
    rewritten: serde_json::Value,
}

#[async_trait::async_trait]
impl crate::can_use_tool::CanUseToolHandle for AlwaysAllowRewriteHandle {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &crate::can_use_tool::CanUseToolCallContext,
    ) -> crate::can_use_tool::CanUseToolDecision {
        crate::can_use_tool::CanUseToolDecision::Allow {
            updated_input: Some(self.rewritten.clone()),
            decision_reason: crate::can_use_tool::DecisionReason::Other {
                reason: "rewrite".into(),
            },
        }
    }
}

#[derive(Debug)]
struct AlwaysAllowPassthroughHandle;

#[async_trait::async_trait]
impl crate::can_use_tool::CanUseToolHandle for AlwaysAllowPassthroughHandle {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &crate::can_use_tool::CanUseToolCallContext,
    ) -> crate::can_use_tool::CanUseToolDecision {
        crate::can_use_tool::CanUseToolDecision::Allow {
            updated_input: None,
            decision_reason: crate::can_use_tool::DecisionReason::ModeAllow,
        }
    }
}

#[derive(Debug)]
struct AlwaysAskHandle;

#[async_trait::async_trait]
impl crate::can_use_tool::CanUseToolHandle for AlwaysAskHandle {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &crate::can_use_tool::CanUseToolCallContext,
    ) -> crate::can_use_tool::CanUseToolDecision {
        crate::can_use_tool::CanUseToolDecision::Ask {
            decision_reason: crate::can_use_tool::DecisionReason::Other {
                reason: "ask".into(),
            },
        }
    }
}

#[tokio::test]
async fn test_can_use_tool_deny_short_circuits_before_check_permissions() {
    let tools = ToolRegistry::new();
    tools.register(std::sync::Arc::new(EchoTool {
        deny_in_check: false,
    }));

    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(std::sync::Arc::new(AlwaysDenyHandle));

    let result = execute_tool_call("tu-1", "Echo", serde_json::json!({"x": 1}), &tools, &ctx).await;

    assert!(result.permission_denied);
    match result.result {
        Err(ToolError::PermissionDenied { message }) => {
            assert!(message.contains("fork deny"), "got {message}");
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn test_can_use_tool_allow_with_updated_input_rewrites() {
    let tools = ToolRegistry::new();
    tools.register(std::sync::Arc::new(EchoTool {
        deny_in_check: false,
    }));

    let mut ctx = ToolUseContext::test_default();
    let rewritten = serde_json::json!({"file_path": "/overlay/foo.txt"});
    ctx.can_use_tool = Some(std::sync::Arc::new(AlwaysAllowRewriteHandle {
        rewritten: rewritten.clone(),
    }));

    let result = execute_tool_call(
        "tu-1",
        "Echo",
        serde_json::json!({"file_path": "/main/foo.txt"}),
        &tools,
        &ctx,
    )
    .await;

    let data = result.result.unwrap().data;
    assert_eq!(
        data, rewritten,
        "Allow{{updated_input}} must rewrite the value reaching execute"
    );
}

#[tokio::test]
async fn test_can_use_tool_allow_skips_builtin_check_permissions() {
    let tools = ToolRegistry::new();
    // Built-in opinion would Deny — but Allow from canUseTool MUST
    // win (TS parity: callback is authoritative for the Allow path).
    tools.register(std::sync::Arc::new(EchoTool {
        deny_in_check: true,
    }));

    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(std::sync::Arc::new(AlwaysAllowPassthroughHandle));

    let result = execute_tool_call("tu-1", "Echo", serde_json::json!({"x": 1}), &tools, &ctx).await;

    assert!(
        !result.permission_denied,
        "Allow from canUseTool should win over built-in Deny"
    );
    assert_eq!(result.result.unwrap().data, serde_json::json!({"x": 1}));
}

#[tokio::test]
async fn test_can_use_tool_ask_falls_through_to_builtin_check() {
    let tools = ToolRegistry::new();
    // Built-in opinion Denies. Ask from canUseTool MUST fall through
    // and let the built-in opinion stand.
    tools.register(std::sync::Arc::new(EchoTool {
        deny_in_check: true,
    }));

    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(std::sync::Arc::new(AlwaysAskHandle));

    let result = execute_tool_call("tu-1", "Echo", serde_json::json!({"x": 1}), &tools, &ctx).await;

    assert!(
        result.permission_denied,
        "Ask should fall through to built-in Deny"
    );
}

#[tokio::test]
async fn test_no_op_can_use_tool_handle_falls_through() {
    let tools = ToolRegistry::new();
    tools.register(std::sync::Arc::new(EchoTool {
        deny_in_check: false,
    }));

    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(std::sync::Arc::new(
        crate::can_use_tool::NoOpCanUseToolHandle,
    ));

    let result = execute_tool_call("tu-1", "Echo", serde_json::json!({"x": 1}), &tools, &ctx).await;

    // Ask → falls through to Passthrough (built-in) → execute.
    assert!(!result.permission_denied);
    assert_eq!(result.result.unwrap().data, serde_json::json!({"x": 1}));
}

#[tokio::test]
async fn test_no_can_use_tool_handle_preserves_pre_step_3_5_behavior() {
    // When `ctx.can_use_tool` is None, step 3.5 is skipped entirely
    // and the built-in `check_permissions` is the only gate. This
    // confirms non-fork code paths see no behavior change.
    let tools = ToolRegistry::new();
    tools.register(std::sync::Arc::new(EchoTool {
        deny_in_check: true,
    }));

    let ctx = ToolUseContext::test_default();
    assert!(ctx.can_use_tool.is_none());

    let result = execute_tool_call("tu-1", "Echo", serde_json::json!({"x": 1}), &tools, &ctx).await;

    assert!(
        result.permission_denied,
        "without canUseTool, built-in Deny applies unchanged"
    );
}
