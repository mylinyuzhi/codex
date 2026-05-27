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

#[tokio::test]
async fn build_dialog_payload_groups_project_skills_under_project() {
    // Drop a project SKILL.md and verify the dialog payload buckets it
    // under `Project` (proves source-tagging works through `load_scoped`
    // — the earlier `load_from_dirs` flavor would have tagged it as
    // `User`, collapsing the project/user split in the overlay).
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("project");
    let skill_dir = cwd.join(".claude").join("skills").join("foo");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: a project skill\n---\n\nbody",
    )
    .unwrap();
    let config_home = tmp.path().join("home");
    fs::create_dir_all(&config_home).unwrap();

    let payload = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || {
            build_dialog_payload(
                &config_home,
                &cwd,
                &coco_config::SkillOverrideTiers::default(),
            )
        }
    })
    .await
    .unwrap();

    // Project skill `foo` shows up under Project.
    let project_names: Vec<_> = payload
        .entries
        .iter()
        .filter(|e| matches!(e.source, SkillsDialogSource::Project))
        .map(|e| e.name.as_str())
        .collect();
    assert!(
        project_names.contains(&"foo"),
        "expected project skill `foo` in payload, got: {project_names:?}"
    );

    // 2.1.142 dialog ships a flat list (no per-source subtitles).
    // The renderer derives source labels inline from each entry's
    // `source` field, so we only assert the entry surfaced.
    assert!(payload.bytes_per_token > 0);
}

#[tokio::test]
async fn build_dialog_payload_loads_coco_skills_dir_too() {
    // coco-rs supports a second project root: `<cwd>/.coco/skills/`
    // (canonical) in addition to `<cwd>/.claude/skills/` (TS-compat).
    // Both must surface in the dialog under `Project`.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("project");
    let coco_skill = cwd.join(".coco").join("skills").join("bar");
    fs::create_dir_all(&coco_skill).unwrap();
    fs::write(
        coco_skill.join("SKILL.md"),
        "---\ndescription: a coco-rs project skill\n---\n\nbody",
    )
    .unwrap();
    let config_home = tmp.path().join("home");
    fs::create_dir_all(&config_home).unwrap();

    let payload = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || {
            build_dialog_payload(
                &config_home,
                &cwd,
                &coco_config::SkillOverrideTiers::default(),
            )
        }
    })
    .await
    .unwrap();

    let project_names: Vec<_> = payload
        .entries
        .iter()
        .filter(|e| matches!(e.source, SkillsDialogSource::Project))
        .map(|e| e.name.as_str())
        .collect();
    assert!(
        project_names.contains(&"bar"),
        "expected `.coco/skills/bar` in payload as Project, got: {project_names:?}"
    );
}

#[tokio::test]
async fn skills_handler_no_args_opens_dialog() {
    // No-args path emits OpenDialog so the TUI gets a real overlay.
    let h = SkillsHandler;
    let result = h.execute_command("").await.unwrap();
    // Variant shape only — entry contents depend on the process cwd
    // (`SkillsHandler` uses `std::env::current_dir()`), so a content
    // assertion would be flaky across test runners. Content checks
    // live in `build_dialog_payload_*` tests with controlled tmpdirs.
    assert!(
        matches!(
            result,
            CommandResult::OpenDialog(DialogSpec::SkillsList { .. })
        ),
        "expected OpenDialog(SkillsList), got: {result:?}"
    );
}

#[tokio::test]
async fn build_dialog_payload_includes_bundled_skills_as_built_in_source() {
    // 2.1.142 parity: bundled skills surface in the dialog so users
    // can toggle a noisy in-binary skill. Source is `BuiltIn`.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("empty-project");
    fs::create_dir_all(&cwd).unwrap();
    let config_home = tmp.path().join("empty-home");
    fs::create_dir_all(&config_home).unwrap();

    let payload = tokio::task::spawn_blocking({
        let config_home = config_home.clone();
        let cwd = cwd.clone();
        move || {
            build_dialog_payload(
                &config_home,
                &cwd,
                &coco_config::SkillOverrideTiers::default(),
            )
        }
    })
    .await
    .unwrap();

    let built_in_count = payload
        .entries
        .iter()
        .filter(|e| matches!(e.source, SkillsDialogSource::BuiltIn))
        .count();
    assert!(
        built_in_count > 0,
        "bundled skills must surface as BuiltIn entries; got: {names:?}",
        names = payload
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.source))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn skills_handler_with_args_returns_text() {
    // Sub-command path stays text so SDK / scripted callers can parse it.
    let h = SkillsHandler;
    let result = h.execute_command("list").await.unwrap();
    match result {
        CommandResult::Text(out) => {
            assert!(out.contains("skill(s) loaded"), "unexpected text: {out}");
        }
        other => panic!("expected Text, got: {other:?}"),
    }
}
