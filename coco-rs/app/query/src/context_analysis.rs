//! Runtime-backed context-window analysis for `/context` and SDK control.
//!
//! This module is the single rich analyzer. It builds the same prompt and
//! tool definitions as the next main-loop API call, then estimates the
//! breakdown locally without provider token-count calls.

use std::sync::Arc;

use coco_error::ErrorExt;
use coco_error::Location;
use coco_error::StatusCode;
use coco_error::stack_trace_debug;
use coco_inference::LanguageModelTool;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_types::MessageBreakdown;
use coco_types::ProviderModelSelection;
use coco_types::ToolAppState;
use snafu::Snafu;

use crate::engine::QueryEngine;
use crate::engine_prompt::ModelToolSource;

#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub), module)]
pub enum ContextAnalysisError {
    #[snafu(display("active main model has no resolved ModelInfo; context window is unavailable"))]
    MissingModelInfo {
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for ContextAnalysisError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingModelInfo { .. } => StatusCode::InvalidArguments,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub type Result<T> = std::result::Result<T, ContextAnalysisError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextCategoryKind {
    SystemPrompt,
    Tools,
    McpTools,
    Agents,
    MemoryFiles,
    Skills,
    Messages,
    Free,
}

impl ContextCategoryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemPrompt => "System prompt",
            Self::Tools => "Built-in tools",
            Self::McpTools => "MCP tools",
            Self::Agents => "Agents",
            Self::MemoryFiles => "Memory files",
            Self::Skills => "Skills",
            Self::Messages => "Messages",
            Self::Free => "Free",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsageCategory {
    pub kind: ContextCategoryKind,
    pub tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFileEstimate {
    pub path: String,
    pub source: String,
    pub tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolEstimate {
    pub name: String,
    pub server_name: String,
    pub tokens: i64,
    pub deferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEstimate {
    pub agent_type: String,
    pub source: String,
    pub tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEstimate {
    pub name: String,
    pub source: String,
    pub tokens: i64,
}

#[derive(Debug, Clone)]
pub struct ContextUsageReport {
    pub total_tokens: i64,
    pub max_tokens: i64,
    pub raw_max_tokens: i64,
    pub percentage: f64,
    pub model: ProviderModelSelection,
    pub categories: Vec<ContextUsageCategory>,
    pub memory_files: Vec<MemoryFileEstimate>,
    pub mcp_tools: Vec<McpToolEstimate>,
    pub agents: Vec<AgentEstimate>,
    pub skills: Vec<SkillEstimate>,
    pub message_breakdown: MessageBreakdown,
    pub is_auto_compact_enabled: bool,
    pub auto_compact_threshold: Option<i64>,
}

impl ContextUsageReport {
    pub fn to_wire(&self) -> coco_types::ContextUsageResult {
        coco_types::ContextUsageResult {
            total_tokens: self.total_tokens,
            max_tokens: self.max_tokens,
            raw_max_tokens: self.raw_max_tokens,
            percentage: self.percentage,
            model: format!("{}/{}", self.model.provider, self.model.model_id),
            categories: self
                .categories
                .iter()
                .filter(|c| c.tokens > 0 || c.kind == ContextCategoryKind::Free)
                .map(|c| coco_types::ContextUsageCategory {
                    name: c.kind.label().to_string(),
                    tokens: c.tokens,
                })
                .collect(),
            is_auto_compact_enabled: self.is_auto_compact_enabled,
            auto_compact_threshold: self.auto_compact_threshold,
            message_breakdown: Some(self.message_breakdown.clone()),
            memory_files: self
                .memory_files
                .iter()
                .map(|m| coco_types::ContextMemoryFile {
                    path: m.path.clone(),
                    source: m.source.clone(),
                    tokens: m.tokens,
                })
                .collect(),
            mcp_tools: self
                .mcp_tools
                .iter()
                .map(|m| coco_types::ContextMcpTool {
                    name: m.name.clone(),
                    server_name: m.server_name.clone(),
                    tokens: m.tokens,
                    is_loaded: !m.deferred,
                })
                .collect(),
            agents: self
                .agents
                .iter()
                .map(|a| coco_types::ContextAgent {
                    agent_type: a.agent_type.clone(),
                    source: a.source.clone(),
                    tokens: a.tokens,
                })
                .collect(),
            skills: self
                .skills
                .iter()
                .map(|s| coco_types::ContextSkill {
                    name: s.name.clone(),
                    source: s.source.clone(),
                    tokens: s.tokens,
                })
                .collect(),
        }
    }
}

pub async fn analyze_engine_context(
    engine: &QueryEngine,
    history: &MessageHistory,
) -> Result<ContextUsageReport> {
    analyze_engine_context_with_sources(engine, history, None).await
}

pub async fn analyze_engine_context_with_sources(
    engine: &QueryEngine,
    history: &MessageHistory,
    skill_manager: Option<Arc<coco_skills::SkillManager>>,
) -> Result<ContextUsageReport> {
    let snapshot = engine
        .runtime_snapshot()
        .ok_or_else(|| context_analysis_error::MissingModelInfoSnafu.build())?;
    let model_info = snapshot
        .model_info
        .as_ref()
        .ok_or_else(|| context_analysis_error::MissingModelInfoSnafu.build())?;
    let raw_max_tokens = i64::from(model_info.context_window);
    let max_tokens = raw_max_tokens;
    let model = ProviderModelSelection {
        provider: snapshot.provider.clone(),
        model_id: snapshot.model_id.clone(),
    };

    let app_state = match &engine.app_state {
        Some(state) => state.read().await.clone(),
        None => ToolAppState::default(),
    };
    let built = engine.build_prompt(history).await;
    let tool_defs = engine.build_tool_definitions_detailed(&app_state).await;

    let system_text = first_system_text(&built.prompt);
    let system_total = coco_messages::estimate_text_tokens(system_text);
    let memory_files = built
        .memory_files
        .iter()
        .map(|m| MemoryFileEstimate {
            path: m.path.clone(),
            source: memory_file_source_label(m.source).to_string(),
            tokens: m.tokens,
        })
        .collect::<Vec<_>>();
    let memory_tokens = memory_files.iter().map(|m| m.tokens).sum::<i64>();
    let system_tokens = system_total.saturating_sub(memory_tokens);

    let mut builtin_tool_tokens = 0;
    let mut mcp_tool_tokens = 0;
    let mut agent_tool_tokens = 0;
    let mut skill_tool_tokens = 0;
    let mut mcp_tools = Vec::new();

    for built_tool in &tool_defs {
        let estimate = estimate_tool_tokens(&built_tool.tool);
        let name = built_tool.tool.name().to_string();
        match &built_tool.source {
            ModelToolSource::Agent => {
                if !built_tool.deferred {
                    agent_tool_tokens += estimate;
                }
            }
            ModelToolSource::Skill => {
                if !built_tool.deferred {
                    skill_tool_tokens += estimate;
                }
            }
            ModelToolSource::Mcp { server_name } => {
                mcp_tools.push(McpToolEstimate {
                    name,
                    server_name: server_name.clone(),
                    tokens: estimate,
                    deferred: built_tool.deferred,
                });
                if !built_tool.deferred {
                    mcp_tool_tokens += estimate;
                }
            }
            ModelToolSource::BuiltIn if !built_tool.deferred => {
                builtin_tool_tokens += estimate;
            }
            ModelToolSource::BuiltIn => {}
        }
    }

    let agents = agent_estimates(engine).await;

    let skills = skill_estimates(engine, skill_manager.as_deref());

    // Category totals describe model-visible prompt/tool bytes. Detail
    // rows are explanatory source slices and may not sum to their
    // category: AgentTool already embeds agent listings in its prompt,
    // while skill rows come from the session catalog for parity with
    // SDK `/context`.
    let message_tokens =
        coco_messages::estimate_tokens_for_messages(built.messages_snapshot.as_ref().as_slice());
    let message_breakdown = message_breakdown(built.messages_snapshot.as_ref().as_slice());
    let total_tokens = history.tokens_with_last_usage();
    let percentage = if max_tokens > 0 {
        (total_tokens as f64 / max_tokens as f64) * 100.0
    } else {
        0.0
    };
    let free_tokens = max_tokens.saturating_sub(total_tokens);

    Ok(ContextUsageReport {
        total_tokens,
        max_tokens,
        raw_max_tokens,
        percentage,
        model,
        categories: vec![
            ContextUsageCategory {
                kind: ContextCategoryKind::SystemPrompt,
                tokens: system_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::Tools,
                tokens: builtin_tool_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::McpTools,
                tokens: mcp_tool_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::Agents,
                tokens: agent_tool_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::MemoryFiles,
                tokens: memory_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::Skills,
                tokens: skill_tool_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::Messages,
                tokens: message_tokens,
            },
            ContextUsageCategory {
                kind: ContextCategoryKind::Free,
                tokens: free_tokens,
            },
        ],
        memory_files,
        mcp_tools,
        agents,
        skills,
        message_breakdown,
        is_auto_compact_enabled: engine.config.is_auto_compact_active(),
        auto_compact_threshold: engine.config.is_auto_compact_active().then(|| {
            coco_compact::auto_compact_threshold(
                max_tokens,
                engine.config.max_output_tokens,
                &engine.config.compact.auto,
            )
        }),
    })
}

fn memory_file_source_label(source: coco_context::MemoryFileSource) -> &'static str {
    match source {
        coco_context::MemoryFileSource::UserGlobal => "user",
        coco_context::MemoryFileSource::ProjectConfig => "project_config",
        coco_context::MemoryFileSource::Project => "project",
        coco_context::MemoryFileSource::Local => "local",
    }
}

pub fn format_markdown(report: &ContextUsageReport) -> String {
    let mut out = String::new();
    out.push_str("## Context Window Usage\n\n");
    out.push_str(&format!(
        "**Model:** {}/{}\n",
        report.model.provider, report.model.model_id
    ));
    out.push_str(&format!(
        "**Used:** {} / {} ({:.1}%)\n",
        format_tokens(report.total_tokens),
        format_tokens(report.max_tokens),
        report.percentage
    ));
    if let Some(threshold) = report.auto_compact_threshold {
        out.push_str(&format!(
            "**Auto-compact:** enabled at ~{} tokens\n",
            format_tokens(threshold)
        ));
    } else {
        out.push_str("**Auto-compact:** disabled\n");
    }

    out.push_str("\n### Estimated Breakdown\n\n");
    out.push_str("| Category | Tokens | Pct |\n");
    out.push_str("|---|---:|---:|\n");
    for category in &report.categories {
        if category.tokens == 0 && category.kind != ContextCategoryKind::Free {
            continue;
        }
        let pct = if report.max_tokens > 0 {
            category.tokens as f64 / report.max_tokens as f64 * 100.0
        } else {
            0.0
        };
        out.push_str(&format!(
            "| {} | {} | {:.1}% |\n",
            category.kind.label(),
            format_tokens(category.tokens),
            pct
        ));
    }

    append_memory_section(&mut out, &report.memory_files);
    append_mcp_section(&mut out, &report.mcp_tools);
    append_agent_section(&mut out, &report.agents);
    append_skill_section(&mut out, &report.skills);

    out.push_str("\n### Messages\n\n");
    out.push_str("| Type | Tokens |\n");
    out.push_str("|---|---:|\n");
    out.push_str(&format!(
        "| User messages | {} |\n",
        format_tokens(report.message_breakdown.user_message_tokens)
    ));
    out.push_str(&format!(
        "| Assistant messages | {} |\n",
        format_tokens(report.message_breakdown.assistant_message_tokens)
    ));
    out.push_str(&format!(
        "| Tool calls | {} |\n",
        format_tokens(report.message_breakdown.tool_call_tokens)
    ));
    out.push_str(&format!(
        "| Tool results | {} |\n",
        format_tokens(report.message_breakdown.tool_result_tokens)
    ));
    out.push_str(&format!(
        "| Attachments | {} |\n",
        format_tokens(report.message_breakdown.attachment_tokens)
    ));

    out
}

fn first_system_text(prompt: &[LlmMessage]) -> &str {
    match prompt.first() {
        Some(LlmMessage::System { content, .. }) => content
            .iter()
            .find_map(|part| match part {
                coco_messages::UserContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .unwrap_or(""),
        _ => "",
    }
}

fn estimate_tool_tokens(tool: &LanguageModelTool) -> i64 {
    match tool {
        LanguageModelTool::Function(t) => {
            let schema = serde_json::to_string(&t.input_schema).unwrap_or_default();
            let description = t.description.as_deref().unwrap_or_default();
            coco_messages::estimate_text_tokens(&format!("{}\n{}\n{}", t.name, description, schema))
        }
        LanguageModelTool::Provider(t) => {
            let serialized = serde_json::to_string(t).unwrap_or_default();
            coco_messages::estimate_text_tokens(&serialized)
        }
    }
}

fn message_breakdown(messages: &[std::sync::Arc<Message>]) -> MessageBreakdown {
    let mut breakdown = MessageBreakdown {
        tool_call_tokens: 0,
        tool_result_tokens: 0,
        attachment_tokens: 0,
        assistant_message_tokens: 0,
        user_message_tokens: 0,
    };
    for msg in messages {
        let tokens = coco_messages::estimate_message_tokens(msg.as_ref());
        match msg.as_ref() {
            Message::User(_) => breakdown.user_message_tokens += tokens,
            Message::Assistant(_) => {
                breakdown.assistant_message_tokens += tokens;
                breakdown.tool_call_tokens += estimate_assistant_tool_call_tokens(msg.as_ref());
            }
            Message::ToolResult(_) => breakdown.tool_result_tokens += tokens,
            Message::Attachment(_) => breakdown.attachment_tokens += tokens,
            Message::System(_) | Message::Progress(_) | Message::Tombstone(_) => {}
        }
    }
    breakdown
}

fn estimate_assistant_tool_call_tokens(msg: &Message) -> i64 {
    let Message::Assistant(a) = msg else {
        return 0;
    };
    let LlmMessage::Assistant { content, .. } = &a.message else {
        return 0;
    };
    content
        .iter()
        .filter_map(|part| match part {
            coco_messages::AssistantContent::ToolCall(call) => {
                let serialized = serde_json::to_string(call).unwrap_or_default();
                Some(coco_messages::estimate_text_tokens(&serialized))
            }
            _ => None,
        })
        .sum()
}

async fn agent_estimates(engine: &QueryEngine) -> Vec<AgentEstimate> {
    let Some(catalog) = &engine.agent_catalog else {
        return Vec::new();
    };
    let ready_mcp_servers = engine.mcp_servers_ready_snapshot().await;
    let mut rows = Vec::new();
    match ready_mcp_servers {
        Some(servers) => {
            for def in catalog.active_with_mcp(&servers) {
                push_agent_estimate(&mut rows, def);
            }
        }
        None => {
            for def in catalog.active() {
                push_agent_estimate(&mut rows, def);
            }
        }
    }
    rows
}

fn push_agent_estimate(rows: &mut Vec<AgentEstimate>, def: &coco_types::AgentDefinition) {
    if def.source == coco_types::AgentSource::BuiltIn {
        return;
    }
    let when_to_use = def
        .when_to_use
        .as_deref()
        .or(def.description.as_deref())
        .unwrap_or_default();
    let agent_type = def.agent_type.to_string();
    rows.push(AgentEstimate {
        tokens: coco_messages::estimate_text_tokens(&format!("{agent_type}\n{when_to_use}")),
        agent_type,
        source: def.source.as_str().to_string(),
    });
}

fn skill_estimates(
    engine: &QueryEngine,
    skill_manager: Option<&coco_skills::SkillManager>,
) -> Vec<SkillEstimate> {
    let Some(manager) = skill_manager else {
        return Vec::new();
    };
    manager
        .visible(&engine.config.features)
        .into_iter()
        .map(|skill| SkillEstimate {
            name: skill
                .display_name
                .as_deref()
                .unwrap_or(&skill.name)
                .to_string(),
            source: skill_source_label(&skill.source),
            tokens: coco_skills::estimate_skill_tokens(&skill),
        })
        .collect()
}

fn skill_source_label(source: &coco_skills::SkillSource) -> String {
    match source {
        coco_skills::SkillSource::Bundled => "bundled".to_string(),
        coco_skills::SkillSource::User { path } => format!("user:{}", path.display()),
        coco_skills::SkillSource::Project { path } => format!("project:{}", path.display()),
        coco_skills::SkillSource::Plugin { plugin_name } => format!("plugin:{plugin_name}"),
        coco_skills::SkillSource::Managed { path } => format!("managed:{}", path.display()),
        coco_skills::SkillSource::Mcp { server_name } => format!("mcp:{server_name}"),
    }
}

fn append_memory_section(out: &mut String, rows: &[MemoryFileEstimate]) {
    if rows.is_empty() {
        return;
    }
    out.push_str("\n### Memory Files\n\n");
    out.push_str("| Path | Source | Tokens |\n");
    out.push_str("|---|---|---:|\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_cell(&row.path),
            markdown_cell(&row.source),
            format_tokens(row.tokens)
        ));
    }
}

fn append_mcp_section(out: &mut String, rows: &[McpToolEstimate]) {
    if rows.is_empty() {
        return;
    }
    out.push_str("\n### MCP Tools\n\n");
    out.push_str("| Tool | Server | Tokens | State |\n");
    out.push_str("|---|---|---:|---|\n");
    for row in rows {
        let state = if row.deferred { "deferred" } else { "loaded" };
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            markdown_cell(&row.name),
            markdown_cell(&row.server_name),
            format_tokens(row.tokens),
            state
        ));
    }
}

fn append_agent_section(out: &mut String, rows: &[AgentEstimate]) {
    if rows.is_empty() {
        return;
    }
    out.push_str("\n### Agents\n\n");
    out.push_str("| Agent Type | Source | Tokens |\n");
    out.push_str("|---|---|---:|\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_cell(&row.agent_type),
            markdown_cell(&row.source),
            format_tokens(row.tokens)
        ));
    }
}

fn append_skill_section(out: &mut String, rows: &[SkillEstimate]) {
    if rows.is_empty() {
        return;
    }
    out.push_str("\n### Skills\n\n");
    out.push_str("| Skill | Source | Tokens |\n");
    out.push_str("|---|---|---:|\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_cell(&row.name),
            markdown_cell(&row.source),
            format_tokens(row.tokens)
        ));
    }
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn format_tokens(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
#[path = "context_analysis.test.rs"]
mod tests;
