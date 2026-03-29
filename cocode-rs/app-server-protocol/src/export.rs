//! JSON Schema export binary.
//!
//! Generates JSON Schema files for all protocol types into
//! `app-server-protocol/schema/json/`. These schemas are the source of
//! truth for multi-language SDK type generation (Python, TypeScript, etc.).
//!
//! Usage: `cargo run --bin export-app-server-schema`

use std::fs;
use std::path::Path;

use schemars::schema_for;

fn main() -> anyhow::Result<()> {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/json");
    fs::create_dir_all(&out_dir)?;

    // Generate individual schemas
    write_schema::<cocode_app_server_protocol::ServerNotification>(
        &out_dir,
        "server_notification",
    )?;
    write_schema::<cocode_app_server_protocol::ClientRequest>(&out_dir, "client_request")?;
    write_schema::<cocode_app_server_protocol::ServerRequest>(&out_dir, "server_request")?;
    write_schema::<cocode_app_server_protocol::ThreadItem>(&out_dir, "thread_item")?;
    write_schema::<cocode_app_server_protocol::Usage>(&out_dir, "usage")?;
    write_schema::<cocode_app_server_protocol::SessionStartRequestParams>(
        &out_dir,
        "session_start_request",
    )?;

    // Generate bundled schema (all types in one file)
    let mut bundle = serde_json::Map::new();
    bundle.insert(
        "$schema".into(),
        serde_json::Value::String("https://json-schema.org/draft/2020-12/schema".into()),
    );
    bundle.insert(
        "title".into(),
        serde_json::Value::String("cocode-app-server-protocol".into()),
    );

    let mut definitions = serde_json::Map::new();
    let type_schemas: Vec<(&str, schemars::schema::RootSchema)> = vec![
        (
            "ServerNotification",
            schema_for!(cocode_app_server_protocol::ServerNotification),
        ),
        (
            "ClientRequest",
            schema_for!(cocode_app_server_protocol::ClientRequest),
        ),
        (
            "ServerRequest",
            schema_for!(cocode_app_server_protocol::ServerRequest),
        ),
        (
            "ThreadItem",
            schema_for!(cocode_app_server_protocol::ThreadItem),
        ),
        ("Usage", schema_for!(cocode_app_server_protocol::Usage)),
        (
            "SessionStartRequestParams",
            schema_for!(cocode_app_server_protocol::SessionStartRequestParams),
        ),
        (
            "ItemStatus",
            schema_for!(cocode_app_server_protocol::ItemStatus),
        ),
        (
            "ApprovalDecision",
            schema_for!(cocode_app_server_protocol::ApprovalDecision),
        ),
        (
            "McpServerConfig",
            schema_for!(cocode_app_server_protocol::McpServerConfig),
        ),
        (
            "AgentDefinitionConfig",
            schema_for!(cocode_app_server_protocol::AgentDefinitionConfig),
        ),
        (
            "AgentIsolationMode",
            schema_for!(cocode_app_server_protocol::AgentIsolationMode),
        ),
        (
            "AgentMemoryScope",
            schema_for!(cocode_app_server_protocol::AgentMemoryScope),
        ),
        (
            "AgentHookConfig",
            schema_for!(cocode_app_server_protocol::AgentHookConfig),
        ),
        (
            "HookCallbackConfig",
            schema_for!(cocode_app_server_protocol::HookCallbackConfig),
        ),
        (
            "SandboxConfig",
            schema_for!(cocode_app_server_protocol::SandboxConfig),
        ),
        (
            "SandboxMode",
            schema_for!(cocode_app_server_protocol::SandboxMode),
        ),
        (
            "ThinkingConfig",
            schema_for!(cocode_app_server_protocol::ThinkingConfig),
        ),
        (
            "ThinkingMode",
            schema_for!(cocode_app_server_protocol::ThinkingMode),
        ),
        (
            "SystemPromptConfig",
            schema_for!(cocode_app_server_protocol::SystemPromptConfig),
        ),
        (
            "ToolsConfig",
            schema_for!(cocode_app_server_protocol::ToolsConfig),
        ),
        (
            "OutputFormatConfig",
            schema_for!(cocode_app_server_protocol::OutputFormatConfig),
        ),
        (
            "JsonRpcRequest",
            schema_for!(cocode_app_server_protocol::JsonRpcRequest),
        ),
        (
            "JsonRpcNotification",
            schema_for!(cocode_app_server_protocol::JsonRpcNotification),
        ),
        (
            "JsonRpcResponse",
            schema_for!(cocode_app_server_protocol::JsonRpcResponse),
        ),
        (
            "JsonRpcError",
            schema_for!(cocode_app_server_protocol::JsonRpcError),
        ),
        (
            "RequestId",
            schema_for!(cocode_app_server_protocol::RequestId),
        ),
        (
            "SessionEndedReason",
            schema_for!(cocode_app_server_protocol::SessionEndedReason),
        ),
        (
            "PreToolUseHookInput",
            schema_for!(cocode_app_server_protocol::PreToolUseHookInput),
        ),
        (
            "PostToolUseHookInput",
            schema_for!(cocode_app_server_protocol::PostToolUseHookInput),
        ),
        (
            "PostToolUseFailureHookInput",
            schema_for!(cocode_app_server_protocol::PostToolUseFailureHookInput),
        ),
        (
            "PreCompactHookInput",
            schema_for!(cocode_app_server_protocol::PreCompactHookInput),
        ),
        (
            "PermissionRequestHookInput",
            schema_for!(cocode_app_server_protocol::PermissionRequestHookInput),
        ),
        (
            "SessionStartHookInput",
            schema_for!(cocode_app_server_protocol::SessionStartHookInput),
        ),
        (
            "SessionEndHookInput",
            schema_for!(cocode_app_server_protocol::SessionEndHookInput),
        ),
        (
            "SdkMcpToolDef",
            schema_for!(cocode_app_server_protocol::SdkMcpToolDef),
        ),
        (
            "HookCallbackOutput",
            schema_for!(cocode_app_server_protocol::HookCallbackOutput),
        ),
        (
            "HookBehavior",
            schema_for!(cocode_app_server_protocol::HookBehavior),
        ),
        (
            "PermissionSuggestion",
            schema_for!(cocode_app_server_protocol::PermissionSuggestion),
        ),
        (
            "SessionResultParams",
            schema_for!(cocode_app_server_protocol::SessionResultParams),
        ),
        (
            "PromptSuggestionParams",
            schema_for!(cocode_app_server_protocol::PromptSuggestionParams),
        ),
        (
            "SetThinkingRequestParams",
            schema_for!(cocode_app_server_protocol::SetThinkingRequestParams),
        ),
        (
            "RewindFilesRequestParams",
            schema_for!(cocode_app_server_protocol::RewindFilesRequestParams),
        ),
        (
            "StopHookInput",
            schema_for!(cocode_app_server_protocol::StopHookInput),
        ),
        (
            "SubagentStartHookInput",
            schema_for!(cocode_app_server_protocol::SubagentStartHookInput),
        ),
        (
            "SubagentStopHookInput",
            schema_for!(cocode_app_server_protocol::SubagentStopHookInput),
        ),
        (
            "UserPromptSubmitHookInput",
            schema_for!(cocode_app_server_protocol::UserPromptSubmitHookInput),
        ),
        (
            "NotificationHookInput",
            schema_for!(cocode_app_server_protocol::NotificationHookInput),
        ),
        // ── Initialize / session management result types ──────────────
        (
            "InitializeRequestParams",
            schema_for!(cocode_app_server_protocol::InitializeRequestParams),
        ),
        (
            "InitializeResult",
            schema_for!(cocode_app_server_protocol::InitializeResult),
        ),
        (
            "ClientInfo",
            schema_for!(cocode_app_server_protocol::ClientInfo),
        ),
        (
            "InitializeCapabilities",
            schema_for!(cocode_app_server_protocol::InitializeCapabilities),
        ),
        (
            "SessionListResult",
            schema_for!(cocode_app_server_protocol::SessionListResult),
        ),
        (
            "SessionSummary",
            schema_for!(cocode_app_server_protocol::SessionSummary),
        ),
        (
            "ConfigReadResult",
            schema_for!(cocode_app_server_protocol::ConfigReadResult),
        ),
        (
            "ConfigWriteScope",
            schema_for!(cocode_app_server_protocol::ConfigWriteScope),
        ),
        // ── Client request param types ────────────────────────────────
        (
            "SessionResumeRequestParams",
            schema_for!(cocode_app_server_protocol::SessionResumeRequestParams),
        ),
        (
            "TurnStartRequestParams",
            schema_for!(cocode_app_server_protocol::TurnStartRequestParams),
        ),
        (
            "TurnInterruptRequestParams",
            schema_for!(cocode_app_server_protocol::TurnInterruptRequestParams),
        ),
        (
            "ApprovalResolveRequestParams",
            schema_for!(cocode_app_server_protocol::ApprovalResolveRequestParams),
        ),
        (
            "UserInputResolveRequestParams",
            schema_for!(cocode_app_server_protocol::UserInputResolveRequestParams),
        ),
        (
            "SetModelRequestParams",
            schema_for!(cocode_app_server_protocol::SetModelRequestParams),
        ),
        (
            "SetPermissionModeRequestParams",
            schema_for!(cocode_app_server_protocol::SetPermissionModeRequestParams),
        ),
        (
            "StopTaskRequestParams",
            schema_for!(cocode_app_server_protocol::StopTaskRequestParams),
        ),
        (
            "HookCallbackResponseParams",
            schema_for!(cocode_app_server_protocol::HookCallbackResponseParams),
        ),
        (
            "UpdateEnvRequestParams",
            schema_for!(cocode_app_server_protocol::UpdateEnvRequestParams),
        ),
        (
            "KeepAliveRequestParams",
            schema_for!(cocode_app_server_protocol::KeepAliveRequestParams),
        ),
        (
            "SessionListRequestParams",
            schema_for!(cocode_app_server_protocol::SessionListRequestParams),
        ),
        (
            "SessionReadRequestParams",
            schema_for!(cocode_app_server_protocol::SessionReadRequestParams),
        ),
        (
            "SessionArchiveRequestParams",
            schema_for!(cocode_app_server_protocol::SessionArchiveRequestParams),
        ),
        (
            "ConfigReadRequestParams",
            schema_for!(cocode_app_server_protocol::ConfigReadRequestParams),
        ),
        (
            "ConfigWriteRequestParams",
            schema_for!(cocode_app_server_protocol::ConfigWriteRequestParams),
        ),
        (
            "McpRouteMessageResponseParams",
            schema_for!(cocode_app_server_protocol::McpRouteMessageResponseParams),
        ),
        (
            "CancelRequestParams",
            schema_for!(cocode_app_server_protocol::CancelRequestParams),
        ),
        // ── Server request param types ────────────────────────────────
        (
            "AskForApprovalParams",
            schema_for!(cocode_app_server_protocol::AskForApprovalParams),
        ),
        (
            "RequestUserInputParams",
            schema_for!(cocode_app_server_protocol::RequestUserInputParams),
        ),
        (
            "McpRouteMessageParams",
            schema_for!(cocode_app_server_protocol::McpRouteMessageParams),
        ),
        (
            "HookCallbackParams",
            schema_for!(cocode_app_server_protocol::HookCallbackParams),
        ),
        (
            "ServerCancelRequestParams",
            schema_for!(cocode_app_server_protocol::ServerCancelRequestParams),
        ),
        // ── Error info types ──────────────────────────────────────────
        (
            "ErrorInfo",
            schema_for!(cocode_app_server_protocol::ErrorInfo),
        ),
        (
            "ErrorNotificationParams",
            schema_for!(cocode_app_server_protocol::ErrorNotificationParams),
        ),
        (
            "ErrorCategory",
            schema_for!(cocode_app_server_protocol::ErrorCategory),
        ),
        // ── Item detail types ─────────────────────────────────────────
        (
            "McpToolCallResult",
            schema_for!(cocode_app_server_protocol::McpToolCallResult),
        ),
        (
            "McpToolCallError",
            schema_for!(cocode_app_server_protocol::McpToolCallError),
        ),
        // ── Config detail types ───────────────────────────────────────
        (
            "HookMatcherConfig",
            schema_for!(cocode_app_server_protocol::HookMatcherConfig),
        ),
        // ── Category B notification param types ─────────────────────
        (
            "PlanModeChangedParams",
            schema_for!(cocode_app_server_protocol::PlanModeChangedParams),
        ),
        (
            "QueueStateChangedParams",
            schema_for!(cocode_app_server_protocol::QueueStateChangedParams),
        ),
        (
            "RewindCompletedParams",
            schema_for!(cocode_app_server_protocol::RewindCompletedParams),
        ),
        (
            "RewindFailedParams",
            schema_for!(cocode_app_server_protocol::RewindFailedParams),
        ),
        (
            "CostWarningParams",
            schema_for!(cocode_app_server_protocol::CostWarningParams),
        ),
        (
            "SandboxStateChangedParams",
            schema_for!(cocode_app_server_protocol::SandboxStateChangedParams),
        ),
        (
            "FastModeChangedParams",
            schema_for!(cocode_app_server_protocol::FastModeChangedParams),
        ),
        (
            "AgentsRegisteredParams",
            schema_for!(cocode_app_server_protocol::AgentsRegisteredParams),
        ),
        (
            "AgentInfo",
            schema_for!(cocode_app_server_protocol::AgentInfo),
        ),
        (
            "HookExecutedParams",
            schema_for!(cocode_app_server_protocol::HookExecutedParams),
        ),
        (
            "SummarizeCompletedParams",
            schema_for!(cocode_app_server_protocol::SummarizeCompletedParams),
        ),
        (
            "SummarizeFailedParams",
            schema_for!(cocode_app_server_protocol::SummarizeFailedParams),
        ),
    ];
    for (name, schema) in type_schemas {
        definitions.insert(name.into(), serde_json::to_value(schema)?);
    }
    bundle.insert("definitions".into(), serde_json::Value::Object(definitions));

    let bundle_path = out_dir.join("cocode_app_server_protocol.schemas.json");
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(bundle))?;
    fs::write(&bundle_path, json)?;

    println!("Schemas written to {}", out_dir.display());
    Ok(())
}

fn write_schema<T: schemars::JsonSchema>(out_dir: &Path, name: &str) -> anyhow::Result<()> {
    let schema = schema_for!(T);
    let json = serde_json::to_string_pretty(&schema)?;
    let path = out_dir.join(format!("{name}.json"));
    fs::write(&path, &json)?;
    println!("  {}", path.display());
    Ok(())
}
