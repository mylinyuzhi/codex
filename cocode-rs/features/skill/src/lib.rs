//! Skill system for cocode-rs.
//!
//! This crate implements a skill loading and management system that supports:
//! - Scanning directories for `SKILL.md` files
//! - Parsing YAML frontmatter and markdown body as prompt content
//! - Validating skill metadata (name, description, prompt)
//! - Bundled skills with SHA-256 fingerprinting
//! - Deduplication of loaded skills by name
//! - Hook registration for skill-scoped hooks
//!
//! # Architecture
//!
//! Skills are discovered from multiple sources (bundled, project-local,
//! user-global, plugin) and loaded through a pipeline:
//!
//! 1. **Scan** - [`scanner::SkillScanner`] discovers skill directories
//! 2. **Load** - [`loader`] reads `SKILL.md`, parses frontmatter + body
//! 3. **Validate** - [`validator`] checks constraints on skill metadata
//! 4. **Dedup** - [`dedup`] removes duplicate skills by name
//! 5. **Hooks** - [`hooks`] registers skill-scoped hooks with the registry

pub mod bundled;
pub mod command;
pub mod dedup;
pub mod frontmatter;
pub mod hooks;
pub mod interface;
pub mod loader;
pub mod local;
pub mod manager;
pub mod outcome;
pub mod scanner;
pub mod source;
pub mod validator;

mod error;

// Re-export primary types
pub use bundled::BundledSkill;
pub use bundled::bundled_skills;
pub use bundled::compute_fingerprint;
pub use command::CommandType;
pub use command::SkillContext;
pub use command::SkillPromptCommand;
pub use command::SlashCommand;
pub use dedup::SkillDeduplicator;
pub use dedup::dedup_skills;
pub use interface::SkillInterface;
pub use loader::load_all_skills;
pub use loader::load_skills_from_dir;
pub use manager::SkillExecutionResult;
pub use manager::SkillLoadResult;
pub use manager::SkillManager;
pub use manager::execute_skill;
pub use manager::parse_skill_command;
pub use outcome::SkillLoadOutcome;
pub use scanner::SkillScanner;
pub use source::LoadedFrom;
pub use source::SkillSource;
pub use validator::validate_skill;

// Re-export local command types
pub use local::LocalCommandDef;
pub use local::builtin_local_commands;
pub use local::find_local_command;

// Re-export hook functionality
pub use hooks::cleanup_skill_hooks;
pub use hooks::convert_skill_hooks;
pub use hooks::register_skill_hooks;

// Re-export the error type
pub use error::Result;
pub use error::SkillError;
