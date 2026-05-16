//! `/help` slash-command output renderer.
//!
//! Builds the markdown response shown when the user types `/help` in the
//! composer. Two data sources feed this view:
//!
//! - **`crate::keymap::KEYMAP`** — the structured source of truth for
//!   shortcuts, prompt prefixes, and vim normal-mode keys. Rendered via
//!   `keymap::export_markdown` so a subagent calling the same export
//!   sees the identical view.
//! - The slash-command catalog ([`CATEGORIES`] below) — slash-command
//!   names, aliases, and usage hints. Stays here because the data shape
//!   (`name`/`aliases`/`usage`) doesn't overlap with the keymap entry
//!   shape and would dilute the keymap if forced into the same table.

use crate::i18n::t;
use crate::keymap;

/// Render `/help` (no argument) — full overview of commands, prompt
/// prefixes, keyboard shortcuts, and vim mode.
pub(crate) fn render_overview() -> String {
    let mut out = String::new();
    out.push_str(&format!("## {}\n\n", t!("help.slash.title")));
    out.push_str(&format!("{}\n\n", t!("help.slash.tagline")));

    out.push_str(&format!("### {}\n\n", t!("help.slash.section.commands")));
    for category in CATEGORIES {
        let cat_key = format!("help.slash.cat.{}", category.key);
        out.push_str(&format!("**{}**\n\n", t!(cat_key.as_str())));
        for cmd in category.commands {
            let desc_key = format!("help.slash.cmd.{}", cmd.name);
            let desc = t!(desc_key.as_str()).to_string();
            let aliases = if cmd.aliases.is_empty() {
                String::new()
            } else {
                format!(
                    " _({})_",
                    cmd.aliases
                        .iter()
                        .map(|a| format!("/{a}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            out.push_str(&format!("- `/{}` — {desc}{aliases}\n", cmd.name));
        }
        out.push('\n');
    }

    // Keyboard shortcuts + prompt prefixes + vim mode all come from the
    // structured `keymap` module. A subagent invoking
    // `keymap::export_markdown` sees the identical rendering.
    out.push_str(&format!("### {}\n\n", t!("help.slash.section.shortcuts")));
    out.push_str(&keymap::export_markdown());

    out.push_str(&format!("_{}_\n", t!("help.slash.footer.hint")));

    out
}

/// Render `/help <command>` — details for a single command. Returns `None`
/// if no command matches.
pub(crate) fn render_command_detail(query: &str) -> Option<String> {
    let name = query.trim_start_matches('/');
    for category in CATEGORIES {
        for cmd in category.commands {
            if cmd.name == name || cmd.aliases.contains(&name) {
                let mut out = format!("## /{}\n\n", cmd.name);
                let desc_key = format!("help.slash.cmd.{}", cmd.name);
                out.push_str(&format!("{}\n\n", t!(desc_key.as_str())));
                out.push_str(&format!(
                    "**{}** `{}`\n",
                    t!("help.slash.field.usage"),
                    cmd.usage
                ));
                if !cmd.aliases.is_empty() {
                    let aliases = cmd
                        .aliases
                        .iter()
                        .map(|a| format!("/{a}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "**{}** {aliases}\n",
                        t!("help.slash.field.aliases")
                    ));
                }
                return Some(out);
            }
        }
    }
    None
}

/// Render the "command not found" message.
pub(crate) fn render_not_found(name: &str) -> String {
    t!("help.slash.not_found", name = name).to_string()
}

// ────────────────────────── Slash-command catalog ──────────────────────────

struct Category {
    /// Stable identifier used to look up the localized category name.
    /// Lives at `help.slash.cat.<key>` in the locale YAML.
    key: &'static str,
    commands: &'static [CommandEntry],
}

struct CommandEntry {
    /// Slash command name without the leading `/`.
    name: &'static str,
    aliases: &'static [&'static str],
    /// Usage hint (stays as a single literal — argument names are not
    /// localized to match TS conventions where flags / params are English).
    usage: &'static str,
}

const CATEGORIES: &[Category] = &[
    Category {
        key: "core",
        commands: &[
            CommandEntry {
                name: "help",
                aliases: &["h", "?"],
                usage: "/help [command]",
            },
            CommandEntry {
                name: "clear",
                aliases: &["reset", "new"],
                usage: "/clear",
            },
            CommandEntry {
                name: "compact",
                aliases: &[],
                usage: "/compact [instructions]",
            },
            CommandEntry {
                name: "status",
                aliases: &["st"],
                usage: "/status",
            },
            CommandEntry {
                name: "exit",
                aliases: &["quit"],
                usage: "/exit",
            },
            CommandEntry {
                name: "version",
                aliases: &[],
                usage: "/version",
            },
        ],
    },
    Category {
        key: "config",
        commands: &[
            CommandEntry {
                name: "config",
                aliases: &["configuration"],
                usage: "/config [key] [value]",
            },
            CommandEntry {
                name: "model",
                aliases: &[],
                usage: "/model [model]",
            },
            CommandEntry {
                name: "effort",
                aliases: &[],
                usage: "/effort [low|medium|high]",
            },
            CommandEntry {
                name: "permissions",
                aliases: &["perms", "allowed-tools"],
                usage: "/permissions [allow|deny] [tool]",
            },
            CommandEntry {
                name: "theme",
                aliases: &[],
                usage: "/theme [name]",
            },
            CommandEntry {
                name: "vim",
                aliases: &[],
                usage: "/vim [on|off|toggle]",
            },
            CommandEntry {
                name: "keybindings",
                aliases: &[],
                usage: "/keybindings",
            },
            CommandEntry {
                name: "output-style",
                aliases: &[],
                usage: "/output-style [name]",
            },
            CommandEntry {
                name: "sandbox",
                aliases: &[],
                usage: "/sandbox [on|off|status]",
            },
            CommandEntry {
                name: "fast",
                aliases: &[],
                usage: "/fast",
            },
        ],
    },
    Category {
        key: "session",
        commands: &[
            CommandEntry {
                name: "cost",
                aliases: &[],
                usage: "/cost",
            },
            CommandEntry {
                name: "context",
                aliases: &["ctx"],
                usage: "/context",
            },
            CommandEntry {
                name: "resume",
                aliases: &["continue"],
                usage: "/resume [session-id]",
            },
            CommandEntry {
                name: "session",
                aliases: &["remote"],
                usage: "/session [list|delete|info] [id]",
            },
            CommandEntry {
                name: "rewind",
                aliases: &["checkpoint"],
                usage: "/rewind",
            },
            CommandEntry {
                name: "copy",
                aliases: &[],
                usage: "/copy",
            },
            CommandEntry {
                name: "export",
                aliases: &[],
                usage: "/export [filename]",
            },
            CommandEntry {
                name: "usage",
                aliases: &[],
                usage: "/usage",
            },
        ],
    },
    Category {
        key: "dev",
        commands: &[
            CommandEntry {
                name: "diff",
                aliases: &[],
                usage: "/diff",
            },
            CommandEntry {
                name: "commit",
                aliases: &[],
                usage: "/commit [message]",
            },
            CommandEntry {
                name: "pr",
                aliases: &["pr-create"],
                usage: "/pr [title]",
            },
            CommandEntry {
                name: "review",
                aliases: &[],
                usage: "/review [PR number]",
            },
            CommandEntry {
                name: "init",
                aliases: &[],
                usage: "/init",
            },
            CommandEntry {
                name: "add-dir",
                aliases: &[],
                usage: "/add-dir <path>",
            },
            CommandEntry {
                name: "doctor",
                aliases: &[],
                usage: "/doctor",
            },
        ],
    },
    Category {
        key: "tools",
        commands: &[
            CommandEntry {
                name: "mcp",
                aliases: &[],
                usage: "/mcp [list|add|remove|enable|disable] [name]",
            },
            CommandEntry {
                name: "plugin",
                aliases: &["plugins", "marketplace"],
                usage: "/plugin [list|install|uninstall] [name]",
            },
            CommandEntry {
                name: "agents",
                aliases: &[],
                usage: "/agents",
            },
            CommandEntry {
                name: "tasks",
                aliases: &["todo"],
                usage: "/tasks",
            },
            CommandEntry {
                name: "skills",
                aliases: &[],
                usage: "/skills",
            },
            CommandEntry {
                name: "hooks",
                aliases: &[],
                usage: "/hooks",
            },
            CommandEntry {
                name: "memory",
                aliases: &[],
                usage: "/memory [edit|refresh]",
            },
            CommandEntry {
                name: "plan",
                aliases: &["planning"],
                usage: "/plan [open|<description>]",
            },
        ],
    },
];

#[cfg(test)]
#[path = "help_slash.test.rs"]
mod tests;
