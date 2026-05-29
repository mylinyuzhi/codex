use coco_types::MCP_TOOL_PREFIX;
use coco_types::PermissionMode;
use coco_types::ToolId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use crate::context::ToolUseContext;
use crate::traits::DynTool;

/// Whether the given mode permits exposing this tool to the model
/// **at schema-definition time** (before any input exists).
///
/// `Plan` is the only mode that filters at this layer — it narrows the
/// schema to statically-read-only tools. Every other mode lets the
/// schema through unchanged; runtime permission rules still apply on
/// the actual call.
fn mode_permits_tool(mode: PermissionMode, tool: &dyn DynTool) -> bool {
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
fn passes_filter_pipeline(tool: &dyn DynTool, ctx: &ToolUseContext) -> bool {
    let id = tool.id();
    tool.is_enabled(ctx)
        && ctx.tool_overrides.permits(&id)
        && mode_permits_tool(ctx.permission_context.mode, tool)
        && ctx.tool_filter.allows(&id)
}

/// Inner state protected by a single RwLock.
///
/// Both maps are always mutated together (every `register` touches
/// `tools` and may also touch `aliases`; every `deregister_by_server`
/// touches both). A single lock ensures the two maps are always
/// consistent — no window where `tools` has a new entry but `aliases`
/// does not, or vice versa.
#[derive(Default)]
struct RegistryInner {
    /// Primary lookup: canonical name → tool.
    tools: HashMap<String, Arc<dyn DynTool>>,
    /// Alias lookup: alias → canonical name.
    aliases: HashMap<String, String>,
}

impl RegistryInner {
    /// Insert `tool` under its canonical name + aliases, replicating the
    /// **MCP-namespace promotion** (a hostile MCP `Read` is stored as
    /// `mcp__srv__Read`, never shadowing the built-in). Shared by
    /// [`ToolRegistry::register`] and [`ToolRegistry::replace_server_tools`]
    /// so the namespacing is identical on both paths.
    fn register_with_aliases(&mut self, tool: Arc<dyn DynTool>) {
        let native_name = tool.name().to_string();
        let canonical = if let Some(info) = tool.mcp_info() {
            let qualified = info.qualified_name();
            if native_name == qualified || native_name.starts_with(MCP_TOOL_PREFIX) {
                native_name
            } else {
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

    /// Remove a tool by `ToolId` (canonical name is `id.to_string()`). Does
    /// NOT touch aliases — callers wipe aliases separately.
    fn remove_tool_by_id(&mut self, id: &ToolId) {
        self.tools.remove(&id.to_string());
    }
}

/// Registry of available tools. Populated at startup by coco-cli.
///
/// Supports lookup by name, alias, and ToolId.
/// Feature-gated tools are registered but may return is_enabled() == false.
///
/// Interior mutability via a single `RwLock` allows `register` and
/// `deregister_by_server` to take `&self`, so the registry can be
/// mutated after it is wrapped in `Arc` (required for runtime MCP
/// tool registration after servers connect).
pub struct ToolRegistry {
    inner: RwLock<RegistryInner>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            inner: RwLock::new(RegistryInner::default()),
        }
    }
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
    pub fn register(&self, tool: Arc<dyn DynTool>) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.register_with_aliases(tool);
    }

    /// Look up a tool by ToolId.
    pub fn get(&self, id: &ToolId) -> Option<Arc<dyn DynTool>> {
        self.get_by_name(&id.to_string())
    }

    /// Look up a tool by name or alias.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn DynTool>> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.tools.get(name).cloned().or_else(|| {
            inner
                .aliases
                .get(name)
                .and_then(|canonical| inner.tools.get(canonical))
                .cloned()
        })
    }

    /// Get all registered tools (clones the Arc handles).
    pub fn all(&self) -> Vec<Arc<dyn DynTool>> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.tools.values().cloned().collect()
    }

    /// Get enabled tools after running the full 5-layer filter pipeline.
    /// See `docs/coco-rs/feature-gates-and-tool-filtering.md` §7.
    pub fn enabled(&self, ctx: &ToolUseContext) -> Vec<Arc<dyn DynTool>> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .tools
            .values()
            .filter(|t| passes_filter_pipeline(t.as_ref(), ctx))
            .cloned()
            .collect()
    }

    /// Get non-deferred enabled tools (loaded immediately).
    ///
    /// A deferred tool whose wire-name appears in
    /// `ctx.discovered_tool_names` is treated as if it were not
    /// deferred — that is the TS-parity mechanism through which the
    /// model "loads" a tool via `ToolSearch`. `always_load()` still
    /// short-circuits the deferral check independent of discovery.
    ///
    /// When [`coco_types::Feature::ToolSearch`] is **off**, the
    /// deferral check is bypassed entirely (TS `'standard'` mode):
    /// every enabled tool's full schema lands in turn-1 requests.
    /// Keeps the per-Provider serialization path identical, just
    /// without the lazy-loading optimization.
    pub fn loaded_tools(&self, ctx: &ToolUseContext) -> Vec<Arc<dyn DynTool>> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tool_search_active = ctx.tool_search_active();
        inner
            .tools
            .values()
            .filter(|t| {
                passes_filter_pipeline(t.as_ref(), ctx)
                    && (!tool_search_active
                        || !t.should_defer()
                        || t.always_load()
                        || ctx.discovered_tool_names.contains(t.name()))
            })
            .cloned()
            .collect()
    }

    /// Get deferred tools (discovered via ToolSearch).
    ///
    /// Symmetric to [`Self::loaded_tools`]: deferred tools that have
    /// been discovered are *excluded* — they have moved into the
    /// loaded set for this turn.
    ///
    /// Returns empty when [`coco_types::Feature::ToolSearch`] is
    /// off — there is no deferred pool to surface, every tool is
    /// already loaded via [`Self::loaded_tools`].
    pub fn deferred_tools(&self, ctx: &ToolUseContext) -> Vec<Arc<dyn DynTool>> {
        if !ctx.tool_search_active() {
            return Vec::new();
        }
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .tools
            .values()
            .filter(|t| {
                passes_filter_pipeline(t.as_ref(), ctx)
                    && t.should_defer()
                    && !t.always_load()
                    && !ctx.discovered_tool_names.contains(t.name())
            })
            .cloned()
            .collect()
    }

    /// Deregister all tools from a specific MCP server.
    ///
    /// Called when an MCP server disconnects. Removes all tools whose
    /// `mcp_info().server_name` matches the given server name, plus
    /// their aliases.
    ///
    /// TS: full re-discovery on reconnect, old tools cleaned up.
    pub fn deregister_by_server(&self, server_name: &str) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let to_remove: Vec<String> = inner
            .tools
            .iter()
            .filter(|(_, tool)| {
                tool.mcp_info()
                    .is_some_and(|info| info.server_name == server_name)
            })
            .map(|(name, _)| name.clone())
            .collect();

        for name in &to_remove {
            inner.tools.remove(name);
        }

        // Also remove aliases that point to removed tools
        inner
            .aliases
            .retain(|_, canonical| !to_remove.contains(canonical));
    }

    /// Atomically replace all tools belonging to `server_name` with
    /// `new_tools`, under a SINGLE write lock (no window where readers see a
    /// partial set — fixes the non-transactional `deregister`+loop-`register`
    /// reconnect path). Returns the tombstoned `ToolId`s: present in the
    /// previous batch but absent from `new_tools`.
    ///
    /// All server-owned aliases are wiped by **full membership** BEFORE
    /// re-registering, so a retained tool whose advertised alias set changed
    /// across reconnect leaves no stale alias (v4.2 finding 6).
    pub fn replace_server_tools(
        &self,
        server_name: &str,
        new_tools: Vec<Arc<dyn DynTool>>,
    ) -> Vec<ToolId> {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // 1. Snapshot the server's current canonical names + ToolIds.
        let owned: Vec<(String, ToolId)> = inner
            .tools
            .iter()
            .filter(|(_, t)| t.mcp_info().is_some_and(|i| i.server_name == server_name))
            .map(|(name, t)| (name.clone(), t.id()))
            .collect();
        let owned_names: std::collections::HashSet<String> =
            owned.iter().map(|(n, _)| n.clone()).collect();
        let old_ids: std::collections::HashSet<ToolId> =
            owned.into_iter().map(|(_, id)| id).collect();
        let new_ids: std::collections::HashSet<ToolId> = new_tools.iter().map(|t| t.id()).collect();
        let tombstones: Vec<ToolId> = old_ids.difference(&new_ids).cloned().collect();

        // 2. Wipe ALL server-owned aliases (full membership, not just tombstones).
        inner
            .aliases
            .retain(|_, canonical| !owned_names.contains(canonical.as_str()));

        // 3. Drop tombstoned tools (their aliases already gone via step 2).
        for id in &tombstones {
            inner.remove_tool_by_id(id);
        }

        // 4. Re-register the new batch — re-establishes aliases fresh and
        //    overwrites retained tools with their new (reconnect) instance.
        for tool in new_tools {
            inner.register_with_aliases(tool);
        }

        tombstones
    }

    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tools
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tools
            .is_empty()
    }
}

#[cfg(test)]
#[path = "registry.test.rs"]
mod tests;
