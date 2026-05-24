//! Single source of truth for TS-equivalent project, session, and
//! memory path layout.
//!
//! Why this crate exists: the pre-fix coco-rs codebase duplicated the
//! `sanitizePath` algorithm across `memory`, `core/context`, `tasks`,
//! and `plugins`, with at least one variant producing slugs that did
//! NOT match TS Claude Code's. The result was silent cross-tool data
//! isolation breakage — a session run under TS and one under coco-rs
//! against the same cwd would land in different `projects/<slug>/`
//! directories, with no error surfaced.
//!
//! TS source files this crate mirrors (consolidated here so callers
//! don't need to track them individually):
//!
//! - `utils/sessionStoragePortable.ts:311-319` `sanitizePath`
//! - `utils/sessionStoragePortable.ts:295-297` `simpleHash` (djb2)
//! - `utils/sessionStoragePortable.ts:325-360` `getProjectsDir` / `getProjectDir`
//! - `utils/sessionStoragePortable.ts:354-380` `findProjectDir` (prefix fallback)
//! - `utils/sessionStoragePortable.ts:403-466` `resolveSessionFilePath`
//! - `memdir/paths.ts:85-90,203-235,246-251` memory base / project / daily-log
//! - `tools/AgentTool/agentMemory.ts` `sanitizeAgentTypeForPath`
//!
//! The crate is dependency-light on purpose: only `unicode-normalization`
//! at runtime — no env var reads, no subprocesses, no filesystem walks
//! beyond what `find_project_dir`'s prefix scan requires. Git operations
//! (worktree enumeration, canonical-root resolution) live in
//! [`coco_git`](https://docs.rs/coco-git); callers compose the two
//! crates explicitly so the path layer stays platform-neutral.

pub mod djb2;
pub mod nfc;
pub mod project_paths;
pub mod projects_root;
pub mod relative;
pub mod sanitize;
pub mod slug;

pub use djb2::{djb2, simple_hash};
pub use nfc::normalize_nfc;
pub use project_paths::ProjectPaths;
pub use projects_root::{find_project_dir, project_dir, projects_root};
pub use relative::{normalize_lexical, path_to_posix, relative_posix_path};
pub use sanitize::{MAX_SANITIZED_LENGTH, sanitize_agent_type_for_path, sanitize_path};
pub use slug::ProjectSlug;
