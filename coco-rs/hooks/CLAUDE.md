# coco-hooks

Pre/post event hooks: bash/prompt/http/agent handlers, file change watcher, session hooks.

## TS Source
- `src/schemas/hooks.ts` (hook schema definitions)
- `src/utils/hooks/` (15+ files -- execution, fileChangedWatcher, postSamplingHooks, sessionHooks, ssrfGuard, skillImprovement)

## Key Types
HookDefinition, HookExecutor, HookMatcher, AsyncHookRegistry, HookHandler
