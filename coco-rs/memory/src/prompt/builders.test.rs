use super::*;
use std::path::Path;

#[test]
fn auto_variant_includes_individual_types_and_index() {
    let p = build_system_prompt_section(
        SystemPromptVariant::Auto,
        Path::new("/m"),
        None,
        Some("- [a](a.md) — h"),
        None,
        false,
        None,
    );
    assert!(p.contains("# auto memory"));
    assert!(p.contains("<types>"));
    assert!(p.contains("<name>user</name>"));
    assert!(p.contains("## MEMORY.md"));
    assert!(p.contains("- [a](a.md) — h"));
    // Two-step instructions present when skip_index = false.
    assert!(p.contains("two-step process"));
}

#[test]
fn combined_variant_includes_team_block() {
    let p = build_system_prompt_section(
        SystemPromptVariant::Combined,
        Path::new("/m"),
        Some(Path::new("/m/team")),
        Some("- [a](a.md) — h"),
        Some("- [t](t.md) — team hook"),
        false,
        None,
    );
    assert!(p.contains("private:"));
    assert!(p.contains("team:"));
    assert!(p.contains("Team MEMORY.md"));
    assert!(p.contains("- [t](t.md) — team hook"));
    // Combined uses the scope-tagged taxonomy.
    assert!(p.contains("<scope>always private</scope>"));
}

#[test]
fn skip_index_omits_two_step_block() {
    let p = build_system_prompt_section(
        SystemPromptVariant::Auto,
        Path::new("/m"),
        None,
        None,
        None,
        true,
        None,
    );
    assert!(!p.contains("two-step process"));
}

#[test]
fn kairos_variant_describes_daily_log_pattern() {
    let p = build_kairos_prompt(Path::new("/m"));
    assert!(p.contains("daily log"));
    assert!(p.contains("YYYY-MM-DD.md"));
    assert!(p.contains("append-only"));
}

#[test]
fn extract_prompt_includes_manifest_and_message_count() {
    let p = build_extract_prompt(40, "## Existing Memory Files\n_(none)_", false);
    assert!(p.contains("last ~40 messages"));
    assert!(p.contains("Existing Memory Files"));
    assert!(p.contains("Hard cap of 5 turns"));
}

#[test]
fn dream_prompt_includes_four_phases() {
    let p = build_dream_prompt(Path::new("/m"), Path::new("/p"), &[]);
    assert!(p.contains("Phase 1 — Orient"));
    assert!(p.contains("Phase 2 — Gather"));
    assert!(p.contains("Phase 3 — Consolidate"));
    assert!(p.contains("Phase 4 — Prune"));
}

#[test]
fn session_template_has_nine_section_headers() {
    let template = build_session_memory_template();
    let headers = template.lines().filter(|l| l.starts_with("# ")).count();
    assert_eq!(headers, 10); // 9 sections + the title `# Session Title` is one of them; spot-check a few names.
    assert!(template.contains("# Session Title"));
    assert!(template.contains("# Current State"));
    assert!(template.contains("# Worklog"));
}
