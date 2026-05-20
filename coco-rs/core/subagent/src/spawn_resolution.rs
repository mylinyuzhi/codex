//! Spawn-time subagent identity resolution.
//!
//! Combines the four inputs that determine a subagent's model identity
//! into a single typed [`SubagentSelection`]. Pure logic — no IO, no
//! tokio. Callers (coordinator's `SwarmAgentHandle`) read the selection
//! and either thread the explicit `model` override into the child's
//! `AgentQueryConfig` or hand the role to the engine factory's
//! `ModelRoles::get` lookup.
//!
//! ## Precedence
//!
//! ```text
//! model:  request_model > definition.model > None  (caller substitutes role-resolved spec)
//! role:   request_model_role > definition.model_role > role_for_builtin(SubagentType) > ModelRole::Subagent
//! ```
//!
//! - **`request_model`** — the AgentTool input field, e.g.
//!   `Agent({ model: "provider/model-id", … })`. Wins when set.
//! - **`definition.model`** — the agent `.md` frontmatter `model:` field.
//!   Used only when the AgentTool input omits `model`. Already
//!   normalized to lowercase / `"inherit"` by the frontmatter parser.
//! - **`request_model_role`** — explicit role pin on the spawn request
//!   itself. Memory-crate forks (extract / dream / session-memory) set
//!   this to `ModelRole::Memory` so the operator can swap provider+model
//!   via `settings.models.memory` without touching memory crate code.
//!   Wins over both definition and built-in mapping.
//! - **`definition.model_role`** — explicit role declaration on the
//!   agent definition (the `.md` frontmatter `model_role:` field).
//!   Wins over the `subagent_type` mapping.
//! - **`subagent_type`** — the built-in classifier. Maps to a role via
//!   [`crate::subagent_role::role_for_builtin`].
//!
//! The `model` override and `model_role` are independent: a subagent
//! can pin a concrete model id while still carrying its semantic role
//! (used for fallback chains, recovery policy, telemetry).

use coco_types::{AgentDefinition, AgentTypeId, LlmModelSelection, ModelRole};

use crate::subagent_role::resolve_subagent_role;

/// Resolved spawn-time identity for a subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentSelection {
    /// Explicit model override — the AgentTool `model` parameter or the
    /// agent definition's `model` frontmatter field. `None` means
    /// "no override; use whatever the role's primary `ModelSpec`
    /// resolves to".
    pub model: Option<String>,
    /// Resolved `ModelRole`. Falls back to `Subagent` for custom agents
    /// without a `model_role` declaration.
    pub model_role: ModelRole,
    /// Unified model routing selection for the execution factory.
    pub model_selection: LlmModelSelection,
}

/// Pure resolver — see module doc for precedence rules.
pub fn resolve_subagent_selection(
    request_model: Option<&str>,
    request_model_role: Option<ModelRole>,
    definition: Option<&AgentDefinition>,
    subagent_type: Option<&AgentTypeId>,
) -> SubagentSelection {
    let model = request_model
        .map(str::to_owned)
        .or_else(|| definition.and_then(|d| d.model.clone()));
    // Explicit request_model_role wins (memory forks pin to Memory);
    // otherwise fall through to definition / subagent_type mapping.
    let model_role =
        request_model_role.unwrap_or_else(|| resolve_subagent_role(definition, subagent_type));
    let model_selection =
        LlmModelSelection::from_model_and_role(model.as_deref(), Some(model_role));
    SubagentSelection {
        model,
        model_role,
        model_selection,
    }
}

#[cfg(test)]
#[path = "spawn_resolution.test.rs"]
mod tests;
