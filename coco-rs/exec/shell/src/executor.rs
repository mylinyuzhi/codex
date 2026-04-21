use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use coco_config::ShellConfig;

use crate::result::CommandResult;
use crate::result::ExecOptions;
use crate::safety::SafetyResult;
use crate::shell_types::Shell;
use crate::shell_types::ShellType;
use crate::shell_types::default_user_shell;
use crate::shell_types::shell_from_config;
use crate::snapshot::ShellSnapshot;
use crate::snapshot::SnapshotConfig;

/// CWD tracking marker — appended to commands to detect directory changes.
const CWD_MARKER_PREFIX: &str = "___COCO_CWD___";

/// Shell executor with CWD tracking, timeout, and environment snapshot support.
pub struct ShellExecutor {
    /// Current working directory.
    cwd: PathBuf,
    /// Shell configuration with type, path, and optional snapshot.
    shell: Shell,
    /// Resolved user settings — controls snapshot disable, shell-prefix, etc.
    ///
    /// `None` keeps legacy behavior for callers constructed via `new()`
    /// before the config pipeline existed (tests + pre-runtime paths).
    shell_config: Option<ShellConfig>,
}

impl ShellExecutor {
    /// Construct an executor that auto-detects the shell from `$SHELL`
    /// + platform defaults. No user settings applied.
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            shell: default_user_shell(),
            shell_config: None,
        }
    }

    /// Construct an executor that honors a resolved [`ShellConfig`]:
    ///   - `default_shell` acts as the bash/zsh override
    ///   - `disable_snapshot` gates [`start_snapshotting`]
    ///   - `shell_prefix` is reserved for future consumption (logged today)
    pub fn new_with_config(cwd: &Path, shell_config: &ShellConfig) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            shell: shell_from_config(shell_config),
            shell_config: Some(shell_config.clone()),
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    /// Returns the underlying Shell.
    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    /// Returns the current snapshot if available.
    pub fn shell_snapshot(&self) -> Option<Arc<ShellSnapshot>> {
        self.shell.shell_snapshot()
    }

    /// Starts async shell snapshotting in a background task.
    ///
    /// The snapshot captures the user's shell environment (functions, aliases,
    /// options, exports) and sources it before each command, avoiding login
    /// shell overhead while preserving the user's interactive environment.
    ///
    /// When `shell_config.disable_snapshot` is true (via settings.json or
    /// `COCO_DISABLE_SHELL_SNAPSHOT` folded into the config at resolve time),
    /// this is a no-op.
    pub fn start_snapshotting(&mut self, coco_home: PathBuf, session_id: &str) {
        if self
            .shell_config
            .as_ref()
            .is_some_and(|c| c.disable_snapshot)
        {
            return;
        }
        let config = SnapshotConfig::new(&coco_home);
        ShellSnapshot::start_snapshotting(config, session_id, &mut self.shell);
    }

    /// Execute a shell command with timeout and CWD tracking.
    ///
    /// When a shell snapshot is available, it is sourced before the command
    /// to restore the user's interactive environment without login shell overhead.
    /// When no snapshot, falls back to login shell (`-l`) so the user still
    /// gets their default environment.
    ///
    /// R6-T17: if `options.cancel` is set, a `tokio::select!` races the
    /// child wait against the cancel token; when the token fires, the
    /// child future is dropped which kills the process via
    /// `kill_on_drop(true)`, and the return `CommandResult.interrupted`
    /// flag is set to `true` so the caller can distinguish a cancel
    /// from a normal completion or a timeout.
    pub async fn execute(
        &mut self,
        command: &str,
        options: &ExecOptions,
    ) -> anyhow::Result<CommandResult> {
        let effective_cwd = options.cwd_override.as_deref().unwrap_or(&self.cwd);

        let (shell_command, use_login_shell) = self.build_exec_command(command);
        let tracked_command = if options.prevent_cwd_changes {
            shell_command
        } else {
            format!("{shell_command}; echo \"{CWD_MARKER_PREFIX}$(pwd -P)\"")
        };

        let shell_flag = if use_login_shell { "-lc" } else { "-c" };

        let mut cmd = tokio::process::Command::new(self.shell.shell_path());
        cmd.arg(shell_flag).arg(&tracked_command);
        cmd.current_dir(effective_cwd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        for (key, value) in &options.extra_env {
            cmd.env(key, value);
        }

        let child = cmd.kill_on_drop(true).spawn()?;

        let timeout_ms = options.timeout_ms.unwrap_or(120_000);
        let timeout_duration = std::time::Duration::from_millis(timeout_ms as u64);

        // Build the wait future and race it against the cancel token
        // (if present) plus the timeout. `wait_with_output()` consumes
        // the child, so we spawn the future only once and drop it on
        // cancel to trigger `kill_on_drop`.
        let wait_future = child.wait_with_output();
        tokio::pin!(wait_future);

        let output = if let Some(cancel) = &options.cancel {
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    // Dropping `wait_future` on the next line kills the
                    // child via `kill_on_drop(true)`. We return a
                    // CommandResult with `interrupted = true` so the
                    // caller can surface that distinctly from a timeout.
                    return Ok(CommandResult {
                        exit_code: -1,
                        stdout: String::new(),
                        stdout_bytes: None,
                        stderr: "Command interrupted".to_string(),
                        new_cwd: None,
                        timed_out: false,
                        interrupted: true,
                    });
                }
                result = tokio::time::timeout(timeout_duration, &mut wait_future) => {
                    match result {
                        Ok(Ok(output)) => output,
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => {
                            return Ok(CommandResult {
                                exit_code: -1,
                                stdout: String::new(),
                                stdout_bytes: None,
                                stderr: format!("Command timed out after {timeout_ms}ms"),
                                new_cwd: None,
                                timed_out: true,
                                interrupted: false,
                            });
                        }
                    }
                }
            }
        } else {
            match tokio::time::timeout(timeout_duration, wait_future).await {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    return Ok(CommandResult {
                        exit_code: -1,
                        stdout: String::new(),
                        stdout_bytes: None,
                        stderr: format!("Command timed out after {timeout_ms}ms"),
                        new_cwd: None,
                        timed_out: true,
                        interrupted: false,
                    });
                }
            }
        };

        // Preserve the raw stdout bytes BEFORE the lossy UTF-8 conversion
        // so binary-aware consumers (e.g. BashTool's image-detection
        // path) can inspect the original magic-byte signature.
        let stdout_raw = output.stdout;
        let mut stdout = String::from_utf8_lossy(&stdout_raw).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let new_cwd = if !options.prevent_cwd_changes {
            extract_cwd_from_output(&mut stdout)
        } else {
            None
        };

        if let Some(ref new) = new_cwd {
            self.cwd = new.clone();
        }

        Ok(CommandResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout,
            stdout_bytes: Some(stdout_raw),
            stderr,
            new_cwd,
            timed_out: false,
            interrupted: false,
        })
    }

    /// Check command safety before execution.
    pub fn check_safety(&self, command: &str) -> SafetyResult {
        if let Some(warning) = crate::destructive::get_destructive_warning(command) {
            return SafetyResult::Denied { reason: warning };
        }

        if crate::read_only::is_read_only_command(command) {
            return SafetyResult::Safe;
        }

        SafetyResult::RequiresApproval {
            reason: "command may have side effects".into(),
        }
    }

    /// Execute a command with streaming progress via a callback.
    ///
    /// Reader tasks share an atomic byte counter with the progress loop so
    /// the callback receives real-time output volume approximately every second.
    pub async fn execute_with_progress<F>(
        &mut self,
        command: &str,
        options: &ExecOptions,
        mut on_progress: F,
    ) -> anyhow::Result<CommandResult>
    where
        F: FnMut(ShellProgress) + Send + 'static,
    {
        use std::sync::atomic::AtomicI64;
        use std::sync::atomic::Ordering;
        use tokio::io::AsyncReadExt;

        let effective_cwd = options.cwd_override.as_deref().unwrap_or(&self.cwd);
        let (shell_command, use_login_shell) = self.build_exec_command(command);
        let tracked_command = if options.prevent_cwd_changes {
            shell_command
        } else {
            format!("{shell_command}; echo \"{CWD_MARKER_PREFIX}$(pwd -P)\"")
        };

        let shell_flag = if use_login_shell { "-lc" } else { "-c" };

        let mut cmd = tokio::process::Command::new(self.shell.shell_path());
        cmd.arg(shell_flag).arg(&tracked_command);
        cmd.current_dir(effective_cwd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        for (key, value) in &options.extra_env {
            cmd.env(key, value);
        }

        let mut child = cmd.kill_on_drop(true).spawn()?;

        let stdout_bytes = Arc::new(AtomicI64::new(0));
        let stderr_bytes = Arc::new(AtomicI64::new(0));

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let timeout_ms = options.timeout_ms.unwrap_or(120_000);
        let timeout_duration = std::time::Duration::from_millis(timeout_ms as u64);
        let start = std::time::Instant::now();

        // R7-T18: stdout reader collects raw `Vec<u8>` instead of
        // immediately UTF-8-lossy-converting each 8KB chunk. Collecting
        // bytes lets BashTool's image detector inspect the original
        // magic-byte signature for `cat image.png` style commands.
        // Lossy conversion still happens at the end for the `stdout`
        // String field.
        let stdout_counter = stdout_bytes.clone();
        let stdout_handle = tokio::spawn(async move {
            let mut collected: Vec<u8> = Vec::new();
            if let Some(mut pipe) = stdout_pipe {
                let mut buf = vec![0u8; 8192];
                loop {
                    match pipe.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            collected.extend_from_slice(&buf[..n]);
                            stdout_counter.fetch_add(n as i64, Ordering::Relaxed);
                        }
                        Err(_) => break,
                    }
                }
            }
            collected
        });

        // stderr stays String-based — image detection only inspects
        // stdout, so we don't need to keep stderr's raw bytes.
        let stderr_counter = stderr_bytes.clone();
        let stderr_handle = tokio::spawn(async move {
            let mut collected = String::new();
            if let Some(mut pipe) = stderr_pipe {
                let mut buf = vec![0u8; 8192];
                loop {
                    match pipe.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = String::from_utf8_lossy(&buf[..n]);
                            collected.push_str(&chunk);
                            stderr_counter.fetch_add(n as i64, Ordering::Relaxed);
                        }
                        Err(_) => break,
                    }
                }
            }
            collected
        });

        let progress_interval = std::time::Duration::from_secs(1);
        // R6-T17: honour the cancel token inside the streaming loop so
        // the caller can interrupt mid-stream. The `biased` selector
        // checks cancel first, then child completion, then the progress
        // tick — so a pending cancel always wins over a late progress
        // update.
        let cancel = options.cancel.clone();
        let wait_result = loop {
            tokio::select! {
                biased;
                () = async {
                    if let Some(tok) = &cancel {
                        tok.cancelled().await
                    } else {
                        std::future::pending::<()>().await
                    }
                } => {
                    let _ = child.kill().await;
                    // Drain whatever output was produced before cancel.
                    let stdout_partial_bytes = stdout_handle.await.unwrap_or_default();
                    let stdout_partial = String::from_utf8_lossy(&stdout_partial_bytes).to_string();
                    let stderr_partial = stderr_handle.await.unwrap_or_default();
                    return Ok(CommandResult {
                        exit_code: -1,
                        stdout: stdout_partial,
                        stdout_bytes: Some(stdout_partial_bytes),
                        stderr: if stderr_partial.is_empty() {
                            "Command interrupted".to_string()
                        } else {
                            format!("{stderr_partial}\nCommand interrupted")
                        },
                        new_cwd: None,
                        timed_out: false,
                        interrupted: true,
                    });
                }
                result = child.wait() => {
                    break result.map(Some);
                }
                () = tokio::time::sleep(progress_interval) => {
                    let elapsed = start.elapsed();
                    if elapsed > timeout_duration {
                        let _ = child.kill().await;
                        return Ok(CommandResult {
                            exit_code: -1,
                            stdout: String::new(),
                            stdout_bytes: None,
                            stderr: format!("Command timed out after {timeout_ms}ms"),
                            new_cwd: None,
                            timed_out: true,
                            interrupted: false,
                        });
                    }
                    let total = stdout_bytes.load(Ordering::Relaxed)
                        + stderr_bytes.load(Ordering::Relaxed);
                    on_progress(ShellProgress {
                        elapsed_seconds: elapsed.as_secs_f64(),
                        total_bytes: total,
                    });
                }
            }
        };

        let status = wait_result?;
        let stdout_raw = stdout_handle.await.unwrap_or_default();
        let mut stdout = String::from_utf8_lossy(&stdout_raw).to_string();
        let stderr = stderr_handle.await.unwrap_or_default();

        let new_cwd = if !options.prevent_cwd_changes {
            extract_cwd_from_output(&mut stdout)
        } else {
            None
        };

        if let Some(ref new) = new_cwd {
            self.cwd = new.clone();
        }

        let exit_code = status.and_then(|s| s.code()).unwrap_or(-1);

        Ok(CommandResult {
            exit_code,
            stdout,
            stdout_bytes: Some(stdout_raw),
            stderr,
            new_cwd,
            timed_out: false,
            interrupted: false,
        })
    }

    /// Create a forked executor for subagent with isolated CWD.
    pub fn fork_for_subagent(&self) -> Self {
        Self {
            cwd: self.cwd.clone(),
            shell: self.shell.clone(),
            shell_config: self.shell_config.clone(),
        }
    }

    /// Builds the full command string with snapshot sourcing, extglob disable,
    /// and eval wrapping.
    ///
    /// Returns `(command_string, use_login_shell)`.
    ///
    /// TS alignment (`bashProvider.ts:buildExecCommand`):
    /// - If snapshot exists and file accessible: source snapshot, disable extglob,
    ///   eval-wrap the command. Use `-c` (no login shell needed).
    /// - If snapshot missing/inaccessible: use `-c -l` (login shell provides
    ///   user environment as fallback).
    fn build_exec_command(&self, command: &str) -> (String, bool) {
        if let Some(snapshot) = self.shell.shell_snapshot()
            && snapshot.path().exists()
        {
            let snapshot_path = snapshot.path().display();
            let extglob_cmd = disable_extglob_command(self.shell.shell_type());
            // TS: commandParts.join(' && ') — source, extglob, eval
            let cmd = format!(
                ". '{snapshot_path}' 2>/dev/null || true && \
                 {extglob_cmd} && \
                 eval {command}"
            );
            return (cmd, /*use_login_shell*/ false);
        }

        // No snapshot: fall back to login shell for user environment
        (command.to_string(), /*use_login_shell*/ true)
    }
}

/// Returns the shell command to disable extended globbing.
///
/// TS: `bashProvider.ts:getDisableExtglobCommand()` — security measure to
/// prevent malicious filename expansion after sourcing user config.
fn disable_extglob_command(shell_type: &ShellType) -> &'static str {
    match shell_type {
        ShellType::Bash => "shopt -u extglob 2>/dev/null || true",
        ShellType::Zsh => "setopt NO_EXTENDED_GLOB 2>/dev/null || true",
        // For unknown or sh, try both.
        _ => "{ shopt -u extglob || setopt NO_EXTENDED_GLOB; } >/dev/null 2>&1 || true",
    }
}

/// Progress update during streaming shell execution.
#[derive(Debug, Clone)]
pub struct ShellProgress {
    /// Elapsed time in seconds since command started.
    pub elapsed_seconds: f64,
    /// Total bytes of combined stdout+stderr output so far.
    pub total_bytes: i64,
}

/// Extract CWD from output by finding the marker line and removing it.
fn extract_cwd_from_output(stdout: &mut String) -> Option<PathBuf> {
    if let Some(marker_pos) = stdout.rfind(CWD_MARKER_PREFIX) {
        let cwd_line = stdout[marker_pos + CWD_MARKER_PREFIX.len()..].trim();
        let cwd = PathBuf::from(cwd_line);

        *stdout = stdout[..marker_pos].trim_end().to_string();

        if cwd.is_absolute() { Some(cwd) } else { None }
    } else {
        None
    }
}

#[cfg(test)]
#[path = "executor.test.rs"]
mod tests;
