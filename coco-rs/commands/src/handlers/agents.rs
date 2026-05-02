//! `/agents` — list, show, validate, reload, and inspect agent definitions.
//!
//! TS: `src/commands/agents/` — `/agents list`, `/agents show <name>`,
//! `/agents validate`, `/agents reload`, `/agents paths`.
//!
//! Backed by the `coco-subagent` catalog: built-ins from
//! [`coco_subagent::BuiltinAgentCatalog::interactive`] plus markdown agents
//! discovered under `~/.coco/agents` (user) and `<cwd>/.claude/agents`
//! (project). Source precedence is applied by the store; we only render
//! the snapshot here.

use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

use coco_subagent::AgentDefinitionStore;
use coco_subagent::BuiltinAgentCatalog;
use coco_subagent::definition_store::AgentSearchPaths;

/// Async handler for `/agents [list|show|validate|reload|paths]`.
pub fn handler(
    args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let cwd = std::env::current_dir().unwrap_or_default();
        let config_home = coco_config::global_config::config_home();
        let paths = standard_search_paths(&config_home, &cwd);

        // Disk reads via std::fs in the store — push to the blocking pool
        // so a slow filesystem doesn't stall the TUI event loop.
        let trimmed = args.trim().to_string();
        tokio::task::spawn_blocking(move || render(&trimmed, paths))
            .await
            .map_err(|e| anyhow::anyhow!("agents handler join error: {e}"))?
    })
}

fn render(args: &str, paths: AgentSearchPaths) -> anyhow::Result<String> {
    let mut store = AgentDefinitionStore::new(BuiltinAgentCatalog::interactive(), paths.clone());
    store.load();
    let snapshot = store.snapshot();
    let report = store.last_report();

    let (cmd, rest) = match args.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (args, ""),
    };

    Ok(match cmd {
        "" | "list" => render_list(&snapshot),
        "show" => render_show(&snapshot, rest),
        "paths" => render_paths(&paths),
        "validate" => render_validate(report),
        "reload" => render_list(&snapshot), // store.load() above already reloaded
        other => format!(
            "Unknown /agents subcommand: {other}\n\nUsage: /agents [list|show <name>|paths|validate|reload]"
        ),
    })
}

fn render_list(snapshot: &coco_subagent::AgentCatalogSnapshot) -> String {
    if snapshot.active_count() == 0 {
        return "No agents found.\n\
                Place markdown agent definitions in ~/.coco/agents (user) \
                or .claude/agents (project)."
            .to_string();
    }

    let mut out = format!("{} agent(s):\n\n", snapshot.active_count());
    for def in snapshot.active() {
        let model = def.model.as_deref().unwrap_or("inherit");
        let desc = def.description.as_deref().unwrap_or("(no description)");
        let source = def.source.as_str();
        out.push_str(&format!("  {}  [{source} · {model}]\n", def.name));
        out.push_str(&format!("    {desc}\n"));
    }
    out
}

fn render_show(snapshot: &coco_subagent::AgentCatalogSnapshot, name: &str) -> String {
    if name.is_empty() {
        return "Usage: /agents show <name>".to_string();
    }
    let Some(def) = snapshot.find_active(name) else {
        return format!("No active agent named: {name}");
    };

    let mut out = format!("# {}\n\n", def.name);
    out.push_str(&format!("Source:        {}\n", def.source.as_str()));
    if let Some(desc) = &def.description {
        out.push_str(&format!("Description:   {desc}\n"));
    }
    if let Some(model) = &def.model {
        out.push_str(&format!("Model:         {model}\n"));
    }
    if let Some(turns) = def.max_turns {
        out.push_str(&format!("Max turns:     {turns}\n"));
    }
    if def.background {
        out.push_str("Background:    true\n");
    }
    if !def.allowed_tools.is_empty() {
        out.push_str(&format!(
            "Tools:         {}\n",
            def.allowed_tools.join(", ")
        ));
    }
    if !def.disallowed_tools.is_empty() {
        out.push_str(&format!(
            "Disallowed:    {}\n",
            def.disallowed_tools.join(", ")
        ));
    }
    if !def.mcp_servers.is_empty() {
        let formatted = def
            .mcp_servers
            .iter()
            .map(|spec| match spec {
                coco_types::AgentMcpServerSpec::Name(s) => s.clone(),
                coco_types::AgentMcpServerSpec::Inline(map) => map
                    .keys()
                    .next()
                    .map(|n| format!("{n} (inline)"))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("MCP servers:   {formatted}\n"));
    }
    if let Some(prompt) = &def.initial_prompt {
        let preview = prompt.lines().take(10).collect::<Vec<_>>().join("\n");
        out.push_str(&format!("\nPrompt preview:\n{preview}\n"));
        if prompt.lines().count() > 10 {
            out.push_str("...\n");
        }
    }
    out
}

fn render_paths(paths: &AgentSearchPaths) -> String {
    let mut out = String::from("Agent search paths (later sources override earlier):\n\n");
    out.push_str("  built-in     (compiled-in catalog)\n");
    for d in &paths.plugin_dirs {
        out.push_str(&format!("  plugin       {}\n", d.display()));
    }
    if let Some(d) = &paths.user_dir {
        out.push_str(&format!("  user         {}\n", d.display()));
    }
    for d in &paths.project_dirs {
        out.push_str(&format!("  project      {}\n", d.display()));
    }
    for d in &paths.flag_dirs {
        out.push_str(&format!("  flag         {}\n", d.display()));
    }
    for d in &paths.policy_dirs {
        out.push_str(&format!("  policy       {}\n", d.display()));
    }
    out
}

fn render_validate(report: &coco_subagent::AgentLoadReport) -> String {
    if report.is_silent() {
        return "All agent definitions loaded with no warnings.".to_string();
    }
    let mut out = String::new();
    if !report.failed.is_empty() {
        out.push_str(&format!("{} failed:\n", report.failed.len()));
        for diag in &report.failed {
            out.push_str(&format!("  {}\n    {}\n", diag.path.display(), diag.error));
        }
    }
    if !report.warnings.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("{} warning(s):\n", report.warnings.len()));
        for diag in &report.warnings {
            out.push_str(&format!("  {}\n    {}\n", diag.path.display(), diag.error));
        }
    }
    out
}

/// Standard slash-command agent search paths: `~/.coco/agents` (user) plus
/// `<cwd>/.claude/agents` (project). Mirrors the CLI helper of the same
/// shape — kept here so the handler stays self-contained.
fn standard_search_paths(config_home: &Path, cwd: &Path) -> AgentSearchPaths {
    AgentSearchPaths {
        user_dir: Some(config_home.join("agents")),
        project_dirs: vec![cwd.join(".claude").join("agents")],
        flag_dirs: Vec::<PathBuf>::new(),
        policy_dirs: Vec::<PathBuf>::new(),
        plugin_dirs: Vec::<PathBuf>::new(),
    }
}

#[cfg(test)]
#[path = "agents.test.rs"]
mod tests;
