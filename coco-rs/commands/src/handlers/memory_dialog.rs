//! `/memory` command — open the memory-file selector dialog.
//!
//! TS source: `commands/memory/memory.tsx:1-89` (`local-jsx` command).
//!
//! Behavior:
//! 1. Pre-flight: `clearMemoryFileCaches() + await getMemoryFiles()` to
//!    populate the selector list.
//! 2. Render `<MemoryFileSelector>` — entries listed in TS scope order:
//!    Managed → User → Project → ProjectLocal → Subdirs.
//! 3. On select: caller (TUI) writes the file (mode `wx` semantics) and
//!    opens it in `$VISUAL || $EDITOR`.
//! 4. On cancel: caller emits `Cancelled memory editing` system message.
//!
//! Rust handler scope: enumerate the entries and emit
//! `CommandResult::OpenDialog(DialogSpec::MemoryFileSelector)`. The dialog
//! widget in `coco-tui` handles steps 3–4.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::CommandHandler;
use crate::CommandResult;
use crate::DialogSpec;
use crate::MemoryFileEntry;
use crate::MemoryScope;

pub struct MemoryDialogHandler {
    /// Project root (cwd).
    pub project_root: PathBuf,
    /// User config home (typically `~/.coco`).
    pub user_home: PathBuf,
    /// Optional managed dir (enterprise).
    pub managed_root: Option<PathBuf>,
}

impl MemoryDialogHandler {
    pub fn new(project_root: PathBuf, user_home: PathBuf, managed_root: Option<PathBuf>) -> Self {
        Self {
            project_root,
            user_home,
            managed_root,
        }
    }

    /// Build the entry list in TS-mirroring order.
    pub fn entries(&self) -> Vec<MemoryFileEntry> {
        let mut entries = Vec::new();
        if let Some(m) = &self.managed_root {
            entries.push(MemoryFileEntry {
                path: m.join("CLAUDE.md"),
                label: "Managed (enterprise) CLAUDE.md".into(),
                scope: MemoryScope::Managed,
            });
        }
        entries.push(MemoryFileEntry {
            path: self.user_home.join("CLAUDE.md"),
            label: format!("User-global CLAUDE.md ({})", self.user_home.display()),
            scope: MemoryScope::User,
        });
        entries.push(MemoryFileEntry {
            path: self.project_root.join("CLAUDE.md"),
            label: "Project CLAUDE.md".into(),
            scope: MemoryScope::Project,
        });
        entries.push(MemoryFileEntry {
            path: self.project_root.join("CLAUDE.local.md"),
            label: "Project-local CLAUDE.local.md (gitignored)".into(),
            scope: MemoryScope::ProjectLocal,
        });
        entries
    }
}

#[async_trait]
impl CommandHandler for MemoryDialogHandler {
    async fn execute_command(&self, _args: &str) -> crate::Result<CommandResult> {
        Ok(CommandResult::OpenDialog(DialogSpec::MemoryFileSelector {
            entries: self.entries(),
        }))
    }

    fn handler_name(&self) -> &str {
        "memory"
    }
}

#[cfg(test)]
#[path = "memory_dialog.test.rs"]
mod tests;
