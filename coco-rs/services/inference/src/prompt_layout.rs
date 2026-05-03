//! Semantic prompt envelope and provider layout payload types.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use vercel_ai_provider::JSONValue;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::ProviderOptions;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::Warning;

pub const PROMPT_LAYOUT_NAMESPACE: &str = "prompt_layout";

pub type PromptPart = UserContentPart;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptEnvelope {
    pub sections: Vec<PromptSection>,
    pub history: Vec<LanguageModelV4Message>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSection {
    pub kind: PromptSectionKind,
    pub content: Vec<PromptPart>,
    pub cache: CacheHint,
    pub source: PromptSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionKind {
    Identity,
    ModelBaseInstructions,
    DeveloperPolicy,
    ToolPolicy,
    ProjectInstructions,
    Environment,
    Memory,
    LoadedContext,
    SkillListing,
    McpInstructions,
    IdeContext,
    HookContext,
    ActiveTopic,
    UserContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSource {
    BuiltIn,
    Config,
    Project,
    Memory,
    Runtime,
    Tool,
    User,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheHint {
    #[default]
    Ephemeral,
    Stable,
    Breakpoint,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptLayoutOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_blocks: Option<Vec<AnthropicSystemBlock>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_warnings: Vec<Warning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_hash_inputs: Option<PromptHashInputs>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicSystemBlock {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicCacheControl {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptHashInputs {
    pub system_text_hash: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control_hash: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_text_hash: Option<u64>,
    pub contextual_user_text_hash: u64,
    pub contextual_user_char_count: i64,
    pub tools_hash: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub per_tool_hashes: Vec<(String, u64)>,
}

pub fn put_layout_options(opts: &mut ProviderOptions, layout: &PromptLayoutOptions) {
    let value = serde_json::to_value(layout).unwrap_or(JSONValue::Null);
    let Some(object) = value.as_object() else {
        return;
    };

    let mut namespace = HashMap::new();
    for (key, value) in object {
        if !value.is_null() {
            namespace.insert(key.clone(), value.clone());
        }
    }
    opts.set(PROMPT_LAYOUT_NAMESPACE, namespace);
}

pub fn take_layout_options(opts: &ProviderOptions) -> Option<PromptLayoutOptions> {
    let namespace = opts.get(PROMPT_LAYOUT_NAMESPACE)?;
    let mut object = serde_json::Map::new();
    for (key, value) in namespace {
        object.insert(key.clone(), value.clone());
    }
    serde_json::from_value(JSONValue::Object(object)).ok()
}

#[cfg(test)]
#[path = "prompt_layout.test.rs"]
mod tests;
