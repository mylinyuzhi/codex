use pretty_assertions::assert_eq;

use super::*;

// ── Parsing ──

#[test]
fn test_parse_exact() {
    let rule = ShellPermissionRule::parse("git commit");
    assert_eq!(
        rule,
        ShellPermissionRule::Exact {
            command: "git commit".into()
        }
    );
}

#[test]
fn test_parse_prefix_trailing_space() {
    let rule = ShellPermissionRule::parse("git ");
    assert_eq!(
        rule,
        ShellPermissionRule::Prefix {
            prefix: "git ".into()
        }
    );
}

#[test]
fn test_parse_prefix_legacy_colon_star() {
    let rule = ShellPermissionRule::parse("npm:*");
    assert_eq!(
        rule,
        ShellPermissionRule::Prefix {
            prefix: "npm".into()
        }
    );
}

#[test]
fn test_parse_wildcard() {
    let rule = ShellPermissionRule::parse("git *");
    assert_eq!(
        rule,
        ShellPermissionRule::Wildcard {
            pattern: "git *".into()
        }
    );
}

#[test]
fn test_parse_escaped_star_not_wildcard() {
    // \* is an escaped literal star — not a wildcard
    let rule = ShellPermissionRule::parse(r"echo \*");
    assert_eq!(
        rule,
        ShellPermissionRule::Exact {
            command: r"echo \*".into()
        }
    );
}

// ── Exact matching ──

#[test]
fn test_exact_match() {
    let rule = ShellPermissionRule::parse("git commit");
    assert!(rule.matches("git commit"));
    assert!(!rule.matches("git push"));
    assert!(!rule.matches("git commit -m 'x'"));
}

// ── Prefix matching ──

#[test]
fn test_prefix_match() {
    let rule = ShellPermissionRule::parse("git ");
    assert!(rule.matches("git commit"));
    assert!(rule.matches("git push"));
    assert!(rule.matches("git status"));
    assert!(!rule.matches("gitk"));
    assert!(!rule.matches("grep foo"));
}

#[test]
fn test_legacy_prefix_match() {
    let rule = ShellPermissionRule::parse("npm:*");
    assert!(rule.matches("npm install"));
    assert!(rule.matches("npm run build"));
    assert!(!rule.matches("npx foo"));
}

// ── Wildcard matching ──

#[test]
fn test_wildcard_match() {
    let rule = ShellPermissionRule::parse("git *");
    assert!(rule.matches("git commit"));
    assert!(rule.matches("git push origin main"));
    assert!(!rule.matches("grep foo"));
}

#[test]
fn test_wildcard_trailing_space_star_optional() {
    // "git *" should also match bare "git" (trailing ` *` is optional when single wildcard)
    let rule = ShellPermissionRule::parse("git *");
    assert!(rule.matches("git"));
    assert!(rule.matches("git add"));
}

#[test]
fn test_wildcard_middle() {
    let rule = ShellPermissionRule::parse("docker * --read-only");
    assert!(rule.matches("docker run --read-only"));
    assert!(rule.matches("docker exec abc --read-only"));
    assert!(!rule.matches("docker run --privileged"));
}

#[test]
fn test_wildcard_multi_star_no_optional_trailing() {
    // Multi-wildcard: trailing ` *` is NOT optional
    let rule = ShellPermissionRule::parse("* run *");
    assert!(rule.matches("npm run build"));
    assert!(!rule.matches("npm run")); // "run" without trailing arg
}

#[test]
fn test_escaped_star_matches_literal() {
    // \* in pattern matches literal * in command
    assert!(match_wildcard_pattern(r"echo \*", "echo *"));
    assert!(!match_wildcard_pattern(r"echo \*", "echo hello"));
}

#[test]
fn test_escaped_backslash_matches_literal() {
    // \\ in pattern matches literal \ in command
    assert!(match_wildcard_pattern(r"echo \\", r"echo \"));
}

#[test]
fn test_wildcard_with_regex_special_chars() {
    // Regex special chars in pattern should be escaped
    assert!(match_wildcard_pattern("foo.bar *", "foo.bar baz"));
    assert!(!match_wildcard_pattern("foo.bar *", "fooXbar baz"));
}

// ── matches_bash_rule ──

#[test]
fn test_matches_bash_rule() {
    assert!(matches_bash_rule("git *", "git status"));
    assert!(matches_bash_rule("ls", "ls"));
    assert!(!matches_bash_rule("ls", "ls -la"));
    assert!(matches_bash_rule("ls ", "ls -la"));
}

// ── dangerous patterns ──

#[test]
fn test_dangerous_bash_permissions() {
    assert!(is_dangerous_bash_permission("*"));
    assert!(is_dangerous_bash_permission("bash *"));
    assert!(is_dangerous_bash_permission("sh *"));
    assert!(is_dangerous_bash_permission("eval *"));
    assert!(!is_dangerous_bash_permission("git *"));
    assert!(!is_dangerous_bash_permission("ls"));
}

// ── has_wildcards ──

#[test]
fn test_has_wildcards() {
    assert!(has_wildcards("git *"));
    assert!(has_wildcards("* foo"));
    assert!(!has_wildcards("npm:*")); // legacy prefix, not wildcard
    assert!(!has_wildcards(r"echo \*")); // escaped star
    assert!(has_wildcards(r"echo \\*")); // escaped backslash then real star
}
