use serde_json::Value;
use serde_json::json;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::LanguageModelV4ToolChoice;
use vercel_ai_provider::Warning;

/// Result of preparing tools for the Responses API.
pub struct PreparedResponsesTools {
    pub tools: Option<Vec<Value>>,
    pub tool_choice: Option<Value>,
    pub warnings: Vec<Warning>,
}

/// Convert SDK tools + tool_choice to OpenAI Responses API format.
///
/// Unlike Chat, the Responses API supports provider-specific tools
/// (web_search, file_search, code_interpreter, shell, etc.).
pub fn prepare_responses_tools(
    tools: &Option<Vec<LanguageModelV4Tool>>,
    tool_choice: &Option<LanguageModelV4ToolChoice>,
) -> PreparedResponsesTools {
    let warnings = Vec::new();

    let tools_value = tools.as_ref().and_then(|ts| {
        if ts.is_empty() {
            return None;
        }

        let mut openai_tools = Vec::new();
        for tool in ts {
            match tool {
                LanguageModelV4Tool::Function(ft) => {
                    let mut params = ft.input_schema.clone();
                    if !params.is_object() {
                        params = json!({ "type": "object", "properties": {} });
                    }
                    let mut tool_obj = json!({
                        "type": "function",
                        "name": ft.name,
                        "parameters": params,
                    });
                    if let Some(ref desc) = ft.description {
                        tool_obj["description"] = Value::String(desc.clone());
                    }
                    if let Some(strict) = ft.strict {
                        tool_obj["strict"] = Value::Bool(strict);
                    }
                    openai_tools.push(tool_obj);
                }
                LanguageModelV4Tool::Provider(pt) => {
                    // Provider tools use their args as the tool definition
                    let mut tool_obj = json!({ "type": pt.name });
                    // Merge args into the tool object
                    for (k, v) in &pt.args {
                        tool_obj[k] = v.clone();
                    }
                    openai_tools.push(tool_obj);
                }
            }
        }

        if openai_tools.is_empty() {
            None
        } else {
            Some(openai_tools)
        }
    });

    let tool_choice_value = tool_choice.as_ref().map(|tc| match tc {
        LanguageModelV4ToolChoice::Auto => json!("auto"),
        LanguageModelV4ToolChoice::None => json!("none"),
        LanguageModelV4ToolChoice::Required => json!("required"),
        LanguageModelV4ToolChoice::Tool { tool_name } => json!({
            "type": "function",
            "name": tool_name
        }),
    });

    PreparedResponsesTools {
        tools: tools_value,
        tool_choice: tool_choice_value,
        warnings,
    }
}

#[cfg(test)]
#[path = "prepare_tools.test.rs"]
mod tests;
