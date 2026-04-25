//! Pure-logic subagent rules: definition loading, source precedence, built-in
//! catalog, AgentTool prompt rendering, tool filter planning, validation.
//!
//! TS: `tools/AgentTool/loadAgentsDir.ts`, `builtInAgents.ts`,
//! `prompt.ts`, `agentToolUtils.ts`, `built-in/*.ts`.
//!
//! This crate has no tokio, no app state, no QueryEngine. All side effects
//! are synchronous `std::fs` reads triggered by an explicit `load()` call.
//! See `docs/coco-rs/subagent-refactor-plan.md` § D8 for ownership boundary.

pub mod builtins;
pub mod definition_store;
pub mod filter;
pub mod frontmatter;
pub mod prompt;
pub mod snapshot;
pub mod validation;

pub use builtins::{BuiltinAgentCatalog, builtin_definition, builtin_definitions};
pub use definition_store::{AgentDefinitionStore, AgentLoadReport, LoadedAgentDefinition};
pub use filter::{AgentToolFilter, AllowedAgentTypes, ToolFilterPlan, parse_allowed_agent_types};
pub use frontmatter::{
    FrontmatterParseError, parse_agent_markdown, parse_color_value, parse_isolation_value,
    parse_memory_value,
};
pub use prompt::{AgentToolPromptRenderer, PromptOptions, format_tools_description};
pub use snapshot::AgentCatalogSnapshot;
pub use validation::{AgentDefinitionValidator, ValidationDiagnostic, ValidationError};

/// One-shot built-in agent types — TS `ONE_SHOT_BUILTIN_AGENT_TYPES`
/// (`constants.ts:9-12`). **Case-sensitive** — `"explore"`/`"plan"` do not
/// hit. The set short-circuits the SendMessage continuation trailer in
/// AgentTool result rendering.
pub const ONE_SHOT_BUILTIN_AGENT_TYPES: &[&str] = &["Explore", "Plan"];

/// Empty-content marker injected by AgentTool when the subagent returned
/// no text. **Exact** TS literal (`AgentTool.tsx:1347-1350`).
pub const EMPTY_AGENT_OUTPUT_MARKER: &str = "(Subagent completed but returned no output.)";

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
