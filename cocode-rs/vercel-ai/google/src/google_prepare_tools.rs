//! Prepare tools for Google Generative AI API calls.

use serde_json::Value;
use serde_json::json;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::LanguageModelV4ToolChoice;

use crate::convert_json_schema_to_openapi_schema::convert_json_schema_to_openapi_schema;
use crate::tool::code_execution::CODE_EXECUTION_TOOL_ID;
use crate::tool::enterprise_web_search::ENTERPRISE_WEB_SEARCH_TOOL_ID;
use crate::tool::file_search::FILE_SEARCH_TOOL_ID;
use crate::tool::google_maps::GOOGLE_MAPS_TOOL_ID;
use crate::tool::google_search::GOOGLE_SEARCH_TOOL_ID;
use crate::tool::url_context::URL_CONTEXT_TOOL_ID;
use crate::tool::vertex_rag_store::VERTEX_RAG_STORE_TOOL_ID;

/// Result of preparing tools for the Google API.
#[derive(Debug, Clone, Default)]
pub struct PreparedTools {
    /// Function declarations for Google API.
    pub function_declarations: Option<Value>,
    /// Tool config (mode and allowed function names).
    pub tool_config: Option<Value>,
    /// Additional tool entries (google_search, code_execution, etc.).
    pub tool_entries: Vec<Value>,
}

/// Prepare tools for a Google API request.
///
/// Converts Vercel AI SDK tool definitions and tool choice into
/// the Google API format.
pub fn prepare_tools(
    tools: &Option<Vec<LanguageModelV4Tool>>,
    tool_choice: &Option<LanguageModelV4ToolChoice>,
    _model_id: &str,
) -> PreparedTools {
    let tools = match tools {
        Some(t) if !t.is_empty() => t,
        _ => return PreparedTools::default(),
    };

    let mut function_declarations: Vec<Value> = Vec::new();
    let mut tool_entries: Vec<Value> = Vec::new();

    for tool in tools {
        match tool {
            LanguageModelV4Tool::Function(func_tool) => {
                let mut decl = json!({
                    "name": func_tool.name,
                });

                if let Some(ref desc) = func_tool.description {
                    decl["description"] = json!(desc);
                }

                // Convert JSON Schema to OpenAPI schema
                let schema_value = serde_json::to_value(&func_tool.input_schema)
                    .unwrap_or(Value::Object(Default::default()));
                if let Some(openapi_schema) = convert_json_schema_to_openapi_schema(&schema_value) {
                    decl["parameters"] = openapi_schema;
                }

                function_declarations.push(decl);
            }
            LanguageModelV4Tool::Provider(provider_tool) => {
                match provider_tool.id.as_str() {
                    GOOGLE_SEARCH_TOOL_ID => {
                        let mut search_config = json!({});
                        // Apply retrieval config from args if present
                        if let Some(dynamic_retrieval_config) =
                            provider_tool.args.get("dynamicRetrievalConfig")
                        {
                            search_config["dynamicRetrievalConfig"] =
                                dynamic_retrieval_config.clone();
                        }
                        tool_entries.push(json!({ "googleSearch": search_config }));
                    }
                    URL_CONTEXT_TOOL_ID => {
                        tool_entries.push(json!({ "urlContext": {} }));
                    }
                    CODE_EXECUTION_TOOL_ID => {
                        tool_entries.push(json!({ "codeExecution": {} }));
                    }
                    ENTERPRISE_WEB_SEARCH_TOOL_ID => {
                        let mut config = json!({});
                        if let Some(search_engine_id) = provider_tool.args.get("searchEngineId") {
                            config["searchEngineId"] = search_engine_id.clone();
                        }
                        tool_entries.push(json!({ "enterpriseWebSearch": config }));
                    }
                    FILE_SEARCH_TOOL_ID => {
                        let mut config = json!({});
                        if let Some(data_store_specs) = provider_tool.args.get("dataStoreSpecs") {
                            config["dataStoreSpecs"] = data_store_specs.clone();
                        }
                        tool_entries.push(json!({ "fileSearch": config }));
                    }
                    GOOGLE_MAPS_TOOL_ID => {
                        tool_entries.push(json!({ "googleMaps": {} }));
                    }
                    VERTEX_RAG_STORE_TOOL_ID => {
                        let mut config = json!({});
                        for (key, value) in &provider_tool.args {
                            config[key] = value.clone();
                        }
                        tool_entries.push(json!({ "retrieval": { "vertexRagStore": config } }));
                    }
                    _ => {
                        // Unknown provider tool, skip
                    }
                }
            }
        }
    }

    // Build tool config from tool_choice
    let tool_config = tool_choice.as_ref().map(|choice| match choice {
        LanguageModelV4ToolChoice::Auto => {
            json!({ "functionCallingConfig": { "mode": "AUTO" } })
        }
        LanguageModelV4ToolChoice::None => {
            json!({ "functionCallingConfig": { "mode": "NONE" } })
        }
        LanguageModelV4ToolChoice::Required => {
            json!({ "functionCallingConfig": { "mode": "ANY" } })
        }
        LanguageModelV4ToolChoice::Tool { tool_name } => {
            json!({
                "functionCallingConfig": {
                    "mode": "ANY",
                    "allowedFunctionNames": [tool_name]
                }
            })
        }
    });

    let function_declarations = if function_declarations.is_empty() {
        None
    } else {
        Some(json!(function_declarations))
    };

    PreparedTools {
        function_declarations,
        tool_config,
        tool_entries,
    }
}

#[cfg(test)]
#[path = "google_prepare_tools.test.rs"]
mod tests;
