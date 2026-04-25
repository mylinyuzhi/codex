//! AskUserQuestionTool — structured multi-choice questions to the user.
//!
//! TS: `tools/AskUserQuestionTool/AskUserQuestionTool.tsx`,
//!     `tools/AskUserQuestionTool/prompt.ts`.
//!
//! The tool returns the questions payload as its result; the TUI/CLI layer
//! intercepts, presents the interactive overlay (see
//! `app/tui/src/render_overlays/question.rs`), collects answers, and fills
//! them in before the result goes back to the model.

use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::PromptOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

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
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::AskUserQuestion)
    }
    fn name(&self) -> &str {
        ToolName::AskUserQuestion.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        ASK_USER_QUESTION_DESCRIPTION.into()
    }
    async fn prompt(&self, _options: &PromptOptions) -> String {
        ASK_USER_QUESTION_PROMPT.clone()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "questions".into(),
            serde_json::json!({
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
            }),
        );
        ToolInputSchema { properties: p }
    }

    fn requires_user_interaction(&self) -> bool {
        true
    }

    /// TS `AskUserQuestionTool.tsx`: `isConcurrencySafe() { return true }`.
    /// Multiple questions issued in the same turn are presented together by
    /// the TUI, so the executor can batch them concurrently rather than
    /// serializing.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let questions = input
            .get("questions")
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        // Return the questions as the result. The TUI/CLI layer intercepts
        // this tool's output, presents the UI, and fills in answers.
        Ok(ToolResult {
            data: serde_json::json!({"questions": questions}),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

#[cfg(test)]
#[path = "ask_user_question.test.rs"]
mod tests;
