//! `/theme` — open the standalone theme picker.
//!
//! TS source: `commands/theme/theme.tsx`. Its `call` is `(onDone, _context)` —
//! it ignores any argument and always renders `<ThemePicker>` (and
//! `commands/theme/index.ts` declares no `argumentHint`). The picker is the
//! only mode; coco-rs mirrors that exactly.

use async_trait::async_trait;

use crate::CommandHandler;
use crate::CommandResult;
use crate::DialogSpec;

pub struct ThemeHandler;

#[async_trait]
impl CommandHandler for ThemeHandler {
    /// Always open the picker overlay. TS ignores any typed argument, so we do
    /// too — the live-preview picker is the single entry point.
    async fn execute_command(&self, _args: &str) -> crate::Result<CommandResult> {
        Ok(CommandResult::OpenDialog(DialogSpec::ThemePicker))
    }

    fn handler_name(&self) -> &str {
        "theme"
    }
}

#[cfg(test)]
#[path = "theme.test.rs"]
mod tests;
