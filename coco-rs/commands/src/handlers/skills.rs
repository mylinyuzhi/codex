//! `/skills` — list discovered skills (bundled + user + project + plugin).
//!
//! TS: `commands/skills/skills.tsx` opens a `<SkillsMenu>` overlay listing
//! every loaded skill with metadata. The no-arg invocation routes through
//! [`SkillsHandler`] which returns
//! [`crate::CommandResult::OpenDialog`] carrying a fully-built
//! [`coco_types::SkillsDialogPayload`] — the TUI consumer renders the
//! same shape as the TS `<SkillsMenu>` (5 source groups, token estimate
//! per row, Esc to close).
//!
//! The sub-commands (`list` / `show <name>` / `paths`) stay text-only
//! via the legacy [`handler`] function so SDK / headless / scripted
//! callers get a flat enumeration they can parse.
//!
//! Mirrors the same load order used by `tui_runner` when it builds the
//! command registry (`SkillManager::load_from_dirs(&[user, project])`),
//! plus the bundled in-binary set.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use coco_skills::SkillDefinition;
use coco_skills::SkillManager;
use coco_skills::SkillScopes;
use coco_skills::SkillSource;
use coco_skills::bundled::register_bundled_default;
use coco_skills::estimate_skill_tokens;
use coco_skills::get_managed_skills_path;
use coco_types::SkillsDialogEntry;
use coco_types::SkillsDialogGroupSubtitle;
use coco_types::SkillsDialogPayload;
use coco_types::SkillsDialogSource;

use crate::CommandHandler;
use crate::CommandResult;
use crate::DialogSpec;

/// `CommandHandler` impl for `/skills`. No args → open the TUI dialog;
/// `list` / `show` / `paths` → reuse the text path.
///
/// TS parity: `commands/skills/skills.tsx::call` (no args opens
/// `<SkillsMenu>`). coco-rs adds sub-commands for non-TUI surfaces.
pub struct SkillsHandler;

#[async_trait]
impl CommandHandler for SkillsHandler {
    async fn execute_command(&self, args: &str) -> crate::Result<CommandResult> {
        let trimmed = args.trim().to_string();
        let cwd = std::env::current_dir().unwrap_or_default();
        let config_home = coco_config::global_config::config_home();

        // Discovery is sync (`std::fs`) — keep the TUI event loop
        // unblocked.
        tokio::task::spawn_blocking(move || -> crate::Result<CommandResult> {
            if trimmed.is_empty() {
                let payload = build_dialog_payload(&config_home, &cwd);
                Ok(CommandResult::OpenDialog(DialogSpec::SkillsList {
                    payload,
                }))
            } else {
                Ok(CommandResult::Text(render(&trimmed, &config_home, &cwd)?))
            }
        })
        .await
        .map_err(|e| crate::CommandsError::generic(format!("skills handler join error: {e}")))?
    }

    fn handler_name(&self) -> &str {
        "skills"
    }
}

/// Build the dialog payload from the freshly-discovered skill catalog.
/// Grouping + per-group subtitle resolution happens here so the TUI is
/// a pure projection.
///
/// TS: `SkillsMenu` does the same grouping + subtitle work inline; we
/// hoist it here so the slash dispatcher owns it and the TUI doesn't
/// need access to `SkillManager` / `get_skill_paths` directly.
fn build_dialog_payload(config_home: &Path, cwd: &Path) -> SkillsDialogPayload {
    let manager = build_manager(config_home, cwd);
    let skills = manager.all();

    // TS-parity filter: drop disabled skills, then drop anything whose
    // source the dialog excludes (`Bundled`). `source_to_dialog`
    // returns `None` for excluded sources so `filter_map` drops them.
    let entries: Vec<SkillsDialogEntry> = skills
        .iter()
        .filter(|s| !s.disabled)
        .filter_map(|s| {
            Some(SkillsDialogEntry {
                name: s.name.clone(),
                source: source_to_dialog(&s.source)?,
                plugin_name: plugin_name_for(s),
                token_estimate: estimate_skill_tokens(s),
            })
        })
        .collect();

    SkillsDialogPayload {
        group_subtitles: build_group_subtitles(config_home, cwd, &entries, &skills),
        entries,
    }
}

/// Map a `SkillSource` to the dialog's source discriminant. Returns
/// `None` for sources the TS dialog filters out — currently just
/// `Bundled` (TS `SkillsMenu` filters
/// `loadedFrom in ['skills', 'commands_DEPRECATED', 'plugin', 'mcp']`,
/// explicitly excluding `bundled`). Caller drops the entry on `None`.
fn source_to_dialog(source: &SkillSource) -> Option<SkillsDialogSource> {
    Some(match source {
        SkillSource::Project { .. } => SkillsDialogSource::Project,
        SkillSource::User { .. } => SkillsDialogSource::User,
        SkillSource::Managed { .. } => SkillsDialogSource::Policy,
        SkillSource::Plugin { .. } => SkillsDialogSource::Plugin,
        SkillSource::Mcp { .. } => SkillsDialogSource::Mcp,
        // TS-parity exclusion: bundled skills don't appear in the dialog.
        SkillSource::Bundled => return None,
    })
}

fn plugin_name_for(s: &SkillDefinition) -> Option<String> {
    match &s.source {
        SkillSource::Plugin { plugin_name } => Some(plugin_name.clone()),
        _ => None,
    }
}

/// TS `getSourceSubtitle`: file-based groups get the skills directory
/// display path; MCP gets a comma-joined unique server-name list.
///
/// **Only emit subtitles for groups that have visible entries** — TS
/// computes subtitle per-group inside the render loop, so empty groups
/// never get a subtitle. We keep the wire payload tight at the source.
fn build_group_subtitles(
    config_home: &Path,
    cwd: &Path,
    entries: &[SkillsDialogEntry],
    skills: &[Arc<SkillDefinition>],
) -> Vec<SkillsDialogGroupSubtitle> {
    let present: std::collections::HashSet<SkillsDialogSource> =
        entries.iter().map(|e| e.source).collect();
    let mut out = Vec::new();

    if present.contains(&SkillsDialogSource::Policy) {
        out.push(SkillsDialogGroupSubtitle {
            source: SkillsDialogSource::Policy,
            subtitle: get_managed_skills_path().display().to_string(),
        });
    }
    if present.contains(&SkillsDialogSource::User) {
        // TS `getSourceSubtitle`: append the legacy `commands/` path
        // only when a user-scope skill actually came from the
        // `commands_DEPRECATED` flat-`.md` layout. Otherwise users see
        // an extra path that has no skills behind it.
        let user_commands_dir = config_home.join("commands");
        let mut parts = vec![config_home.join("skills").display().to_string()];
        if has_legacy_commands_skills(skills, SkillsDialogSource::User, &user_commands_dir) {
            parts.push(user_commands_dir.display().to_string());
        }
        out.push(SkillsDialogGroupSubtitle {
            source: SkillsDialogSource::User,
            subtitle: parts.join(", "),
        });
    }
    if present.contains(&SkillsDialogSource::Project) {
        // coco-rs supports two project skill roots — the canonical
        // `.coco/skills/` and TS-compat `.claude/skills/`. Comma-join
        // both so users can locate any project skill from the dialog.
        // TS-parity addition: if any project skill came from the legacy
        // `.claude/commands/` directory, append it too.
        let project_commands_dir = cwd.join(".claude").join("commands");
        let mut parts: Vec<String> = [
            cwd.join(".coco").join("skills"),
            cwd.join(".claude").join("skills"),
        ]
        .iter()
        .map(|p| p.display().to_string())
        .collect();
        if has_legacy_commands_skills(skills, SkillsDialogSource::Project, &project_commands_dir) {
            parts.push(project_commands_dir.display().to_string());
        }
        out.push(SkillsDialogGroupSubtitle {
            source: SkillsDialogSource::Project,
            subtitle: parts.join(", "),
        });
    }

    if present.contains(&SkillsDialogSource::Plugin) {
        // TS-parity: `getSkillsPath('plugin', 'skills')` returns the
        // literal string `"plugin"` and the dialog renders it as the
        // group subtitle (`SkillsMenu.tsx:135`). We mirror that here
        // — see [[project_coco_rs_phase2_skills_dialog]]. The plugin
        // *name* still appears inline on each row, so the subtitle is
        // a low-information group label, not a precise data point.
        out.push(SkillsDialogGroupSubtitle {
            source: SkillsDialogSource::Plugin,
            subtitle: "plugin".to_string(),
        });
    }

    if present.contains(&SkillsDialogSource::Mcp) {
        // Matches TS `getSourceSubtitle` for `mcp`: joined unique
        // server-name list.
        let mut mcp_servers: Vec<String> = skills
            .iter()
            .filter_map(|s| match &s.source {
                SkillSource::Mcp { server_name } => Some(server_name.clone()),
                _ => None,
            })
            .collect();
        mcp_servers.sort();
        mcp_servers.dedup();
        if !mcp_servers.is_empty() {
            out.push(SkillsDialogGroupSubtitle {
                source: SkillsDialogSource::Mcp,
                subtitle: mcp_servers.join(", "),
            });
        }
    }

    out
}

/// Whether any visible skill in `scope` was loaded from the legacy
/// `.claude/commands/` directory layout (TS `loadedFrom ===
/// 'commands_DEPRECATED'`). Used to gate the commands-dir entry in the
/// User/Project subtitle.
///
/// The `SkillSource::User { path } | Project { path }` field carries
/// the **skill file path** (set in `SkillManager::load_with_source`),
/// so `starts_with(commands_dir)` reliably distinguishes the two.
fn has_legacy_commands_skills(
    skills: &[Arc<SkillDefinition>],
    scope: SkillsDialogSource,
    commands_dir: &Path,
) -> bool {
    skills.iter().any(|s| match (&s.source, scope) {
        (SkillSource::User { path }, SkillsDialogSource::User)
        | (SkillSource::Project { path }, SkillsDialogSource::Project) => {
            path.starts_with(commands_dir)
        }
        _ => false,
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

/// Build a `SkillManager` with **source-correct tagging** so both the
/// dialog (which groups by source) and the text `list` output (which
/// labels each row by source) get the right `[user]` / `[project]` /
/// `[managed]` attribution.
///
/// Built fresh per invocation so newly-added skills surface without a
/// session restart — the engine's live registry still loads only at
/// startup, but `/skills` reflects current disk truth.
///
/// **Two project paths.** coco-rs supports BOTH the canonical
/// `.coco/skills/` and the TS-compat `.claude/skills/` as project
/// skill roots. We invoke `load_scoped` twice: once for the standard
/// scopes (managed / user / `.claude/skills` / `.claude/commands`)
/// and once again with only `project_skills = .coco/skills` so those
/// also get `SkillSource::Project { path }`. Last-write-wins on name
/// collisions, with `.coco/skills` winning since it's loaded second
/// (the newer convention is preferred).
fn build_manager(config_home: &Path, cwd: &Path) -> SkillManager {
    let manager = SkillManager::new();
    register_bundled_default(&manager);

    // Standard scopes: managed / user / `.claude/skills` / `.claude/commands`.
    manager.load_scoped(&SkillScopes {
        managed: Some(get_managed_skills_path()),
        user_skills: Some(config_home.join("skills")),
        project_skills: Some(cwd.join(".claude").join("skills")),
        user_commands: Some(config_home.join("commands")),
        project_commands: Some(cwd.join(".claude").join("commands")),
    });
    // coco-rs extension: `.coco/skills/` as an additional project path.
    manager.load_scoped(&SkillScopes {
        project_skills: Some(cwd.join(".coco").join("skills")),
        ..SkillScopes::default()
    });

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
