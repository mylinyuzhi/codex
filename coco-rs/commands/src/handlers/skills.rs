//! `/skills` — list discovered skills (bundled + user + project + plugin).
//!
//! TS: `commands/skills/skills.tsx` opens a `<SkillsMenu>` overlay listing
//! every loaded skill with metadata. Rust does the flat-text equivalent so
//! both SDK and TUI palette get a real enumeration. The TUI dispatcher may
//! later surface a richer overlay, but the underlying enumeration must
//! be honest — anything else lies to the user about what's loaded.
//!
//! Mirrors the same load order used by `tui_runner` when it builds the
//! command registry (`SkillManager::load_from_dirs(&[user, project])`),
//! plus the bundled in-binary set.

use std::path::Path;
use std::pin::Pin;

use coco_skills::SkillManager;
use coco_skills::SkillSource;
use coco_skills::bundled::register_bundled_default;

/// Async handler for `/skills [list|show <name>]`.
pub fn handler(
    args: String,
) -> Pin<Box<dyn std::future::Future<Output = crate::Result<String>> + Send>> {
    Box::pin(async move {
        let cwd = std::env::current_dir().unwrap_or_default();
        let config_home = coco_config::global_config::config_home();

        // Filesystem walk under `spawn_blocking` — std::fs is sync inside
        // the discovery routines, and a slow project tree shouldn't stall
        // the TUI event loop.
        let trimmed = args.trim().to_string();
        tokio::task::spawn_blocking(move || render(&trimmed, &config_home, &cwd))
            .await
            .map_err(|e| crate::CommandsError::generic(format!("skills handler join error: {e}")))?
    })
}

fn render(args: &str, config_home: &Path, cwd: &Path) -> crate::Result<String> {
    let manager = build_manager(config_home, cwd);

    let (cmd, rest) = match args.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (args, ""),
    };

    Ok(match cmd {
        "" | "list" => render_list(&manager),
        "show" => render_show(&manager, rest),
        "paths" => render_paths(config_home, cwd),
        // `/skills <name>` is a UX shorthand for `/skills show <name>`.
        // TS doesn't expose this — its `<SkillsMenu>` is read-only and
        // skills are invoked by typing `/<name>` directly. We accept the
        // shorthand so the flat-text path matches `/agents <name>` and
        // saves a keystroke; users still invoke a skill via `/<name>`.
        other if manager.get(other).is_some() => render_show(&manager, other),
        other => format!(
            "Unknown /skills subcommand: {other}\n\nUsage: /skills [list|show <name>|paths]\nTo run a skill, type /<skill-name>."
        ),
    })
}

/// Build a `SkillManager` from the same set of dirs `tui_runner` uses.
/// Built fresh per invocation so newly-added skills surface without a
/// session restart — the engine's live registry still loads only at
/// startup, but `/skills` reflects current disk truth.
fn build_manager(config_home: &Path, cwd: &Path) -> SkillManager {
    let manager = SkillManager::new();
    register_bundled_default(&manager);
    manager.load_from_dirs(&[
        config_home.join("skills"),
        cwd.join(".coco").join("skills"),
        cwd.join(".claude").join("skills"),
    ]);
    manager
}

fn render_list(manager: &SkillManager) -> String {
    let mut skills = manager.all();
    if skills.is_empty() {
        return "No skills found.\n\
                Place SKILL.md directories in ~/.coco/skills (user) or \
                .claude/skills (project)."
            .to_string();
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = format!("{} skill(s) loaded:\n\n", skills.len());
    for s in &skills {
        let source = source_label(&s.source);
        out.push_str(&format!("  /{}  [{source}]\n", s.name));
        let desc = s.description.lines().next().unwrap_or(&s.description);
        out.push_str(&format!("    {desc}\n"));
        if !s.aliases.is_empty() {
            out.push_str(&format!("    aliases: {}\n", s.aliases.join(", ")));
        }
    }
    // Invocation hint — the registry registers each skill as a command
    // (commands::register_skills_as_commands), so `/<name>` runs it.
    out.push_str(
        "\nTo run a skill: type /<skill-name>.\n\
         Details: /skills show <name>",
    );
    out
}

fn render_show(manager: &SkillManager, name: &str) -> String {
    if name.is_empty() {
        return "Usage: /skills show <name>".to_string();
    }
    let Some(s) = manager.get(name) else {
        return format!("No skill named: {name}");
    };

    let mut out = format!("# {}\n\n", s.name);
    out.push_str(&format!("Source:        {}\n", source_label(&s.source)));
    out.push_str(&format!("Description:   {}\n", s.description));
    if let Some(model) = &s.model {
        out.push_str(&format!("Model:         {model}\n"));
    }
    if let Some(when) = &s.when_to_use {
        out.push_str(&format!("When to use:   {when}\n"));
    }
    if let Some(hint) = &s.argument_hint {
        out.push_str(&format!("Args:          {hint}\n"));
    }
    if let Some(tools) = &s.allowed_tools
        && !tools.is_empty()
    {
        out.push_str(&format!("Tools:         {}\n", tools.join(", ")));
    }
    if !s.aliases.is_empty() {
        out.push_str(&format!("Aliases:       {}\n", s.aliases.join(", ")));
    }

    let preview = s.prompt.lines().take(10).collect::<Vec<_>>().join("\n");
    if !preview.is_empty() {
        out.push_str("\nPrompt preview:\n");
        out.push_str(&preview);
        if s.prompt.lines().count() > 10 {
            out.push_str("\n...");
        }
    }
    out
}

fn render_paths(config_home: &Path, cwd: &Path) -> String {
    let mut out = String::from("Skill search paths (later sources override earlier):\n\n");
    out.push_str("  bundled  (compiled-in catalog)\n");
    out.push_str(&format!(
        "  user     {}\n",
        config_home.join("skills").display()
    ));
    out.push_str(&format!(
        "  project  {}\n",
        cwd.join(".coco").join("skills").display()
    ));
    out.push_str(&format!(
        "  project  {}  (legacy)\n",
        cwd.join(".claude").join("skills").display()
    ));
    out
}

fn source_label(source: &SkillSource) -> String {
    match source {
        SkillSource::Bundled => "bundled".to_string(),
        SkillSource::User { path } => format!("user · {}", trim_path(path)),
        SkillSource::Project { path } => format!("project · {}", trim_path(path)),
        SkillSource::Plugin { plugin_name } => format!("plugin · {plugin_name}"),
        SkillSource::Managed { path } => format!("managed · {}", trim_path(path)),
        SkillSource::Mcp { server_name } => format!("mcp · {server_name}"),
    }
}

fn trim_path(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
#[path = "skills.test.rs"]
mod tests;
