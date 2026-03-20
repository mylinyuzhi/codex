use super::*;
use crate::generator::SkillInfo;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_skills() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = AvailableSkillsGenerator::new();
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_initial_call_returns_none() {
    let config = test_config();
    let skills: Vec<SkillInfo> = vec![SkillInfo {
        name: "commit".to_string(),
        description: "Generate a commit message".to_string(),
        when_to_use: None,
        is_bundled: true,
        plugin_name: None,
    }];

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .available_skills(skills)
        .build();

    let generator = AvailableSkillsGenerator::new();
    // First call should return None (initial population)
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delta_returns_new_skills() {
    let config = test_config();
    let initial_skills: Vec<SkillInfo> = vec![SkillInfo {
        name: "commit".to_string(),
        description: "Generate a commit message".to_string(),
        when_to_use: None,
        is_bundled: true,
        plugin_name: None,
    }];

    let ctx1 = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .available_skills(initial_skills.clone())
        .build();

    let generator = AvailableSkillsGenerator::new();
    // First call: populate set
    generator.generate(&ctx1).await.expect("generate");

    // Second call with same skills: should return None (no delta)
    let ctx2 = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .available_skills(initial_skills)
        .build();
    let result = generator.generate(&ctx2).await.expect("generate");
    assert!(result.is_none());

    // Third call with a new skill: should return the new one only
    let skills_with_new: Vec<SkillInfo> = vec![
        SkillInfo {
            name: "commit".to_string(),
            description: "Generate a commit message".to_string(),
            when_to_use: None,
            is_bundled: true,
            plugin_name: None,
        },
        SkillInfo {
            name: "review-pr".to_string(),
            description: "Review a pull request".to_string(),
            when_to_use: Some("Use when the user asks to review a PR".to_string()),
            is_bundled: false,
            plugin_name: None,
        },
    ];

    let ctx3 = GeneratorContext::builder()
        .config(&config)
        .turn_number(3)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .available_skills(skills_with_new)
        .build();
    let result = generator.generate(&ctx3).await.expect("generate");
    assert!(result.is_some());
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("review-pr"));
    assert!(!content.contains("commit")); // Already sent
}

#[test]
fn test_generator_properties() {
    let generator = AvailableSkillsGenerator::new();
    assert_eq!(generator.name(), "AvailableSkillsGenerator");
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 1);
}

#[test]
fn test_budget_tier1_fits() {
    let skill = SkillInfo {
        name: "commit".to_string(),
        description: "Generate a commit message".to_string(),
        when_to_use: None,
        is_bundled: true,
        plugin_name: None,
    };
    let skills: Vec<&SkillInfo> = vec![&skill];
    let result = format_skills_within_budget(&skills, 200_000);
    assert!(result.contains("commit"));
    assert!(result.contains("Generate a commit message"));
}

#[test]
fn test_budget_tier3_names_only() {
    // Create many non-bundled skills with very long descriptions to exceed budget
    let skills_data: Vec<SkillInfo> = (0..100)
        .map(|i| SkillInfo {
            name: format!("skill-{i}"),
            description: "A".repeat(500),
            when_to_use: Some("B".repeat(500)),
            is_bundled: false,
            plugin_name: None,
        })
        .collect();
    let skills: Vec<&SkillInfo> = skills_data.iter().collect();

    // Use a very small context window so budget is tiny
    let result = format_skills_within_budget(&skills, 1000);
    // Should have skill names but not full descriptions
    assert!(result.contains("skill-0"));
    assert!(!result.contains(&"A".repeat(500)));
}

#[test]
fn test_plugin_skill_attribution() {
    let skill = SkillInfo {
        name: "deploy".to_string(),
        description: "Deploy the application".to_string(),
        when_to_use: None,
        is_bundled: false,
        plugin_name: Some("my-deploy-plugin".to_string()),
    };
    let skills: Vec<&SkillInfo> = vec![&skill];
    let result = format_skills_within_budget(&skills, 200_000);
    assert!(result.contains("deploy (from my-deploy-plugin)"));
}

#[test]
fn test_plugin_skill_attribution_names_only() {
    let skill = SkillInfo {
        name: "lint".to_string(),
        description: "A".repeat(500),
        when_to_use: Some("B".repeat(500)),
        is_bundled: false,
        plugin_name: Some("code-quality".to_string()),
    };
    let skills: Vec<&SkillInfo> = vec![&skill];
    // Tiny budget forces names-only tier
    let result = format_skills_within_budget(&skills, 50);
    assert!(result.contains("lint (from code-quality)"));
}
