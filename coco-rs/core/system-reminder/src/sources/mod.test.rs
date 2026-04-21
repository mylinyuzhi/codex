use super::*;
use coco_config::SystemReminderConfig;
use std::time::Duration;

#[tokio::test]
async fn empty_sources_materializes_all_defaults() {
    let sources = ReminderSources::default();
    let cfg = SystemReminderConfig::default();
    let out = sources
        .materialize(MaterializeContext {
            config: &cfg,
            agent_id: None,
            user_input: None,
            mentioned_paths: &[],
            just_compacted: false,
            per_source_timeout: Duration::from_millis(1000),
        })
        .await;
    assert!(out.hook_events.is_empty());
    assert!(out.diagnostics.is_empty());
    assert!(out.task_statuses.is_empty());
    assert!(out.skill_listing.is_none());
    assert!(out.invoked_skills.is_empty());
    assert!(out.mcp_instructions_current.is_empty());
    assert!(out.mcp_resources.is_empty());
    assert!(out.teammate_mailbox.is_none());
    assert!(out.team_context.is_none());
    assert!(out.agent_pending_messages.is_empty());
    assert!(out.ide_selection.is_none());
    assert!(out.ide_opened_file.is_none());
    assert!(out.nested_memories.is_empty());
    assert!(out.relevant_memories.is_empty());
}

#[tokio::test]
async fn noop_bundle_also_materializes_defaults_through_trait_dispatch() {
    let sources = ReminderSources::noop();
    let cfg = SystemReminderConfig::default();
    let out = sources
        .materialize(MaterializeContext {
            config: &cfg,
            agent_id: None,
            user_input: Some("hello"),
            mentioned_paths: &[],
            just_compacted: false,
            per_source_timeout: Duration::from_millis(1000),
        })
        .await;
    // Every field should still be default — NoOps return empty/None.
    assert!(out.hook_events.is_empty());
    assert!(out.diagnostics.is_empty());
    assert!(out.task_statuses.is_empty());
    assert!(out.mcp_resources.is_empty());
    assert!(out.nested_memories.is_empty());
    assert!(out.relevant_memories.is_empty());
}

#[tokio::test]
async fn disabled_config_skips_sources_even_when_present() {
    // Custom source that would return non-empty — if called.
    use crate::generator::DiagnosticFileSummary;
    use async_trait::async_trait;
    use std::sync::Arc;

    #[derive(Debug, Default)]
    struct SpyDiagSource {
        called: std::sync::atomic::AtomicBool,
    }

    #[async_trait]
    impl DiagnosticsSource for SpyDiagSource {
        async fn snapshot(&self, _agent_id: Option<&str>) -> Vec<DiagnosticFileSummary> {
            self.called
                .store(true, std::sync::atomic::Ordering::Relaxed);
            vec![DiagnosticFileSummary {
                path: "x".into(),
                formatted: "x: error".into(),
            }]
        }
    }

    let spy = Arc::new(SpyDiagSource::default());
    let sources = ReminderSources {
        diagnostics: Some(spy.clone()),
        ..Default::default()
    };
    let mut cfg = SystemReminderConfig::default();
    cfg.attachments.diagnostics = false;

    let out = sources
        .materialize(MaterializeContext {
            config: &cfg,
            agent_id: None,
            user_input: None,
            mentioned_paths: &[],
            just_compacted: false,
            per_source_timeout: Duration::from_millis(1000),
        })
        .await;

    assert!(out.diagnostics.is_empty());
    assert!(
        !spy.called.load(std::sync::atomic::Ordering::Relaxed),
        "config-gated source must not be called when disabled"
    );
}
