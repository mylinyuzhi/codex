//! LLM-based safety checks for shell commands.
//!
//! Provides [`PrefixExtractor`] which uses a fast LLM call to extract the
//! semantic "command prefix" from a shell command. The prefix is used for
//! permission rule matching — e.g., `"git diff HEAD~1"` → prefix `"git diff"`,
//! which matches the rule `"Bash(git *)"`.
//!
//! This aligns with Claude Code's `bashPreFlightCheck` / `extractPrefixCached`
//! (Layer 2 of the shell security pipeline).
//!
//! # Architecture
//!
//! - Defines its own `LlmCallFn` callback type to avoid depending on `core/tools`
//! - Uses LRU cache to avoid redundant LLM calls for repeated commands
//! - Feature-gated: only active when explicitly enabled
//! - Timeout: 10 seconds per extraction (matching CC)

pub mod prefix_extractor;

pub use prefix_extractor::PrefixExtractor;
pub use prefix_extractor::PrefixResult;
