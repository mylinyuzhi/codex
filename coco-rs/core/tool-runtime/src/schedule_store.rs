//! Schedule store trait — abstraction for cron/trigger persistence.
//!
//! Same injection pattern as `SideQuery` and `McpHandle`.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// A stored schedule entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub command: String,
    #[serde(default)]
    pub enabled: bool,
}

/// A remote trigger entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEntry {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// Trait for schedule/trigger CRUD operations.
///
/// TS: CronCreateTool, CronDeleteTool, CronListTool, RemoteTriggerTool
#[async_trait::async_trait]
pub trait ScheduleStore: Send + Sync {
    // ── Schedule (Cron) CRUD ──
    async fn create_schedule(
        &self,
        name: &str,
        schedule: &str,
        command: &str,
    ) -> anyhow::Result<ScheduleEntry>;
    async fn delete_schedule(&self, id: &str) -> anyhow::Result<()>;
    async fn list_schedules(&self) -> anyhow::Result<Vec<ScheduleEntry>>;

    // ── Trigger CRUD ──
    async fn create_trigger(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> anyhow::Result<TriggerEntry>;
    async fn list_triggers(&self) -> anyhow::Result<Vec<TriggerEntry>>;
    async fn get_trigger(&self, id: &str) -> anyhow::Result<TriggerEntry>;
    async fn update_trigger(&self, id: &str, body: Value) -> anyhow::Result<TriggerEntry>;
    async fn run_trigger(&self, id: &str) -> anyhow::Result<String>;
}

pub type ScheduleStoreRef = Arc<dyn ScheduleStore>;

/// In-memory no-op store for contexts without scheduling.
#[derive(Debug, Clone)]
pub struct NoOpScheduleStore;

#[async_trait::async_trait]
impl ScheduleStore for NoOpScheduleStore {
    async fn create_schedule(
        &self,
        _name: &str,
        _schedule: &str,
        _command: &str,
    ) -> anyhow::Result<ScheduleEntry> {
        anyhow::bail!("scheduling not available in this context")
    }
    async fn delete_schedule(&self, _id: &str) -> anyhow::Result<()> {
        anyhow::bail!("scheduling not available in this context")
    }
    async fn list_schedules(&self) -> anyhow::Result<Vec<ScheduleEntry>> {
        Ok(vec![])
    }
    async fn create_trigger(
        &self,
        _name: &str,
        _description: Option<&str>,
    ) -> anyhow::Result<TriggerEntry> {
        anyhow::bail!("scheduling not available in this context")
    }
    async fn list_triggers(&self) -> anyhow::Result<Vec<TriggerEntry>> {
        Ok(vec![])
    }
    async fn get_trigger(&self, id: &str) -> anyhow::Result<TriggerEntry> {
        anyhow::bail!("trigger '{id}' not found (scheduling not available)")
    }
    async fn update_trigger(&self, id: &str, _body: Value) -> anyhow::Result<TriggerEntry> {
        anyhow::bail!("trigger '{id}' not found (scheduling not available)")
    }
    async fn run_trigger(&self, id: &str) -> anyhow::Result<String> {
        anyhow::bail!("trigger '{id}' not found (scheduling not available)")
    }
}
