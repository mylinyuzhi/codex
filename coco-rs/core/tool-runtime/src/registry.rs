use coco_types::MCP_TOOL_PREFIX;
use coco_types::PermissionMode;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use std::collections::HashMap;
use std::sync::Arc;

use crate::context::ToolUseContext;
use crate::traits::Tool;

/// Whether the given mode permits exposing this tool to the model
/// **at schema-definition time** (before any input exists).
///
/// `Plan` is the only mode that filters at this layer — it narrows the
/// schema to statically-read-only tools. Every other mode lets the
/// schema through unchanged; runtime permission rules still apply on
/// the actual call.
fn mode_permits_tool(mode: PermissionMode, tool: &dyn Tool) -> bool {
    match mode {
        PermissionMode::Plan => tool.is_always_read_only(),
        PermissionMode::Default
        | PermissionMode::AcceptEdits
        | PermissionMode::BypassPermissions
        | PermissionMode::DontAsk
        | PermissionMode::Auto
        | PermissionMode::Bubble => true,
    }
}

/// Run the full filter pipeline against one tool.
///
/// 1. `Tool::is_enabled(ctx)`     — Feature gate / OS / hard deps
/// 2. `ToolOverrides::permits`    — what the active model accepts
/// 3. `PermissionMode::permits`   — Plan-mode read-only narrowing
/// 4. `ToolFilter::allows`        — agent allow/deny lists
///
/// Layer 5 (MCP server reachability) is **not** implemented here; MCP
/// tools whose backing server disconnects are removed from the registry
/// via `ToolRegistry::deregister_by_server`, so they never reach this
/// pipeline. If a future requirement needs schema-time probing
/// (e.g. show as "unavailable" without deregistering), add a 5th
/// filter using `ctx.mcp` here.
fn passes_filter_pipeline(tool: &dyn Tool, ctx: &ToolUseContext) -> bool {
    let id = tool.id();
    tool.is_enabled(ctx)
        && ctx.tool_overrides.permits(&id)
        && mode_permits_tool(ctx.permission_context.mode, tool)
        && ctx.tool_filter.allows(&id)
}

/// Registry of available tools. Populated at startup by coco-cli.
///
/// Supports lookup by name, alias, and ToolId.
/// Feature-gated tools are registered but may return is_enabled() == false.
#[derive(Default)]
pub struct ToolRegistry {
    /// Primary lookup: name → tool.
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Alias lookup: alias → canonical name.
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. Also registers all its aliases.
    ///
    /// **MCP naming convention** (B3.3): tools that report `mcp_info()`
    /// are normalized to their `qualified_name()` form
    /// `mcp__<server>__<tool>` if their primary name doesn't already
    /// follow that convention. This mirrors TS `toolExecution.ts:287-300`
    /// + `mcpStringUtils.ts` behavior and prevents hostile MCP servers
    ///   from shadowing built-in tools (e.g. an MCP server advertising a
    ///   tool named "Read" is registered as "mcp__foo__Read" rather than
    ///   overwriting the real Read tool).
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let native_name = tool.name().to_string();

        // MCP namespace enforcement: if the tool has MCP info but its
        // name doesn't start with `mcp__`, silently promote to the
        // qualified form so the real name is preserved as an alias.
        let canonical = if let Some(info) = tool.mcp_info() {
            let qualified = info.qualified_name();
            if native_name == qualified || native_name.starts_with(MCP_TOOL_PREFIX) {
                // Already correctly namespaced — nothing to do.
                native_name
            } else {
                // Native name differs: use the qualified form as the
                // canonical entry and map the original name back via
                // alias so the model can still reference it both ways.
                self.aliases.insert(native_name, qualified.clone());
                qualified
            }
        } else {
            native_name
        };

        for alias in tool.aliases() {
            self.aliases.insert(alias.to_string(), canonical.clone());
        }
        self.tools.insert(canonical, tool);
    }

    /// Look up a tool by ToolId.
    pub fn get(&self, id: &ToolId) -> Option<&Arc<dyn Tool>> {
        self.get_by_name(&id.to_string())
    }

    /// Look up a tool by name or alias.
    pub fn get_by_name(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name).or_else(|| {
            self.aliases
                .get(name)
                .and_then(|canonical| self.tools.get(canonical))
        })
    }

    /// Get all registered tools.
    pub fn all(&self) -> impl Iterator<Item = &Arc<dyn Tool>> {
        self.tools.values()
    }

    /// Get enabled tools after running the full 5-layer filter pipeline.
    /// See `docs/coco-rs/feature-gates-and-tool-filtering.md` §7.
    pub fn enabled(&self, ctx: &ToolUseContext) -> Vec<&Arc<dyn Tool>> {
        self.tools
            .values()
            .filter(|t| passes_filter_pipeline(t.as_ref(), ctx))
            .collect()
    }

    /// Get non-deferred enabled tools (loaded immediately).
    pub fn loaded_tools(&self, ctx: &ToolUseContext) -> Vec<&Arc<dyn Tool>> {
        self.tools
            .values()
            .filter(|t| {
                passes_filter_pipeline(t.as_ref(), ctx) && (!t.should_defer() || t.always_load())
            })
            .collect()
    }

    /// Get deferred tools (discovered via ToolSearch).
    pub fn deferred_tools(&self, ctx: &ToolUseContext) -> Vec<&Arc<dyn Tool>> {
        self.tools
            .values()
            .filter(|t| {
                passes_filter_pipeline(t.as_ref(), ctx) && t.should_defer() && !t.always_load()
            })
            .collect()
    }

    /// Get tool definitions for the model (name + schema pairs).
    /// Only includes non-deferred enabled tools.
    pub fn definitions(&self, ctx: &ToolUseContext) -> Vec<(String, ToolInputSchema)> {
        self.loaded_tools(ctx)
            .into_iter()
            .map(|t| (t.name().to_string(), t.input_schema()))
            .collect()
    }

    /// Deregister all tools from a specific MCP server.
    ///
    /// Called when an MCP server disconnects. Removes all tools whose
    /// `mcp_info().server_name` matches the given server name, plus
    /// their aliases.
    ///
    /// TS: full re-discovery on reconnect, old tools cleaned up.
    pub fn deregister_by_server(&mut self, server_name: &str) {
        let to_remove: Vec<String> = self
            .tools
            .iter()
            .filter(|(_, tool)| {
                tool.mcp_info()
                    .is_some_and(|info| info.server_name == server_name)
            })
            .map(|(name, _)| name.clone())
            .collect();

        for name in &to_remove {
            self.tools.remove(name);
        }

        // Also remove aliases that point to removed tools
        self.aliases
            .retain(|_, canonical| !to_remove.contains(canonical));
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
#[path = "registry.test.rs"]
mod tests;
