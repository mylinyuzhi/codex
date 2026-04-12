# coco-tool

Tool trait, streaming executor, tool registry. Split from coco-tools (implementations).

## TS Source
- `src/Tool.ts` (Tool interface, ToolUseContext)
- `src/services/tools/` (StreamingToolExecutor.ts, toolOrchestration.ts)
- `src/tools.ts` (tool registry, feature-gated loading)
- `src/utils/stream.ts` (async stream abstraction)

## Key Types
Tool trait, ToolUseContext, ToolError, StreamingToolExecutor, ToolRegistry, ToolBatch, ValidationResult
