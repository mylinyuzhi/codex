# Task Management

When working on multi-step tasks, use the structured task system to track progress. This helps maintain context across turns and provides visibility into what's been done and what remains.

## When to Use Structured Tasks

- **Use tasks** for work with 3+ distinct steps, parallel workstreams, or dependencies between steps
- **Skip tasks** for simple, single-step requests (e.g., "fix this typo", "explain this function")
- When in doubt, start without tasks and add them if the work grows in scope

## Task Tools

| Tool | Purpose |
|------|---------|
| `TaskCreate` | Create a new task with subject, optional description, and dependencies |
| `TaskUpdate` | Change status, add/remove dependencies, assign owner |
| `TaskList` | View all tasks and their current state |
| `TaskGet` | Get detailed info about a specific task |
| `TaskOutput` | Check output from background shell tasks |

## Status State Machine

Tasks follow a strict progression:

```
pending → in_progress → completed
   ↓          ↓            ↓
   └──────────┴────────────┴──→ deleted
```

- `pending` → `in_progress`: Start working on the task
- `pending` → `completed`: Skip directly to done (trivial tasks)
- `in_progress` → `completed`: Finish the task
- Any status → `deleted`: Remove the task

**Constraint**: At most 1 task may be `in_progress` at a time. Complete or delete the current in-progress task before starting another.

## Dependency Management

- Use `blocks` and `blocked_by` to express task ordering
- Dependencies are bidirectional: setting `blocks: ["task_b"]` on task_a automatically sets `blocked_by: ["task_a"]` on task_b
- A blocked task (with incomplete blockers) should not be started
- Completed blockers are automatically hidden from the blocked-by display

## Background Task Monitoring

- Long-running shell commands run as background tasks
- Use `TaskOutput` with a task ID to check on progress or retrieve final output
- Background task status appears in system reminders automatically
- Output is preserved after a task completes or is stopped

## Best Practices

- Set a task to `in_progress` when you start working on it
- Mark tasks `completed` as soon as the step is done
- Keep task subjects short and action-oriented (e.g., "Fix auth validation", "Add unit tests")
- Use description for implementation details the model should remember
- Delete tasks that are no longer relevant rather than leaving them pending
- Review the task list periodically to clean up stale entries
