//! `/rewind` command — opens the message-selector overlay.
//!
//! TS source: `commands/rewind/rewind.ts:1-13`.
//!
//! Behavior verbatim from TS:
//! ```ts
//! call: async (_args, context) => {
//!   if (context.openMessageSelector) context.openMessageSelector();
//!   return { type: 'skip' };
//! }
//! ```
//! The overlay (TUI), filter (`selectableUserMessagesFilter`), and file
//! history snapshot/restore live elsewhere — this handler just emits the
//! [`CommandResult::OpenDialog`] event.

use async_trait::async_trait;

use crate::CommandHandler;
use crate::CommandResult;
use crate::DialogSpec;

pub struct RewindHandler;

#[async_trait]
impl CommandHandler for RewindHandler {
    async fn execute_command(&self, _args: &str) -> anyhow::Result<CommandResult> {
        Ok(CommandResult::OpenDialog(DialogSpec::MessageSelector))
    }

    fn handler_name(&self) -> &str {
        "rewind"
    }
}

#[cfg(test)]
#[path = "rewind.test.rs"]
mod tests;
