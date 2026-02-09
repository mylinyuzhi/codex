use super::*;

#[test]
fn test_exact_match() {
    let m = HookMatcher::Exact {
        value: "bash".to_string(),
    };
    assert!(m.matches("bash"));
    assert!(!m.matches("Bash"));
    assert!(!m.matches("bash_tool"));
}

#[test]
fn test_wildcard_star() {
    let m = HookMatcher::Wildcard {
        pattern: "read_*".to_string(),
    };
    assert!(m.matches("read_file"));
    assert!(m.matches("read_dir"));
    assert!(m.matches("read_"));
    assert!(!m.matches("write_file"));
}

#[test]
fn test_wildcard_question() {
    let m = HookMatcher::Wildcard {
        pattern: "tool_?".to_string(),
    };
    assert!(m.matches("tool_a"));
    assert!(m.matches("tool_1"));
    assert!(!m.matches("tool_ab"));
    assert!(!m.matches("tool_"));
}

#[test]
fn test_wildcard_complex() {
    let m = HookMatcher::Wildcard {
        pattern: "*_file_*".to_string(),
    };
    assert!(m.matches("read_file_sync"));
    assert!(m.matches("write_file_async"));
    assert!(m.matches("_file_"));
    assert!(!m.matches("file"));
}

#[test]
fn test_or_matcher() {
    let m = HookMatcher::Or {
        matchers: vec![
            HookMatcher::Exact {
                value: "bash".to_string(),
            },
            HookMatcher::Exact {
                value: "shell".to_string(),
            },
        ],
    };
    assert!(m.matches("bash"));
    assert!(m.matches("shell"));
    assert!(!m.matches("python"));
}

#[test]
fn test_regex_match() {
    let m = HookMatcher::Regex {
        pattern: r"^(read|write)_\w+$".to_string(),
    };
    assert!(m.matches("read_file"));
    assert!(m.matches("write_data"));
    assert!(!m.matches("delete_file"));
    assert!(!m.matches("read file"));
}

#[test]
fn test_regex_invalid_pattern() {
    let m = HookMatcher::Regex {
        pattern: r"[invalid".to_string(),
    };
    // Invalid regex should return false (not panic)
    assert!(!m.matches("anything"));
}

#[test]
fn test_all_matcher() {
    let m = HookMatcher::All;
    assert!(m.matches("anything"));
    assert!(m.matches(""));
    assert!(m.matches("literally anything"));
}

#[test]
fn test_validate_valid_regex() {
    let m = HookMatcher::Regex {
        pattern: r"^test$".to_string(),
    };
    assert!(m.validate().is_ok());
}

#[test]
fn test_validate_invalid_regex() {
    let m = HookMatcher::Regex {
        pattern: r"[invalid".to_string(),
    };
    assert!(m.validate().is_err());
}

#[test]
fn test_validate_or_with_invalid_regex() {
    let m = HookMatcher::Or {
        matchers: vec![
            HookMatcher::Exact {
                value: "ok".to_string(),
            },
            HookMatcher::Regex {
                pattern: r"[bad".to_string(),
            },
        ],
    };
    assert!(m.validate().is_err());
}

#[test]
fn test_validate_non_regex() {
    let exact = HookMatcher::Exact {
        value: "test".to_string(),
    };
    assert!(exact.validate().is_ok());

    let wildcard = HookMatcher::Wildcard {
        pattern: "t*".to_string(),
    };
    assert!(wildcard.validate().is_ok());

    let all = HookMatcher::All;
    assert!(all.validate().is_ok());
}

#[test]
fn test_serde_roundtrip() {
    let m = HookMatcher::Or {
        matchers: vec![
            HookMatcher::Exact {
                value: "bash".to_string(),
            },
            HookMatcher::Wildcard {
                pattern: "read_*".to_string(),
            },
        ],
    };
    let json = serde_json::to_string(&m).expect("serialize");
    let parsed: HookMatcher = serde_json::from_str(&json).expect("deserialize");
    assert!(parsed.matches("bash"));
    assert!(parsed.matches("read_file"));
    assert!(!parsed.matches("write_file"));
}
