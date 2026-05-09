//! ToolSearchTool — keyword search and direct selection for the tool registry.
//!
//! TS: `tools/ToolSearchTool/ToolSearchTool.ts:358-406`. Two query modes:
//!
//!   1. **Direct selection**: `select:Tool1,Tool2,Tool3` — the model
//!      explicitly names which deferred tools to load. Comma-separated,
//!      whitespace-tolerant, case-insensitive matching against tool
//!      names and aliases. No ranking — every matched tool is returned.
//!
//!   2. **Keyword search**: any other query string. Substring match
//!      against name, description, search_hint, and aliases. Ranked by
//!      hit priority (name > hint > description), capped at `max_results`.
//!
//! Direct selection is how the model "un-defers" MCP tools that are
//! hidden by default (`should_defer() = true`). Without ToolSearch, the
//! model would never know those tools exist. With it, the system prompt
//! can tell the model "use ToolSearch with query=select:MyTool to load
//! the MyTool MCP tool" and the tool gets surfaced back into the
//! runtime registry.
//!
//! For now, coco-rs ToolSearch returns metadata only — the actual
//! promotion of deferred tools into the active registry is handled at a
//! higher layer (query engine) via context modifiers. The `select:` path
//! sets a `selected_tools` array in the result payload that the query
//! layer can pick up.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;
use std::collections::HashMap;

/// Parse a `select:Tool1,Tool2,...` query into a list of tool names.
/// Returns `None` if the query isn't in select mode. Whitespace around
/// each name is trimmed; empty names are dropped.
///
/// **Prefix is case-insensitive** — `select:`, `Select:`, `SELECT:` all
/// trigger select mode. TS `ToolSearchTool.ts:363` uses the regex
/// `/^select:(.+)$/i` (the `/i` flag is case-insensitive). We mirror
/// that behavior by lowercasing the prefix check.
pub(super) fn parse_select_query(query: &str) -> Option<Vec<String>> {
    // Case-insensitive prefix match: if the first 7 chars (lowercased)
    // equal `"select:"`, strip them. Otherwise return None.
    if query.len() < 7 {
        return None;
    }
    let prefix = &query[..7];
    if !prefix.eq_ignore_ascii_case("select:") {
        return None;
    }
    let rest = &query[7..];
    Some(
        rest.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

pub struct ToolSearchTool;

#[async_trait::async_trait]
impl Tool for ToolSearchTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ToolSearch)
    }
    fn name(&self) -> &str {
        ToolName::ToolSearch.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Search for available tools by keyword, or directly select tools by name. \
         Use 'select:Tool1,Tool2' to load specific deferred tools, or a plain keyword \
         query to search by name/description/alias."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "query".into(),
            serde_json::json!({
                "type": "string",
                "description": "Keyword search query, or 'select:Tool1,Tool2' for direct selection"
            }),
        );
        p.insert(
            "max_results".into(),
            serde_json::json!({"type": "number", "description": "Maximum number of results (default 5)"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    /// Render the search envelope as a list of matched tool names. TS
    /// `ToolSearchTool.ts:444-470` returns `tool_reference` content
    /// blocks that Anthropic expands inline — coco-rs doesn't have
    /// that primitive, so we emit a text list. The empty-match branch
    /// matches TS exactly (`No matching deferred tools found` + the
    /// pending-MCP-server suffix when servers are still connecting).
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let matches: Vec<&str> = data
            .get("matches")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let text = if matches.is_empty() {
            let mut out = "No matching deferred tools found".to_string();
            if let Some(pending) = data.get("pending_mcp_servers").and_then(Value::as_array) {
                let names: Vec<&str> = pending.iter().filter_map(Value::as_str).collect();
                if !names.is_empty() {
                    use std::fmt::Write;
                    let _ = write!(
                        out,
                        ". Some MCP servers are still connecting: {}. Their tools will become available shortly — try searching again.",
                        names.join(", ")
                    );
                }
            }
            out
        } else {
            format!("Matched tools:\n{}", matches.join("\n"))
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let raw_query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if raw_query.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "query parameter is required".into(),
                error_code: None,
            });
        }

        // Count of deferred tools — both modes include this in the output.
        let total_deferred_tools = ctx
            .tools
            .all()
            .into_iter()
            .filter(|t| t.should_defer())
            .count();

        // Direct selection mode: `select:Tool1,Tool2,...`
        //
        // TS `ToolSearchTool.ts:37-45, 110-126, 363-405` returns a
        // UNIFIED output shape for both select and keyword modes:
        //
        //   { matches: string[], query: string,
        //     total_deferred_tools: number,
        //     pending_mcp_servers?: string[] }
        //
        // There's NO separate `mode` field and NO `selected_tools` /
        // `missing` fields — TS just filters the requested names down
        // to the ones that resolved and puts the found names in
        // `matches`. Tools that fail to resolve are silently dropped.
        //
        // Previously coco-rs returned `{mode, requested, selected_tools,
        // missing}` which broke downstream code expecting the TS shape;
        // R2 from round-2 deep-review fixes that.
        if let Some(names) = parse_select_query(&raw_query) {
            if names.is_empty() {
                return Err(ToolError::InvalidInput {
                    message: "select: query must name at least one tool (e.g. 'select:Read,Grep')"
                        .into(),
                    error_code: None,
                });
            }
            // Resolve each requested name (case-insensitive on name +
            // aliases). TS uses `findToolByName` which does the same
            // case-insensitive lookup.
            let mut matches: Vec<String> = Vec::new();
            for name in &names {
                let name_lower = name.to_lowercase();
                let hit = ctx.tools.all().into_iter().find(|t| {
                    t.name().eq_ignore_ascii_case(name)
                        || t.aliases()
                            .iter()
                            .any(|a| a.eq_ignore_ascii_case(&name_lower))
                });
                if let Some(tool) = hit {
                    matches.push(tool.name().to_string());
                }
            }
            return Ok(ToolResult {
                data: serde_json::json!({
                    "matches": matches,
                    "query": raw_query,
                    "total_deferred_tools": total_deferred_tools,
                }),
                new_messages: vec![],
                app_state_patch: None,
            });
        }

        // Keyword search mode — same output shape as select mode.
        //
        // Note: `matches` is an array of tool NAMES (strings), not
        // full objects. Downstream code resolves the names via the
        // registry if it needs descriptions. This matches TS exactly.
        let query = raw_query.to_lowercase();

        let max_results = input
            .get("max_results")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(5) as usize;

        let mut matches: Vec<String> = Vec::new();

        // Pass the current permission context so tools can tailor their
        // descriptions to the mode — matches TS `getToolPermissionContext()`.
        let tool_names: Vec<String> = ctx
            .tools
            .all()
            .into_iter()
            .map(|t| t.name().to_string())
            .collect();
        let desc_opts = DescriptionOptions {
            is_non_interactive: false,
            tool_names,
            permission_context: Some(ctx.permission_context.clone()),
        };

        for tool in ctx.tools.all() {
            let name_lower = tool.name().to_lowercase();
            let desc_lower = tool.description(&Value::Null, &desc_opts).to_lowercase();
            let hint_lower = tool
                .search_hint()
                .map(str::to_lowercase)
                .unwrap_or_default();
            let alias_match = tool
                .aliases()
                .iter()
                .any(|a| a.to_lowercase().contains(&query));

            if name_lower.contains(&query)
                || desc_lower.contains(&query)
                || hint_lower.contains(&query)
                || alias_match
            {
                matches.push(tool.name().to_string());
            }

            if matches.len() >= max_results {
                break;
            }
        }

        // TS `ToolSearchTool.ts:422-433` only adds `pending_mcp_servers`
        // in the keyword-mode empty-matches branch. Match that exactly
        // so the model gets the retry hint when MCP servers are still
        // mid-handshake but stays quiet otherwise.
        let mut envelope = serde_json::json!({
            "matches": matches,
            "query": raw_query,
            "total_deferred_tools": total_deferred_tools,
        });
        if matches.is_empty() {
            let pending = ctx.mcp.pending_server_names().await;
            if !pending.is_empty() {
                envelope["pending_mcp_servers"] = serde_json::json!(pending);
            }
        }
        Ok(ToolResult {
            data: envelope,
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

#[cfg(test)]
#[path = "tool_search.test.rs"]
mod tests;
