//! `AgentMention` reminder — fires when the user prompt contains a
//! `@agent-…` token. The mention parser distinguishes agents from file
//! paths via the `agent-` prefix, no agent registry lookup needed for
//! reminder emission (the reminder lists requested agents; actual
//! resolution happens elsewhere).

use std::time::Duration;

use anyhow::Result;
use coco_types::AttachmentKind;

use crate::common::reminders;
use crate::tui_real::harness::RealTuiHarness;

const AGENT_NEEDLE: &str = "agent-imaginary-helper";

pub async fn run(provider: &str, model: &str) -> Result<()> {
    let mut harness = RealTuiHarness::builder()
        .with_provider(provider)
        .with_model(model)
        .with_max_turns(2)
        .build()
        .await?;

    let prompt =
        format!("Consider routing this to @{AGENT_NEEDLE} but for now just reply with: ok",);
    harness.submit(&prompt).await;

    let _ = harness.pump_until_idle(Duration::from_secs(60)).await?;

    let history = harness.history_snapshot().await;
    reminders::assert_reminder_contains(
        &history,
        AttachmentKind::AgentMention,
        "imaginary-helper",
        &format!("{provider}/{model} agent_mention"),
    );

    harness.shutdown().await;
    Ok(())
}
