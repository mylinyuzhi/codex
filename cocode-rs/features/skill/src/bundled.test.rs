use super::*;

#[test]
fn test_compute_fingerprint_deterministic() {
    let fp1 = compute_fingerprint(b"hello world");
    let fp2 = compute_fingerprint(b"hello world");
    assert_eq!(fp1, fp2);
}

#[test]
fn test_compute_fingerprint_different_input() {
    let fp1 = compute_fingerprint(b"hello");
    let fp2 = compute_fingerprint(b"world");
    assert_ne!(fp1, fp2);
}

#[test]
fn test_compute_fingerprint_known_value() {
    // SHA-256 of "hello world" is well-known
    let fp = compute_fingerprint(b"hello world");
    assert_eq!(
        fp,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn test_compute_fingerprint_empty() {
    let fp = compute_fingerprint(b"");
    // SHA-256 of empty string
    assert_eq!(
        fp,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_compute_fingerprint_length() {
    let fp = compute_fingerprint(b"test");
    assert_eq!(fp.len(), 64);
}

#[test]
fn test_bundled_skills_returns_vec() {
    let skills = bundled_skills();
    // Should contain output-style skill
    assert!(!skills.is_empty());
    assert!(skills.iter().any(|s| s.name == "output-style"));
}

#[test]
fn test_output_style_skill() {
    let skills = bundled_skills();
    let output_style = skills.iter().find(|s| s.name == "output-style").unwrap();
    assert_eq!(
        output_style.description,
        "Manage response output styles (explanatory, learning, etc.)"
    );
    assert!(output_style.prompt.contains("/output-style"));
    assert_eq!(output_style.fingerprint.len(), 64);
}

#[test]
fn test_bundled_skills_are_local_jsx() {
    let skills = bundled_skills();
    for skill in &skills {
        assert_eq!(
            skill.command_type,
            crate::command::CommandType::LocalJsx,
            "bundled skill '{}' should be LocalJsx",
            skill.name
        );
    }
}

#[test]
fn test_bundled_skill_struct() {
    let skill = BundledSkill {
        name: "test".to_string(),
        description: "Test skill".to_string(),
        prompt: "Do something".to_string(),
        fingerprint: compute_fingerprint(b"Do something"),
        command_type: crate::command::CommandType::Prompt,
    };
    assert_eq!(skill.name, "test");
    assert_eq!(skill.fingerprint.len(), 64);
}
