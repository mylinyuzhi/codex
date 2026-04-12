use std::collections::HashMap;

use crate::LoadedPlugin;
use crate::PluginManifest;
use crate::PluginSource;

use super::*;

fn test_plugin(name: &str, path: &Path, skills: Vec<&str>) -> LoadedPlugin {
    LoadedPlugin {
        name: name.to_string(),
        manifest: PluginManifest {
            name: name.to_string(),
            version: None,
            description: "Test plugin".to_string(),
            skills: skills.into_iter().map(String::from).collect(),
            hooks: HashMap::new(),
            mcp_servers: HashMap::new(),
        },
        path: path.to_path_buf(),
        source: PluginSource::User,
        enabled: true,
    }
}

#[test]
fn test_load_plugin_skills_from_manifest_paths() {
    let dir = tempfile::tempdir().unwrap();

    // Create a skill .md file
    std::fs::write(
        dir.path().join("my-tool.md"),
        "# my-tool\n---\ndescription: A tool\n---\nDo things.\n",
    )
    .unwrap();

    let plugin = test_plugin("test-plugin", dir.path(), vec!["my-tool.md"]);
    let skills = load_plugin_skills(&plugin);

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "test-plugin:my-tool");
    assert_eq!(skills[0].description, "A tool");
    assert!(matches!(
        skills[0].source,
        SkillSource::Plugin { ref plugin_name } if plugin_name == "test-plugin"
    ));
}

#[test]
fn test_load_plugin_skills_from_skills_dir() {
    let dir = tempfile::tempdir().unwrap();

    // Create skills/ directory with .md files
    let skills_dir = dir.path().join("skills");
    std::fs::create_dir(&skills_dir).unwrap();
    std::fs::write(
        skills_dir.join("deploy.md"),
        "# deploy\n\nDeploy the app.\n",
    )
    .unwrap();
    std::fs::write(skills_dir.join("test.md"), "# test\n\nRun tests.\n").unwrap();

    let plugin = test_plugin("my-plugin", dir.path(), vec![]);
    let skills = load_plugin_skills(&plugin);

    assert_eq!(skills.len(), 2);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"my-plugin:deploy"));
    assert!(names.contains(&"my-plugin:test"));
}

#[test]
fn test_load_plugin_skills_skill_md_dir_format() {
    let dir = tempfile::tempdir().unwrap();

    // Create skills/my-skill/SKILL.md directory format
    let skills_dir = dir.path().join("skills");
    let skill_subdir = skills_dir.join("my-skill");
    std::fs::create_dir_all(&skill_subdir).unwrap();
    std::fs::write(
        skill_subdir.join("SKILL.md"),
        "# heading\n---\ndescription: Skill from dir\n---\nContent.\n",
    )
    .unwrap();

    let plugin = test_plugin("cool-plugin", dir.path(), vec![]);
    let skills = load_plugin_skills(&plugin);

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "cool-plugin:my-skill");
}

#[test]
fn test_load_plugin_skills_nonexistent_path() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = test_plugin("test", dir.path(), vec!["nonexistent.md"]);
    let skills = load_plugin_skills(&plugin);
    assert!(skills.is_empty());
}

#[test]
fn test_load_all_plugin_skills() {
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();

    let skills_dir1 = dir1.path().join("skills");
    std::fs::create_dir(&skills_dir1).unwrap();
    std::fs::write(skills_dir1.join("a.md"), "# a\n\nSkill A.\n").unwrap();

    let skills_dir2 = dir2.path().join("skills");
    std::fs::create_dir(&skills_dir2).unwrap();
    std::fs::write(skills_dir2.join("b.md"), "# b\n\nSkill B.\n").unwrap();

    let p1 = test_plugin("plugin1", dir1.path(), vec![]);
    let p2 = test_plugin("plugin2", dir2.path(), vec![]);

    let skills = load_all_plugin_skills(&[&p1, &p2]);
    assert_eq!(skills.len(), 2);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"plugin1:a"));
    assert!(names.contains(&"plugin2:b"));
}
