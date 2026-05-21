//! Shell-task spawning, driver, and terminal-state composition.
//!
//! TS source:
//! - `tasks/LocalShellTask/LocalShellTask.tsx` — full lifecycle
//!   (spawn / drain / killTask / terminal envelope).
//! - `LocalShellTask.tsx:105-172 enqueueShellNotification` — terminal
//!   `<task-notification>` envelope.
//! - `LocalShellTask.tsx:148-156` — exit-code → status mapping
//!   (zero = completed, non-zero = failed).
//! - `BashTool.tsx:1128-1140` — progress yield cadence inside
//!   `runShellCommand`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use async_trait::async_trait;
use coco_tasks::{
    NotificationKind, NotificationSink, TaskManager, TaskNotification, TerminalStatus,
};
use coco_tool_runtime::{BackgroundShellRequest, ShellTaskSpawner};
use coco_types::{TaskStatus, TaskType};
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument, warn};

use super::stall;
use super::timers::{spawn_auto_detach_timer, spawn_progress_timer};
use super::{TaskEntry, TaskRuntime};
use crate::disk_task_output::DiskTaskOutput;

#[async_trait]
impl ShellTaskSpawner for TaskRuntime {
    #[instrument(
        level = "info",
        skip(self, request),
        fields(
            command_preview = %command_preview(&request.command),
            timeout_ms = ?request.timeout_ms,
            agent_id = ?request.agent_id,
        )
    )]
    async fn spawn_shell_task(
        &self,
        request: BackgroundShellRequest,
    ) -> Result<String, coco_error::BoxedError> {
        let task_id = coco_types::generate_task_id(TaskType::LocalBash);
        let dto = self.disk.get_or_create(&task_id).await;
        let output_path = dto.path().display().to_string();
        // Shell tasks reach this path only via `BashTool` with
        // `run_in_background=true` — by definition backgrounded.
        let assigned = self
            .manager
            .create_running_with_id(
                task_id.clone(),
                TaskType::LocalBash,
                &request.description,
                &output_path,
                /* is_backgrounded */ true,
            )
            .await;
        debug_assert_eq!(assigned, task_id);
        if let Some(tu_id) = request.tool_use_id.as_deref() {
            self.manager
                .set_tool_use_id(&task_id, tu_id.to_string())
                .await;
        }
        let cancel = CancellationToken::new();
        let (status_tx, _) = watch::channel(TaskStatus::Running);
        let detach = Arc::new(Notify::new());
        let detached = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(std::sync::OnceLock::new());
        self.entries.write().await.insert(
            task_id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                status_tx: status_tx.clone(),
                invoking_agent_id: request.agent_id.clone(),
                detach: detach.clone(),
                detached: detached.clone(),
                exit_code: exit_code.clone(),
            },
        );
        info!(
            target: "coco::task_runtime::shell",
            task_id = %task_id,
            description = %request.description,
            output_file = %output_path,
            "background shell task spawned"
        );

        let manager = self.manager.clone();
        let sink = self.notification_sink.clone();
        let driver_task_id = task_id.clone();
        let driver_description = request.description.clone();
        let driver_tool_use_id = request.tool_use_id.clone();
        let driver_agent_id = request.agent_id.clone();
        let driver_output_path = output_path.clone();
        let command_str = request.command.clone();
        let timeout_ms = request.timeout_ms.unwrap_or(120_000);

        let dto_for_driver = dto.clone();
        let cancel_for_driver = cancel.clone();
        // W3: rename `stall_cancel` → `drain_done`. Fired by the driver
        // tokio task after `run_shell_task` returns; stops the stall
        // watchdog, progress timer, and auto-detach timer in one
        // coordinated signal.
        let drain_done = CancellationToken::new();
        tokio::spawn(stall::watchdog(
            task_id.clone(),
            request.description.clone(),
            request.tool_use_id.clone(),
            request.agent_id.clone(),
            output_path.clone(),
            dto.clone(),
            sink.clone(),
            drain_done.clone(),
        ));

        // W3: progress timer — emits `bash_progress` events through
        // `progress_tx` every `progress_throttle_ms` while the task
        // runs. Matches TS's `~1s` `yield { type: 'progress', ... }`
        // cadence (`tools/BashTool/BashTool.tsx:1128-1140`). The
        // unified fg/bg path lets fg `tool.execute` observe progress
        // via the same `ctx.progress_tx` channel it always used.
        if let Some(progress_tx) = request.progress_tx.clone() {
            spawn_progress_timer(
                task_id.clone(),
                request.tool_use_id.clone().unwrap_or_default(),
                request.progress_throttle_ms.max(100),
                dto.clone(),
                progress_tx,
                drain_done.clone(),
            );
        }

        // W3: auto-detach timer — fires `signal_detach(task_id)` after
        // `auto_detach_ms` of fg execution. Mirrors TS
        // `ASSISTANT_BLOCKING_BUDGET_MS` (15 s) auto-background. Stops
        // when the task terminates (`drain_done` fires). Bails when
        // the task is already terminal at fire time.
        if let Some(ms) = request.auto_detach_ms {
            spawn_auto_detach_timer(
                task_id.clone(),
                ms,
                self.entries.clone(),
                self.manager.clone(),
                drain_done.clone(),
            );
        }

        let driver_status_tx = status_tx;
        let drain_done_for_driver = drain_done;
        let exit_code_for_driver = exit_code;
        // W6: thread sandbox state into the driver. `None` = no
        // wrapping (current bg-default behavior). `Some` = apply
        // `SandboxState::try_wrap_command_with_binds` before spawn.
        let sandbox_for_driver = request.sandbox_state.clone();
        let sandbox_bypass_for_driver = request.sandbox_bypass;
        tokio::spawn(async move {
            let outcome = run_shell_task(
                &command_str,
                timeout_ms,
                cancel_for_driver,
                dto_for_driver,
                sandbox_for_driver,
                sandbox_bypass_for_driver,
            )
            .await;
            drain_done_for_driver.cancel();
            apply_shell_terminal_state(
                &manager,
                &driver_status_tx,
                &driver_task_id,
                &driver_description,
                driver_tool_use_id.as_deref(),
                driver_agent_id.as_deref(),
                &driver_output_path,
                sink.as_ref(),
                &exit_code_for_driver,
                outcome,
            )
            .await;
        });

        Ok(task_id)
    }
}

/// Result of one shell-task execution. Carries enough information
/// for `apply_shell_terminal_state` to compose the TS-aligned
/// summary string + status.
enum WaitOutcome {
    Exited { code: i32 },
    TimedOut { budget_ms: i64 },
    Cancelled,
    SpawnFailed,
}

/// Subset of [`WaitOutcome`] the terminal-apply step needs.
/// `stderr_tail` lives separately so it can be threaded through
/// without polluting the lifecycle enum.
struct ShellOutcome {
    wait: WaitOutcome,
}

/// Spawn the child process directly (bypassing `coco_shell::ShellExecutor`
/// — the BashTool security pipeline already cleared the command at
/// the foreground entry point in `bash.rs::execute`, and TS streams
/// stdout straight to disk which `ShellExecutor::execute_with_progress`
/// doesn't expose). Streams stdout + stderr to the per-task disk file
/// in real time so the stall watchdog observes growth.
///
/// W6: applies sandbox wrap (`bwrap` / Seatbelt) when `sandbox_state`
/// is `Some` and the command isn't excluded by the sandbox settings.
/// Mirrors `coco_shell::executor::apply_sandbox_wrap` so the
/// TaskRuntime unified path doesn't lose the sandbox guarantee that
/// the legacy `ShellExecutor` foreground path provided.
#[instrument(
    level = "debug",
    skip(cancel, dto, sandbox_state),
    fields(command_preview = %command_preview(command), timeout_ms, sandboxed = sandbox_state.is_some())
)]
async fn run_shell_task(
    command: &str,
    timeout_ms: i64,
    cancel: CancellationToken,
    dto: DiskTaskOutput,
    sandbox_state: Option<Arc<coco_sandbox::SandboxState>>,
    sandbox_bypass: coco_sandbox::SandboxBypass,
) -> ShellOutcome {
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    #[cfg(windows)]
    let (program, args) = ("cmd.exe", vec!["/C", command]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/bash", vec!["-c", command]);

    let mut cmd = Command::new(program);
    cmd.args(&args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    // W6: sandbox wrap. `try_wrap_command_with_binds` mutates `cmd`
    // in place to swap the program/args with the platform-specific
    // wrapper (bwrap on Linux, Seatbelt sandbox-exec on macOS).
    // No-op when sandbox is None / inactive / command excluded.
    if let Some(state) = &sandbox_state
        && let Err(e) = state.try_wrap_command_with_binds(command, sandbox_bypass, &[], &mut cmd)
    {
        warn!(
            target: "coco::task_runtime::shell",
            error = %e,
            "sandbox wrap failed; spawning unsandboxed"
        );
    }
    let mut child = match cmd.kill_on_drop(true).spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(
                target: "coco::task_runtime::shell",
                error = %e,
                "failed to spawn child process"
            );
            let msg = format!("\n[failed to spawn shell child: {e}]\n");
            dto.append(&msg);
            let _ = dto.flush().await;
            return ShellOutcome {
                wait: WaitOutcome::SpawnFailed,
            };
        }
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let dto_stdout = dto.clone();
    let stdout_handle = tokio::spawn(async move {
        if let Some(mut pipe) = stdout_pipe {
            let mut buf = vec![0u8; 8192];
            loop {
                match pipe.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        dto_stdout.append(&chunk);
                    }
                    Err(_) => break,
                }
            }
        }
    });

    let dto_stderr = dto.clone();
    let stderr_handle = tokio::spawn(async move {
        let mut tail = String::new();
        if let Some(mut pipe) = stderr_pipe {
            let mut buf = vec![0u8; 8192];
            loop {
                match pipe.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                        dto_stderr.append(&chunk);
                        tail.push_str(&chunk);
                        if tail.len() > 4096 {
                            let cut = tail.len() - 4096;
                            tail.drain(..cut);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        tail
    });

    let timeout_duration = Duration::from_millis(timeout_ms.max(0) as u64);
    let outcome = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            let _ = child.kill().await;
            WaitOutcome::Cancelled
        }
        result = child.wait() => match result {
            Ok(status) => WaitOutcome::Exited { code: status.code().unwrap_or(-1) },
            Err(e) => {
                warn!(target: "coco::task_runtime::shell", error = %e, "child wait failed");
                WaitOutcome::SpawnFailed
            }
        },
        () = tokio::time::sleep(timeout_duration) => {
            let _ = child.kill().await;
            WaitOutcome::TimedOut { budget_ms: timeout_ms }
        }
    };

    let _ = stdout_handle.await;
    // stderr tail is already on disk; the join result is intentionally
    // dropped — the terminal-apply step reads task state from
    // TaskManager, not from in-memory tails.
    let _ = stderr_handle.await;
    let _ = dto.flush().await;

    ShellOutcome { wait: outcome }
}

/// Final lifecycle update for a shell task: flip status,
/// broadcast on the watch, persist exit_code into the per-task
/// `OnceLock` (W3: so `read_terminal_outputs` can return it to the
/// fg `tool.execute` caller), and push the TS-aligned terminal
/// notification.
#[allow(clippy::too_many_arguments)]
async fn apply_shell_terminal_state(
    manager: &TaskManager,
    status_tx: &watch::Sender<TaskStatus>,
    task_id: &str,
    description: &str,
    tool_use_id: Option<&str>,
    agent_id: Option<&str>,
    output_path: &str,
    sink: &dyn NotificationSink,
    exit_code_slot: &Arc<std::sync::OnceLock<i32>>,
    outcome: ShellOutcome,
) {
    let (status, terminal, exit_code) = match outcome.wait {
        WaitOutcome::Exited { code: 0 } => {
            (TaskStatus::Completed, TerminalStatus::Completed, Some(0))
        }
        WaitOutcome::Exited { code } => {
            // Non-zero exit. TS treats any non-zero as failure
            // (`LocalShellTask.tsx:148-156`).
            (TaskStatus::Failed, TerminalStatus::Failed, Some(code))
        }
        WaitOutcome::TimedOut { budget_ms } => {
            // TS doesn't distinguish timeout from failed in the
            // status enum; coco-rs surfaces a clearer log line via
            // the budget but the status remains Failed for the model.
            warn!(
                target: "coco::task_runtime::shell",
                task_id,
                budget_ms,
                "shell task exceeded budget"
            );
            (TaskStatus::Failed, TerminalStatus::Failed, None)
        }
        WaitOutcome::Cancelled => (TaskStatus::Killed, TerminalStatus::Killed, None),
        WaitOutcome::SpawnFailed => (TaskStatus::Failed, TerminalStatus::Failed, None),
    };
    // W3: persist exit_code BEFORE update_status, so any awaiter
    // racing the terminal signal sees a consistent snapshot.
    if let Some(code) = exit_code {
        let _ = exit_code_slot.set(code);
    }
    manager.update_status(task_id, status).await;
    status_tx.send_replace(status);

    let n = TaskNotification {
        task_id: task_id.to_string(),
        tool_use_id: tool_use_id.map(String::from),
        agent_id: agent_id.map(String::from),
        output_file: output_path.to_string(),
        description: description.to_string(),
        kind: NotificationKind::ShellTerminal {
            status: terminal,
            exit_code,
        },
    };
    sink.push(n).await;
    info!(
        target: "coco::task_runtime::shell",
        task_id,
        status = ?status,
        "background shell task terminal"
    );
}

/// Short identifier shown in `tracing` spans for shell commands.
/// Avoids logging the entire command which can be hundreds of
/// characters with heredocs / pipes.
fn command_preview(cmd: &str) -> String {
    truncate_for_label(cmd, 60)
}

fn truncate_for_label(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
