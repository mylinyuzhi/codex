//! Shell command executor with timeout, background support, and shell snapshotting.
//!
//! ## Sandbox Mode
//!
//! This executor currently runs in **non-sandbox mode** by default, which means
//! commands execute directly without any sandbox restrictions. This matches
//! Claude Code's architecture where sandbox is optional and disabled by default.
//!
//! To check if a command should be sandboxed, use [`cocode_sandbox::SandboxSettings::is_sandboxed()`].
//! When sandbox mode is enabled in the future, the executor will wrap commands with
//! platform-specific sandbox enforcement (Landlock on Linux, Seatbelt on macOS).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::background::BackgroundProcess;
use crate::background::BackgroundTaskRegistry;
use crate::command::CommandResult;
use crate::command::ExecuteResult;
use crate::command::ExtractedPaths;
use crate::path_extractor::PathExtractor;
use crate::path_extractor::filter_existing_files;
use crate::path_extractor::truncate_for_extraction;
use crate::shell_types::Shell;
use crate::shell_types::default_user_shell;
use crate::signal;
use crate::snapshot::ShellSnapshot;
use crate::snapshot::SnapshotConfig;

/// Default command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: i64 = 120;

/// Maximum output size in bytes before truncation (30KB).
const MAX_OUTPUT_BYTES: i64 = 30_000;

/// Environment variable to disable shell snapshotting.
const DISABLE_SNAPSHOT_ENV: &str = "COCODE_DISABLE_SHELL_SNAPSHOT";

/// Marker for CWD extraction from command output (start).
const CWD_MARKER_START: &str = "__COCODE_CWD_START__";

/// Marker for CWD extraction from command output (end).
const CWD_MARKER_END: &str = "__COCODE_CWD_END__";

/// Shell command executor.
///
/// Provides async execution of shell commands with timeout support,
/// output capture, background task management, and optional shell
/// environment snapshotting.
///
/// ## Shell Snapshotting
///
/// When enabled (default), the executor captures the user's shell environment
/// (aliases, functions, exports, options) and sources it before each command.
/// This ensures commands run with the same environment as the user's interactive shell.
///
/// To disable snapshotting, set the environment variable:
/// ```sh
/// export COCODE_DISABLE_SHELL_SNAPSHOT=1
/// ```
///
/// ## Path Extraction
///
/// When a path extractor is configured (via `with_path_extractor`), the executor
/// can extract file paths from command output for fast model pre-reading.
/// Use `execute_with_extraction` to enable this feature.
#[derive(Clone)]
pub struct ShellExecutor {
    /// Default timeout for command execution in seconds.
    pub default_timeout_secs: i64,
    /// Working directory for command execution (shared across clones).
    cwd: Arc<StdMutex<PathBuf>>,
    /// Registry for background tasks.
    pub background_registry: BackgroundTaskRegistry,
    /// Shell configuration with optional snapshot.
    shell: Option<Shell>,
    /// Whether snapshot was initialized.
    snapshot_initialized: bool,
    /// Optional path extractor for extracting file paths from command output.
    path_extractor: Option<Arc<dyn PathExtractor>>,
}

impl std::fmt::Debug for ShellExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellExecutor")
            .field("default_timeout_secs", &self.default_timeout_secs)
            .field("cwd", &*self.cwd.lock().unwrap())
            .field("background_registry", &self.background_registry)
            .field("shell", &self.shell)
            .field("snapshot_initialized", &self.snapshot_initialized)
            .field("path_extractor", &self.path_extractor.is_some())
            .finish()
    }
}

impl ShellExecutor {
    /// Creates a new executor with the given working directory.
    ///
    /// Shell snapshotting is **not** automatically started. Call `start_snapshotting()`
    /// or `with_shell()` to enable snapshot support.
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            default_timeout_secs: DEFAULT_TIMEOUT_SECS,
            cwd: Arc::new(StdMutex::new(cwd)),
            background_registry: BackgroundTaskRegistry::new(),
            shell: None,
            snapshot_initialized: false,
            path_extractor: None,
        }
    }

    /// Creates a new executor with the given shell configuration.
    ///
    /// The shell's snapshot receiver will be used if available.
    pub fn with_shell(cwd: PathBuf, shell: Shell) -> Self {
        Self {
            default_timeout_secs: DEFAULT_TIMEOUT_SECS,
            cwd: Arc::new(StdMutex::new(cwd)),
            background_registry: BackgroundTaskRegistry::new(),
            shell: Some(shell),
            snapshot_initialized: false,
            path_extractor: None,
        }
    }

    /// Creates a new executor with the user's default shell.
    pub fn with_default_shell(cwd: PathBuf) -> Self {
        Self::with_shell(cwd, default_user_shell())
    }

    /// Sets the path extractor for extracting file paths from command output.
    ///
    /// When a path extractor is configured, `execute_with_extraction()` can
    /// analyze command output to find file paths for fast model pre-reading.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cocode_shell::{ShellExecutor, NoOpExtractor};
    /// use std::sync::Arc;
    /// use std::path::PathBuf;
    ///
    /// let executor = ShellExecutor::new(PathBuf::from("/project"))
    ///     .with_path_extractor(Arc::new(NoOpExtractor));
    /// ```
    pub fn with_path_extractor(mut self, extractor: Arc<dyn PathExtractor>) -> Self {
        self.path_extractor = Some(extractor);
        self
    }

    /// Returns the configured path extractor, if any.
    pub fn path_extractor(&self) -> Option<&Arc<dyn PathExtractor>> {
        self.path_extractor.as_ref()
    }

    /// Returns true if a path extractor is configured and enabled.
    pub fn has_path_extractor(&self) -> bool {
        self.path_extractor.as_ref().is_some_and(|e| e.is_enabled())
    }

    /// Starts asynchronous shell snapshotting.
    ///
    /// This captures the user's shell environment in the background.
    /// The snapshot will be sourced before each command once available.
    ///
    /// If snapshotting is disabled via environment variable, this is a no-op.
    ///
    /// # Arguments
    ///
    /// * `cocode_home` - Path to cocode home directory (e.g., `~/.cocode`)
    /// * `session_id` - Unique session identifier for the snapshot file
    pub fn start_snapshotting(&mut self, cocode_home: PathBuf, session_id: &str) {
        if is_snapshot_disabled() {
            tracing::debug!("Shell snapshotting disabled via {DISABLE_SNAPSHOT_ENV}");
            self.snapshot_initialized = true;
            return;
        }

        // Initialize shell if not already set
        if self.shell.is_none() {
            self.shell = Some(default_user_shell());
        }

        if let Some(ref mut shell) = self.shell {
            let config = SnapshotConfig::new(&cocode_home);
            ShellSnapshot::start_snapshotting(config, session_id, shell);
            self.snapshot_initialized = true;
            tracing::debug!("Started shell snapshotting for session {session_id}");
        }
    }

    /// Returns the current shell configuration.
    pub fn shell(&self) -> Option<&Shell> {
        self.shell.as_ref()
    }

    /// Returns the current shell snapshot if available.
    pub fn shell_snapshot(&self) -> Option<Arc<ShellSnapshot>> {
        self.shell.as_ref().and_then(|s| s.shell_snapshot())
    }

    /// Returns whether snapshotting has been initialized.
    pub fn is_snapshot_initialized(&self) -> bool {
        self.snapshot_initialized
    }

    /// Returns the current working directory.
    pub fn cwd(&self) -> PathBuf {
        self.cwd.lock().unwrap().clone()
    }

    /// Updates the working directory.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        *self.cwd.lock().unwrap() = cwd;
    }

    /// Creates a shell executor for subagent use.
    ///
    /// The forked executor:
    /// - Uses the provided `initial_cwd` (not the current tracked CWD)
    /// - Shares the shell configuration and snapshot (Arc, read-only)
    /// - Has its own independent background task registry
    /// - Does NOT track CWD changes (always resets to initial)
    ///
    /// This matches Claude Code's behavior where subagents always
    /// have their CWD reset between bash calls. Subagents should use
    /// absolute paths since CWD resets between calls.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cocode_shell::ShellExecutor;
    /// use std::path::PathBuf;
    ///
    /// let main_executor = ShellExecutor::with_default_shell(PathBuf::from("/project"));
    /// let subagent_executor = main_executor.fork_for_subagent(PathBuf::from("/project"));
    ///
    /// // Subagent bash calls always start from initial CWD
    /// // cd in one call does NOT affect the next call
    /// ```
    pub fn fork_for_subagent(&self, initial_cwd: PathBuf) -> Self {
        Self {
            default_timeout_secs: self.default_timeout_secs,
            cwd: Arc::new(StdMutex::new(initial_cwd)), // Independent CWD for subagent
            background_registry: BackgroundTaskRegistry::new(), // Independent registry
            shell: self.shell.clone(), // Share shell config (Arc snapshot is shared)
            snapshot_initialized: self.snapshot_initialized,
            path_extractor: self.path_extractor.clone(), // Share path extractor
        }
    }

    /// Executes a command for subagent use (no CWD tracking).
    ///
    /// Unlike `execute_with_cwd_tracking`, this method:
    /// - Always uses the executor's current CWD setting
    /// - Does NOT update internal CWD state after execution
    /// - Suitable for subagent use where CWD should reset between calls
    ///
    /// This is essentially an alias for `execute()` to make the intent clear
    /// when used in subagent contexts.
    pub async fn execute_for_subagent(&self, command: &str, timeout_secs: i64) -> CommandResult {
        self.execute(command, timeout_secs).await
    }

    /// Executes a shell command with the given timeout.
    ///
    /// The command is run via the configured shell with the executor's working directory.
    /// If a shell snapshot is available and the command uses login shell mode (`-lc`),
    /// it is rewritten to source the snapshot via non-login shell (`-c`).
    /// Output is truncated if it exceeds the maximum size limit.
    ///
    /// If the command times out, a `CommandResult` is returned with exit code -1
    /// and a timeout message in stderr.
    pub async fn execute(&self, command: &str, timeout_secs: i64) -> CommandResult {
        let start = Instant::now();

        let timeout = if timeout_secs > 0 {
            timeout_secs
        } else {
            self.default_timeout_secs
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout as u64),
            self.run_command(command),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as i64;

        match result {
            Ok(cmd_result) => {
                let mut cmd_result = cmd_result;
                cmd_result.duration_ms = duration_ms;
                cmd_result
            }
            Err(_) => CommandResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Command timed out after {timeout} seconds"),
                duration_ms,
                truncated: false,
                new_cwd: None,
                extracted_paths: None,
            },
        }
    }

    /// Executes a command and updates CWD if changed.
    ///
    /// This is similar to `execute()` but also tracks working directory changes.
    /// If the command succeeds and the CWD changed, the executor's internal CWD
    /// is updated to match.
    pub async fn execute_with_cwd_tracking(
        &mut self,
        command: &str,
        timeout_secs: i64,
    ) -> CommandResult {
        let result = self.execute(command, timeout_secs).await;

        // Update internal CWD if command succeeded and CWD changed
        if result.exit_code == 0 {
            if let Some(ref new_cwd) = result.new_cwd {
                let current_cwd = self.cwd.lock().unwrap().clone();
                if new_cwd.exists() && *new_cwd != current_cwd {
                    tracing::debug!(
                        "CWD changed: {} -> {}",
                        current_cwd.display(),
                        new_cwd.display()
                    );
                    *self.cwd.lock().unwrap() = new_cwd.clone();
                }
            }
        }

        result
    }

    /// Executes a command and extracts file paths from output.
    ///
    /// This combines command execution with path extraction for fast model pre-reading.
    /// If a path extractor is configured and the command succeeds, file paths are
    /// extracted from the output for preloading.
    ///
    /// The output is truncated to 2000 characters for extraction efficiency
    /// (matching Claude Code's behavior).
    ///
    /// # Arguments
    ///
    /// * `command` - The shell command to execute
    /// * `timeout_secs` - Timeout in seconds (0 uses default)
    ///
    /// # Returns
    ///
    /// A `CommandResult` with `extracted_paths` populated if extraction was performed.
    pub async fn execute_with_extraction(&self, command: &str, timeout_secs: i64) -> CommandResult {
        let mut result = self.execute(command, timeout_secs).await;

        // Only extract paths if command succeeded and extractor is available
        if result.exit_code == 0 && self.has_path_extractor() {
            if let Some(ref extractor) = self.path_extractor {
                let extraction_start = Instant::now();
                let cwd = self.cwd.lock().unwrap().clone();

                // Truncate output for extraction efficiency
                let output_for_extraction = truncate_for_extraction(&result.stdout);

                match extractor
                    .extract_paths(command, output_for_extraction, &cwd)
                    .await
                {
                    Ok(extraction_result) => {
                        // Filter to only existing files
                        let existing_paths = filter_existing_files(extraction_result.paths, &cwd);

                        let extraction_ms = extraction_start.elapsed().as_millis() as i64;

                        if !existing_paths.is_empty() {
                            tracing::debug!(
                                "Extracted {} file paths from command output in {}ms",
                                existing_paths.len(),
                                extraction_ms
                            );
                        }

                        result.extracted_paths =
                            Some(ExtractedPaths::new(existing_paths, extraction_ms));
                    }
                    Err(e) => {
                        // Log warning but don't fail the command
                        tracing::warn!("Path extraction failed: {e}");
                        result.extracted_paths = Some(ExtractedPaths::not_attempted());
                    }
                }
            }
        }

        result
    }

    /// Executes a command with both CWD tracking and path extraction.
    ///
    /// Combines the functionality of `execute_with_cwd_tracking` and
    /// `execute_with_extraction` for main agent use cases.
    pub async fn execute_with_cwd_tracking_and_extraction(
        &mut self,
        command: &str,
        timeout_secs: i64,
    ) -> CommandResult {
        let result = self.execute_with_extraction(command, timeout_secs).await;

        // Update internal CWD if command succeeded and CWD changed
        if result.exit_code == 0 {
            if let Some(ref new_cwd) = result.new_cwd {
                let current_cwd = self.cwd.lock().unwrap().clone();
                if new_cwd.exists() && *new_cwd != current_cwd {
                    tracing::debug!(
                        "CWD changed: {} -> {}",
                        current_cwd.display(),
                        new_cwd.display()
                    );
                    *self.cwd.lock().unwrap() = new_cwd.clone();
                }
            }
        }

        result
    }

    /// Executes a command that can be transitioned to background mid-flight.
    ///
    /// This method registers the command as backgroundable using the given
    /// `signal_id`, then runs the command. If the user triggers a background
    /// signal (e.g. via Ctrl+B), the child process is handed off to the
    /// `BackgroundTaskRegistry` and `ExecuteResult::Backgrounded` is returned.
    ///
    /// Otherwise the command completes normally and `ExecuteResult::Completed`
    /// is returned with the usual `CommandResult`.
    pub async fn execute_backgroundable(
        &self,
        command: &str,
        timeout_secs: i64,
        signal_id: &str,
    ) -> ExecuteResult {
        let start = Instant::now();
        let timeout = if timeout_secs > 0 {
            timeout_secs
        } else {
            self.default_timeout_secs
        };

        // Register for background signal
        let bg_rx = signal::register_backgroundable_bash(signal_id.to_string());

        let args = self.get_shell_args(command);
        let args = self.maybe_wrap_shell_lc_with_snapshot(args);
        let cwd = self.cwd.lock().unwrap().clone();

        // Wrap the script to capture CWD after execution
        let wrapped_script = format!(
            "{}; __cocode_exit=$?; echo '{}' \"$(pwd)\" '{}'; exit $__cocode_exit",
            &args[2], CWD_MARKER_START, CWD_MARKER_END
        );
        let shell_args = vec![args[0].clone(), args[1].clone(), wrapped_script];

        let child = tokio::process::Command::new(&shell_args[0])
            .args(&shell_args[1..])
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                signal::unregister_backgroundable_bash(signal_id);
                return ExecuteResult::Completed(CommandResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Failed to spawn command: {e}"),
                    duration_ms: 0,
                    truncated: false,
                    new_cwd: None,
                    extracted_paths: None,
                });
            }
        };

        // Take stdout/stderr handles and spawn async readers into shared buffers
        let stdout_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let stderr_buf = Arc::new(Mutex::new(Vec::<u8>::new()));

        let stdout_handle = if let Some(mut stdout) = child.stdout.take() {
            let buf = Arc::clone(&stdout_buf);
            Some(tokio::spawn(async move {
                let mut tmp = vec![0u8; 4096];
                loop {
                    match stdout.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => buf.lock().await.extend_from_slice(&tmp[..n]),
                        Err(_) => break,
                    }
                }
            }))
        } else {
            None
        };

        let stderr_handle = if let Some(mut stderr) = child.stderr.take() {
            let buf = Arc::clone(&stderr_buf);
            Some(tokio::spawn(async move {
                let mut tmp = vec![0u8; 4096];
                loop {
                    match stderr.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => buf.lock().await.extend_from_slice(&tmp[..n]),
                        Err(_) => break,
                    }
                }
            }))
        } else {
            None
        };

        // Race: process completion vs background signal vs timeout
        let timeout_dur = std::time::Duration::from_secs(timeout as u64);

        tokio::select! {
            // Background signal received — transition to background.
            // Only act on Ok(()) — Err means the sender was dropped (not a real signal).
            result = bg_rx => {
                if result.is_ok() {
                    signal::unregister_backgroundable_bash(signal_id);
                    let task_id = self
                        .transition_to_background(
                            command,
                            child,
                            Arc::clone(&stdout_buf),
                            Arc::clone(&stderr_buf),
                        )
                        .await;
                    return ExecuteResult::Backgrounded { task_id };
                }

                // Sender dropped without signaling — fall through to wait for completion
                signal::unregister_backgroundable_bash(signal_id);
                let status = child.wait().await;

                if let Some(h) = stdout_handle { let _ = h.await; }
                if let Some(h) = stderr_handle { let _ = h.await; }

                let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                let duration_ms = start.elapsed().as_millis() as i64;

                let raw_stdout_bytes = stdout_buf.lock().await;
                let raw_stderr_bytes = stderr_buf.lock().await;

                let (raw_stdout, truncated_stdout) = truncate_output(&raw_stdout_bytes);
                let (stderr, truncated_stderr) = truncate_output(&raw_stderr_bytes);
                let (stdout, new_cwd) = extract_cwd_from_output(&raw_stdout);

                ExecuteResult::Completed(CommandResult {
                    exit_code,
                    stdout,
                    stderr,
                    duration_ms,
                    truncated: truncated_stdout || truncated_stderr,
                    new_cwd,
                    extracted_paths: None,
                })
            }

            // Process completed
            status = child.wait() => {
                signal::unregister_backgroundable_bash(signal_id);

                // Wait for readers to finish
                if let Some(h) = stdout_handle { let _ = h.await; }
                if let Some(h) = stderr_handle { let _ = h.await; }

                let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                let duration_ms = start.elapsed().as_millis() as i64;

                let raw_stdout_bytes = stdout_buf.lock().await;
                let raw_stderr_bytes = stderr_buf.lock().await;

                let (raw_stdout, truncated_stdout) = truncate_output(&raw_stdout_bytes);
                let (stderr, truncated_stderr) = truncate_output(&raw_stderr_bytes);
                let (stdout, new_cwd) = extract_cwd_from_output(&raw_stdout);

                ExecuteResult::Completed(CommandResult {
                    exit_code,
                    stdout,
                    stderr,
                    duration_ms,
                    truncated: truncated_stdout || truncated_stderr,
                    new_cwd,
                    extracted_paths: None,
                })
            }

            // Timeout
            _ = tokio::time::sleep(timeout_dur) => {
                signal::unregister_backgroundable_bash(signal_id);
                // child is dropped here → kill_on_drop triggers
                drop(child);
                let duration_ms = start.elapsed().as_millis() as i64;
                ExecuteResult::Completed(CommandResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Command timed out after {timeout} seconds"),
                    duration_ms,
                    truncated: false,
                    new_cwd: None,
                    extracted_paths: None,
                })
            }
        }
    }

    /// Transition a running child process to the background task registry.
    ///
    /// Seeds the background output with any content already captured in
    /// stdout/stderr, then spawns a task that continues reading and waits
    /// for the process to complete.
    async fn transition_to_background(
        &self,
        command: &str,
        mut child: tokio::process::Child,
        stdout_buf: Arc<Mutex<Vec<u8>>>,
        stderr_buf: Arc<Mutex<Vec<u8>>>,
    ) -> String {
        let task_id = format!("bg-{}", uuid_simple());

        // Seed combined output with what we have so far
        let combined_output = Arc::new(Mutex::new(String::new()));
        {
            let stdout_data = stdout_buf.lock().await;
            let stderr_data = stderr_buf.lock().await;
            let mut out = combined_output.lock().await;
            let stdout_str = String::from_utf8_lossy(&stdout_data);
            let stderr_str = String::from_utf8_lossy(&stderr_data);
            if !stdout_str.is_empty() {
                out.push_str(&stdout_str);
            }
            if !stderr_str.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&stderr_str);
            }
        }

        let completed = Arc::new(Notify::new());
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let process = BackgroundProcess {
            id: task_id.clone(),
            command: command.to_string(),
            output: Arc::clone(&combined_output),
            completed: Arc::clone(&completed),
            cancel_token: cancel_token.clone(),
        };

        self.background_registry
            .register(task_id.clone(), process)
            .await;

        let registry = self.background_registry.clone();
        let bg_task_id = task_id.clone();

        tokio::spawn(async move {
            // Continue reading from stdout/stderr buffers and syncing to combined
            // output. The reader tasks spawned earlier are still running.
            let sync_output = Arc::clone(&combined_output);
            let sync_stdout = Arc::clone(&stdout_buf);
            let sync_stderr = Arc::clone(&stderr_buf);

            // Periodically sync buffer contents into the combined output
            let sync_task = tokio::spawn(async move {
                let mut last_stdout_len = 0usize;
                let mut last_stderr_len = 0usize;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    let stdout_data = sync_stdout.lock().await;
                    let stderr_data = sync_stderr.lock().await;

                    let new_stdout = stdout_data.len().saturating_sub(last_stdout_len);
                    let new_stderr = stderr_data.len().saturating_sub(last_stderr_len);

                    if new_stdout > 0 || new_stderr > 0 {
                        let mut out = sync_output.lock().await;
                        if new_stdout > 0 {
                            let chunk = String::from_utf8_lossy(&stdout_data[last_stdout_len..]);
                            out.push_str(&chunk);
                            last_stdout_len = stdout_data.len();
                        }
                        if new_stderr > 0 {
                            let chunk = String::from_utf8_lossy(&stderr_data[last_stderr_len..]);
                            out.push_str(&chunk);
                            last_stderr_len = stderr_data.len();
                        }
                    }
                }
            });

            tokio::select! {
                _ = child.wait() => {}
                _ = cancel_token.cancelled() => {}
            }

            sync_task.abort();

            // Final sync of remaining data — strip CWD markers from stdout
            {
                let stdout_data = stdout_buf.lock().await;
                let stderr_data = stderr_buf.lock().await;
                let mut out = combined_output.lock().await;
                out.clear();
                let raw_stdout = String::from_utf8_lossy(&stdout_data);
                let (clean_stdout, _cwd) = extract_cwd_from_output(&raw_stdout);
                let stderr_str = String::from_utf8_lossy(&stderr_data);
                if !clean_stdout.is_empty() {
                    out.push_str(&clean_stdout);
                }
                if !stderr_str.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&stderr_str);
                }
            }

            completed.notify_waiters();
            registry.stop(&bg_task_id).await;
        });

        task_id
    }

    /// Executes a command with backgrounding support and CWD tracking.
    ///
    /// Combines `execute_backgroundable()` with CWD update on completion.
    pub async fn execute_backgroundable_with_cwd_tracking(
        &mut self,
        command: &str,
        timeout_secs: i64,
        signal_id: &str,
    ) -> ExecuteResult {
        let result = self
            .execute_backgroundable(command, timeout_secs, signal_id)
            .await;

        if let ExecuteResult::Completed(ref cmd_result) = result {
            if cmd_result.exit_code == 0 {
                if let Some(ref new_cwd) = cmd_result.new_cwd {
                    let current_cwd = self.cwd.lock().unwrap().clone();
                    if new_cwd.exists() && *new_cwd != current_cwd {
                        tracing::debug!(
                            "CWD changed: {} -> {}",
                            current_cwd.display(),
                            new_cwd.display()
                        );
                        *self.cwd.lock().unwrap() = new_cwd.clone();
                    }
                }
            }
        }

        result
    }

    /// POSIX-only: rewrite login shell commands to source snapshot.
    ///
    /// For commands of the form `[shell, "-lc", "<script>"]`, when a snapshot
    /// is available, rewrite to `[shell, "-c", ". SNAPSHOT && <script>"]`.
    ///
    /// This preserves the semantic that login shell is used for snapshot capture,
    /// while non-login shell with snapshot sourcing is used for execution.
    fn maybe_wrap_shell_lc_with_snapshot(&self, args: Vec<String>) -> Vec<String> {
        let Some(snapshot) = self.shell_snapshot() else {
            return args;
        };

        // Only rewrite if snapshot file exists
        if !snapshot.path.exists() {
            return args;
        }

        // Require at least [shell, flag, script]
        if args.len() < 3 {
            return args;
        }

        // Only rewrite login shell commands (-lc)
        if args[1] != "-lc" {
            return args;
        }

        let snapshot_path = snapshot.path.to_string_lossy();
        let rewritten_script = format!(". \"{snapshot_path}\" && {}", args[2]);

        vec![args[0].clone(), "-c".to_string(), rewritten_script]
    }

    /// Spawns a command in the background and returns a task ID.
    ///
    /// The command output is captured asynchronously and can be retrieved
    /// via the background registry using the returned task ID.
    pub async fn spawn_background(&self, command: &str) -> Result<String, String> {
        let task_id = format!("bg-{}", uuid_simple());
        let output = Arc::new(Mutex::new(String::new()));
        let completed = Arc::new(Notify::new());
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let process = BackgroundProcess {
            id: task_id.clone(),
            command: command.to_string(),
            output: Arc::clone(&output),
            completed: Arc::clone(&completed),
            cancel_token: cancel_token.clone(),
        };

        self.background_registry
            .register(task_id.clone(), process)
            .await;

        let cwd = self.cwd.lock().unwrap().clone();
        let registry = self.background_registry.clone();
        let bg_task_id = task_id.clone();
        let shell_args = self.get_shell_args(command);
        let shell_args = self.maybe_wrap_shell_lc_with_snapshot(shell_args);

        tokio::spawn(async move {
            let child = tokio::process::Command::new(&shell_args[0])
                .args(&shell_args[1..])
                .current_dir(&cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn();

            match child {
                Ok(mut child) => {
                    // Read stdout
                    if let Some(mut stdout) = child.stdout.take() {
                        let output_stdout = Arc::clone(&output);
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 4096];
                            loop {
                                match stdout.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if let Ok(text) = String::from_utf8(buf[..n].to_vec()) {
                                            output_stdout.lock().await.push_str(&text);
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }

                    // Read stderr
                    if let Some(mut stderr) = child.stderr.take() {
                        let output_stderr = Arc::clone(&output);
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 4096];
                            loop {
                                match stderr.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if let Ok(text) = String::from_utf8(buf[..n].to_vec()) {
                                            output_stderr.lock().await.push_str(&text);
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }

                    // Wait for process to complete or cancellation
                    tokio::select! {
                        _ = child.wait() => {}
                        _ = cancel_token.cancelled() => {
                            // Token cancelled via stop() — child will be killed
                            // on drop due to kill_on_drop(true)
                        }
                    }
                }
                Err(e) => {
                    let mut out = output.lock().await;
                    out.push_str(&format!("Failed to spawn command: {e}"));
                }
            }

            completed.notify_waiters();

            // Remove from registry when done
            registry.stop(&bg_task_id).await;
        });

        Ok(task_id)
    }

    /// Gets shell arguments for executing a command.
    ///
    /// Uses login shell (`-lc`) when a shell is configured, as `maybe_wrap_shell_lc_with_snapshot`
    /// will rewrite to `-c` with snapshot sourcing if needed.
    fn get_shell_args(&self, command: &str) -> Vec<String> {
        if let Some(ref shell) = self.shell {
            // Use login shell (-lc) when snapshot might be available
            // maybe_wrap_shell_lc_with_snapshot will rewrite to -c if needed
            shell.derive_exec_args(command, true)
        } else {
            // Fallback to bash (non-login, no snapshot support)
            vec!["bash".to_string(), "-c".to_string(), command.to_string()]
        }
    }

    /// Internal: runs a command and captures output, tracking CWD changes.
    async fn run_command(&self, command: &str) -> CommandResult {
        let args = self.get_shell_args(command);
        let args = self.maybe_wrap_shell_lc_with_snapshot(args);
        let cwd = self.cwd.lock().unwrap().clone();

        // Wrap the script to capture CWD after execution
        let wrapped_script = format!(
            "{}; __cocode_exit=$?; echo '{}' \"$(pwd)\" '{}'; exit $__cocode_exit",
            &args[2], CWD_MARKER_START, CWD_MARKER_END
        );
        let args = vec![args[0].clone(), args[1].clone(), wrapped_script];

        let child = tokio::process::Command::new(&args[0])
            .args(&args[1..])
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let child = match child {
            Ok(c) => c,
            Err(e) => {
                return CommandResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Failed to spawn command: {e}"),
                    duration_ms: 0,
                    truncated: false,
                    new_cwd: None,
                    extracted_paths: None,
                };
            }
        };

        let output = match child.wait_with_output().await {
            Ok(o) => o,
            Err(e) => {
                return CommandResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Failed to wait for command: {e}"),
                    duration_ms: 0,
                    truncated: false,
                    new_cwd: None,
                    extracted_paths: None,
                };
            }
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let (raw_stdout, truncated_stdout) = truncate_output(&output.stdout);
        let (stderr, truncated_stderr) = truncate_output(&output.stderr);

        // Extract CWD from output and clean the stdout
        let (stdout, new_cwd) = extract_cwd_from_output(&raw_stdout);

        CommandResult {
            exit_code,
            stdout,
            stderr,
            duration_ms: 0, // Will be set by caller
            truncated: truncated_stdout || truncated_stderr,
            new_cwd,
            extracted_paths: None,
        }
    }
}

/// Checks if shell snapshotting is disabled via environment variable.
fn is_snapshot_disabled() -> bool {
    std::env::var(DISABLE_SNAPSHOT_ENV)
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Truncates output bytes to a string, returning (text, was_truncated).
fn truncate_output(bytes: &[u8]) -> (String, bool) {
    let max = MAX_OUTPUT_BYTES as usize;
    if bytes.len() > max {
        let truncated_bytes = &bytes[..max];
        let text = String::from_utf8_lossy(truncated_bytes).into_owned();
        (text, true)
    } else {
        let text = String::from_utf8_lossy(bytes).into_owned();
        (text, false)
    }
}

/// Extracts CWD from command output that contains CWD markers.
///
/// Returns (cleaned_output, Option<new_cwd>).
/// The markers are removed from the output.
fn extract_cwd_from_output(output: &str) -> (String, Option<PathBuf>) {
    // Look for the CWD marker line at the end of output
    if let Some(start) = output.rfind(CWD_MARKER_START) {
        if let Some(end_offset) = output[start..].find(CWD_MARKER_END) {
            let cwd_start = start + CWD_MARKER_START.len();
            let cwd_end = start + end_offset;
            let cwd_str = output[cwd_start..cwd_end].trim();

            // Clean the output: remove from the marker start to end of marker
            let marker_end = start + end_offset + CWD_MARKER_END.len();
            let cleaned = format!(
                "{}{}",
                output[..start].trim_end_matches('\n'),
                &output[marker_end..]
            )
            .trim_end()
            .to_string();

            // Only return CWD if it's a valid non-empty path
            if !cwd_str.is_empty() {
                return (cleaned, Some(PathBuf::from(cwd_str)));
            }

            return (cleaned, None);
        }
    }

    (output.to_string(), None)
}

/// Generates a simple unique identifier (timestamp-based).
fn uuid_simple() -> String {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
#[path = "executor.test.rs"]
mod tests;
