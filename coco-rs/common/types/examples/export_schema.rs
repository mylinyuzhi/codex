//! JSON Schema export tool.
//!
//! Generates JSON Schema files for all SDK protocol types into
//! `coco-sdk/schemas/json/` (or a custom output directory). These schemas
//! are the source of truth for multi-language SDK type generation (Python
//! via `scripts/generate_python.sh`, TypeScript, Go, etc.).
//!
//! Usage:
//!   cargo run -p coco-types --features schema --example export_schema
//!   cargo run -p coco-types --features schema --example export_schema -- /custom/path
//!
//! TS reference: cocode-rs `app-server-protocol/src/export.rs` (454 lines).
//! The coco-rs version is simpler because our types are centralized in
//! `coco-types` instead of a separate app-server-protocol crate.

// The bulk of this example requires the `schema` feature; without it the
// `schemars::JsonSchema` derives are not generated. Provide a `main`
// under both configurations so `cargo test --all-targets` (which does not
// enable the feature) still links successfully.

#[cfg(not(feature = "schema"))]
fn main() {
    eprintln!(
        "error: export_schema requires the `schema` feature.\n\
         Run: cargo run -p coco-types --features schema --example export_schema"
    );
    std::process::exit(1);
}

#[cfg(feature = "schema")]
use std::fs;
#[cfg(feature = "schema")]
use std::path::Path;
#[cfg(feature = "schema")]
use std::path::PathBuf;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "schema")]
use schemars::schema::RootSchema;
#[cfg(feature = "schema")]
use schemars::schema_for;
#[cfg(feature = "schema")]
use serde_json::Value;
#[cfg(feature = "schema")]
use serde_json::json;

#[cfg(feature = "schema")]
/// Write a single type's schema to `<out_dir>/<name>.json`.
fn write_schema<T: JsonSchema>(out_dir: &Path, name: &str) {
    let schema = schema_for!(T);
    let path = out_dir.join(format!("{name}.json"));
    let json = serde_json::to_string_pretty(&schema).expect("serialize schema");
    fs::write(&path, json).unwrap_or_else(|e| {
        panic!("write {} failed: {e}", path.display());
    });
    println!("  ✓ {}", path.display());
}

#[cfg(feature = "schema")]
/// Build one entry in the bundled schema file.
fn bundle_entry<T: JsonSchema>(name: &str) -> (String, RootSchema) {
    (name.to_string(), schema_for!(T))
}

#[cfg(feature = "schema")]
fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Default to coco-sdk/schemas/json relative to the repo root.
    // CARGO_MANIFEST_DIR = coco-rs/common/types, so repo root is ../../.. from there.
    let out_dir: PathBuf = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest
            .join("../../..")
            .join("coco-sdk/schemas/json")
            .canonicalize()
            .unwrap_or_else(|_| manifest.join("../../../coco-sdk/schemas/json"))
    };

    fs::create_dir_all(&out_dir).expect("create out_dir");
    println!("Writing schemas to {}", out_dir.display());
    println!();

    // --- Individual top-level schema files ---
    //
    // Note: `CoreEvent` itself is NOT exported — it's an in-process dispatch
    // wrapper around Protocol/Stream/Tui and is not meant to cross the wire.
    // The three child enums are the actual wire types.
    println!("Individual schemas:");
    write_schema::<coco_types::ServerNotification>(&out_dir, "server_notification");
    write_schema::<coco_types::AgentStreamEvent>(&out_dir, "agent_stream_event");
    write_schema::<coco_types::TuiOnlyEvent>(&out_dir, "tui_only_event");
    write_schema::<coco_types::ClientRequest>(&out_dir, "client_request");
    write_schema::<coco_types::ServerRequest>(&out_dir, "server_request");
    write_schema::<coco_types::JsonRpcMessage>(&out_dir, "jsonrpc_message");
    write_schema::<coco_types::ThreadItem>(&out_dir, "thread_item");
    write_schema::<coco_types::SessionStartParams>(&out_dir, "session_start_request");
    println!();

    // --- Bundled schema: all types in one file, matching the cocode-sdk layout ---
    // The Python SDK generator expects `coco_app_server_protocol.schemas.json`.
    println!("Bundled schema:");

    let type_schemas: Vec<(String, RootSchema)> = vec![
        // 3-layer CoreEvent sub-enums (the envelope itself is in-process only).
        bundle_entry::<coco_types::ServerNotification>("ServerNotification"),
        bundle_entry::<coco_types::AgentStreamEvent>("AgentStreamEvent"),
        bundle_entry::<coco_types::TuiOnlyEvent>("TuiOnlyEvent"),
        // ThreadItem + ItemStatus
        bundle_entry::<coco_types::ThreadItem>("ThreadItem"),
        bundle_entry::<coco_types::ThreadItemDetails>("ThreadItemDetails"),
        bundle_entry::<coco_types::ItemStatus>("ItemStatus"),
        bundle_entry::<coco_types::FileChangeInfo>("FileChangeInfo"),
        // Control protocol
        bundle_entry::<coco_types::ClientRequest>("ClientRequest"),
        bundle_entry::<coco_types::ServerRequest>("ServerRequest"),
        bundle_entry::<coco_types::JsonRpcMessage>("JsonRpcMessage"),
        bundle_entry::<coco_types::JsonRpcRequest>("JsonRpcRequest"),
        bundle_entry::<coco_types::JsonRpcResponse>("JsonRpcResponse"),
        bundle_entry::<coco_types::JsonRpcError>("JsonRpcError"),
        bundle_entry::<coco_types::JsonRpcNotification>("JsonRpcNotification"),
        bundle_entry::<coco_types::RequestId>("RequestId"),
        bundle_entry::<coco_types::ApprovalDecision>("ApprovalDecision"),
        // Session + turn params
        bundle_entry::<coco_types::SessionStartedParams>("SessionStartedParams"),
        bundle_entry::<coco_types::SessionResultParams>("SessionResultParams"),
        bundle_entry::<coco_types::SessionEndedParams>("SessionEndedParams"),
        bundle_entry::<coco_types::SessionState>("SessionState"),
        bundle_entry::<coco_types::SessionStartParams>("SessionStartParams"),
        bundle_entry::<coco_types::TurnStartedParams>("TurnStartedParams"),
        bundle_entry::<coco_types::TurnCompletedParams>("TurnCompletedParams"),
        bundle_entry::<coco_types::TurnFailedParams>("TurnFailedParams"),
        bundle_entry::<coco_types::TurnInterruptedParams>("TurnInterruptedParams"),
        bundle_entry::<coco_types::TurnStartParams>("TurnStartParams"),
        // Streaming content
        bundle_entry::<coco_types::ContentDeltaParams>("ContentDeltaParams"),
        // Subagent
        bundle_entry::<coco_types::SubagentSpawnedParams>("SubagentSpawnedParams"),
        bundle_entry::<coco_types::SubagentCompletedParams>("SubagentCompletedParams"),
        bundle_entry::<coco_types::SubagentBackgroundedParams>("SubagentBackgroundedParams"),
        bundle_entry::<coco_types::SubagentProgressParams>("SubagentProgressParams"),
        // MCP
        bundle_entry::<coco_types::McpStartupStatusParams>("McpStartupStatusParams"),
        bundle_entry::<coco_types::McpStartupCompleteParams>("McpStartupCompleteParams"),
        bundle_entry::<coco_types::McpServerStatus>("McpServerStatus"),
        bundle_entry::<coco_types::McpStatusResult>("McpStatusResult"),
        bundle_entry::<coco_types::McpSetServersParams>("McpSetServersParams"),
        bundle_entry::<coco_types::McpSetServersResult>("McpSetServersResult"),
        bundle_entry::<coco_types::McpReconnectParams>("McpReconnectParams"),
        bundle_entry::<coco_types::McpToggleParams>("McpToggleParams"),
        bundle_entry::<coco_types::McpServerInit>("McpServerInit"),
        // Context
        bundle_entry::<coco_types::ContextCompactedParams>("ContextCompactedParams"),
        bundle_entry::<coco_types::ContextUsageWarningParams>("ContextUsageWarningParams"),
        bundle_entry::<coco_types::ContextClearedParams>("ContextClearedParams"),
        bundle_entry::<coco_types::CompactionFailedParams>("CompactionFailedParams"),
        bundle_entry::<coco_types::ContextUsageResult>("ContextUsageResult"),
        bundle_entry::<coco_types::ContextUsageCategory>("ContextUsageCategory"),
        bundle_entry::<coco_types::MessageBreakdown>("MessageBreakdown"),
        // Task
        bundle_entry::<coco_types::TaskStartedParams>("TaskStartedParams"),
        bundle_entry::<coco_types::TaskCompletedParams>("TaskCompletedParams"),
        bundle_entry::<coco_types::TaskProgressParams>("TaskProgressParams"),
        bundle_entry::<coco_types::TaskCompletionStatus>("TaskCompletionStatus"),
        bundle_entry::<coco_types::TaskUsage>("TaskUsage"),
        // Model + permission + system
        bundle_entry::<coco_types::ModelFallbackParams>("ModelFallbackParams"),
        bundle_entry::<coco_types::PermissionModeChangedParams>("PermissionModeChangedParams"),
        bundle_entry::<coco_types::PermissionMode>("PermissionMode"),
        bundle_entry::<coco_types::PermissionDenialInfo>("PermissionDenialInfo"),
        bundle_entry::<coco_types::SessionModelUsage>("SessionModelUsage"),
        bundle_entry::<coco_types::FastModeState>("FastModeState"),
        bundle_entry::<coco_types::ErrorParams>("ErrorParams"),
        bundle_entry::<coco_types::RateLimitParams>("RateLimitParams"),
        bundle_entry::<coco_types::RateLimitStatus>("RateLimitStatus"),
        // IDE + plan + queue
        bundle_entry::<coco_types::IdeSelectionChangedParams>("IdeSelectionChangedParams"),
        bundle_entry::<coco_types::IdeDiagnosticsUpdatedParams>("IdeDiagnosticsUpdatedParams"),
        bundle_entry::<coco_types::PlanModeChangedParams>("PlanModeChangedParams"),
        // Hook lifecycle
        bundle_entry::<coco_types::HookStartedParams>("HookStartedParams"),
        bundle_entry::<coco_types::HookProgressParams>("HookProgressParams"),
        bundle_entry::<coco_types::HookResponseParams>("HookResponseParams"),
        bundle_entry::<coco_types::HookOutcomeStatus>("HookOutcomeStatus"),
        // Worktree + summarize
        bundle_entry::<coco_types::WorktreeEnteredParams>("WorktreeEnteredParams"),
        bundle_entry::<coco_types::WorktreeExitedParams>("WorktreeExitedParams"),
        bundle_entry::<coco_types::SummarizeCompletedParams>("SummarizeCompletedParams"),
        // Rewind + cost + sandbox
        bundle_entry::<coco_types::RewindCompletedParams>("RewindCompletedParams"),
        bundle_entry::<coco_types::CostWarningParams>("CostWarningParams"),
        bundle_entry::<coco_types::SandboxStateChangedParams>("SandboxStateChangedParams"),
        // TS gap additions (Phase 0)
        bundle_entry::<coco_types::LocalCommandOutputParams>("LocalCommandOutputParams"),
        bundle_entry::<coco_types::FilesPersistedParams>("FilesPersistedParams"),
        bundle_entry::<coco_types::PersistedFileInfo>("PersistedFileInfo"),
        bundle_entry::<coco_types::PersistedFileError>("PersistedFileError"),
        bundle_entry::<coco_types::ElicitationCompleteParams>("ElicitationCompleteParams"),
        bundle_entry::<coco_types::ToolUseSummaryParams>("ToolUseSummaryParams"),
        bundle_entry::<coco_types::ToolProgressParams>("ToolProgressParams"),
        // Core value types
        bundle_entry::<coco_types::TokenUsage>("TokenUsage"),
        bundle_entry::<coco_types::ThinkingLevel>("ThinkingLevel"),
        bundle_entry::<coco_types::AgentInfo>("AgentInfo"),
        bundle_entry::<coco_types::PluginInit>("PluginInit"),
        // Control protocol param structs (ClientRequest variants)
        bundle_entry::<coco_types::InitializeParams>("InitializeParams"),
        bundle_entry::<coco_types::HookCallbackMatcher>("HookCallbackMatcher"),
        bundle_entry::<coco_types::SessionResumeParams>("SessionResumeParams"),
        bundle_entry::<coco_types::SessionReadParams>("SessionReadParams"),
        bundle_entry::<coco_types::SessionArchiveParams>("SessionArchiveParams"),
        bundle_entry::<coco_types::ApprovalResolveParams>("ApprovalResolveParams"),
        bundle_entry::<coco_types::UserInputResolveParams>("UserInputResolveParams"),
        bundle_entry::<coco_types::SetModelParams>("SetModelParams"),
        bundle_entry::<coco_types::SetPermissionModeParams>("SetPermissionModeParams"),
        bundle_entry::<coco_types::SetThinkingParams>("SetThinkingParams"),
        bundle_entry::<coco_types::StopTaskParams>("StopTaskParams"),
        bundle_entry::<coco_types::RewindFilesParams>("RewindFilesParams"),
        bundle_entry::<coco_types::UpdateEnvParams>("UpdateEnvParams"),
        bundle_entry::<coco_types::CancelRequestParams>("CancelRequestParams"),
        bundle_entry::<coco_types::ConfigWriteParams>("ConfigWriteParams"),
        bundle_entry::<coco_types::ClientHookCallbackResponseParams>(
            "ClientHookCallbackResponseParams",
        ),
        bundle_entry::<coco_types::McpRouteMessageResponseParams>("McpRouteMessageResponseParams"),
        bundle_entry::<coco_types::ConfigApplyFlagsParams>("ConfigApplyFlagsParams"),
        // ServerRequest param structs
        bundle_entry::<coco_types::ServerAskForApprovalParams>("ServerAskForApprovalParams"),
        bundle_entry::<coco_types::ServerRequestUserInputParams>("ServerRequestUserInputParams"),
        bundle_entry::<coco_types::ServerMcpRouteMessageParams>("ServerMcpRouteMessageParams"),
        bundle_entry::<coco_types::ServerHookCallbackParams>("ServerHookCallbackParams"),
        bundle_entry::<coco_types::ServerCancelRequestParams>("ServerCancelRequestParams"),
        bundle_entry::<coco_types::ConfigReadResult>("ConfigReadResult"),
        bundle_entry::<coco_types::PluginReloadResult>("PluginReloadResult"),
    ];

    let mut definitions = serde_json::Map::new();
    for (name, schema) in type_schemas {
        let schema_json: Value = serde_json::to_value(schema).expect("serialize entry");
        definitions.insert(name, schema_json);
    }

    let bundle = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "coco-app-server-protocol",
        "description": "JSON Schema bundle for the coco-rs SDK protocol types. \
            Source: coco-rs/common/types. Generated by `cargo run -p coco-types \
            --features schema --example export_schema`.",
        "definitions": definitions,
    });

    let bundle_path = out_dir.join("coco_app_server_protocol.schemas.json");
    let bundle_json = serde_json::to_string_pretty(&bundle).expect("serialize bundle");
    fs::write(&bundle_path, bundle_json).expect("write bundle");
    println!("  ✓ {}", bundle_path.display());

    println!();
    println!("Done.");
}
