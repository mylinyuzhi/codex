use coco_types::{AgentDefinition, AgentTypeId, LlmModelSelection, ModelRole, SubagentType};
use pretty_assertions::assert_eq;

use super::{SubagentSelection, resolve_subagent_selection};

#[test]
fn test_request_model_wins_over_definition_model() {
    let def = AgentDefinition {
        model: Some("anthropic/claude-haiku-4-5".into()),
        ..Default::default()
    };
    let id = AgentTypeId::Builtin(SubagentType::Explore);
    let sel =
        resolve_subagent_selection(Some("anthropic/claude-opus-4-7"), None, Some(&def), Some(&id));
    assert_eq!(
        sel,
        SubagentSelection {
            model: Some("anthropic/claude-opus-4-7".into()),
            model_role: ModelRole::Explore,
            model_selection: LlmModelSelection::ExplicitWithFallbackRole {
                primary: coco_types::ProviderModelSelection {
                    provider: "anthropic".into(),
                    model_id: "claude-opus-4-7".into(),
                },
                fallback_role: ModelRole::Explore,
            },
        }
    );
}

#[test]
fn test_definition_model_used_when_request_omits() {
    let def = AgentDefinition {
        model: Some("anthropic/claude-haiku-4-5".into()),
        ..Default::default()
    };
    let id = AgentTypeId::Builtin(SubagentType::Explore);
    let sel = resolve_subagent_selection(None, None, Some(&def), Some(&id));
    assert_eq!(
        sel,
        SubagentSelection {
            model: Some("anthropic/claude-haiku-4-5".into()),
            model_role: ModelRole::Explore,
            model_selection: LlmModelSelection::ExplicitWithFallbackRole {
                primary: coco_types::ProviderModelSelection {
                    provider: "anthropic".into(),
                    model_id: "claude-haiku-4-5".into(),
                },
                fallback_role: ModelRole::Explore,
            },
        }
    );
}

#[test]
fn test_no_model_override_returns_none_for_role_fallback() {
    let id = AgentTypeId::Builtin(SubagentType::Plan);
    let sel = resolve_subagent_selection(None, None, None, Some(&id));
    assert_eq!(
        sel,
        SubagentSelection {
            model: None,
            model_role: ModelRole::Plan,
            model_selection: LlmModelSelection::Role {
                role: ModelRole::Plan,
            },
        }
    );
}

#[test]
fn test_definition_model_role_wins_over_subagent_type_mapping() {
    let def = AgentDefinition {
        model_role: Some(ModelRole::Fast),
        ..Default::default()
    };
    let id = AgentTypeId::Builtin(SubagentType::Explore);
    let sel = resolve_subagent_selection(None, None, Some(&def), Some(&id));
    // Explore would normally map to Explore; the definition's Fast wins.
    assert_eq!(sel.model_role, ModelRole::Fast);
    assert!(sel.model.is_none());
}

#[test]
fn test_inherit_model_string_is_passed_through_verbatim() {
    // The frontmatter parser normalizes `inherit` to a literal lowercase
    // `"inherit"` string. The resolver doesn't try to interpret it —
    // downstream (engine factory) decides what `inherit` means
    // (currently: use the parent's main loop model). The resolver just
    // forwards.
    let def = AgentDefinition {
        model: Some("inherit".into()),
        ..Default::default()
    };
    let id = AgentTypeId::Builtin(SubagentType::Plan);
    let sel = resolve_subagent_selection(None, None, Some(&def), Some(&id));
    assert_eq!(sel.model.as_deref(), Some("inherit"));
    assert_eq!(sel.model_role, ModelRole::Plan);
}

#[test]
fn test_custom_agent_without_definition_falls_back_to_subagent_role() {
    let id = AgentTypeId::Custom("research-bot".into());
    let sel = resolve_subagent_selection(None, None, None, Some(&id));
    assert_eq!(sel.model_role, ModelRole::Subagent);
    assert!(sel.model.is_none());
}

#[test]
fn test_no_inputs_at_all_defaults_to_subagent_role() {
    let sel = resolve_subagent_selection(None, None, None, None);
    assert_eq!(
        sel,
        SubagentSelection {
            model: None,
            model_role: ModelRole::Subagent,
            model_selection: LlmModelSelection::Role {
                role: ModelRole::Subagent,
            },
        }
    );
}

#[test]
fn test_verification_subagent_maps_to_review_role() {
    let id = AgentTypeId::Builtin(SubagentType::Verification);
    let sel = resolve_subagent_selection(None, None, None, Some(&id));
    assert_eq!(sel.model_role, ModelRole::Review);
}

#[test]
fn test_general_purpose_falls_back_to_subagent_role() {
    let id = AgentTypeId::Builtin(SubagentType::GeneralPurpose);
    let sel = resolve_subagent_selection(None, None, None, Some(&id));
    assert_eq!(sel.model_role, ModelRole::Subagent);
}

#[test]
fn test_request_model_role_overrides_definition_and_subagent_type() {
    // Memory forks use general-purpose subagent_type (which maps to
    // ModelRole::Subagent) but pin model_role to Memory via the
    // request. The request_model_role must win over both the
    // definition's declared role AND the subagent_type mapping.
    let def = AgentDefinition {
        model_role: Some(ModelRole::Fast),
        ..Default::default()
    };
    let id = AgentTypeId::Builtin(SubagentType::GeneralPurpose);
    let sel = resolve_subagent_selection(None, Some(ModelRole::Memory), Some(&def), Some(&id));
    assert_eq!(sel.model_role, ModelRole::Memory);
}

#[test]
fn test_request_model_role_pins_role_without_definition() {
    // The common case for memory forks: no definition installed,
    // general-purpose subagent_type, request pins Memory.
    let id = AgentTypeId::Builtin(SubagentType::GeneralPurpose);
    let sel = resolve_subagent_selection(None, Some(ModelRole::Memory), None, Some(&id));
    assert_eq!(sel.model_role, ModelRole::Memory);
    assert!(sel.model.is_none());
}
