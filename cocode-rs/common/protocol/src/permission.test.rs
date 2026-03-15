use super::*;

#[test]
fn test_permission_mode_default() {
    assert_eq!(PermissionMode::default(), PermissionMode::Default);
}

#[test]
fn test_permission_mode_methods() {
    assert!(PermissionMode::Default.requires_write_approval());
    assert!(PermissionMode::Plan.requires_write_approval());
    assert!(!PermissionMode::AcceptEdits.requires_write_approval());
    assert!(!PermissionMode::Bypass.requires_write_approval());

    assert!(!PermissionMode::Default.auto_accept_edits());
    assert!(PermissionMode::AcceptEdits.auto_accept_edits());
    assert!(PermissionMode::Bypass.auto_accept_edits());

    assert!(!PermissionMode::Default.is_bypass());
    assert!(PermissionMode::Bypass.is_bypass());
}

#[test]
fn test_permission_behavior_default() {
    assert_eq!(PermissionBehavior::default(), PermissionBehavior::Ask);
}

#[test]
fn test_permission_behavior_methods() {
    assert!(PermissionBehavior::Allow.is_allowed());
    assert!(!PermissionBehavior::Ask.is_allowed());
    assert!(!PermissionBehavior::Deny.is_allowed());

    assert!(!PermissionBehavior::Allow.requires_approval());
    assert!(PermissionBehavior::Ask.requires_approval());
    assert!(!PermissionBehavior::Deny.requires_approval());

    assert!(!PermissionBehavior::Allow.is_denied());
    assert!(!PermissionBehavior::Ask.is_denied());
    assert!(PermissionBehavior::Deny.is_denied());
}

#[test]
fn test_permission_result_methods() {
    assert!(PermissionResult::Allowed.is_allowed());
    assert!(!PermissionResult::Allowed.is_denied());
    assert!(!PermissionResult::Allowed.needs_approval());
    assert!(!PermissionResult::Allowed.is_passthrough());

    let denied = PermissionResult::Denied {
        reason: "test".to_string(),
    };
    assert!(!denied.is_allowed());
    assert!(denied.is_denied());
    assert!(!denied.needs_approval());

    let needs_approval = PermissionResult::NeedsApproval {
        request: ApprovalRequest {
            request_id: "1".to_string(),
            tool_name: "test".to_string(),
            description: "test".to_string(),
            risks: vec![],
            allow_remember: false,
            proposed_prefix_pattern: None,
        },
    };
    assert!(!needs_approval.is_allowed());
    assert!(!needs_approval.is_denied());
    assert!(needs_approval.needs_approval());

    assert!(PermissionResult::Passthrough.is_passthrough());
    assert!(!PermissionResult::Passthrough.is_allowed());
}

#[test]
fn test_risk_severity_ordering() {
    assert!(RiskSeverity::Low < RiskSeverity::Medium);
    assert!(RiskSeverity::Medium < RiskSeverity::High);
    assert!(RiskSeverity::High < RiskSeverity::Critical);

    assert!(RiskSeverity::Critical.at_least(RiskSeverity::Low));
    assert!(RiskSeverity::Medium.at_least(RiskSeverity::Medium));
    assert!(!RiskSeverity::Low.at_least(RiskSeverity::High));
}

#[test]
fn test_permission_decision_constructors() {
    let allowed = PermissionDecision::allowed("bypass mode");
    assert!(allowed.is_allowed());
    assert_eq!(allowed.reason, "bypass mode");

    let denied = PermissionDecision::denied("read-only command");
    assert!(!denied.is_allowed());
}

#[test]
fn test_permission_decision_with_source() {
    let decision = PermissionDecision::allowed("matched rule")
        .with_source(RuleSource::Project)
        .with_pattern("Edit:src/**/*.rs");
    assert_eq!(decision.source, Some(RuleSource::Project));
    assert_eq!(
        decision.matched_pattern.as_deref(),
        Some("Edit:src/**/*.rs")
    );
}

#[test]
fn test_rule_source_ordering() {
    // Session has highest priority (smallest value)
    assert!(RuleSource::Session < RuleSource::Command);
    assert!(RuleSource::Command < RuleSource::Cli);
    assert!(RuleSource::Cli < RuleSource::Flag);
    assert!(RuleSource::Flag < RuleSource::Local);
    assert!(RuleSource::Local < RuleSource::Project);
    assert!(RuleSource::Project < RuleSource::Policy);
    assert!(RuleSource::Policy < RuleSource::User);
}

#[test]
fn test_permission_decision_serde() {
    let decision = PermissionDecision::allowed("test reason").with_source(RuleSource::Project);
    let json = serde_json::to_string(&decision).unwrap();
    let parsed: PermissionDecision = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_allowed());
    assert_eq!(parsed.source, Some(RuleSource::Project));
}

#[test]
fn test_serde_roundtrip() {
    let mode = PermissionMode::AcceptEdits;
    let json = serde_json::to_string(&mode).unwrap();
    let parsed: PermissionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, mode);

    let behavior = PermissionBehavior::Allow;
    let json = serde_json::to_string(&behavior).unwrap();
    let parsed: PermissionBehavior = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, behavior);

    let risk = SecurityRisk {
        risk_type: RiskType::Destructive,
        severity: RiskSeverity::High,
        message: "May delete files".to_string(),
    };
    let json = serde_json::to_string(&risk).unwrap();
    let parsed: SecurityRisk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, risk);
}

#[test]
fn test_approval_decision_serde_roundtrip() {
    // Approved
    let decision = ApprovalDecision::Approved;
    let json = serde_json::to_string(&decision).unwrap();
    let parsed: ApprovalDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ApprovalDecision::Approved);

    // ApprovedWithPrefix
    let decision = ApprovalDecision::ApprovedWithPrefix {
        prefix_pattern: "git *".to_string(),
    };
    let json = serde_json::to_string(&decision).unwrap();
    assert!(json.contains("approved-with-prefix"));
    assert!(json.contains("git *"));
    let parsed: ApprovalDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, decision);

    // Denied
    let decision = ApprovalDecision::Denied;
    let json = serde_json::to_string(&decision).unwrap();
    let parsed: ApprovalDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ApprovalDecision::Denied);
}

#[test]
fn test_approval_request_proposed_prefix_pattern() {
    // Without prefix pattern (backward compat)
    let request = ApprovalRequest {
        request_id: "1".to_string(),
        tool_name: "Bash".to_string(),
        description: "git push".to_string(),
        risks: vec![],
        allow_remember: true,
        proposed_prefix_pattern: None,
    };
    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("proposed_prefix_pattern"));

    // With prefix pattern
    let request = ApprovalRequest {
        request_id: "2".to_string(),
        tool_name: "Bash".to_string(),
        description: "git push origin main".to_string(),
        risks: vec![],
        allow_remember: true,
        proposed_prefix_pattern: Some("git *".to_string()),
    };
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("proposed_prefix_pattern"));
    let parsed: ApprovalRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.proposed_prefix_pattern, Some("git *".to_string()));
}
