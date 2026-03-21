use super::is_valid_team_name;

#[test]
fn valid_team_names() {
    assert!(is_valid_team_name("my-team"));
    assert!(is_valid_team_name("team_1"));
    assert!(is_valid_team_name("a"));
    assert!(is_valid_team_name("Team-Alpha-2"));
}

#[test]
fn invalid_team_names() {
    assert!(!is_valid_team_name(""));
    assert!(!is_valid_team_name("-starts-with-dash"));
    assert!(!is_valid_team_name("_starts-with-underscore"));
    assert!(!is_valid_team_name("has spaces"));
    assert!(!is_valid_team_name("../evil"));
    assert!(!is_valid_team_name("has/slash"));
    assert!(!is_valid_team_name("has.dot"));
    assert!(!is_valid_team_name(
        &"a".repeat(65) // exceeds 64 char limit
    ));
}

#[test]
fn boundary_length() {
    assert!(is_valid_team_name(&"a".repeat(64)));
    assert!(!is_valid_team_name(&"a".repeat(65)));
}
