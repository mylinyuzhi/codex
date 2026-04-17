use super::*;
use serde_json::json;

#[test]
fn test_generate_word_slug_format() {
    let slug = generate_word_slug();
    // At least 3 parts (adjective-verb-noun), though some words contain dashes
    assert!(
        slug.split('-').count() >= 3,
        "slug should have at least 3 parts: {slug}"
    );
    assert!(!slug.is_empty());
}

#[test]
fn test_generate_word_slug_uniqueness() {
    let slugs: Vec<String> = (0..10).map(|_| generate_word_slug()).collect();
    // Not all identical (statistically near-impossible)
    let unique: std::collections::HashSet<&str> = slugs.iter().map(String::as_str).collect();
    assert!(unique.len() > 1, "expected varied slugs, got: {slugs:?}");
}

#[test]
fn test_slug_cache_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path();

    let slug1 = get_plan_slug("test-session-1", plans_dir);
    let slug2 = get_plan_slug("test-session-1", plans_dir);
    assert_eq!(slug1, slug2, "same session should return cached slug");

    let slug3 = get_plan_slug("test-session-2", plans_dir);
    // Different sessions may (rarely) collide, but usually differ
    let _ = slug3; // just ensure it doesn't panic

    clear_plan_slug("test-session-1");
    let slug4 = get_plan_slug("test-session-1", plans_dir);
    // After clearing, a new slug is generated (may differ)
    let _ = slug4;
}

#[test]
fn test_set_plan_slug() {
    let dir = tempfile::tempdir().unwrap();
    set_plan_slug("set-test", "custom-slug-here");
    let slug = get_plan_slug("set-test", dir.path());
    assert_eq!(slug, "custom-slug-here");
    clear_plan_slug("set-test");
}

#[test]
fn test_resolve_plans_directory_default() {
    let config = PathBuf::from("/home/user/.cocode");
    let result = resolve_plans_directory(&config, None, None);
    assert_eq!(result, PathBuf::from("/home/user/.cocode/plans"));
}

#[test]
fn test_resolve_plans_directory_with_setting() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    let plans_sub = project.join("my-plans");
    std::fs::create_dir_all(&plans_sub).unwrap();

    let config = PathBuf::from("/home/user/.cocode");
    let result = resolve_plans_directory(&config, Some(project), Some("my-plans"));
    assert!(result.ends_with("my-plans"));
}

#[test]
fn test_plan_crud() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("plans");
    let sid = "crud-test";

    // Initially no plan
    assert!(!plan_exists(sid, &plans_dir, None));
    assert!(get_plan(sid, &plans_dir, None).is_none());

    // Write
    write_plan(sid, &plans_dir, "# My Plan\n\n1. Do stuff", None).unwrap();
    assert!(plan_exists(sid, &plans_dir, None));

    // Read
    let content = get_plan(sid, &plans_dir, None).unwrap();
    assert_eq!(content, "# My Plan\n\n1. Do stuff");

    // Update
    write_plan(sid, &plans_dir, "# Updated Plan", None).unwrap();
    let content = get_plan(sid, &plans_dir, None).unwrap();
    assert_eq!(content, "# Updated Plan");

    // Delete
    delete_plan(sid, &plans_dir, None).unwrap();
    assert!(!plan_exists(sid, &plans_dir, None));

    clear_plan_slug(sid);
}

#[test]
fn test_subagent_plan_path() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path();
    let sid = "agent-test";
    set_plan_slug(sid, "bright-dancing-fox");

    let main_path = get_plan_file_path(sid, plans_dir, None);
    assert!(main_path.ends_with("bright-dancing-fox.md"));

    let agent_path = get_plan_file_path(sid, plans_dir, Some("agent-42"));
    assert!(agent_path.ends_with("bright-dancing-fox-agent-agent-42.md"));

    clear_plan_slug(sid);
}

#[test]
fn test_recover_plan_from_exit_tool_input() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("plans");
    let sid = "recover-test";

    let entries = vec![json!({
        "role": "assistant",
        "content": [{
            "type": "tool_use",
            "name": "ExitPlanMode",
            "input": { "plan": "# Recovered Plan\n\n- Step 1\n- Step 2" }
        }]
    })];

    let result = recover_plan_for_resume(sid, &plans_dir, "test-slug", &entries);
    assert!(result);

    let content = std::fs::read_to_string(plans_dir.join("test-slug.md")).unwrap();
    assert_eq!(content, "# Recovered Plan\n\n- Step 1\n- Step 2");

    clear_plan_slug(sid);
}

#[test]
fn test_recover_plan_from_user_plan_content() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("plans");
    let sid = "recover-user-test";

    let entries = vec![json!({
        "role": "user",
        "planContent": "# User Plan Content"
    })];

    let result = recover_plan_for_resume(sid, &plans_dir, "user-slug", &entries);
    assert!(result);

    let content = std::fs::read_to_string(plans_dir.join("user-slug.md")).unwrap();
    assert_eq!(content, "# User Plan Content");

    clear_plan_slug(sid);
}

#[test]
fn test_recover_plan_file_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    std::fs::write(plans_dir.join("existing-slug.md"), "existing").unwrap();
    let sid = "exists-test";

    let result = recover_plan_for_resume(sid, &plans_dir, "existing-slug", &[]);
    assert!(result);

    clear_plan_slug(sid);
}

#[test]
fn test_copy_plan_for_fork() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("plans");
    let src_sid = "fork-src";
    let dst_sid = "fork-dst";

    write_plan(src_sid, &plans_dir, "# Source Plan", None).unwrap();

    let result = copy_plan_for_fork(src_sid, dst_sid, &plans_dir);
    assert!(result);

    let dst_content = get_plan(dst_sid, &plans_dir, None).unwrap();
    assert_eq!(dst_content, "# Source Plan");

    clear_plan_slug(src_sid);
    clear_plan_slug(dst_sid);
}
