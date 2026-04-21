# coco-hooks

Pre/post event interception with scoped priority: Command / Prompt / Http / Agent handlers, SSRF guard, async-rewake protocol, `if` permission-rule conditions, matcher patterns (exact / pipe-separated / regex / glob), dedup + `once` tracking.

## TS Source
- `schemas/hooks.ts` — HookMatcher / HookCommand zod schemas
- `utils/hooks/hooksSettings.ts`, `hooksConfigManager.ts`, `hooksConfigSnapshot.ts` — settings layer
- `utils/hooks/hookHelpers.ts`, `execAgentHook.ts`, `execHttpHook.ts`, `execPromptHook.ts` — executors
- `utils/hooks/AsyncHookRegistry.ts` — async hook state
- `utils/hooks/ssrfGuard.ts` — private-IP blocklist for HTTP hooks
- `utils/hooks/sessionHooks.ts`, `postSamplingHooks.ts`, `skillImprovement.ts` — callback-style hooks
- `utils/hooks/registerFrontmatterHooks.ts`, `registerSkillHooks.ts` — integration with skills/commands
- `utils/hooks/apiQueryHookHelper.ts`, `fileChangedWatcher.ts`, `hookEvents.ts` — pipeline glue

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types
- `HookDefinition` — `event` (`HookEventType`, 27 variants), `matcher`, `handler`, `priority` (asc), `scope` (Session>Local>Project>User>Builtin), `if_condition`, `once`, `is_async`, `async_rewake`, `shell`, `status_message`
- `HookHandler` — enum: `Command{command,timeout_ms,shell}`, `Prompt{prompt}`, `Http{url,method,headers,timeout_ms}`, `Agent{agent_name,prompt}`
- `HookExecutionResult` — `CommandOutput{exit_code,stdout,stderr}` or `PromptText(String)`
- `HookExecutionMeta`, `HookExecutionEvent` — progress display payloads
- `HooksSettings` — deserialized config wrapper
- `HookRegistry` — `register_deduped`, `find_matching[_with_if]`, `execute_hooks`, `mark_once_fired`; sorts by scope desc then priority asc
- `IfConditionContext` — tool name + content for `"Bash(git *)"`-style conditions
- `PromptRequest` / `PromptResponse` / `PromptOption` — interactive hook prompts via stdout/stdin
- `SESSION_HOOK_EVENTS` — 10 session-level event names

## Key Functions
- `execute_hook()` — dispatch across handler types with env injection + stdin piping, CRLF-sanitized HTTP headers, `$VAR` interpolation, SSRF gate for HTTP, shell prefix via `COCO_SHELL_PREFIX`
- `load_hooks_from_config()` — deserialize snake_case event-keyed JSON, apply top-level `timeout` seconds to handler ms
- `matcher_matches()` — TS parity: `None` all, `"*"` needs value, simple alnum/`_`/`|`, else regex with glob fallback

## Modules
- `async_registry` — async hook bookkeeping + polling
- `inputs` — event-specific match-value extraction
- `orchestration` — parallel hook execution with env vars and stdin
- `ssrf` — URL → IP resolution + private/link-local blocklist
