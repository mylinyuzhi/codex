//! Shared test fixtures for live integration tests.

use cocode_inference::LanguageModelFunctionTool;
use cocode_inference::LanguageModelTool;
use serde_json::json;

/// Create a weather tool definition for tool-calling tests.
pub fn weather_tool() -> LanguageModelTool {
    LanguageModelTool::function(LanguageModelFunctionTool::with_description(
        "get_weather",
        "Get the current weather for a city",
        json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "The city name"
                }
            },
            "required": ["city"]
        }),
    ))
}
