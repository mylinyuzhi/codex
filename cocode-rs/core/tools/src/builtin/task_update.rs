//! TaskUpdate tool for updating structured tasks.

use super::prompts;
use super::structured_tasks::StructuredTaskStore;
use super::structured_tasks::TaskStatus;
use super::structured_tasks::{self};
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Which dependency array to modify on a related task.
enum DepField {
    Blocks,
    BlockedBy,
}

/// Whether to add or remove the current task's ID.
enum DepOp {
    Add,
    Remove,
}

pub struct TaskUpdateTool {
    store: StructuredTaskStore,
}

impl TaskUpdateTool {
    pub fn new(store: StructuredTaskStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TaskUpdate.as_str()
    }

    fn description(&self) -> &str {
        prompts::TASK_UPDATE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "The ID of the task to update"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted"],
                    "description": "New status for the task"
                },
                "subject": {
                    "type": "string",
                    "description": "Updated task title"
                },
                "description": {
                    "type": "string",
                    "description": "Updated task description"
                },
                "activeForm": {
                    "type": "string",
                    "description": "Updated progress display text"
                },
                "addBlocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to add to 'blocks' list"
                },
                "addBlockedBy": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to add to 'blockedBy' list"
                },
                "removeBlocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to remove from 'blocks' list"
                },
                "removeBlockedBy": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to remove from 'blockedBy' list"
                },
                "owner": {
                    "type": "string",
                    "description": "Updated owner (agent/team name) for task assignment"
                },
                "metadata": {
                    "type": "object",
                    "description": "Metadata to merge into the task"
                }
            },
            "required": ["taskId"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        // Safe: uses Arc<Mutex<...>> internally for synchronization
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn should_defer(&self) -> bool {
        true
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::StructuredTasks)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let id = super::input_helpers::require_str(&input, "taskId")?;

        // Parse new status once (if provided) — reused for validation, hooks, and mutation.
        let new_status = input["status"]
            .as_str()
            .map(|s| {
                TaskStatus::parse(s).ok_or_else(|| {
                    crate::error::tool_error::InvalidInputSnafu {
                        message: format!("Invalid status: {s}"),
                    }
                    .build()
                })
            })
            .transpose()?;

        // Phase 4A: Execute TaskCompleted hooks BEFORE mutating the store.
        // Hooks can reject the transition.
        if matches!(new_status, Some(TaskStatus::Completed)) {
            let subject = {
                let store = self.store.lock().await;
                store.get(id).map(|t| t.subject.clone()).unwrap_or_default()
            };
            if let Some(hooks) = &ctx.services.hook_registry {
                let hook_ctx = cocode_hooks::HookContext::new(
                    cocode_hooks::HookEventType::TaskCompleted,
                    ctx.identity.session_id.clone(),
                    ctx.env.cwd.clone(),
                )
                .with_task_id(id)
                .with_task_subject(&subject);
                let outcomes = hooks.execute(&hook_ctx).await;
                for outcome in &outcomes {
                    if let cocode_hooks::HookResult::Reject { reason } = &outcome.result {
                        return Err(crate::error::tool_error::InvalidInputSnafu {
                            message: format!(
                                "TaskCompleted hook '{}' rejected: {reason}",
                                outcome.hook_name
                            ),
                        }
                        .build());
                    }
                }
            }
        }

        let snapshot = {
            let mut store = self.store.lock().await;

            // Validate status transition
            if let Some(new_status) = new_status {
                let current_status = store
                    .get(id)
                    .ok_or_else(|| {
                        crate::error::tool_error::InvalidInputSnafu {
                            message: format!("Task not found: {id}"),
                        }
                        .build()
                    })?
                    .status;

                if !current_status.can_transition_to(new_status) {
                    return Err(crate::error::tool_error::InvalidInputSnafu {
                        message: format!(
                            "Invalid status transition: {} → {}",
                            current_status.as_str(),
                            new_status.as_str()
                        ),
                    }
                    .build());
                }

                // Enforce max 1 in_progress
                if matches!(new_status, TaskStatus::InProgress)
                    && !matches!(current_status, TaskStatus::InProgress)
                    && store
                        .values()
                        .any(|t| t.id != id && matches!(t.status, TaskStatus::InProgress))
                {
                    return Err(crate::error::tool_error::InvalidInputSnafu {
                        message: "At most 1 task can be in_progress at a time",
                    }
                    .build());
                }
            } else {
                // Even without status change, ensure task exists
                if !store.contains_key(id) {
                    return Err(crate::error::tool_error::InvalidInputSnafu {
                        message: format!("Task not found: {id}"),
                    }
                    .build());
                }
            }

            // Collect bidirectional dependency changes to apply to other tasks.
            // We'll collect (target_id, field, add_or_remove, this_task_id) tuples.
            let id_owned = id.to_string();
            let mut dep_changes: Vec<(String, DepField, DepOp)> = Vec::new();

            // Get mutable ref to the primary task for all mutations.
            // Safety: task existence was validated above.
            let Some(task) = store.get_mut(id) else {
                return Err(crate::error::tool_error::InvalidInputSnafu {
                    message: format!("Task not found: {id}"),
                }
                .build());
            };

            // Apply status update
            if let Some(new_status) = new_status {
                task.status = new_status;
            }

            // Update other fields
            if let Some(subject) = input["subject"].as_str() {
                task.subject = subject.to_string();
            }
            if let Some(desc) = input["description"].as_str() {
                task.description = Some(desc.to_string());
            }
            if let Some(active_form) = input["activeForm"].as_str() {
                task.active_form = Some(active_form.to_string());
            }
            if let Some(owner) = input["owner"].as_str() {
                task.owner = Some(owner.to_string());
            }

            // Update dependencies (with bidirectional tracking)
            if let Some(add_blocks) = input["addBlocks"].as_array() {
                for v in add_blocks {
                    if let Some(s) = v.as_str() {
                        let s = s.to_string();
                        if !task.blocks.contains(&s) {
                            task.blocks.push(s.clone());
                        }
                        // Inverse: add id to target's blocked_by
                        dep_changes.push((s, DepField::BlockedBy, DepOp::Add));
                    }
                }
            }
            if let Some(add_blocked_by) = input["addBlockedBy"].as_array() {
                for v in add_blocked_by {
                    if let Some(s) = v.as_str() {
                        let s = s.to_string();
                        if !task.blocked_by.contains(&s) {
                            task.blocked_by.push(s.clone());
                        }
                        // Inverse: add id to source's blocks
                        dep_changes.push((s, DepField::Blocks, DepOp::Add));
                    }
                }
            }
            if let Some(remove_blocks) = input["removeBlocks"].as_array() {
                let remove: Vec<String> = remove_blocks
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                task.blocks.retain(|b| !remove.contains(b));
                for target in remove {
                    // Inverse: remove id from target's blocked_by
                    dep_changes.push((target, DepField::BlockedBy, DepOp::Remove));
                }
            }
            if let Some(remove_blocked_by) = input["removeBlockedBy"].as_array() {
                let remove: Vec<String> = remove_blocked_by
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                task.blocked_by.retain(|b| !remove.contains(b));
                for source in remove {
                    // Inverse: remove id from source's blocks
                    dep_changes.push((source, DepField::Blocks, DepOp::Remove));
                }
            }

            // Update metadata (merge, with null key deletion)
            if let Some(meta) = input.get("metadata") {
                if let (Value::Object(existing), Value::Object(new)) = (&mut task.metadata, meta) {
                    for (k, v) in new {
                        if v.is_null() {
                            existing.remove(k);
                        } else {
                            existing.insert(k.clone(), v.clone());
                        }
                    }
                } else if !meta.is_null() {
                    task.metadata = meta.clone();
                }
            }

            let is_deleted = matches!(new_status, Some(TaskStatus::Deleted));

            // End mutable borrow on primary task before iterating other tasks
            let _ = task;

            // Apply bidirectional dependency changes to other tasks
            for (target_id, field, op) in dep_changes {
                if let Some(target) = store.get_mut(&target_id) {
                    let arr = match field {
                        DepField::Blocks => &mut target.blocks,
                        DepField::BlockedBy => &mut target.blocked_by,
                    };
                    match op {
                        DepOp::Add => {
                            if !arr.contains(&id_owned) {
                                arr.push(id_owned.clone());
                            }
                        }
                        DepOp::Remove => {
                            arr.retain(|x| x != &id_owned);
                        }
                    }
                }
            }

            // Cascading cleanup on deletion: remove this task's ID from all
            // other tasks' blocks/blocked_by arrays.
            if is_deleted {
                for (tid, other) in store.iter_mut() {
                    if tid == id {
                        continue;
                    }
                    other.blocks.retain(|x| x != id);
                    other.blocked_by.retain(|x| x != id);
                }
            }

            structured_tasks::tasks_to_value(&store)
        };

        ctx.emit_progress(format!("Updated task {id}")).await;

        Ok(
            ToolOutput::text(format!("Task {id} updated successfully.")).with_modifier(
                cocode_protocol::ContextModifier::StructuredTasksUpdated { tasks: snapshot },
            ),
        )
    }
}

#[cfg(test)]
#[path = "task_update.test.rs"]
mod tests;
