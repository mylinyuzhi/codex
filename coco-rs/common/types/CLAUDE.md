# coco-types

Foundation types shared across all crates. Zero internal dependencies.

## TS Source
- `src/types/` (message.ts, permissions.ts, command.ts, hooks.ts, plugin.ts)
- `src/Tool.ts` (ToolInputSchema, ToolResult<T>, ToolProgress)
- `src/Task.ts` (TaskType, TaskStatus, TaskStateBase, TaskHandle)

## Key Types
Message, UserMessage, AssistantMessage, PermissionMode, PermissionRule, CommandBase, ToolName (41 variants), SubagentType, HookEventType, SandboxMode, TokenUsage, ThinkingLevel, ProviderApi, ModelRole, Capability

Re-exports vercel-ai types as version-agnostic aliases (LlmMessage, LlmPrompt, UserContent, etc.).
