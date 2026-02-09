use super::*;
use std::fs;

#[test]
fn test_scanner_default() {
    let scanner = SkillScanner::default();
    assert_eq!(scanner.max_scan_depth, 6);
    assert_eq!(scanner.max_skills_dirs_per_root, 2000);
}

#[test]
fn test_scan_finds_skill_directories() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    // Create two skill directories
    let skill1 = root.join("skill1");
    fs::create_dir_all(&skill1).expect("mkdir skill1");
    fs::write(
        skill1.join("SKILL.toml"),
        "name = \"s1\"\ndescription = \"d\"\nprompt_inline = \"p\"",
    )
    .expect("write SKILL.toml");

    let skill2 = root.join("nested").join("skill2");
    fs::create_dir_all(&skill2).expect("mkdir skill2");
    fs::write(
        skill2.join("SKILL.toml"),
        "name = \"s2\"\ndescription = \"d\"\nprompt_inline = \"p\"",
    )
    .expect("write SKILL.toml");

    // Create a directory without SKILL.toml
    let no_skill = root.join("no-skill");
    fs::create_dir_all(&no_skill).expect("mkdir no-skill");
    fs::write(no_skill.join("README.md"), "not a skill").expect("write README");

    let scanner = SkillScanner::new();
    let found = scanner.scan(root);

    assert_eq!(found.len(), 2);
    assert!(found.contains(&skill1));
    assert!(found.contains(&skill2));
}

#[test]
fn test_scan_respects_depth_limit() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    // Create skill at depth 3 (root/a/b/c/SKILL.toml)
    let deep = root.join("a").join("b").join("c");
    fs::create_dir_all(&deep).expect("mkdir deep");
    fs::write(
        deep.join("SKILL.toml"),
        "name = \"deep\"\ndescription = \"d\"\nprompt_inline = \"p\"",
    )
    .expect("write SKILL.toml");

    // Scanner with depth 2 should not find it
    let scanner = SkillScanner {
        max_scan_depth: 2,
        max_skills_dirs_per_root: 2000,
    };
    let found = scanner.scan(root);
    assert!(found.is_empty());

    // Scanner with depth 4 should find it
    let scanner = SkillScanner {
        max_scan_depth: 4,
        max_skills_dirs_per_root: 2000,
    };
    let found = scanner.scan(root);
    assert_eq!(found.len(), 1);
}

#[test]
fn test_scan_respects_max_dirs_limit() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    // Create 5 skill directories
    for i in 0..5 {
        let skill = root.join(format!("skill{i}"));
        fs::create_dir_all(&skill).expect("mkdir");
        fs::write(
            skill.join("SKILL.toml"),
            format!("name = \"s{i}\"\ndescription = \"d\"\nprompt_inline = \"p\""),
        )
        .expect("write");
    }

    let scanner = SkillScanner {
        max_scan_depth: 6,
        max_skills_dirs_per_root: 3,
    };
    let found = scanner.scan(root);
    assert!(found.len() <= 3);
}

#[test]
fn test_scan_nonexistent_root() {
    let scanner = SkillScanner::new();
    let found = scanner.scan(Path::new("/nonexistent/path/xyz"));
    assert!(found.is_empty());
}

#[test]
fn test_scan_roots_skips_missing() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill = root.join("skill");
    fs::create_dir_all(&skill).expect("mkdir");
    fs::write(
        skill.join("SKILL.toml"),
        "name = \"s\"\ndescription = \"d\"\nprompt_inline = \"p\"",
    )
    .expect("write");

    let scanner = SkillScanner::new();
    let roots = vec![root.to_path_buf(), PathBuf::from("/nonexistent/root")];
    let found = scanner.scan_roots(&roots);
    assert_eq!(found.len(), 1);
}
