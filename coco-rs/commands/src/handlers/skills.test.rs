use super::*;
use std::fs;

#[tokio::test]
async fn handler_lists_bundled_skills() {
    // No project/user skills — should still list the bundled set.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("home");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();

    // Bundled skills are always present (register_bundled_skills runs in
    // build_manager). We only check the shape — skill names drift as new
    // bundled skills land.
    assert!(
        out.contains("skill(s) loaded"),
        "expected count line, got: {out}"
    );
    assert!(
        out.contains("[bundled]"),
        "expected bundled tag, got: {out}"
    );
}

#[tokio::test]
async fn handler_picks_up_project_skill_md() {
    // Drop a SKILL.md into <cwd>/.claude/skills/foo/, expect /skills list
    // to include it tagged as project source.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("project");
    let skill_dir = cwd.join(".claude").join("skills").join("foo");
    fs::create_dir_all(&skill_dir).unwrap();
    // Strict TS-parity layout: frontmatter at the top of the file, no
    // leading `# Name` heading. The skill name is taken from the parent
    // directory (`foo`), never from a heading or the frontmatter `name`
    // field — see TS `loadSkillsDir.ts:452 const skillName = entry.name`.
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: a project skill\n---\n\nbody",
    )
    .unwrap();

    let config_home = tmp.path().join("home");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();

    assert!(out.contains("/foo"), "expected /foo in output: {out}");
    assert!(out.contains("project"), "expected project tag: {out}");
    assert!(
        out.contains("a project skill"),
        "expected description: {out}"
    );
}

#[tokio::test]
async fn show_unknown_skill_reports_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("p");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("h");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("show no-such-skill", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(out.contains("No skill named"));
    assert!(out.contains("no-such-skill"));
}

#[tokio::test]
async fn show_without_name_returns_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("p");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("h");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("show", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(out.contains("Usage: /skills show"));
}

#[tokio::test]
async fn paths_lists_bundled_first() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("p");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("h");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("paths", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(out.contains("bundled"));
    assert!(out.contains(".coco/skills") || out.contains(".coco\\skills"));
    assert!(out.contains(".claude/skills") || out.contains(".claude\\skills"));
}

#[tokio::test]
async fn list_includes_run_hint() {
    // Listing must tell the user how to actually invoke a skill, since
    // TS doesn't expose invoke-from-menu either — `/<name>` is the path.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("p");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("h");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        out.contains("To run a skill"),
        "expected run hint, got: {out}"
    );
}

#[tokio::test]
async fn name_shortcut_resolves_to_show() {
    // `/skills run-skill-generator` should behave like
    // `/skills show run-skill-generator` — TS analogue of clicking the
    // skill in the menu. Bundled name `run-skill-generator` is stable.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("p");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("h");
    fs::create_dir_all(&config_home).unwrap();

    let bundled_name = "run-skill-generator";
    let direct = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render(bundled_name, &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    let via_show = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render(&format!("show {bundled_name}"), &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(direct, via_show, "shortcut must equal `show <name>` output");
    assert!(direct.contains(&format!("# {bundled_name}")));
}

#[tokio::test]
async fn unknown_subcommand_returns_usage_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("p");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("h");
    fs::create_dir_all(&config_home).unwrap();

    let out = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || render("foobar", &config_home, &cwd)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(out.contains("Unknown /skills subcommand"));
    assert!(out.contains("Usage"));
}
