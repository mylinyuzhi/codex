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

    let definitions = serde_json::json!({
        "ServerNotification": schema_for!(cocode_app_server_protocol::ServerNotification),
        "ClientRequest": schema_for!(cocode_app_server_protocol::ClientRequest),
        "ServerRequest": schema_for!(cocode_app_server_protocol::ServerRequest),
        "ThreadItem": schema_for!(cocode_app_server_protocol::ThreadItem),
        "Usage": schema_for!(cocode_app_server_protocol::Usage),
        "SessionStartRequestParams": schema_for!(cocode_app_server_protocol::SessionStartRequestParams),
        "ItemStatus": schema_for!(cocode_app_server_protocol::ItemStatus),
        "ApprovalDecision": schema_for!(cocode_app_server_protocol::ApprovalDecision),
        "McpServerConfig": schema_for!(cocode_app_server_protocol::McpServerConfig),
        "AgentDefinitionConfig": schema_for!(cocode_app_server_protocol::AgentDefinitionConfig),
        "SandboxConfig": schema_for!(cocode_app_server_protocol::SandboxConfig),
        "ThinkingConfig": schema_for!(cocode_app_server_protocol::ThinkingConfig),
    });
    bundle.insert("definitions".into(), definitions);

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
