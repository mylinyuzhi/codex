use super::*;

fn create_test_skills() -> Vec<SkillInfo> {
    vec![
        SkillInfo {
            name: "commit".to_string(),
            description: "Generate a commit message".to_string(),
        },
        SkillInfo {
            name: "review".to_string(),
            description: "Review code changes".to_string(),
        },
        SkillInfo {
            name: "test".to_string(),
            description: "Run tests".to_string(),
        },
        SkillInfo {
            name: "config".to_string(),
            description: "Configure settings".to_string(),
        },
    ]
}

#[test]
fn test_search_empty_query() {
    let manager = SkillSearchManager::with_skills(create_test_skills());
    let results = manager.search("");

    // Should return all skills
    assert_eq!(results.len(), 4);
    // Should be sorted by name
    assert_eq!(results[0].name, "commit");
    assert_eq!(results[1].name, "config");
    assert_eq!(results[2].name, "review");
    assert_eq!(results[3].name, "test");
}

#[test]
fn test_search_exact_match() {
    let manager = SkillSearchManager::with_skills(create_test_skills());
    let results = manager.search("commit");

    assert!(!results.is_empty());
    assert_eq!(results[0].name, "commit");
}

#[test]
fn test_search_prefix_match() {
    let manager = SkillSearchManager::with_skills(create_test_skills());
    let results = manager.search("com");

    assert!(!results.is_empty());
    assert_eq!(results[0].name, "commit");
}

#[test]
fn test_search_fuzzy_match() {
    let manager = SkillSearchManager::with_skills(create_test_skills());
    let results = manager.search("cmit");

    assert!(!results.is_empty());
    assert_eq!(results[0].name, "commit");
}

#[test]
fn test_search_no_match() {
    let manager = SkillSearchManager::with_skills(create_test_skills());
    let results = manager.search("xyz");

    assert!(results.is_empty());
}

#[test]
fn test_search_case_insensitive() {
    let manager = SkillSearchManager::with_skills(create_test_skills());
    let results = manager.search("COMMIT");

    assert!(!results.is_empty());
    assert_eq!(results[0].name, "commit");
}

#[test]
fn test_add_skill() {
    let mut manager = SkillSearchManager::new();
    assert!(manager.is_empty());

    manager.add_skill(SkillInfo {
        name: "test".to_string(),
        description: "Test skill".to_string(),
    });

    assert_eq!(manager.len(), 1);
    let results = manager.search("test");
    assert_eq!(results[0].name, "test");
}

#[test]
fn test_clear_skills() {
    let mut manager = SkillSearchManager::with_skills(create_test_skills());
    assert!(!manager.is_empty());

    manager.clear();
    assert!(manager.is_empty());
}
