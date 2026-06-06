//! Schedule store trait — abstraction for cron/trigger persistence.
//!
//! Same injection pattern as `SideQuery` and `McpHandle`.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// Session-scoped in-memory schedule store.
#[derive(Debug, Default)]
pub struct InMemoryScheduleStore {
    schedules: RwLock<HashMap<String, ScheduleEntry>>,
    triggers: RwLock<HashMap<String, TriggerEntry>>,
}

impl InMemoryScheduleStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ScheduleStore for InMemoryScheduleStore {
    async fn create_schedule(
        &self,
        name: &str,
        schedule: &str,
        command: &str,
    ) -> Result<ScheduleEntry, coco_error::BoxedError> {
        let entry = ScheduleEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            schedule: schedule.to_string(),
            command: command.to_string(),
            enabled: true,
        };
        self.schedules
            .write()
            .await
            .insert(entry.id.clone(), entry.clone());
        Ok(entry)
    }

    async fn delete_schedule(&self, id: &str) -> Result<(), coco_error::BoxedError> {
        self.schedules.write().await.remove(id);
        Ok(())
    }

    async fn list_schedules(&self) -> Result<Vec<ScheduleEntry>, coco_error::BoxedError> {
        Ok(self.schedules.read().await.values().cloned().collect())
    }

    async fn create_trigger(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<TriggerEntry, coco_error::BoxedError> {
        let entry = TriggerEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: description.map(str::to_string),
        };
        self.triggers
            .write()
            .await
            .insert(entry.id.clone(), entry.clone());
        Ok(entry)
    }

    async fn list_triggers(&self) -> Result<Vec<TriggerEntry>, coco_error::BoxedError> {
        Ok(self.triggers.read().await.values().cloned().collect())
    }

    async fn get_trigger(&self, id: &str) -> Result<TriggerEntry, coco_error::BoxedError> {
        self.triggers
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(format!("trigger '{id}' not found")))
    }

    async fn update_trigger(
        &self,
        id: &str,
        _body: Value,
    ) -> Result<TriggerEntry, coco_error::BoxedError> {
        self.get_trigger(id).await
    }

    async fn run_trigger(&self, id: &str) -> Result<String, coco_error::BoxedError> {
        let trigger = self.get_trigger(id).await?;
        Ok(format!("Triggered {}", trigger.name))
    }
}

fn not_found(message: String) -> coco_error::BoxedError {
    Box::new(coco_error::PlainError::new(
        message,
        coco_error::StatusCode::FileNotFound,
    ))
}

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

#[cfg(test)]
#[path = "schedule_store.test.rs"]
mod tests;
