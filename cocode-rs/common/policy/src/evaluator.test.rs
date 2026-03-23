use super::*;
use crate::rule::PermissionRule;
use crate::rule::RuleAction;
use cocode_protocol::PermissionResult;
use cocode_protocol::RuleSource;
use cocode_protocol::ToolName;
use std::path::Path;

// ── Tool name matching ───────────────────────────────────────────

#[test]
fn test_matches_tool_exact() {
    assert!(PermissionRuleEvaluator::matches_tool(
        ToolName::Edit.as_str(),
        ToolName::Edit.as_str()
    ));
    assert!(!PermissionRuleEvaluator::matches_tool(
        ToolName::Edit.as_str(),
        ToolName::Write.as_str()
    ));
}

#[test]
fn test_matches_tool_wildcard() {
    assert!(PermissionRuleEvaluator::matches_tool(
        "*",
        ToolName::Edit.as_str()
    ));
    assert!(PermissionRuleEvaluator::matches_tool(
        "*",
        ToolName::Bash.as_str()
    ));
}

#[test]
fn test_matches_tool_with_colon_prefix() {
    assert!(PermissionRuleEvaluator::matches_tool(
        "Bash:git *",
        ToolName::Bash.as_str()
    ));
    assert!(!PermissionRuleEvaluator::matches_tool(
        "Bash:git *",
        ToolName::Edit.as_str()
    ));
}

// ── File pattern matching ────────────────────────────────────────

#[test]
fn test_matches_file_none_pattern_always_matches() {
    assert!(PermissionRuleEvaluator::matches_file(
        &None,
        Some(Path::new("/foo/bar.rs"))
    ));
    assert!(PermissionRuleEvaluator::matches_file(&None, None));
}

#[test]
fn test_matches_file_some_pattern_requires_path() {
    assert!(!PermissionRuleEvaluator::matches_file(
        &Some("*.rs".to_string()),
        None
    ));
}

#[test]
fn test_matches_file_extension_glob() {
    let pat = Some("*.rs".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("src/main.rs"))
    ));
    assert!(!PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("src/main.ts"))
    ));
}

#[test]
fn test_matches_file_double_star_glob() {
    let pat = Some("src/**/*.ts".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("src/components/App.ts"))
    ));
    assert!(!PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("lib/util.ts"))
    ));
}

#[test]
fn test_matches_file_wildcard() {
    let pat = Some("*".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("any/path.txt"))
    ));
}

#[test]
fn test_matches_file_substring_fallback() {
    let pat = Some("secret".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("/home/.secret/key"))
    ));
    assert!(!PermissionRuleEvaluator::matches_file(
        &pat,
        Some(Path::new("/home/public/key"))
    ));
}

// ── Rule priority ordering ───────────────────────────────────────

#[test]
fn test_deny_wins_over_allow_same_source() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![
        PermissionRule {
            source: RuleSource::Project,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Allow,
        },
        PermissionRule {
            source: RuleSource::Project,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Deny,
        },
    ]);

    let decision = evaluator
        .evaluate(ToolName::Edit.as_str(), None)
        .expect("should match");
    assert!(decision.result.is_denied());
}

#[test]
fn test_higher_priority_source_wins() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![
        PermissionRule {
            source: RuleSource::Session,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Allow,
        },
        PermissionRule {
            source: RuleSource::Policy,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Deny,
        },
    ]);

    let decision = evaluator
        .evaluate(ToolName::Edit.as_str(), None)
        .expect("should match");
    assert!(decision.result.is_allowed());
    assert_eq!(decision.source, Some(RuleSource::Session));
}

#[test]
fn test_ask_action_returns_needs_approval() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::Project,
        tool_pattern: ToolName::Bash.as_str().to_string(),
        file_pattern: None,
        action: RuleAction::Ask,
    }]);

    let decision = evaluator
        .evaluate(ToolName::Bash.as_str(), None)
        .expect("should match");
    assert!(decision.result.needs_approval());
}

// ── Empty rules ──────────────────────────────────────────────────

#[test]
fn test_empty_rules_returns_none() {
    let evaluator = PermissionRuleEvaluator::new();
    assert!(evaluator.evaluate(ToolName::Edit.as_str(), None).is_none());
}

// ── Multiple rules — most restrictive wins ───────────────────────

#[test]
fn test_multiple_rules_most_restrictive_wins() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![
        PermissionRule {
            source: RuleSource::User,
            tool_pattern: "*".to_string(),
            file_pattern: None,
            action: RuleAction::Allow,
        },
        PermissionRule {
            source: RuleSource::Project,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: Some("*.env".to_string()),
            action: RuleAction::Deny,
        },
    ]);

    let decision = evaluator
        .evaluate(ToolName::Edit.as_str(), Some(Path::new("config/.env")))
        .expect("should match");
    assert!(decision.result.is_denied());
    assert_eq!(decision.source, Some(RuleSource::Project));

    let decision = evaluator
        .evaluate(ToolName::Edit.as_str(), Some(Path::new("src/main.rs")))
        .expect("should match");
    assert!(decision.result.is_allowed());
    assert_eq!(decision.source, Some(RuleSource::User));
}

#[test]
fn test_non_matching_tool_skipped() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::Project,
        tool_pattern: ToolName::Edit.as_str().to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);

    assert!(evaluator.evaluate(ToolName::Bash.as_str(), None).is_none());
}

#[test]
fn test_decision_includes_metadata() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::Local,
        tool_pattern: ToolName::Write.as_str().to_string(),
        file_pattern: None,
        action: RuleAction::Allow,
    }]);

    let decision = evaluator
        .evaluate(ToolName::Write.as_str(), None)
        .expect("should match");
    assert_eq!(decision.source, Some(RuleSource::Local));
    assert_eq!(
        decision.matched_pattern.as_deref(),
        Some(ToolName::Write.as_str())
    );
}

// ── PermissionResult variant checks ──────────────────────────────

#[test]
fn test_deny_returns_denied_result() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::Policy,
        tool_pattern: "*".to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);

    let decision = evaluator
        .evaluate(ToolName::Bash.as_str(), None)
        .expect("should match");
    assert!(matches!(decision.result, PermissionResult::Denied { .. }));
}

#[test]
fn test_allow_returns_allowed_result() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: ToolName::Read.as_str().to_string(),
        file_pattern: None,
        action: RuleAction::Allow,
    }]);

    let decision = evaluator
        .evaluate(ToolName::Read.as_str(), None)
        .expect("should match");
    assert!(matches!(decision.result, PermissionResult::Allowed));
}

// ── evaluate_behavior ───────────────────────────────────────────

#[test]
fn test_evaluate_behavior_deny_only() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![
        PermissionRule {
            source: RuleSource::Project,
            tool_pattern: ToolName::Bash.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Deny,
        },
        PermissionRule {
            source: RuleSource::User,
            tool_pattern: ToolName::Bash.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Allow,
        },
    ]);

    let decision = evaluator
        .evaluate_behavior(ToolName::Bash.as_str(), None, RuleAction::Deny, None)
        .expect("should match deny");
    assert!(decision.result.is_denied());

    let decision = evaluator
        .evaluate_behavior(ToolName::Bash.as_str(), None, RuleAction::Allow, None)
        .expect("should match allow");
    assert!(decision.result.is_allowed());

    assert!(
        evaluator
            .evaluate_behavior(ToolName::Bash.as_str(), None, RuleAction::Ask, None)
            .is_none()
    );
}

#[test]
fn test_evaluate_behavior_highest_priority_source_wins() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![
        PermissionRule {
            source: RuleSource::Session,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Deny,
        },
        PermissionRule {
            source: RuleSource::Policy,
            tool_pattern: ToolName::Edit.as_str().to_string(),
            file_pattern: None,
            action: RuleAction::Deny,
        },
    ]);

    let decision = evaluator
        .evaluate_behavior(ToolName::Edit.as_str(), None, RuleAction::Deny, None)
        .expect("should match");
    assert_eq!(decision.source, Some(RuleSource::Session));
}

// ── Command pattern matching ────────────────────────────────────

#[test]
fn test_matches_command_pattern_trailing_wildcard() {
    assert!(PermissionRuleEvaluator::matches_command_pattern(
        "git *",
        "git status"
    ));
    assert!(PermissionRuleEvaluator::matches_command_pattern(
        "git *",
        "git push origin main"
    ));
    assert!(!PermissionRuleEvaluator::matches_command_pattern(
        "git *", "npm test"
    ));
}

#[test]
fn test_matches_command_pattern_exact() {
    assert!(PermissionRuleEvaluator::matches_command_pattern(
        "npm test", "npm test"
    ));
    assert!(!PermissionRuleEvaluator::matches_command_pattern(
        "npm test",
        "npm run test"
    ));
}

#[test]
fn test_matches_tool_parenthesized_form() {
    assert!(PermissionRuleEvaluator::matches_tool_with_input(
        "Bash(git status)",
        ToolName::Bash.as_str(),
        Some("git status")
    ));
    assert!(!PermissionRuleEvaluator::matches_tool_with_input(
        "Bash(git status)",
        ToolName::Bash.as_str(),
        Some("rm -rf /")
    ));
    assert!(PermissionRuleEvaluator::matches_tool_with_input(
        "Bash(npm *)",
        ToolName::Bash.as_str(),
        Some("npm test")
    ));
}

// ── from_permissions_config ──────────────────────────────────────

#[test]
fn test_rules_from_config() {
    let config = cocode_config::PermissionsConfig {
        allow: vec![
            ToolName::Read.as_str().to_string(),
            "Bash(git *)".to_string(),
        ],
        deny: vec!["Bash(rm -rf *)".to_string()],
        ask: vec!["Bash(sudo *)".to_string()],
    };
    let rules = PermissionRuleEvaluator::rules_from_config(&config, RuleSource::User);
    assert_eq!(rules.len(), 4);

    assert_eq!(rules[0].tool_pattern, ToolName::Read.as_str());
    assert_eq!(rules[0].action, RuleAction::Allow);
    assert_eq!(rules[0].source, RuleSource::User);
    assert_eq!(rules[1].tool_pattern, "Bash(git *)");
    assert_eq!(rules[1].action, RuleAction::Allow);

    assert_eq!(rules[2].tool_pattern, "Bash(rm -rf *)");
    assert_eq!(rules[2].action, RuleAction::Deny);

    assert_eq!(rules[3].tool_pattern, "Bash(sudo *)");
    assert_eq!(rules[3].action, RuleAction::Ask);
}

#[test]
fn test_rules_from_config_integrated() {
    let config = cocode_config::PermissionsConfig {
        allow: vec!["Bash(git *)".to_string()],
        deny: vec!["Bash(rm *)".to_string()],
        ask: vec![],
    };
    let rules = PermissionRuleEvaluator::rules_from_config(&config, RuleSource::Project);
    let evaluator = PermissionRuleEvaluator::with_rules(rules);

    let decision = evaluator.evaluate_behavior(
        ToolName::Bash.as_str(),
        None,
        RuleAction::Allow,
        Some("git status"),
    );
    assert!(decision.is_some());
    assert!(decision.unwrap().result.is_allowed());

    let decision = evaluator.evaluate_behavior(
        ToolName::Bash.as_str(),
        None,
        RuleAction::Deny,
        Some("rm -rf /"),
    );
    assert!(decision.is_some());
    assert!(decision.unwrap().result.is_denied());
}

#[test]
fn test_evaluate_behavior_with_command_input() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::Project,
        tool_pattern: "Bash:rm *".to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);

    let decision = evaluator.evaluate_behavior(
        ToolName::Bash.as_str(),
        None,
        RuleAction::Deny,
        Some("rm -rf /"),
    );
    assert!(decision.is_some());
    assert!(decision.unwrap().result.is_denied());

    let decision = evaluator.evaluate_behavior(
        ToolName::Bash.as_str(),
        None,
        RuleAction::Deny,
        Some("git status"),
    );
    assert!(decision.is_none());
}

// ── Glob matching ───────────────────────────────────────────────

#[test]
fn test_matches_file_complex_double_star() {
    // "src/**/*.ts" should match nested paths
    let pattern = Some("src/**/*.ts".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("src/components/Button.ts"))
    ));
    assert!(PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("src/deep/nested/path/file.ts"))
    ));
    assert!(!PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("src/file.rs"))
    ));
    assert!(!PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("other/file.ts"))
    ));
}

#[test]
fn test_matches_file_no_glob_metachar_is_substring() {
    // Without glob metacharacters, uses substring match
    let pattern = Some("secret".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("/home/.secret/key"))
    ));
}

#[test]
fn test_matches_file_question_mark_wildcard() {
    let pattern = Some("file?.rs".to_string());
    assert!(PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("file1.rs"))
    ));
    assert!(!PermissionRuleEvaluator::matches_file(
        &pattern,
        Some(Path::new("file12.rs"))
    ));
}

#[test]
fn test_empty_command_pattern_colon() {
    // "Bash:" with empty command pattern does not match any specific command
    assert!(!PermissionRuleEvaluator::matches_tool_with_input(
        "Bash:",
        "Bash",
        Some("git push")
    ));
    // But matches when no command input is provided (tool-name-only check)
    assert!(PermissionRuleEvaluator::matches_tool_with_input(
        "Bash:", "Bash", None
    ));
}

#[test]
fn test_empty_command_pattern_parens() {
    // "Bash()" with empty command pattern does not match any specific command
    assert!(!PermissionRuleEvaluator::matches_tool_with_input(
        "Bash()",
        "Bash",
        Some("git push")
    ));
    // But matches when no command input is provided
    assert!(PermissionRuleEvaluator::matches_tool_with_input(
        "Bash()", "Bash", None
    ));
}

#[test]
fn test_deny_rule_catches_env_var_prefix() {
    // "LANG=C rm -rf /" should match a deny rule for "Bash:rm *"
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash:rm *".to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);
    let decision =
        evaluator.evaluate_behavior("Bash", None, RuleAction::Deny, Some("LANG=C rm -rf /"));
    assert!(
        decision.is_some(),
        "deny rule should catch env-var-prefixed command"
    );
}

#[test]
fn test_deny_rule_catches_wrapper_prefix() {
    // "timeout 30 rm -rf /" should match a deny rule for "Bash:rm *"
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash:rm *".to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);
    let decision =
        evaluator.evaluate_behavior("Bash", None, RuleAction::Deny, Some("timeout 30 rm -rf /"));
    assert!(
        decision.is_some(),
        "deny rule should catch wrapper-prefixed command"
    );
}

#[test]
fn test_allow_rule_does_not_normalize() {
    // "LANG=C git push" should NOT match an allow rule for "Bash:git *"
    // because allow rules only match raw commands to prevent over-allowance
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash:git *".to_string(),
        file_pattern: None,
        action: RuleAction::Allow,
    }]);
    let decision =
        evaluator.evaluate_behavior("Bash", None, RuleAction::Allow, Some("LANG=C git push"));
    assert!(
        decision.is_none(),
        "allow rule should not normalize command"
    );
}

#[test]
fn test_evaluate_deny_ask_returns_deny() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash:rm *".to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);
    let decision = evaluator.evaluate_deny_ask("Bash", None, Some("rm -rf /"));
    assert!(decision.is_some());
    assert!(decision.as_ref().unwrap().result.is_denied());
}

#[test]
fn test_evaluate_deny_ask_returns_ask_when_no_deny() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash:npm *".to_string(),
        file_pattern: None,
        action: RuleAction::Ask,
    }]);
    let decision = evaluator.evaluate_deny_ask("Bash", None, Some("npm install"));
    assert!(decision.is_some());
    assert!(decision.as_ref().unwrap().result.needs_approval());
}

#[test]
fn test_evaluate_deny_ask_passthrough_when_no_match() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash:git *".to_string(),
        file_pattern: None,
        action: RuleAction::Allow,
    }]);
    // Allow rules are NOT checked by evaluate_deny_ask
    let decision = evaluator.evaluate_deny_ask("Bash", None, Some("git push"));
    assert!(decision.is_none());
}

#[test]
fn test_evaluate_deny_ask_deny_wins_over_ask() {
    let evaluator = PermissionRuleEvaluator::with_rules(vec![
        PermissionRule {
            source: RuleSource::User,
            tool_pattern: "Bash:rm *".to_string(),
            file_pattern: None,
            action: RuleAction::Ask,
        },
        PermissionRule {
            source: RuleSource::User,
            tool_pattern: "Bash:rm *".to_string(),
            file_pattern: None,
            action: RuleAction::Deny,
        },
    ]);
    let decision = evaluator.evaluate_deny_ask("Bash", None, Some("rm -rf /"));
    assert!(decision.is_some());
    assert!(decision.as_ref().unwrap().result.is_denied());
}

#[test]
fn test_evaluate_does_not_normalize() {
    // evaluate() (not evaluate_behavior) should not normalize commands
    // because it's a general-purpose matcher, not the staged pipeline
    let evaluator = PermissionRuleEvaluator::with_rules(vec![PermissionRule {
        source: RuleSource::User,
        tool_pattern: "Bash".to_string(),
        file_pattern: None,
        action: RuleAction::Deny,
    }]);
    // evaluate() matches on tool name only, no command normalization
    let decision = evaluator.evaluate("Bash", None);
    assert!(decision.is_some());
    assert!(decision.as_ref().unwrap().result.is_denied());
}
