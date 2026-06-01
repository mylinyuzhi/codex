//! ConfigTool — read/list/set/reset known config keys.
//!
//! TS: `tools/ConfigTool/ConfigTool.ts`. Mutating actions currently
//! emit instructional messages instead of writing config — the
//! authoritative path is the CLI `config` subcommand or direct edits
//! to `~/.coco/config.json`.

use crate::input_types::ConfigAction;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

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

/// Typed input for [`ConfigTool`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ConfigInput {
    /// Configuration action to perform.
    #[serde(default)]
    pub action: ConfigAction,
    /// Configuration key (required for `get`/`set`/`reset`).
    #[serde(default)]
    pub key: Option<String>,
    /// Configuration value (for `set`). Free-form JSON — the
    /// authoritative writer is the CLI, so we don't type-narrow.
    #[serde(default)]
    pub value: Option<Value>,
}

/// Typed output. Mirrors the legacy flat JSON shape so transcript
/// replay across the migration boundary round-trips without surprises;
/// optional fields are only populated on the relevant action branches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOutput {
    /// Human-readable status / next-step instruction.
    pub message: String,
    /// Populated only for `list` — the documented key surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
    /// Populated for `get`/`set`/`reset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Populated for `set` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

pub struct ConfigTool;

#[async_trait::async_trait]
impl Tool for ConfigTool {
    type Input = ConfigInput;
    coco_tool_runtime::impl_runtime_schema!(ConfigInput);
    type Output = ConfigOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Config)
    }
    fn name(&self) -> &str {
        ToolName::Config.as_str()
    }
    fn description(&self, _input: &ConfigInput, _options: &DescriptionOptions) -> String {
        "Manage configuration settings. Supports get, set, list, and reset actions.".into()
    }

    /// TS `ConfigTool.ts`: `isConcurrencySafe() { return true }`. Read paths
    /// (get/list) are obviously safe; mutating paths (set/reset) currently
    /// just emit an instructional message rather than writing config, so
    /// they're safe too. Should the tool ever start mutating a shared
    /// settings file, demote to input-conditional safety like BashTool.
    fn is_concurrency_safe(&self, _input: &ConfigInput) -> bool {
        true
    }
    fn is_read_only(&self, input: &ConfigInput) -> bool {
        // `set`/`reset` are documented as future mutations even though the
        // current impl just emits prose. Be conservative.
        matches!(input.action, ConfigAction::Get | ConfigAction::List)
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
    fn render_for_model(&self, out: &ConfigOutput) -> Vec<ToolResultContentPart> {
        let mut text = out.message.clone();
        if let Some(keys) = &out.keys
            && !keys.is_empty()
        {
            text.push_str(":\n");
            text.push_str(&keys.join("\n"));
        }
        if text.is_empty() {
            text = serde_json::to_string(out).unwrap_or_default();
        }
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: ConfigInput,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<ConfigOutput>, ToolError> {
        let key = input.key.clone().unwrap_or_default();

        let data = match input.action {
            ConfigAction::List => ConfigOutput {
                message: "Available configuration keys".into(),
                keys: Some(KNOWN_CONFIG_KEYS.iter().map(|s| (*s).to_string()).collect()),
                key: None,
                value: None,
            },
            ConfigAction::Get => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'get' action".into(),
                        error_code: None,
                    });
                }
                ConfigOutput {
                    message: format!(
                        "Configuration value for '{key}' is managed by ConfigManager. Use the CLI 'config' subcommand to view or edit settings."
                    ),
                    keys: None,
                    key: Some(key),
                    value: None,
                }
            }
            ConfigAction::Set => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'set' action".into(),
                        error_code: None,
                    });
                }
                ConfigOutput {
                    message: format!(
                        "To set '{key}', use the CLI 'config set {key} <value>' command or edit the config file directly."
                    ),
                    keys: None,
                    key: Some(key),
                    value: Some(input.value.unwrap_or(Value::Null)),
                }
            }
            ConfigAction::Reset => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'reset' action".into(),
                        error_code: None,
                    });
                }
                ConfigOutput {
                    message: format!(
                        "To reset '{key}' to default, use the CLI 'config reset {key}' command."
                    ),
                    keys: None,
                    key: Some(key),
                    value: None,
                }
            }
        };

        Ok(ToolResult {
            data,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}
