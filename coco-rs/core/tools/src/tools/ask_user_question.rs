//! AskUserQuestionTool — structured multi-choice questions to the user.
//!
//! TS: `tools/AskUserQuestionTool/AskUserQuestionTool.tsx`,
//!     `tools/AskUserQuestionTool/prompt.ts`.
//!
//! The tool returns the questions payload as its result; the TUI/CLI layer
//! intercepts, presents the interactive overlay (see
//! `app/tui/src/render_overlays/question.rs`), collects answers, and fills
//! them in before the result goes back to the model.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::PromptOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// Typed input for [`AskUserQuestionTool`].
///
/// All four fields stay as opaque `Value` because:
///   - `questions` is a heterogeneous union (`multiSelect` toggles
///     option shape) that the model freely populates; the rich
///     constraints are encoded in [`AskUserQuestionTool::input_schema`].
///   - `answers` / `annotations` are TUI-spliced via
///     `PermissionOutcome::Allow.updated_input` *after* the model emits
///     the call — keeping them as `Value` matches that pipeline.
///   - `metadata` is an analytics passthrough, intentionally lax.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AskUserQuestionInput {
    /// Questions array — model-supplied. See `input_schema()` for the
    /// full constraint set (1-4 questions, each with 2-4 options,
    /// optional `multiSelect`, optional `preview` per option).
    #[serde(default)]
    pub questions: Value,
    /// (Internal) User answers, spliced into the input by the
    /// TUI/CLI host before `tool_call_preparer` re-validates.
    #[serde(default)]
    pub answers: Option<Value>,
    /// (Internal) Per-question annotations (preview / notes).
    #[serde(default)]
    pub annotations: Option<Value>,
    /// Optional analytics metadata.
    #[serde(default)]
    pub metadata: Option<Value>,
}

/// Short description shown in tool-catalog listings / `tools.ts`.
///
/// TS: `AskUserQuestionTool/prompt.ts:7` `DESCRIPTION`.
const ASK_USER_QUESTION_DESCRIPTION: &str = "Asks the user multiple choice questions to gather information, \
     clarify ambiguity, understand preferences, make decisions or offer \
     them choices.";

/// Full system-prompt contribution for the tool. TS:
/// `AskUserQuestionTool/prompt.ts:32-44` + markdown variant of
/// `PREVIEW_FEATURE_PROMPT` (HTML variant only matters to web UIs).
///
/// `ExitPlanMode` is interpolated via `ToolName::ExitPlanMode.as_str()`
/// — matches TS's `EXIT_PLAN_MODE_TOOL_NAME` constant interpolation and
/// follows the "no hardcoded strings for closed sets" rule.
static ASK_USER_QUESTION_PROMPT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let exit = coco_types::ToolName::ExitPlanMode.as_str();
    format!(
        "\
Use this tool when you need to ask the user questions during execution. This allows you to:
1. Gather user preferences or requirements
2. Clarify ambiguous instructions
3. Get decisions on implementation choices as you work
4. Offer choices to the user about what direction to take.

Usage notes:
- Users will always be able to select \"Other\" to provide custom text input
- Use multiSelect: true to allow multiple answers to be selected for a question
- If you recommend a specific option, make that the first option in the list and add \"(Recommended)\" at the end of the label

Plan mode note: In plan mode, use this tool to clarify requirements or choose between approaches BEFORE finalizing your plan. Do NOT use this tool to ask \"Is my plan ready?\" or \"Should I proceed?\" - use {exit} for plan approval. IMPORTANT: Do not reference \"the plan\" in your questions (e.g., \"Do you have feedback about the plan?\", \"Does the plan look good?\") because the user cannot see the plan in the UI until you call {exit}. If you need plan approval, use {exit} instead.

Preview feature:
Use the optional `preview` field on options when presenting concrete artifacts that users need to visually compare:
- ASCII mockups of UI layouts or components
- Code snippets showing different implementations
- Diagram variations
- Configuration examples

Preview content is rendered as markdown in a monospace box. Multi-line text with newlines is supported. When any option has a preview, the UI switches to a side-by-side layout with a vertical option list on the left and preview on the right. Do not use previews for simple preference questions where labels and descriptions suffice. Note: previews are only supported for single-select questions (not multiSelect).
"
    )
});

pub struct AskUserQuestionTool;

#[async_trait::async_trait]
impl Tool for AskUserQuestionTool {
    type Input = AskUserQuestionInput;
    fn runtime_validation_schema(&self) -> &coco_tool_runtime::ToolInputSchema {
        static SCHEMA: std::sync::OnceLock<coco_tool_runtime::ToolInputSchema> =
            std::sync::OnceLock::new();
        SCHEMA.get_or_init(|| {
            coco_tool_runtime::ToolInputSchema::from_value(serde_json::json!({
                "type": "object",
                "additionalProperties": false,
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
                                    "description": "The question text"
                                },
                                "header": {
                                    "type": "string",
                                    "description": "Short label displayed as a chip/tag (max 20 chars)"
                                },
                                "options": {
                                    "type": "array",
                                    "description": "Available choices (2-4 options)",
                                    "minItems": 2,
                                    "maxItems": 4,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {
                                                "type": "string",
                                                "description": "Display text for this option (1-5 words)"
                                            },
                                            "description": {
                                                "type": "string",
                                                "description": "Explanation of what this option means"
                                            },
                                            "preview": {
                                                "type": "string",
                                                "description": "Optional preview content when option is focused"
                                            }
                                        },
                                        "required": ["label", "description"]
                                    }
                                },
                                "multiSelect": {
                                    "type": "boolean",
                                    "description": "Allow multiple selections (default: false)"
                                }
                            },
                            "required": ["question", "header", "options"]
                        }
                    },
                    // `answers` and `annotations` are optional fields the TUI/CLI
                    // layer splices into the tool input via
                    // `PermissionOutcome::Allow.updated_input` BEFORE `tool_call_preparer`
                    // re-validates the rewritten input. Declaring them here keeps
                    // schema validation green; the model itself is not expected to
                    // populate these — the prompt teaches it to emit `questions`
                    // only. TS parity: `mapToolResultToToolResultBlockParam` reads
                    // `answers` and `annotations` from the result envelope.
                    "answers": {
                        "type": "object",
                        "description": "(Internal) User-supplied answers, spliced by the host before invocation. Map of question text → selected option label.",
                        "additionalProperties": { "type": "string" }
                    },
                    "annotations": {
                        "type": "object",
                        "description": "(Internal) Per-question annotations (preview / notes), spliced by the host before invocation.",
                        "additionalProperties": { "type": "object" }
                    },
                    // TS `commonFields.metadata` (AskUserQuestionTool.tsx:58-60).
                    // Optional analytics-tracking blob the model may emit alongside
                    // the question (e.g. `{source: "remember"}` for the /remember
                    // command). Echoed straight through to logs; never user-visible.
                    "metadata": {
                        "type": "object",
                        "description": "Optional metadata for tracking and analytics purposes. Not displayed to user.",
                        "properties": {
                            "source": {
                                "type": "string",
                                "description": "Optional identifier for the source of this question (e.g., \"remember\" for /remember command). Used for analytics tracking."
                            }
                        }
                    }
                },
                "required": []
            }))
            .expect("AskUserQuestion input schema must be a valid object schema")
        })
    }
    /// Output stays on `Value`: `render_for_model` walks the
    /// `answers` / `annotations` maps generically, and the
    /// envelope is downstream-consumed (TUI overlay) so a typed
    /// envelope would just force redundant round-trips.
    type Output = serde_json::Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::AskUserQuestion)
    }
    fn name(&self) -> &str {
        ToolName::AskUserQuestion.as_str()
    }
    fn description(&self, _input: &AskUserQuestionInput, _options: &DescriptionOptions) -> String {
        ASK_USER_QUESTION_DESCRIPTION.into()
    }
    async fn prompt(&self, _options: &PromptOptions) -> String {
        ASK_USER_QUESTION_PROMPT.clone()
    }
    fn requires_user_interaction(&self) -> bool {
        true
    }

    /// TS `AskUserQuestionTool.tsx`: `isConcurrencySafe() { return true }`.
    /// Multiple questions issued in the same turn are presented together by
    /// the TUI, so the executor can batch them concurrently rather than
    /// serializing.
    fn is_concurrency_safe(&self, _input: &AskUserQuestionInput) -> bool {
        true
    }

    /// Render the user's answers as TS-shaped prose. The TUI/CLI
    /// layer splices answers (and optional `annotations` for preview /
    /// notes) into the tool result before the model sees it; this fn
    /// reads that envelope and produces:
    ///
    ///   `User has answered your questions: "Q1"="A1" selected
    ///   preview:\n... user notes: ..., "Q2"="A2". You can now
    ///   continue with the user's answers in mind.`
    ///
    /// When the splicer hasn't run (test fixtures, dialog declined),
    /// fall back to the defensive JSON-or-string emit.
    /// TS: `AskUserQuestionTool.tsx:224-249 mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let answers = data
            .get("answers")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty());
        let Some(answers) = answers else {
            return coco_tool_runtime::render_text_or_json(data);
        };
        let annotations = data.get("annotations").and_then(Value::as_object);
        let mut entries: Vec<String> = Vec::with_capacity(answers.len());
        for (question, answer_v) in answers {
            let answer = answer_v.as_str().unwrap_or("");
            let mut parts = vec![format!("\"{question}\"=\"{answer}\"")];
            if let Some(annotation) = annotations.and_then(|a| a.get(question)) {
                if let Some(preview) = annotation.get("preview").and_then(Value::as_str) {
                    parts.push(format!("selected preview:\n{preview}"));
                }
                if let Some(notes) = annotation.get("notes").and_then(Value::as_str) {
                    parts.push(format!("user notes: {notes}"));
                }
            }
            entries.push(parts.join(" "));
        }
        let answers_text = entries.join(", ");
        let text = format!(
            "User has answered your questions: {answers_text}. You can now continue with the user's answers in mind."
        );
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: AskUserQuestionInput,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let questions = if input.questions.is_null() {
            Value::Array(vec![])
        } else {
            input.questions
        };

        // Propagate `answers` and `annotations` through to the result
        // when the host (TUI/CLI/test harness) has already spliced
        // them into the tool input via `PermissionOutcome::Allow.updated_input`.
        // `render_for_model` reads them off `data` and produces the
        // user-answered prose; without this propagation the splice
        // would be invisible to the renderer.
        let mut data = serde_json::Map::new();
        data.insert("questions".into(), questions);
        if let Some(answers) = input.answers {
            data.insert("answers".into(), answers);
        }
        if let Some(annotations) = input.annotations {
            data.insert("annotations".into(), annotations);
        }
        Ok(ToolResult {
            data: Value::Object(data),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

#[cfg(test)]
#[path = "ask_user_question.test.rs"]
mod tests;
