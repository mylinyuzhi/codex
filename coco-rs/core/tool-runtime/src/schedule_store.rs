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
    ) -> Result<ScheduleEntry, coco_error::BoxedError>;
    async fn delete_schedule(&self, id: &str) -> Result<(), coco_error::BoxedError>;
    async fn list_schedules(&self) -> Result<Vec<ScheduleEntry>, coco_error::BoxedError>;

    // ── Trigger CRUD ──
    async fn create_trigger(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<TriggerEntry, coco_error::BoxedError>;
    async fn list_triggers(&self) -> Result<Vec<TriggerEntry>, coco_error::BoxedError>;
    async fn get_trigger(&self, id: &str) -> Result<TriggerEntry, coco_error::BoxedError>;
    async fn update_trigger(
        &self,
        id: &str,
        body: Value,
    ) -> Result<TriggerEntry, coco_error::BoxedError>;
    async fn run_trigger(&self, id: &str) -> Result<String, coco_error::BoxedError>;
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
    ) -> Result<ScheduleEntry, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "scheduling not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn delete_schedule(&self, _id: &str) -> Result<(), coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "scheduling not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn list_schedules(&self) -> Result<Vec<ScheduleEntry>, coco_error::BoxedError> {
        Ok(vec![])
    }
    async fn create_trigger(
        &self,
        _name: &str,
        _description: Option<&str>,
    ) -> Result<TriggerEntry, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "scheduling not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn list_triggers(&self) -> Result<Vec<TriggerEntry>, coco_error::BoxedError> {
        Ok(vec![])
    }
    async fn get_trigger(&self, id: &str) -> Result<TriggerEntry, coco_error::BoxedError> {
        return Err(Box::new(coco_error::PlainError::new(
            format!("trigger '{id}' not found (scheduling not available)"),
            coco_error::StatusCode::Internal,
        )));
    }
    async fn update_trigger(
        &self,
        id: &str,
        _body: Value,
    ) -> Result<TriggerEntry, coco_error::BoxedError> {
        return Err(Box::new(coco_error::PlainError::new(
            format!("trigger '{id}' not found (scheduling not available)"),
            coco_error::StatusCode::Internal,
        )));
    }
    async fn run_trigger(&self, id: &str) -> Result<String, coco_error::BoxedError> {
        return Err(Box::new(coco_error::PlainError::new(
            format!("trigger '{id}' not found (scheduling not available)"),
            coco_error::StatusCode::Internal,
        )));
    }
}
