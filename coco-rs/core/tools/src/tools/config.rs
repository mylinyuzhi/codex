//! ConfigTool — read/list/set/reset known config keys.
//!
//! TS: `tools/ConfigTool/ConfigTool.ts`. Mutating actions currently
//! emit instructional messages instead of writing config — the
//! authoritative path is the CLI `config` subcommand or direct edits
//! to `~/.coco/config.json`.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;
use std::collections::HashMap;

pub struct ConfigTool;

/// Known configuration keys for documentation.
const KNOWN_CONFIG_KEYS: &[&str] = &[
    "model",
    "provider",
    "thinking_level",
    "max_budget_usd",
    "permission_mode",
    "sandbox_mode",
    "custom_system_prompt",
    "append_system_prompt",
    "verbose",
    "debug",
];

#[async_trait::async_trait]
impl Tool for ConfigTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Config)
    }
    fn name(&self) -> &str {
        ToolName::Config.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Manage configuration settings. Supports get, set, list, and reset actions.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "action".into(),
            serde_json::json!({"type": "string", "enum": ["get", "set", "list", "reset"], "description": "Configuration action to perform"}),
        );
        p.insert(
            "key".into(),
            serde_json::json!({"type": "string", "description": "Configuration key (for get/set/reset)"}),
        );
        p.insert(
            "value".into(),
            serde_json::json!({"description": "Configuration value (for set)"}),
        );
        ToolInputSchema {
            properties: p,
            required: Vec::new(),
        }
    }

    /// TS `ConfigTool.ts`: `isConcurrencySafe() { return true }`. Read paths
    /// (get/list) are obviously safe; mutating paths (set/reset) currently
    /// just emit an instructional message rather than writing config, so
    /// they're safe too. Should the tool ever start mutating a shared
    /// settings file, demote to input-conditional safety like BashTool.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("get set list or reset config settings")
    }

    /// Render the prebuilt `message` field, optionally followed by the
    /// list of available keys (for the `list` action). Skips JSON
    /// envelope overhead — the model only needs the human prose.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let message = data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut text = message.to_string();
        if let Some(keys) = data.get("keys").and_then(Value::as_array)
            && !keys.is_empty()
        {
            let names: Vec<&str> = keys.iter().filter_map(Value::as_str).collect();
            text.push_str(":\n");
            text.push_str(&names.join("\n"));
        }
        if text.is_empty() {
            text = serde_json::to_string(data).unwrap_or_default();
        }
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");

        let result = match action {
            "list" => {
                serde_json::json!({
                    "message": "Available configuration keys",
                    "keys": KNOWN_CONFIG_KEYS,
                })
            }
            "get" => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'get' action".into(),
                        error_code: None,
                    });
                }
                serde_json::json!({
                    "message": format!("Configuration value for '{key}' is managed by ConfigManager. Use the CLI 'config' subcommand to view or edit settings."),
                    "key": key,
                })
            }
            "set" => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'set' action".into(),
                        error_code: None,
                    });
                }
                let value = input.get("value").cloned().unwrap_or(Value::Null);
                serde_json::json!({
                    "message": format!("To set '{key}', use the CLI 'config set {key} <value>' command or edit the config file directly."),
                    "key": key,
                    "value": value,
                })
            }
            "reset" => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'reset' action".into(),
                        error_code: None,
                    });
                }
                serde_json::json!({
                    "message": format!("To reset '{key}' to default, use the CLI 'config reset {key}' command."),
                    "key": key,
                })
            }
            other => {
                return Err(ToolError::InvalidInput {
                    message: format!("Unknown action '{other}'. Must be get, set, list, or reset"),
                    error_code: None,
                });
            }
        };

        Ok(ToolResult {
            data: result,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}
