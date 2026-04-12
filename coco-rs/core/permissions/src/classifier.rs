//! Yolo classifier — two-stage LLM-based permission classification.
//!
//! TS: utils/permissions/yoloClassifier.ts (3.2K LOC)
//!
//! Stage 1: Fast classification with small token budget (64-256 tokens).
//! Stage 2: Extended thinking with larger budget (4096+ tokens) if Stage 1 uncertain.
//!
//! The classifier maintains a compressed transcript of the conversation
//! and asks the LLM whether a proposed tool action is safe to auto-execute.

use coco_types::ToolName;
use serde::Deserialize;
use serde::Serialize;

/// Re-export from coco-types.
pub use coco_types::ClassifierUsage;

/// Result of yolo classifier.
#[derive(Debug, Clone)]
pub struct YoloClassifierResult {
    /// Whether the action should be blocked.
    pub should_block: bool,
    /// Human-readable reason for the decision.
    pub reason: String,
    /// Model used for classification.
    pub model: String,
    /// Token usage for the classifier call.
    pub usage: Option<ClassifierUsage>,
    /// Duration of the classifier call in milliseconds.
    pub duration_ms: Option<i64>,
    /// Which stage produced this result (1 = fast, 2 = extended thinking).
    pub stage: Option<i32>,
}

/// Auto-mode configuration rules.
///
/// TS: AutoModeRules — user-configurable allow/deny/environment rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoModeRules {
    /// Rules for what to automatically allow.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Rules for what to soft-deny (prompt user).
    #[serde(default)]
    pub soft_deny: Vec<String>,
    /// Environment context for the classifier.
    #[serde(default)]
    pub environment: Vec<String>,
}

/// A compressed transcript entry for the classifier.
#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub role: TranscriptRole,
    pub content: Vec<TranscriptBlock>,
}

/// Role in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Assistant,
}

/// A content block in the transcript.
#[derive(Debug, Clone)]
pub enum TranscriptBlock {
    /// User text message.
    Text(String),
    /// Tool call (name + abbreviated input).
    ToolCall {
        tool_name: String,
        input_summary: String,
    },
    /// Tool result (abbreviated).
    ToolResult {
        tool_name: String,
        output_summary: String,
        is_error: bool,
    },
}

/// Safe-tool allowlist — tools that never need classifier review.
///
/// TS: SAFE_TOOLS constant in yoloClassifier.ts.
/// TS: `SAFE_YOLO_ALLOWLISTED_TOOLS` in classifierShared.ts
const SAFE_TOOLS: &[&str] = &[
    // Read-only file operations
    ToolName::Read.as_str(),
    // Search / read-only
    ToolName::Grep.as_str(),
    ToolName::Glob.as_str(),
    ToolName::Lsp.as_str(),
    ToolName::ToolSearch.as_str(),
    ToolName::ListMcpResources.as_str(),
    ToolName::ReadMcpResource.as_str(),
    // Task management (metadata only)
    ToolName::TodoWrite.as_str(),
    ToolName::TaskCreate.as_str(),
    ToolName::TaskGet.as_str(),
    ToolName::TaskUpdate.as_str(),
    ToolName::TaskList.as_str(),
    ToolName::TaskStop.as_str(),
    ToolName::TaskOutput.as_str(),
    // Plan mode / UI
    ToolName::AskUserQuestion.as_str(),
    ToolName::EnterPlanMode.as_str(),
    ToolName::ExitPlanMode.as_str(),
    // Swarm coordination (internal mailbox/team state only)
    ToolName::TeamCreate.as_str(),
    ToolName::TeamDelete.as_str(),
    ToolName::SendMessage.as_str(),
    // Misc safe
    ToolName::Sleep.as_str(),
    ToolName::Brief.as_str(),
];

/// Check if a tool is in the safe-tool allowlist (no classifier needed).
pub fn is_safe_tool(tool_name: &str) -> bool {
    SAFE_TOOLS.contains(&tool_name)
}

/// Build transcript entries from conversation messages for the classifier.
///
/// Compresses the conversation into a compact format:
/// - User text → preserved (truncated to 2000 chars)
/// - Tool calls → tool name + abbreviated input
/// - Tool results → tool name + abbreviated output
/// - System messages → stripped
pub fn build_transcript_entries(messages: &[coco_types::Message]) -> Vec<TranscriptEntry> {
    let mut entries = Vec::new();

    for msg in messages {
        match msg {
            coco_types::Message::User(u) => {
                let text = extract_user_text(&u.message);
                if !text.is_empty() && !u.is_meta {
                    entries.push(TranscriptEntry {
                        role: TranscriptRole::User,
                        content: vec![TranscriptBlock::Text(truncate(&text, 2000))],
                    });
                }
            }
            coco_types::Message::Assistant(a) => {
                let blocks = extract_assistant_blocks(&a.message);
                if !blocks.is_empty() {
                    entries.push(TranscriptEntry {
                        role: TranscriptRole::Assistant,
                        content: blocks,
                    });
                }
            }
            coco_types::Message::ToolResult(tr) => {
                let output = format!("tool_use_id={}", tr.tool_use_id);
                entries.push(TranscriptEntry {
                    role: TranscriptRole::User,
                    content: vec![TranscriptBlock::ToolResult {
                        tool_name: tr.tool_id.to_string(),
                        output_summary: truncate(&output, 500),
                        is_error: tr.is_error,
                    }],
                });
            }
            _ => {} // Skip system, progress, tombstone
        }
    }

    entries
}

/// Build the classifier system prompt.
///
/// Includes auto-mode rules and environment context.
pub fn build_classifier_system_prompt(rules: &AutoModeRules) -> String {
    let mut prompt = String::from(
        "You are a security classifier for an AI coding assistant. \
         Your job is to determine whether a proposed tool action is safe to auto-execute \
         without user confirmation.\n\n",
    );

    if !rules.allow.is_empty() {
        prompt.push_str("## Auto-Allow Rules\n");
        for rule in &rules.allow {
            prompt.push_str(&format!("- {rule}\n"));
        }
        prompt.push('\n');
    }

    if !rules.soft_deny.is_empty() {
        prompt.push_str("## Soft-Deny Rules (require user approval)\n");
        for rule in &rules.soft_deny {
            prompt.push_str(&format!("- {rule}\n"));
        }
        prompt.push('\n');
    }

    if !rules.environment.is_empty() {
        prompt.push_str("## Environment Context\n");
        for ctx in &rules.environment {
            prompt.push_str(&format!("- {ctx}\n"));
        }
        prompt.push('\n');
    }

    prompt.push_str(
        "Respond with a JSON object: {\"should_block\": true/false, \"reason\": \"...\"}.\n\
         If the action matches an allow rule and seems safe, set should_block to false.\n\
         If the action could be destructive, accesses sensitive data, or matches a deny rule, \
         set should_block to true.",
    );

    prompt
}

/// Format a tool action for the classifier.
///
/// TS: formatActionForClassifier() — produces a compact representation
/// of the tool call for the classifier to evaluate.
pub fn format_action_for_classifier(tool_name: &str, input: &serde_json::Value) -> String {
    let input_str = serde_json::to_string(input).unwrap_or_default();
    let truncated = truncate(&input_str, 1000);
    format!("<action>\ntool: {tool_name}\ninput: {truncated}\n</action>")
}

/// Stage 1 token budget (fast classification).
const STAGE_1_MAX_TOKENS: i64 = 256;

/// Stage 2 token budget (extended thinking).
const STAGE_2_MAX_TOKENS: i64 = 4096;

/// Parameters for the classifier callback.
#[derive(Debug, Clone)]
pub struct ClassifyRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_tokens: i64,
    pub stage: i32,
}

/// Classify a proposed tool action using the two-stage XML classifier.
///
/// Stage 1: Fast classification (256 tokens). Nudged for quick allow/block.
/// Stage 2: Extended thinking (4096 tokens). Full chain-of-thought reasoning.
///
/// Both stages share the same system prompt prefix for cache hits.
/// The `classify_fn` callback calls the LLM with the given request.
pub async fn classify_yolo_action<F, Fut>(
    messages: &[coco_types::Message],
    tool_name: &str,
    input: &serde_json::Value,
    rules: &AutoModeRules,
    classify_fn: F,
) -> YoloClassifierResult
where
    F: Fn(ClassifyRequest) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    // Fast path: safe tools never need classification.
    if is_safe_tool(tool_name) {
        return YoloClassifierResult {
            should_block: false,
            reason: "Safe tool (allowlisted)".into(),
            model: String::new(),
            usage: None,
            duration_ms: None,
            stage: None,
        };
    }

    let system_prompt = build_classifier_system_prompt(rules);
    let transcript = build_transcript_entries(messages);
    let action = format_action_for_classifier(tool_name, input);
    let transcript_text = format_transcript(&transcript);

    let user_prompt = format!(
        "{transcript_text}\n\n\
         Evaluate whether this action is safe to auto-execute:\n\n\
         {action}\n\n\
         Respond with XML: <answer>allow</answer> or <answer>block</answer> \
         followed by <reason>explanation</reason>."
    );

    // Stage 1: Fast classification.
    let stage1_result = classify_fn(ClassifyRequest {
        system_prompt: system_prompt.clone(),
        user_prompt: user_prompt.clone(),
        max_tokens: STAGE_1_MAX_TOKENS,
        stage: 1,
    })
    .await;

    match stage1_result {
        Ok(response) => {
            // Try XML parsing only (not JSON fallback) for Stage 1.
            let answer = extract_xml_tag(&response, "answer");
            match answer.as_deref() {
                Some("allow") | Some("block") => {
                    let reason = extract_xml_tag(&response, "reason").unwrap_or_default();
                    return YoloClassifierResult {
                        should_block: answer.as_deref() == Some("block"),
                        reason,
                        model: String::new(),
                        usage: None,
                        duration_ms: None,
                        stage: Some(1),
                    };
                }
                _ => {
                    // Ambiguous Stage 1 → fall through to Stage 2.
                }
            }
        }
        Err(_) => {
            // Stage 1 error → fall through to Stage 2.
        }
    }

    // Stage 2: Extended thinking classification.
    let stage2_result = classify_fn(ClassifyRequest {
        system_prompt,
        user_prompt,
        max_tokens: STAGE_2_MAX_TOKENS,
        stage: 2,
    })
    .await;

    match stage2_result {
        Ok(response) => parse_xml_classifier_response(&response, 2),
        Err(err) => {
            // On error, default to blocking (safe).
            YoloClassifierResult {
                should_block: true,
                reason: format!("Classifier error: {err}"),
                model: String::new(),
                usage: None,
                duration_ms: None,
                stage: Some(2),
            }
        }
    }
}

/// Format transcript entries into a compact string for the classifier.
fn format_transcript(entries: &[TranscriptEntry]) -> String {
    let mut out = String::new();
    for entry in entries.iter().rev().take(10) {
        let role_str = match entry.role {
            TranscriptRole::User => "User",
            TranscriptRole::Assistant => "Assistant",
        };
        out.push_str(&format!("[{role_str}]\n"));
        for block in &entry.content {
            match block {
                TranscriptBlock::Text(t) => out.push_str(t),
                TranscriptBlock::ToolCall {
                    tool_name,
                    input_summary,
                } => out.push_str(&format!("Called {tool_name}: {input_summary}")),
                TranscriptBlock::ToolResult {
                    tool_name,
                    output_summary,
                    is_error,
                } => {
                    let status = if *is_error { "ERROR" } else { "OK" };
                    out.push_str(&format!("{tool_name} [{status}]: {output_summary}"));
                }
            }
            out.push('\n');
        }
    }
    out
}

/// Parse XML-formatted classifier response.
///
/// Expected format: `<answer>allow|block</answer><reason>...</reason>`
fn parse_xml_classifier_response(response: &str, stage: i32) -> YoloClassifierResult {
    let answer = extract_xml_tag(response, "answer");
    let reason = extract_xml_tag(response, "reason").unwrap_or_default();

    let should_block = match answer.as_deref() {
        Some("allow") => false,
        Some("block") => true,
        _ => {
            // Fallback: try JSON parsing for backward compatibility.
            return parse_classifier_response_with_stage(response, stage);
        }
    };

    YoloClassifierResult {
        should_block,
        reason,
        model: String::new(),
        usage: None,
        duration_ms: None,
        stage: Some(stage),
    }
}

/// Extract content between `<tag>` and `</tag>`.
fn extract_xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)?;
    let end = text.find(&close)?;
    if end > start + open.len() {
        Some(text[start + open.len()..end].trim().to_string())
    } else {
        None
    }
}

/// Fallback: parse JSON response with stage info.
fn parse_classifier_response_with_stage(response: &str, stage: i32) -> YoloClassifierResult {
    let mut result = parse_classifier_response(response);
    result.stage = Some(stage);
    result
}

/// Parse the classifier's JSON response.
fn parse_classifier_response(response: &str) -> YoloClassifierResult {
    // Try to extract JSON from the response
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };

    #[derive(Deserialize)]
    struct ClassifierOutput {
        should_block: bool,
        #[serde(default)]
        reason: Option<String>,
    }

    match serde_json::from_str::<ClassifierOutput>(json_str) {
        Ok(output) => YoloClassifierResult {
            should_block: output.should_block,
            reason: output.reason.unwrap_or_default(),
            model: String::new(),
            usage: None,
            duration_ms: None,
            stage: None,
        },
        Err(_) => {
            // Can't parse → block (safe default)
            YoloClassifierResult {
                should_block: true,
                reason: "Could not parse classifier response".into(),
                model: String::new(),
                usage: None,
                duration_ms: None,
                stage: None,
            }
        }
    }
}

fn extract_user_text(msg: &coco_types::LlmMessage) -> String {
    match msg {
        coco_types::LlmMessage::User { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                coco_types::UserContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn extract_assistant_blocks(msg: &coco_types::LlmMessage) -> Vec<TranscriptBlock> {
    match msg {
        coco_types::LlmMessage::Assistant { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                coco_types::AssistantContent::Text(t) => {
                    Some(TranscriptBlock::Text(truncate(&t.text, 500)))
                }
                coco_types::AssistantContent::ToolCall(tc) => Some(TranscriptBlock::ToolCall {
                    tool_name: tc.tool_name.clone(),
                    input_summary: truncate(
                        &serde_json::to_string(&tc.input).unwrap_or_default(),
                        300,
                    ),
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
#[path = "classifier.test.rs"]
mod tests;
