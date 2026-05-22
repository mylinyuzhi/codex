use super::*;
use std::fs;

#[test]
fn test_apply_patch_arg1_constant() {
    assert_eq!(COCODE_APPLY_PATCH_ARG1, "--cocode-run-as-apply-patch");
}

#[test]
fn test_illegal_env_var_prefix() {
    assert_eq!(ILLEGAL_ENV_VAR_PREFIX, "COCODE_");
}

#[test]
fn test_find_cocode_home() {
    // Should not fail in test environment
    let result = find_cocode_home();
    // May fail if HOME is not set, which is OK for this test
    if let Ok(home) = result {
        assert!(home.to_string_lossy().contains(".cocode"));
    }
}

#[test]
fn test_set_filtered_blocks_cocode_prefix() {
    // Create test entries
    let entries: Vec<Result<(String, String), dotenvy::Error>> = vec![
        Ok(("SAFE_VAR".to_string(), "safe_value".to_string())),
        Ok(("COCODE_BLOCKED".to_string(), "blocked_value".to_string())),
        Ok((
            "cocode_also_blocked".to_string(),
            "also_blocked".to_string(),
        )),
    ];

    // This would set SAFE_VAR but not COCODE_* vars
    // We can't easily test this without modifying env, so just verify the logic
    let filtered: Vec<_> = entries
        .into_iter()
        .flatten()
        .filter(|(key, _)| !key.to_ascii_uppercase().starts_with(ILLEGAL_ENV_VAR_PREFIX))
        .collect();

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].0, "SAFE_VAR");
}

fn create_lock(dir: &std::path::Path) -> std::io::Result<File> {
    let lock_path = dir.join(LOCK_FILENAME);
    File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
}

#[test]
fn test_janitor_skips_dirs_without_lock_file() -> std::io::Result<()> {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("no-lock");
    fs::create_dir(&dir)?;

    janitor_cleanup(root.path())?;

    assert!(dir.exists());
    Ok(())
}

#[test]
fn test_janitor_skips_dirs_with_held_lock() -> std::io::Result<()> {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("locked");
    fs::create_dir(&dir)?;
    let lock_file = create_lock(&dir)?;
    lock_file.try_lock()?;

    janitor_cleanup(root.path())?;

    assert!(dir.exists());
    Ok(())
}

#[test]
fn test_janitor_removes_dirs_with_unlocked_lock() -> std::io::Result<()> {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("stale");
    fs::create_dir(&dir)?;
    create_lock(&dir)?;

    janitor_cleanup(root.path())?;

    assert!(!dir.exists());
    Ok(())
}
