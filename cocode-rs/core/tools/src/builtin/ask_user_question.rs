//! AskUserQuestion tool for interactive user queries.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Label for the "Skip interview" option injected in plan mode.
const SKIP_INTERVIEW_LABEL: &str = "Skip interview and plan immediately";

/// Label for the "Chat about this" option injected on all questions.
///
/// When selected, the tool returns a clarification prompt instead of answers,
/// matching Claude Code's behavior of letting users request reformulated questions.
const CHAT_ABOUT_THIS_LABEL: &str = "Chat about this";

/// Tool for asking the user questions during execution.
///
/// Supports multiple questions with selectable options,
/// including multi-select and custom "Other" input.
pub struct AskUserQuestionTool;

impl AskUserQuestionTool {
    /// Create a new AskUserQuestion tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AskUserQuestionTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::AskUserQuestion.as_str()
    }

    fn description(&self) -> &str {
        prompts::ASK_USER_QUESTION_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "Questions to ask the user (1-4 questions)",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The complete question to ask the user. Should be clear, specific, and end with a question mark."
                            },
                            "header": {
                                "type": "string",
                                "description": "Very short label displayed as a chip/tag (max 12 chars). Examples: 'Auth method', 'Library', 'Approach'."
                            },
                            "options": {
                                "type": "array",
                                "description": "The available choices for this question (2-4 options).",
                                "minItems": 2,
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {
                                            "type": "string",
                                            "description": "The display text for this option (1-5 words)."
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "Explanation of what this option means or what will happen if chosen."
                                        },
                                        "markdown": {
                                            "type": "string",
                                            "description": "Optional preview content shown in a monospace box when this option is focused. Use for ASCII mockups, code snippets, or diagrams."
                                        }
                                    },
                                    "required": ["label", "description"]
                                }
                            },
                            "multiSelect": {
                                "type": "boolean",
                                "description": "Set to true to allow multiple options to be selected. Default: false.",
                                "default": false
                            }
                        },
                        "required": ["question", "header", "options", "multiSelect"]
                    }
                },
                "answers": {
                    "type": "object",
                    "description": "User answers collected by the UI (filled on callback)."
                },
                "metadata": {
                    "type": "object",
                    "description": "Optional metadata for tracking purposes.",
                    "properties": {
                        "source": {
                            "type": "string",
                            "description": "Optional identifier for the source of this question."
                        }
                    }
                },
                "annotations": {
                    "type": "object",
                    "description": "Optional per-question annotations from the user (e.g., notes on preview selections). Keyed by question text."
                }
            },
            "required": ["questions"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn should_defer(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let questions = input["questions"].as_array().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "questions must be an array",
            }
            .build()
        })?;

        if questions.is_empty() || questions.len() > 4 {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "questions must contain 1-4 items",
            }
            .build());
        }

        // Validate each question
        for (i, q) in questions.iter().enumerate() {
            if q["question"].as_str().is_none() {
                return Err(crate::error::tool_error::InvalidInputSnafu {
                    message: format!("questions[{i}] missing required field 'question'"),
                }
                .build());
            }
            let header = q["header"].as_str().ok_or_else(|| {
                crate::error::tool_error::InvalidInputSnafu {
                    message: format!("questions[{i}] missing required field 'header'"),
                }
                .build()
            })?;
            if header.len() > 12 {
                return Err(crate::error::tool_error::InvalidInputSnafu {
                    message: format!(
                        "questions[{i}] header must be at most 12 characters, got {}",
                        header.len()
                    ),
                }
                .build());
            }
            let options = q["options"].as_array().ok_or_else(|| {
                crate::error::tool_error::InvalidInputSnafu {
                    message: format!("questions[{i}] missing 'options' array"),
                }
                .build()
            })?;
            if options.len() < 2 || options.len() > 4 {
                return Err(crate::error::tool_error::InvalidInputSnafu {
                    message: format!("questions[{i}] must have 2-4 options"),
                }
                .build());
            }
        }

        // If answers are provided, this is a callback with user responses
        if let Some(answers) = input.get("answers").and_then(|a| a.as_object()) {
            let parts: Vec<String> = answers
                .iter()
                .map(|(key, value)| {
                    let s = value.to_string();
                    let v = value.as_str().unwrap_or(&s);
                    format!("\"{key}\"=\"{v}\"")
                })
                .collect();
            let output = format!(
                "User has answered your questions: {}. You can now continue with the user's answers in mind.",
                parts.join(", ")
            );
            return Ok(ToolOutput::text(output));
        }

        ctx.emit_progress("Asking user a question").await;

        // Use question responder for interactive TUI flow
        if let Some(responder) = &ctx.question_responder {
            let request_id = ctx.call_id.clone();
            let rx = responder.register(request_id.clone());

            // Inject extra options to each question:
            // - "Chat about this" (always) — lets the user request clarification
            // - "Skip interview" (plan mode only) — skips directly to planning
            let mut questions = questions.clone();
            for q in &mut questions {
                if let Some(opts) = q.get_mut("options").and_then(|o| o.as_array_mut()) {
                    opts.push(serde_json::json!({
                        "label": CHAT_ABOUT_THIS_LABEL,
                        "description": "Request clarification or provide additional context before answering."
                    }));
                    if ctx.is_plan_mode {
                        opts.push(serde_json::json!({
                            "label": SKIP_INTERVIEW_LABEL,
                            "description": "Skip these questions and proceed directly to planning."
                        }));
                    }
                }
            }

            // Emit event for TUI to show question overlay
            ctx.emit_event(cocode_protocol::CoreEvent::Tui(
                cocode_protocol::TuiEvent::QuestionAsked {
                    request_id,
                    questions: Value::Array(questions.clone()),
                },
            ))
            .await;

            // Wait for user response (blocks until TUI sends answers)
            match rx.await {
                Ok(answers) => {
                    if let Some(obj) = answers.as_object() {
                        // Check if user chose to skip the interview.
                        // In Claude Code, this rejects the tool call and injects a user message.
                        // We return a non-error tool result with the directive, matching the
                        // semantic of "user responded, but not with answers."
                        let skipped = obj
                            .values()
                            .any(|v| v.as_str() == Some(SKIP_INTERVIEW_LABEL));
                        if skipped {
                            return Ok(ToolOutput::text(
                                "User has indicated they have provided enough answers. \
                                 Stop asking clarifying questions. Proceed with the plan \
                                 using the information you already have.",
                            ));
                        }

                        // Check if user chose "Chat about this".
                        // In Claude Code, this rejects the tool call and injects a clarification
                        // message. We return a non-error tool result with the clarification prompt.
                        let chat_requested = obj
                            .values()
                            .any(|v| v.as_str() == Some(CHAT_ABOUT_THIS_LABEL));
                        if chat_requested {
                            return Ok(ToolOutput::text(format_chat_about_this_message(
                                &questions, obj,
                            )));
                        }
                    }

                    // Extract images if present (from "Other" text with pasted images)
                    let images: Vec<cocode_protocol::ImageData> = answers
                        .as_object()
                        .and_then(|obj| obj.get("_images"))
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|img| {
                                    let data = img.get("data")?.as_str()?.to_string();
                                    let media_type = img.get("media_type")?.as_str()?.to_string();
                                    Some(cocode_protocol::ImageData { data, media_type })
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    // Format answers for the model (excluding _images metadata)
                    let mut parts = Vec::new();
                    if let Some(obj) = answers.as_object() {
                        for (key, value) in obj {
                            if key == "_images" {
                                continue;
                            }
                            let s = value.to_string();
                            let v = value.as_str().unwrap_or(&s);
                            parts.push(format!("\"{key}\"=\"{v}\""));
                        }
                    }
                    let output = format!(
                        "User has answered your questions: {}. You can now continue with the user's answers in mind.",
                        parts.join(", ")
                    );
                    return Ok(ToolOutput {
                        content: cocode_protocol::ToolResultContent::Text(output),
                        is_error: false,
                        modifiers: Vec::new(),
                        images,
                    });
                }
                Err(_) => {
                    return Ok(ToolOutput::text(
                        "User did not respond (question was cancelled).",
                    ));
                }
            }
        }

        // Fallback: format questions as plain text (non-TUI mode)
        let mut output = String::new();
        for q in questions {
            let question = q["question"].as_str().unwrap_or("?");
            let header = q["header"].as_str().unwrap_or("");
            output.push_str(&format!("[{header}] {question}\n"));
            if let Some(options) = q["options"].as_array() {
                for (i, opt) in options.iter().enumerate() {
                    let label = opt["label"].as_str().unwrap_or("?");
                    let desc = opt["description"].as_str().unwrap_or("");
                    output.push_str(&format!("  {}. {label} — {desc}\n", i + 1));
                }
            }
            output.push('\n');
        }

        Ok(ToolOutput::text(output))
    }
}

/// Build the clarification message when the user selects "Chat about this".
///
/// Formats the questions asked and any partial answers provided, matching
/// Claude Code's behavior of injecting a clarification prompt.
fn format_chat_about_this_message(
    questions: &[Value],
    answers: &serde_json::Map<String, Value>,
) -> String {
    let mut msg = String::from(
        "The user wants to clarify these questions.\n\
         This means they may have additional information, context or questions for you.\n\
         Take their response into account and then reformulate the questions if appropriate.\n\
         Start by asking them what they would like to clarify.\n\n\
         Questions asked:",
    );

    for q in questions {
        let question_text = q["question"].as_str().unwrap_or("?");
        msg.push_str(&format!("\n- \"{question_text}\""));

        // Include partial answer if the user provided one for this question
        let header = q["header"].as_str().unwrap_or("");
        if let Some(answer) = answers.get(header).or_else(|| answers.get(question_text)) {
            let s = answer.to_string();
            let v = answer.as_str().unwrap_or(&s);
            if v != CHAT_ABOUT_THIS_LABEL {
                msg.push_str(&format!("\n  Answer: {v}"));
            } else {
                msg.push_str("\n  Answer: (No answer provided)");
            }
        } else {
            msg.push_str("\n  Answer: (No answer provided)");
        }
    }

    msg
}

#[cfg(test)]
#[path = "ask_user_question.test.rs"]
mod tests;
