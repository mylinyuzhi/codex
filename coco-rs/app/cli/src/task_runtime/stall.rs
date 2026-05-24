//! Per-task stall watchdog loops.
//!
//! Two variants:
//!
//! - [`watchdog`] — for shell tasks. Spawned by `spawn_shell_task`.
//!   Every [`STALL_CHECK_INTERVAL_MS`] checks whether the on-disk
//!   output has grown; if it's been frozen for [`STALL_THRESHOLD_MS`]
//!   AND the tail looks like an interactive prompt, fires a stall
//!   notification and exits. TS source:
//!   `tasks/LocalShellTask/LocalShellTask.tsx:46-104`.
//! - [`agent_watchdog`] — for background agent tasks. Same growth
//!   watching but no shell-prompt heuristic (agents don't emit
//!   prompts). Fires a stall notification when output has been
//!   silent for [`AGENT_STALL_THRESHOLD_MS`] (longer than shell's
//!   to allow legitimate "thinking" time during LLM calls / tool
//!   chains).

use std::time::Duration;

use coco_tasks::{
    NotificationKind, NotificationSinkRef, STALL_CHECK_INTERVAL_MS, STALL_TAIL_BYTES,
    STALL_THRESHOLD_MS, TaskNotification, matches_interactive_prompt,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument, trace};

use crate::disk_task_output::DiskTaskOutput;

/// Stall threshold for agent tasks. Longer than shell's
/// [`STALL_THRESHOLD_MS`] (default 45 s) because agents may
/// legitimately spend minutes on a single tool chain / LLM
/// streaming. 3 minutes balances "alert on real hangs" vs
/// "don't spam during normal work".
pub const AGENT_STALL_THRESHOLD_MS: u64 = 180_000;

/// Check interval for the agent stall watchdog. Wider than shell's
/// 5-second sweep — agents don't need real-time stall detection.
pub const AGENT_STALL_CHECK_INTERVAL_MS: u64 = 30_000;

/// Run the watchdog until either the cancel token fires (task ended)
/// or a stall is detected and notified (one-shot). The first
/// non-prompt tail check resets the growth timer to avoid spurious
/// re-checks every 5 s once a task has paused on a long compile.
#[instrument(
    level = "debug",
    skip(dto, sink, cancel),
    fields(task_id = %task_id, description = %description)
)]
#[allow(clippy::too_many_arguments)]
pub async fn watchdog(
    task_id: String,
    description: String,
    tool_use_id: Option<String>,
    agent_id: Option<String>,
    output_file: String,
    dto: DiskTaskOutput,
    sink: NotificationSinkRef,
    cancel: CancellationToken,
) {
    let interval = Duration::from_millis(STALL_CHECK_INTERVAL_MS);
    let threshold = Duration::from_millis(STALL_THRESHOLD_MS);
    let mut last_size: i64 = 0;
    let mut last_growth = tokio::time::Instant::now();
    let mut ticker = tokio::time::interval(interval);
    // Interval's first tick is immediate; the biased `cancel` arm
    // wins if the task already ended, so no skip needed.
    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                trace!(
                    target: "coco::task_runtime::stall",
                    task_id = %task_id,
                    "stall watchdog exiting (cancelled)"
                );
                return;
            }
            _ = ticker.tick() => {
                let size = dto.size().await;
                if size > last_size {
                    last_size = size;
                    last_growth = tokio::time::Instant::now();
                    continue;
                }
                if last_growth.elapsed() < threshold {
                    continue;
                }
                let tail = match dto.read_tail(STALL_TAIL_BYTES).await {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if !matches_interactive_prompt(&tail) {
                    // Not a prompt — keep watching but reset the
                    // growth marker so next eval is THRESHOLD out.
                    // TS: `LocalShellTask.tsx:66-68`.
                    last_growth = tokio::time::Instant::now();
                    continue;
                }
                info!(
                    target: "coco::task_runtime::stall",
                    task_id = %task_id,
                    tail_bytes = tail.len(),
                    "stall detected — pushing stall notification"
                );
                let n = TaskNotification {
                    task_id: task_id.clone(),
                    tool_use_id: tool_use_id.clone(),
                    agent_id: agent_id.clone(),
                    output_file: output_file.clone(),
                    description: description.clone(),
                    kind: NotificationKind::Stall { output_tail: tail },
                };
                sink.push(n).await;
                // One-shot per stall episode (TS parity).
                return;
            }
        }
    }
}

/// W6 (Item 3 / A4): agent-task stall watchdog.
///
/// Replaces shell's prompt-pattern heuristic with a bare "no output
/// growth past threshold" check. The background agent driver in
/// `coordinator::spawn_background` drains `Stream::TextDelta` events
/// into `dto.append_output`, so disk-size growth is a faithful proxy
/// for "LLM is still streaming / tools are still producing output".
/// When silence exceeds [`AGENT_STALL_THRESHOLD_MS`], we push a stall
/// notification and exit (one-shot per stall episode).
///
/// The tail bytes are best-effort — we include them in the
/// notification so the model can see what the agent was working on
/// when it stalled. Empty tails are fine (the stall notification
/// renderer falls back to a generic message).
#[instrument(
    level = "debug",
    skip(dto, sink, cancel),
    fields(task_id = %task_id, description = %description)
)]
#[allow(clippy::too_many_arguments)]
pub async fn agent_watchdog(
    task_id: String,
    description: String,
    tool_use_id: Option<String>,
    agent_id: Option<String>,
    output_file: String,
    dto: DiskTaskOutput,
    sink: NotificationSinkRef,
    cancel: CancellationToken,
) {
    let interval = Duration::from_millis(AGENT_STALL_CHECK_INTERVAL_MS);
    let threshold = Duration::from_millis(AGENT_STALL_THRESHOLD_MS);
    let mut last_size: i64 = 0;
    let mut last_growth = tokio::time::Instant::now();
    let mut ticker = tokio::time::interval(interval);
    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                trace!(
                    target: "coco::task_runtime::stall",
                    task_id = %task_id,
                    "agent stall watchdog exiting (cancelled)"
                );
                return;
            }
            _ = ticker.tick() => {
                let size = dto.size().await;
                if size > last_size {
                    last_size = size;
                    last_growth = tokio::time::Instant::now();
                    continue;
                }
                if last_growth.elapsed() < threshold {
                    continue;
                }
                let tail = dto.read_tail(STALL_TAIL_BYTES).await.unwrap_or_default();
                info!(
                    target: "coco::task_runtime::stall",
                    task_id = %task_id,
                    silence_ms = last_growth.elapsed().as_millis() as u64,
                    "agent stall detected — pushing stall notification"
                );
                let n = TaskNotification {
                    task_id: task_id.clone(),
                    tool_use_id: tool_use_id.clone(),
                    agent_id: agent_id.clone(),
                    output_file: output_file.clone(),
                    description: description.clone(),
                    kind: NotificationKind::Stall { output_tail: tail },
                };
                sink.push(n).await;
                // One-shot per stall episode.
                return;
            }
        }
    }
}
