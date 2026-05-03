//! Migration helper for commands moved into plugins.
//!
//! TS source: `commands/createMovedToPluginCommand.ts:22-65`.
//!
//! When a command is migrated from a built-in to a plugin, the slash command
//! is kept around as a stub that tells the user how to install the plugin.
//! For ant users, the stub returns a fixed instruction prompt; otherwise it
//! falls back to the original prompt body (until the marketplace is public).

use async_trait::async_trait;
use coco_types::UserType;

use crate::CommandHandler;
use crate::CommandResult;
use crate::PromptPart;

/// A migrated-to-plugin command stub.
///
/// TS: `createMovedToPluginCommand({name, description, progressMessage,
/// pluginName, pluginCommand, getPromptWhileMarketplaceIsPrivate})`.
pub struct MovedToPluginCommand {
    pub name: String,
    pub description: String,
    pub progress_message: String,
    pub plugin_name: String,
    pub plugin_command: String,
    pub user_type: UserType,
    /// Verbatim original prompt body — used while the marketplace is private.
    pub original_body: String,
}

impl MovedToPluginCommand {
    /// Build the ant-only "moved to plugin" instruction prompt.
    /// Mirrors TS `createMovedToPluginCommand.ts:46-58` exactly.
    pub fn instruction_prompt(&self) -> String {
        format!(
            "This command has been moved to a plugin. Tell the user:\n\n\
             1. To install the plugin, run:\n   \
             claude plugin install {plugin}@claude-code-marketplace\n\n\
             2. After installation, use /{plugin}:{cmd} to run this command\n\n\
             3. For more information, see: https://github.com/anthropics/claude-code-marketplace/blob/main/{plugin}/README.md\n\n\
             Do not attempt to run the command. Simply inform the user about the plugin installation.",
            plugin = self.plugin_name,
            cmd = self.plugin_command
        )
    }
}

#[async_trait]
impl CommandHandler for MovedToPluginCommand {
    async fn execute_command(&self, _args: &str) -> anyhow::Result<CommandResult> {
        let text = if self.user_type.is_ant() {
            self.instruction_prompt()
        } else {
            self.original_body.clone()
        };
        Ok(CommandResult::Prompt {
            progress_message: self.progress_message.clone(),
            parts: vec![PromptPart::Text { text }],
        })
    }

    fn handler_name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
#[path = "migration.test.rs"]
mod tests;
