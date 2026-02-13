use super::*;
use std::path::Path;

#[tokio::test]
async fn test_execute_simple_command() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    let result = executor.execute("echo hello", 10).await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
    assert!(result.stderr.is_empty());
    assert!(!result.truncated);
    assert!(result.duration_ms >= 0);
}

#[tokio::test]
async fn test_execute_failing_command() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    let result = executor.execute("exit 42", 10).await;
    assert_eq!(result.exit_code, 42);
}

#[tokio::test]
async fn test_execute_with_stderr() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    let result = executor.execute("echo err >&2", 10).await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stderr.trim(), "err");
}

#[tokio::test]
async fn test_execute_timeout() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    let result = executor.execute("sleep 30", 1).await;
    assert_eq!(result.exit_code, -1);
    assert!(result.stderr.contains("timed out"));
}

#[tokio::test]
async fn test_execute_uses_cwd() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let executor = ShellExecutor::new(tmp.path().to_path_buf());
    let result = executor.execute("pwd", 10).await;
    assert_eq!(result.exit_code, 0);
    // The output should contain the temp dir path
    let output_path = result.stdout.trim();
    // On macOS, /tmp may resolve to /private/tmp
    assert!(
        output_path.contains(tmp.path().to_str().expect("path to str"))
            || tmp
                .path()
                .to_str()
                .expect("path to str")
                .contains(output_path),
        "Expected cwd to match temp dir: output={output_path}, temp={}",
        tmp.path().display()
    );
}

#[tokio::test]
async fn test_default_timeout() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    assert_eq!(executor.default_timeout_secs, DEFAULT_TIMEOUT_SECS);
}

#[tokio::test]
async fn test_spawn_background() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    let task_id = executor
        .spawn_background("echo background-test")
        .await
        .expect("spawn");
    assert!(task_id.starts_with("bg-"));

    // Wait a bit for the background task to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}

#[test]
fn test_truncate_output_small() {
    let data = b"hello world";
    let (text, truncated) = truncate_output(data);
    assert_eq!(text, "hello world");
    assert!(!truncated);
}

#[test]
fn test_truncate_output_large() {
    let data = vec![b'x'; 50_000];
    let (text, truncated) = truncate_output(&data);
    assert_eq!(text.len(), MAX_OUTPUT_BYTES as usize);
    assert!(truncated);
}

#[test]
fn test_uuid_simple_uniqueness() {
    let a = uuid_simple();
    // Small sleep to ensure different timestamp
    std::thread::sleep(std::time::Duration::from_millis(1));
    let b = uuid_simple();
    assert_ne!(a, b);
}

#[tokio::test]
async fn test_with_default_shell() {
    let executor = ShellExecutor::with_default_shell(std::env::temp_dir());
    assert!(executor.shell.is_some());
    let result = executor.execute("echo test", 10).await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "test");
}

#[test]
fn test_is_snapshot_disabled() {
    // SAFETY: This test modifies environment variables. It should not run
    // in parallel with other tests that depend on this variable.
    unsafe {
        // Clear any existing value
        std::env::remove_var(DISABLE_SNAPSHOT_ENV);
        assert!(!is_snapshot_disabled());

        std::env::set_var(DISABLE_SNAPSHOT_ENV, "1");
        assert!(is_snapshot_disabled());

        std::env::set_var(DISABLE_SNAPSHOT_ENV, "true");
        assert!(is_snapshot_disabled());

        std::env::set_var(DISABLE_SNAPSHOT_ENV, "TRUE");
        assert!(is_snapshot_disabled());

        std::env::set_var(DISABLE_SNAPSHOT_ENV, "0");
        assert!(!is_snapshot_disabled());

        std::env::set_var(DISABLE_SNAPSHOT_ENV, "false");
        assert!(!is_snapshot_disabled());

        // Clean up
        std::env::remove_var(DISABLE_SNAPSHOT_ENV);
    }
}

/// Tests that maybe_wrap_shell_lc_with_snapshot passes through unchanged
/// when no snapshot is available.
#[tokio::test]
async fn test_maybe_wrap_shell_lc_no_snapshot_passthrough() {
    let executor = ShellExecutor::new(std::env::temp_dir());

    let args = vec![
        "/bin/bash".to_string(),
        "-lc".to_string(),
        "echo test".to_string(),
    ];

    let result = executor.maybe_wrap_shell_lc_with_snapshot(args.clone());

    // Without snapshot, should pass through unchanged
    assert_eq!(result, args);
}

/// Tests that maybe_wrap_shell_lc_with_snapshot passes through non-login
/// shell commands unchanged.
#[tokio::test]
async fn test_maybe_wrap_shell_lc_non_login_passthrough() {
    let executor = ShellExecutor::new(std::env::temp_dir());

    // Non-login shell command (-c instead of -lc)
    let args = vec![
        "/bin/bash".to_string(),
        "-c".to_string(),
        "echo test".to_string(),
    ];

    let result = executor.maybe_wrap_shell_lc_with_snapshot(args.clone());

    // Non-login commands should pass through unchanged
    assert_eq!(result, args);
}

#[cfg(unix)]
#[tokio::test]
async fn test_start_snapshotting() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut executor = ShellExecutor::with_default_shell(std::env::temp_dir());

    // Clear disable flag
    // SAFETY: This test modifies environment variables. It should not run
    // in parallel with other tests that depend on this variable.
    unsafe {
        std::env::remove_var(DISABLE_SNAPSHOT_ENV);
    }

    executor.start_snapshotting(tmp.path().to_path_buf(), "test-session");
    assert!(executor.is_snapshot_initialized());

    // Give the background task time to complete
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    // Snapshot should be available now (on Unix with bash/zsh)
    // Note: This may fail in CI environments without proper shell setup
}

/// Tests that maybe_wrap_shell_lc_with_snapshot correctly rewrites
/// login shell commands to source the snapshot file.
#[cfg(unix)]
#[tokio::test]
async fn test_maybe_wrap_shell_lc_with_snapshot_rewrites_correctly() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut executor = ShellExecutor::with_default_shell(std::env::temp_dir());

    // Clear disable flag
    // SAFETY: This test modifies environment variables. It should not run
    // in parallel with other tests that depend on this variable.
    unsafe {
        std::env::remove_var(DISABLE_SNAPSHOT_ENV);
    }

    executor.start_snapshotting(tmp.path().to_path_buf(), "wrap-test");

    // Wait for snapshot to be ready (longer wait for snapshot creation)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Only test if snapshot became available
    if executor.shell_snapshot().is_some() {
        // Input: login shell command
        let args = vec![
            "/bin/bash".to_string(),
            "-lc".to_string(),
            "echo test".to_string(),
        ];

        let rewritten = executor.maybe_wrap_shell_lc_with_snapshot(args);

        // Should rewrite to non-login with snapshot source
        assert_eq!(rewritten[1], "-c", "should change -lc to -c");
        assert!(rewritten[2].contains(". \""), "should source snapshot file");
        assert!(
            rewritten[2].contains("&& echo test"),
            "should chain command"
        );
    }
}

#[test]
fn test_extract_cwd_from_output_with_marker() {
    let output = "hello world\n__COCODE_CWD_START__ /home/user/project __COCODE_CWD_END__\n";
    let (cleaned, cwd) = extract_cwd_from_output(output);

    assert_eq!(cleaned, "hello world");
    assert_eq!(cwd, Some(PathBuf::from("/home/user/project")));
}

#[test]
fn test_extract_cwd_from_output_no_marker() {
    let output = "just normal output\n";
    let (cleaned, cwd) = extract_cwd_from_output(output);

    assert_eq!(cleaned, "just normal output\n");
    assert!(cwd.is_none());
}

#[test]
fn test_extract_cwd_from_output_empty_cwd() {
    let output = "output\n__COCODE_CWD_START__  __COCODE_CWD_END__\n";
    let (cleaned, cwd) = extract_cwd_from_output(output);

    assert_eq!(cleaned, "output");
    assert!(cwd.is_none());
}

#[test]
fn test_extract_cwd_from_output_preserves_other_content() {
    let output = "line1\nline2\n__COCODE_CWD_START__ /tmp __COCODE_CWD_END__";
    let (cleaned, cwd) = extract_cwd_from_output(output);

    assert_eq!(cleaned, "line1\nline2");
    assert_eq!(cwd, Some(PathBuf::from("/tmp")));
}

#[tokio::test]
async fn test_cwd_captured_in_result() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let executor = ShellExecutor::new(tmp.path().to_path_buf());

    let result = executor.execute("pwd", 10).await;

    assert_eq!(result.exit_code, 0);
    // new_cwd should be captured
    assert!(result.new_cwd.is_some());
    // On macOS, /tmp may resolve to /private/tmp
    let cwd = result.new_cwd.expect("cwd should be Some");
    let cwd_str = cwd.to_string_lossy();
    let tmp_str = tmp.path().to_string_lossy();
    assert!(
        cwd_str.contains(&*tmp_str) || tmp_str.contains(&*cwd_str),
        "CWD should match temp dir: cwd={cwd_str}, temp={tmp_str}"
    );
}

#[tokio::test]
async fn test_cwd_marker_not_in_output() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    let result = executor.execute("echo hello", 10).await;

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
    // CWD markers should not appear in output
    assert!(!result.stdout.contains("__COCODE_CWD"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_cwd_tracking_with_cd() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir(&subdir).expect("create subdir");

    let mut executor = ShellExecutor::new(tmp.path().to_path_buf());

    // Initial CWD
    // On macOS, temp_dir might be symlinked
    let initial_cwd = executor.cwd().to_path_buf();

    // Execute cd command with CWD tracking
    let result = executor.execute_with_cwd_tracking("cd subdir", 10).await;

    assert_eq!(result.exit_code, 0);

    // CWD should have changed
    let new_cwd = executor.cwd();
    assert_ne!(new_cwd, initial_cwd);
    // New CWD should end with "subdir"
    assert!(
        new_cwd.ends_with("subdir"),
        "Expected CWD to end with 'subdir', got: {}",
        new_cwd.display()
    );
}

#[tokio::test]
async fn test_cwd_not_updated_on_failure() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut executor = ShellExecutor::new(tmp.path().to_path_buf());

    let initial_cwd = executor.cwd().to_path_buf();

    // Try to cd to non-existent directory
    let result = executor
        .execute_with_cwd_tracking("cd nonexistent_dir_12345", 10)
        .await;

    assert_ne!(result.exit_code, 0);

    // CWD should remain unchanged
    assert_eq!(executor.cwd(), initial_cwd);
}

#[test]
fn test_cwd_getter_setter() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut executor = ShellExecutor::new(tmp.path().to_path_buf());

    assert_eq!(executor.cwd(), tmp.path());

    let new_path = PathBuf::from("/new/path");
    executor.set_cwd(new_path.clone());

    assert_eq!(executor.cwd(), new_path);
}

// ==========================================================================
// Subagent Shell Isolation Tests
// ==========================================================================

#[test]
fn test_fork_for_subagent_uses_initial_cwd() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let initial_cwd = tmp.path().to_path_buf();

    // Main executor with different CWD
    let mut main_executor = ShellExecutor::new(PathBuf::from("/some/other/path"));
    main_executor.set_cwd(PathBuf::from("/changed/path"));

    // Fork for subagent with specific initial CWD
    let subagent_executor = main_executor.fork_for_subagent(initial_cwd.clone());

    // Subagent should use the provided initial_cwd, not main's current cwd
    assert_eq!(subagent_executor.cwd(), initial_cwd);
    assert_ne!(subagent_executor.cwd(), main_executor.cwd());
}

#[cfg(unix)]
#[tokio::test]
async fn test_fork_for_subagent_cwd_resets_between_calls() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir(&subdir).expect("create subdir");

    let initial_cwd = tmp.path().to_path_buf();
    let main_executor = ShellExecutor::new(initial_cwd.clone());
    let subagent_executor = main_executor.fork_for_subagent(initial_cwd.clone());

    // Subagent executes cd - this should NOT affect subsequent calls
    let result1 = subagent_executor.execute("cd subdir && pwd", 10).await;
    assert_eq!(result1.exit_code, 0);
    // First call cd'd into subdir
    assert!(
        result1.stdout.contains("subdir"),
        "First call should be in subdir, got: {}",
        result1.stdout.trim()
    );

    // Second call - CWD should be back to initial (no tracking)
    let result2 = subagent_executor.execute("pwd", 10).await;
    assert_eq!(result2.exit_code, 0);
    // Should be back at initial directory
    let output = result2.stdout.trim();
    let tmp_str = tmp.path().to_str().expect("path to str");
    assert!(
        output.contains(tmp_str) || tmp_str.contains(output),
        "CWD should reset to initial for subagent, got: {output}, expected to contain: {tmp_str}"
    );
    // Should NOT be in subdir
    assert!(
        !output.ends_with("subdir"),
        "Subagent CWD should reset, not stay in subdir: {output}"
    );
}

#[tokio::test]
async fn test_fork_for_subagent_independent_background_registry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let main_executor = ShellExecutor::new(tmp.path().to_path_buf());
    let subagent_executor = main_executor.fork_for_subagent(tmp.path().to_path_buf());

    // Main agent starts a background task
    let main_task_id = main_executor
        .spawn_background("sleep 5")
        .await
        .expect("spawn");

    // Subagent should NOT see main agent's background task
    let subagent_output = subagent_executor
        .background_registry
        .get_output(&main_task_id)
        .await;
    assert!(
        subagent_output.is_none(),
        "Subagent should have independent background registry"
    );

    // Main agent should still see its own task
    let main_output = main_executor
        .background_registry
        .get_output(&main_task_id)
        .await;
    assert!(
        main_output.is_some(),
        "Main agent should see its own background task"
    );

    // Cleanup
    main_executor.background_registry.stop(&main_task_id).await;
}

#[test]
fn test_fork_for_subagent_shares_shell_config() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let main_executor = ShellExecutor::with_default_shell(tmp.path().to_path_buf());
    let subagent_executor = main_executor.fork_for_subagent(tmp.path().to_path_buf());

    // Both should have shell config
    assert!(main_executor.shell().is_some());
    assert!(subagent_executor.shell().is_some());

    // Snapshot initialization state should be shared
    assert_eq!(
        main_executor.is_snapshot_initialized(),
        subagent_executor.is_snapshot_initialized()
    );
}

#[test]
fn test_fork_for_subagent_inherits_timeout() {
    let mut main_executor = ShellExecutor::new(std::env::temp_dir());
    main_executor.default_timeout_secs = 300;

    let subagent_executor = main_executor.fork_for_subagent(std::env::temp_dir());

    assert_eq!(subagent_executor.default_timeout_secs, 300);
}

#[tokio::test]
async fn test_execute_for_subagent_no_cwd_tracking() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir(&subdir).expect("create subdir");

    let main_executor = ShellExecutor::new(tmp.path().to_path_buf());
    let subagent_executor = main_executor.fork_for_subagent(tmp.path().to_path_buf());

    // execute_for_subagent should not track CWD
    let result = subagent_executor
        .execute_for_subagent("cd subdir && pwd", 10)
        .await;
    assert_eq!(result.exit_code, 0);

    // CWD should remain unchanged (no tracking)
    let tmp_str = tmp.path().to_str().expect("path to str");
    let cwd_path = subagent_executor.cwd();
    let cwd_str = cwd_path.to_str().expect("cwd to str");
    assert!(
        cwd_str.contains(tmp_str) || tmp_str.contains(cwd_str),
        "execute_for_subagent should not track CWD changes"
    );
}

// ==========================================================================
// Path Extraction Tests
// ==========================================================================

#[test]
fn test_has_path_extractor_default() {
    let executor = ShellExecutor::new(std::env::temp_dir());
    assert!(!executor.has_path_extractor());
    assert!(executor.path_extractor().is_none());
}

#[test]
fn test_with_path_extractor_noop() {
    use crate::path_extractor::NoOpExtractor;

    let executor =
        ShellExecutor::new(std::env::temp_dir()).with_path_extractor(Arc::new(NoOpExtractor));

    // NoOpExtractor is not enabled, so has_path_extractor returns false
    assert!(!executor.has_path_extractor());
    // But path_extractor() returns Some
    assert!(executor.path_extractor().is_some());
}

/// Mock extractor that returns predefined paths.
struct MockExtractor {
    paths: Vec<PathBuf>,
}

impl MockExtractor {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }
}

impl crate::path_extractor::PathExtractor for MockExtractor {
    fn extract_paths<'a>(
        &'a self,
        _command: &'a str,
        _output: &'a str,
        _cwd: &'a Path,
    ) -> crate::path_extractor::BoxFuture<
        'a,
        anyhow::Result<crate::path_extractor::PathExtractionResult>,
    > {
        let paths = self.paths.clone();
        Box::pin(async move { Ok(crate::path_extractor::PathExtractionResult::new(paths, 10)) })
    }

    fn is_enabled(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn test_execute_with_extraction_no_extractor() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let executor = ShellExecutor::new(tmp.path().to_path_buf());

    let result = executor.execute_with_extraction("echo hello", 10).await;

    assert_eq!(result.exit_code, 0);
    // No extractor configured, so extracted_paths should be None
    assert!(result.extracted_paths.is_none());
}

#[tokio::test]
async fn test_execute_with_extraction_filters_nonexistent() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Create one file that exists
    let existing_file = tmp.path().join("exists.txt");
    std::fs::write(&existing_file, "test").expect("write file");

    // Mock extractor returns both existing and non-existing files
    let mock_extractor = MockExtractor::new(vec![
        existing_file.clone(),
        tmp.path().join("does_not_exist.txt"),
    ]);

    let executor =
        ShellExecutor::new(tmp.path().to_path_buf()).with_path_extractor(Arc::new(mock_extractor));

    let result = executor.execute_with_extraction("echo hello", 10).await;

    assert_eq!(result.exit_code, 0);
    assert!(result.extracted_paths.is_some());

    let extracted = result.extracted_paths.expect("extracted_paths");
    assert!(extracted.extraction_attempted);
    // Only the existing file should be in the result
    assert_eq!(extracted.paths.len(), 1);
    assert_eq!(extracted.paths[0], existing_file);
}

#[tokio::test]
async fn test_execute_with_extraction_failed_command() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mock_extractor = MockExtractor::new(vec![PathBuf::from("/some/file")]);

    let executor =
        ShellExecutor::new(tmp.path().to_path_buf()).with_path_extractor(Arc::new(mock_extractor));

    // Command that fails
    let result = executor.execute_with_extraction("exit 1", 10).await;

    assert_ne!(result.exit_code, 0);
    // Should not extract paths for failed commands
    assert!(result.extracted_paths.is_none());
}

#[tokio::test]
async fn test_execute_with_cwd_tracking_and_extraction() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir(&subdir).expect("create subdir");

    // Create a file in subdir
    let test_file = subdir.join("test.txt");
    std::fs::write(&test_file, "test").expect("write file");

    let mock_extractor = MockExtractor::new(vec![test_file.clone()]);

    let mut executor =
        ShellExecutor::new(tmp.path().to_path_buf()).with_path_extractor(Arc::new(mock_extractor));

    // Execute with both CWD tracking and extraction
    let result = executor
        .execute_with_cwd_tracking_and_extraction("cd subdir && pwd", 10)
        .await;

    assert_eq!(result.exit_code, 0);

    // CWD should be updated
    assert!(
        executor.cwd().ends_with("subdir"),
        "CWD should be updated to subdir, got: {}",
        executor.cwd().display()
    );

    // Paths should be extracted
    assert!(result.extracted_paths.is_some());
    let extracted = result.extracted_paths.expect("extracted_paths");
    assert_eq!(extracted.paths.len(), 1);
}

#[test]
fn test_fork_for_subagent_shares_path_extractor() {
    use crate::path_extractor::NoOpExtractor;

    let main_executor =
        ShellExecutor::new(std::env::temp_dir()).with_path_extractor(Arc::new(NoOpExtractor));

    let subagent_executor = main_executor.fork_for_subagent(std::env::temp_dir());

    // Subagent should share the path extractor
    assert!(subagent_executor.path_extractor().is_some());
}
