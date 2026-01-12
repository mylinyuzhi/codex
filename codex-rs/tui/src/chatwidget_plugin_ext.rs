//! Plugin command dispatch helpers for ChatWidget.
//!
//! This module extracts plugin-specific async dispatch logic from chatwidget.rs
//! to minimize upstream merge conflicts per tui/CLAUDE.md guidelines.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use std::path::PathBuf;

/// Handle /plugin command with no arguments (show help).
pub(crate) fn spawn_plugin_help(
    tx: AppEventSender,
    codex_home: PathBuf,
    project_path: Option<PathBuf>,
) {
    tokio::spawn(async move {
        use crate::slash_command_ext::PluginCommandResult;
        use crate::slash_command_ext::PluginManagerContext;
        use crate::slash_command_ext::handle_plugin_command;

        let ctx = PluginManagerContext::new(codex_home, project_path);
        let result = handle_plugin_command("help", &ctx).await;
        let text = match result {
            PluginCommandResult::Help(help) => help,
            _ => "Plugin help not available".to_string(),
        };
        tx.send(AppEvent::PluginResult(text));
    });
}

/// Handle /plugin command with arguments.
pub(crate) fn spawn_plugin_command(
    tx: AppEventSender,
    codex_home: PathBuf,
    project_path: Option<PathBuf>,
    args: String,
) {
    tokio::spawn(async move {
        use crate::slash_command_ext::PluginCommandResult;
        use crate::slash_command_ext::PluginManagerContext;
        use crate::slash_command_ext::format_plugin_list;
        use crate::slash_command_ext::handle_plugin_command;

        let ctx = PluginManagerContext::new(codex_home, project_path);
        let result = handle_plugin_command(&args, &ctx).await;
        let text = match result {
            PluginCommandResult::Success(msg) => msg,
            PluginCommandResult::List(entries) => format_plugin_list(&entries),
            PluginCommandResult::Help(help) => help,
            PluginCommandResult::Error(err) => format!("Error: {err}"),
        };
        tx.send(AppEvent::PluginResult(text));
    });
}

/// Expand and dispatch a plugin command (e.g., /my-plugin:review).
pub(crate) fn spawn_plugin_command_expansion(tx: AppEventSender, name: String, args: String) {
    tokio::spawn(async move {
        match crate::plugin_commands::expand_plugin_command(&name, &args).await {
            Some(expanded) => {
                tx.send(AppEvent::PluginCommandExpanded(expanded));
            }
            None => {
                let text = format!("Plugin command '/{name}' not found or failed to load.");
                tx.send(AppEvent::PluginResult(text));
            }
        }
    });
}
