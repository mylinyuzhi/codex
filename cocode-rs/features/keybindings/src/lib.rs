//! Configurable keybinding system.
//!
//! Provides a context-aware, chord-supporting keybinding engine with
//! hot-reload from `~/.cocode/keybindings.json`. Aligns with Claude Code's
//! 18-context, 73-action keybinding architecture.
//!
//! # Usage
//!
//! ```no_run
//! use cocode_keybindings::manager::KeybindingsManager;
//! use cocode_keybindings::context::KeybindingContext;
//! use cocode_keybindings::manager::KeybindingResult;
//! use std::path::PathBuf;
//!
//! let manager = KeybindingsManager::new(PathBuf::from("~/.cocode"), true);
//!
//! // In the event loop:
//! // let result = manager.process_key(&[KeybindingContext::Chat], &key_event);
//! // match result {
//! //     KeybindingResult::Action(action) => { /* map to TuiCommand */ }
//! //     KeybindingResult::PendingChord => { /* show chord indicator */ }
//! //     _ => { /* fall through to raw input */ }
//! // }
//! ```

pub mod action;
pub mod chord;
pub mod config;
pub mod context;
pub mod defaults;
pub mod error;
pub mod key;
pub mod loader;
pub mod manager;
pub mod merge;
pub mod resolver;
pub mod validator;
pub mod watcher;

#[cfg(test)]
pub(crate) mod test_helpers;
