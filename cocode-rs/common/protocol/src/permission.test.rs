use super::*;
use crate::ToolName;

#[test]
fn test_permission_mode_default() {
    assert_eq!(PermissionMode::default(), PermissionMode::Default);
}

#[test]
fn test_permission_mode_methods() {
    assert!(PermissionMode::Default.requires_write_approval());
    assert!(PermissionMode::Plan.requires_write_approval());
    assert!(!PermissionMode::AcceptEdits.requires_write_approval());
    assert!(!PermissionMode::Auto.requires_write_approval());
    assert!(!PermissionMode::Bypass.requires_write_approval());

    assert!(!PermissionMode::Default.auto_accept_edits());
    assert!(PermissionMode::AcceptEdits.auto_accept_edits());
    assert!(PermissionMode::Auto.auto_accept_edits());
    assert!(PermissionMode::Bypass.auto_accept_edits());

    assert!(!PermissionMode::Default.is_bypass());
    assert!(PermissionMode::Auto.is_bypass());
    assert!(PermissionMode::Bypass.is_bypass());
}

#[test]
fn test_permission_mode_base_cycle() {
    assert_eq!(
        PermissionMode::Default.next_cycle(),
        PermissionMode::AcceptEdits
    );
    assert_eq!(
        PermissionMode::AcceptEdits.next_cycle(),
        PermissionMode::Plan
    );
    assert_eq!(PermissionMode::Plan.next_cycle(), PermissionMode::Default);
    // Sticky modes don't cycle
    assert_eq!(PermissionMode::Bypass.next_cycle(), PermissionMode::Bypass);
    assert_eq!(PermissionMode::Auto.next_cycle(), PermissionMode::Auto);
    assert_eq!(
        PermissionMode::DontAsk.next_cycle(),
        PermissionMode::DontAsk
    );
}

#[test]
fn test_permission_mode_gated_cycle_with_auto() {
    // CC: default → acceptEdits → plan → auto → default
    assert_eq!(
        PermissionMode::Plan.next_cycle_with_gates(/*bypass*/ false, /*auto*/ true),
        PermissionMode::Auto
    );
    assert_eq!(
        PermissionMode::Auto.next_cycle_with_gates(false, true),
        PermissionMode::Default
    );
}

#[test]
fn test_permission_mode_gated_cycle_with_bypass_and_auto() {
    // CC: default → acceptEdits → plan → bypass → auto → default
    assert_eq!(
        PermissionMode::Plan.next_cycle_with_gates(/*bypass*/ true, /*auto*/ true),
        PermissionMode::Bypass
    );
    assert_eq!(
        PermissionMode::Bypass.next_cycle_with_gates(true, true),
        PermissionMode::Auto
    );
    assert_eq!(
        PermissionMode::Auto.next_cycle_with_gates(true, true),
        PermissionMode::Default
    );
}

#[test]
fn test_permission_mode_from_str_auto() {
    assert_eq!(
        "auto".parse::<PermissionMode>().unwrap(),
        PermissionMode::Auto
    );
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
            input: None,
            source_agent_id: None,
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
    // Persistent sources have highest priority (smallest value, checked first).
    // This ensures configured deny rules can't be bypassed by session approvals.
    assert!(RuleSource::User < RuleSource::Project);
    assert!(RuleSource::Project < RuleSource::Local);
    assert!(RuleSource::Local < RuleSource::Flag);
    assert!(RuleSource::Flag < RuleSource::Policy);
    assert!(RuleSource::Policy < RuleSource::Cli);
    assert!(RuleSource::Cli < RuleSource::Command);
    assert!(RuleSource::Command < RuleSource::Session);
}

#[test]
fn test_rule_source_is_persistent() {
    assert!(RuleSource::User.is_persistent());
    assert!(RuleSource::Project.is_persistent());
    assert!(RuleSource::Local.is_persistent());
    assert!(RuleSource::Policy.is_persistent());
    assert!(!RuleSource::Flag.is_persistent());
    assert!(!RuleSource::Cli.is_persistent());
    assert!(!RuleSource::Command.is_persistent());
    assert!(!RuleSource::Session.is_persistent());
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
        tool_name: ToolName::Bash.as_str().to_string(),
        description: "git push".to_string(),
        risks: vec![],
        allow_remember: true,
        proposed_prefix_pattern: None,
        input: None,
        source_agent_id: None,
    };
    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("proposed_prefix_pattern"));

    // With prefix pattern
    let request = ApprovalRequest {
        request_id: "2".to_string(),
        tool_name: ToolName::Bash.as_str().to_string(),
        description: "git push origin main".to_string(),
        risks: vec![],
        allow_remember: true,
        proposed_prefix_pattern: Some("git *".to_string()),
        input: None,
        source_agent_id: None,
    };
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("proposed_prefix_pattern"));
    let parsed: ApprovalRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.proposed_prefix_pattern, Some("git *".to_string()));
}

#[test]
fn test_permission_mode_from_str_camel_case() {
    assert_eq!("default".parse(), Ok(PermissionMode::Default));
    assert_eq!("plan".parse(), Ok(PermissionMode::Plan));
    assert_eq!("acceptEdits".parse(), Ok(PermissionMode::AcceptEdits));
    assert_eq!("bypassPermissions".parse(), Ok(PermissionMode::Bypass));
    assert_eq!("bypass".parse(), Ok(PermissionMode::Bypass));
    assert_eq!("dontAsk".parse(), Ok(PermissionMode::DontAsk));
}

#[test]
fn test_permission_mode_from_str_kebab_case() {
    assert_eq!("accept-edits".parse(), Ok(PermissionMode::AcceptEdits));
    assert_eq!("bypass-permissions".parse(), Ok(PermissionMode::Bypass));
    assert_eq!("dont-ask".parse(), Ok(PermissionMode::DontAsk));
}

#[test]
fn test_permission_mode_from_str_snake_case() {
    assert_eq!("accept_edits".parse(), Ok(PermissionMode::AcceptEdits));
    assert_eq!("dont_ask".parse(), Ok(PermissionMode::DontAsk));
}

#[test]
fn test_permission_mode_from_str_case_insensitive() {
    assert_eq!("AcceptEdits".parse(), Ok(PermissionMode::AcceptEdits));
    assert_eq!("BYPASS".parse(), Ok(PermissionMode::Bypass));
    assert_eq!("DontAsk".parse(), Ok(PermissionMode::DontAsk));
    assert_eq!("PLAN".parse(), Ok(PermissionMode::Plan));
}

#[test]
fn test_permission_mode_from_str_invalid() {
    assert!("unknown".parse::<PermissionMode>().is_err());
    assert!("".parse::<PermissionMode>().is_err());
    assert!("yolo".parse::<PermissionMode>().is_err());
}
