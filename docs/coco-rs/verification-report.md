# coco-rs 文档完整性验证报告

验证日期: 2026-04-02 | 修复完成: 2026-04-02
TS 文件总数: 1884
Plan doc 总数: 22 (含本报告)

---

## 1. 目录级覆盖: 53/53 (100%)

所有 35 个第一层目录 + 18 个顶层文件全部在 ts-to-rust-mapping.md 中有映射。

**已知问题**:
- 7 个 v3 条目 crate 为 "TBD"（可接受，v3 暂不实现）
- 69 个条目缺少 Strategy 列（在补充表格中，主表格完整）

## 2. 子目录级覆盖: 190/190 (100%)

所有 31 个 `utils/` 子目录、36 个 `services/` 条目、40 个 `tools/` 子目录、67 个 `commands/` 子目录全部有映射覆盖。

## 3. Crate Doc 质量: 11/15 通过全部检查

| 检查结果 | 文件 | 问题 |
|----------|------|------|
| FAIL | crate-coco-types.md | 缺 `## Dependencies` 标题 + 无函数签名 |
| FAIL | crate-coco-modules.md | 缺顶层 TS source 行 + 缺 "does NOT depend" |
| FAIL | crate-coco-app.md | 缺 "does NOT depend" |
| FAIL | crate-coco-query.md | "depends on" 格式偏差（有括号插入） |

## 4. 类型唯一性: 154 个类型，0 个 crate 间重复

multi-provider-plan.md 有 2 个类型无 canonical owner:
- `ThinkingLevel` — 应属于 crate-coco-config.md
- `ModelHub` — CLAUDE.md 说属于 crate-coco-inference.md 但未在该文件定义

## 5. 依赖合规: 78 条依赖，0 个层级违反

## 6. 函数级覆盖抽样

| 文件 | TS 项目 | 已覆盖 | 延迟(v2/UI) | 缺失 | 覆盖率 |
|------|---------|--------|-------------|------|--------|
| Tool.ts | 47 | 24 | 15 | 4 | 75% |
| QueryEngine.ts | 7 | 4 | 0 | 3 | 57% |
| StreamingToolExecutor.ts | 19 | 14 | 0 | 4 | 74% |
| utils/messages.ts | ~93 | 7 | 0 | ~70 | 9% (P2) |
| services/mcp/client.ts | 26 | 12 | 0 | 14 | 46% |

## 7. 待修复项清单

### Must Fix (格式/完整性)

| # | 文件 | 问题 | 修复 |
|---|------|------|------|
| F1 | crate-coco-types.md | 缺 `## Dependencies` 标题 | 将 `### Dependency Rule` 改为 `## Dependencies` |
| F2 | crate-coco-types.md | 缺 "does NOT depend" | 添加 |
| F3 | crate-coco-modules.md | 缺顶层 TS source 行 | 在文件头加 `TS source:` |
| F4 | crate-coco-modules.md | 缺 "does NOT depend" | 在依赖块中添加 |
| F5 | crate-coco-app.md | 缺 "does NOT depend" | 在各子 crate 依赖中添加 |
| F6 | crate-coco-query.md | "depends on" 格式偏差 | 移除括号 |
| F7 | multi-provider-plan.md | `ThinkingLevel` 无 canonical owner | 移到 crate-coco-config.md |
| F8 | crate-coco-inference.md | `ModelHub` struct 未定义 | 添加 pub struct ModelHub |
| F9 | crate-coco-tool.md | 缺 `prompt()`, `isLsp`, `strict`, `mapToolResultToToolResultBlockParam` | 添加到 trait |
| F10 | crate-coco-query.md | 缺 `interrupt()`, `getSessionId()`, `setModel()` | 添加到 impl |
| F11 | ts-to-rust-mapping.md | 69 条目缺 Strategy 列 | 补充表格加 Strategy 列 |

### 修复状态

| # | 问题 | 状态 |
|---|------|------|
| F1 | crate-coco-types.md 缺 Dependencies 标题 | **FIXED** |
| F2 | crate-coco-types.md 缺 "does NOT depend" | **FIXED** |
| F3 | crate-coco-modules.md 缺顶层 TS source | **FIXED** |
| F4 | crate-coco-modules.md 缺 "does NOT depend" | **FIXED** |
| F5 | crate-coco-app.md 缺 "does NOT depend" | **FIXED** |
| F6 | crate-coco-query.md 格式偏差 | **FIXED** |
| F7 | ThinkingLevel 无 canonical owner | **FIXED** → crate-coco-config.md |
| F8 | ModelHub struct 未在 inference doc 定义 | **FIXED** → added pub struct + impl |
| F9 | Tool trait 缺 4 个方法 | **FIXED** → added prompt, is_lsp, strict, map_tool_result_to_block |
| F10 | QueryEngine 缺 3 个方法 | **FIXED** → added interrupt, session_id, set_model |
| F11 | 69 条目缺 Strategy 列 | **FIXED** → all supplementary tables now have Strategy column |

### P2 (已知延迟 — 实现时补充)

| # | 文件 | 问题 | 状态 |
|---|------|------|------|
| P1 | crate-coco-messages.md | ~70 个 messages.ts 函数未列出 | 实现时按类别文档化 |
| P2 | crate-coco-mcp.md | 14 个 client.ts 工具函数未列出 | 实现时补充 |
