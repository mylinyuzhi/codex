use super::*;

#[test]
fn test_subagent_type_as_str() {
    assert_eq!(SubagentType::Explore.as_str(), "explore");
    assert_eq!(SubagentType::Plan.as_str(), "plan");
    assert_eq!(SubagentType::Bash.as_str(), "bash");
    assert_eq!(SubagentType::General.as_str(), "general");
    assert_eq!(SubagentType::Guide.as_str(), "guide");
    assert_eq!(SubagentType::Statusline.as_str(), "statusline");
    assert_eq!(SubagentType::CodeSimplifier.as_str(), "code-simplifier");
}

#[test]
fn test_subagent_type_all() {
    assert_eq!(SubagentType::ALL.len(), 7);
    assert!(SubagentType::ALL.contains(&SubagentType::Explore));
    assert!(SubagentType::ALL.contains(&SubagentType::Plan));
    assert!(SubagentType::ALL.contains(&SubagentType::Bash));
    assert!(SubagentType::ALL.contains(&SubagentType::General));
    assert!(SubagentType::ALL.contains(&SubagentType::Guide));
    assert!(SubagentType::ALL.contains(&SubagentType::Statusline));
    assert!(SubagentType::ALL.contains(&SubagentType::CodeSimplifier));
}

#[test]
fn test_subagent_type_from_str() {
    assert_eq!(
        SubagentType::from_str("explore"),
        Some(SubagentType::Explore)
    );
    assert_eq!(SubagentType::from_str("plan"), Some(SubagentType::Plan));
    assert_eq!(SubagentType::from_str("bash"), Some(SubagentType::Bash));
    assert_eq!(
        SubagentType::from_str("general"),
        Some(SubagentType::General)
    );
    assert_eq!(SubagentType::from_str("guide"), Some(SubagentType::Guide));
    assert_eq!(
        SubagentType::from_str("statusline"),
        Some(SubagentType::Statusline)
    );
    assert_eq!(
        SubagentType::from_str("code-simplifier"),
        Some(SubagentType::CodeSimplifier)
    );
    assert_eq!(SubagentType::from_str("unknown"), None);
}

#[test]
fn test_subagent_type_display() {
    assert_eq!(format!("{}", SubagentType::Explore), "explore");
    assert_eq!(format!("{}", SubagentType::Plan), "plan");
    assert_eq!(format!("{}", SubagentType::Bash), "bash");
    assert_eq!(format!("{}", SubagentType::General), "general");
    assert_eq!(format!("{}", SubagentType::Guide), "guide");
    assert_eq!(format!("{}", SubagentType::Statusline), "statusline");
    assert_eq!(
        format!("{}", SubagentType::CodeSimplifier),
        "code-simplifier"
    );
}

#[test]
fn test_subagent_type_serde() {
    let explore = SubagentType::Explore;
    let json = serde_json::to_string(&explore).unwrap();
    assert_eq!(json, r#""explore""#);

    let parsed: SubagentType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, SubagentType::Explore);

    // Test kebab-case for CodeSimplifier
    let code_simplifier = SubagentType::CodeSimplifier;
    let json = serde_json::to_string(&code_simplifier).unwrap();
    assert_eq!(json, r#""code-simplifier""#);
}

#[test]
fn test_subagent_type_has_custom_prompt() {
    assert!(SubagentType::Explore.has_custom_prompt());
    assert!(SubagentType::Plan.has_custom_prompt());
    assert!(!SubagentType::Bash.has_custom_prompt());
    assert!(!SubagentType::General.has_custom_prompt());
    assert!(!SubagentType::Guide.has_custom_prompt());
    assert!(!SubagentType::Statusline.has_custom_prompt());
    assert!(!SubagentType::CodeSimplifier.has_custom_prompt());
}
