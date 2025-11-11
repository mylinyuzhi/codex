# Compact Strategies - Usage Examples

## Overview

Codex supports pluggable compact strategies to customize how conversation history is compressed. This feature allows you to choose between different compaction approaches based on your workflow needs.

## Available Strategies

### 1. `simple` (Default)
- **Focus:** Minimal, handoff-oriented summary
- **Prompt:** Concise 10-line instruction for LLM handoff
- **File Recovery:** None
- **Use Case:** Quick compaction with minimal token usage

### 2. `file-recovery`
- **Focus:** Structured summary + automatic file context preservation
- **Prompt:** 8-section structured template (Technical Context, Code Changes, etc.)
- **File Recovery:** Automatically recovers up to 5 recently accessed files
- **Use Case:** Dense coding sessions where file context is critical

## Configuration

### Method 1: Config File (Recommended)

Add to your `~/.codex/config.toml`:

```toml
[profiles.default]
# Use file-recovery strategy
compact_prompt = "strategy:file-recovery"

# Or use simple strategy (default)
# compact_prompt = "strategy:simple"
```

### Method 2: Profile-Specific Configuration

```toml
[profiles.coding]
model = "gpt-5-codex"
compact_prompt = "strategy:file-recovery"

[profiles.chat]
model = "gpt-4o"
compact_prompt = "strategy:simple"
```

Then run: `codex --profile coding`

### Method 3: Custom Prompt Override

You can still override the compact prompt entirely:

```toml
[profiles.default]
# File-based prompt override
experimental_compact_prompt_file = "~/.codex/my-compact-prompt.md"
```

## File Recovery Behavior

When using `file-recovery` strategy, Codex will:

1. **Parse History:** Extract all `read_file` tool calls from conversation history
2. **Filter Files:** Exclude `node_modules/`, `.git/`, `dist/`, `build/`, `.cache/`, `/tmp`
3. **Prioritize:** Select up to 5 most recently accessed files
4. **Read Current State:** Re-read files from filesystem (not historical output)
5. **Token Budget:** Limit to 10k tokens per file, 50k total
6. **Append:** Add recovered files to compacted history with line numbers

### Example Recovery Output

After compact with `file-recovery`:

```markdown
Context automatically compressed due to token limit. Essential information preserved.

[AI Summary of conversation...]

**Recovered File: src/main.rs**

\`\`\`
1 fn main() {
2     println!("Hello, world!");
3 }
\`\`\`

*Automatically recovered (250 tokens)*

**Recovered File: src/lib.rs**

\`\`\`
1 pub fn add(a: i64, b: i64) -> i64 {
2     a + b
3 }
\`\`\`

*Automatically recovered (180 tokens)*
```

## Comparison

| Feature | simple | file-recovery |
|---------|--------|---------------|
| **Summary Style** | Handoff-focused | 8-section structured |
| **Prompt Length** | ~10 lines | ~30 lines |
| **File Recovery** | ❌ No | ✅ Yes (up to 5 files) |
| **Token Overhead** | Low (~500) | Medium (~2k-50k depending on files) |
| **Best For** | Quick sessions, minimal context | Dense coding, file editing |

## Troubleshooting

### Q: Compact isn't using my selected strategy
**A:** Make sure `compact_prompt` includes the `strategy:` prefix. Without it, the value is treated as a custom prompt text.

### Q: File recovery isn't working
**A:** Check:
1. Files were accessed via `read_file` tool (not other methods)
2. Files aren't in excluded directories (`node_modules`, `.git`, etc.)
3. Files exist at their original paths when compact runs

### Q: Too many tokens after file recovery
**A:** File recovery respects token limits:
- Max 5 files
- Max 10k tokens per file
- Max 50k tokens total
If you're hitting limits, files are automatically truncated or skipped.

## Implementation Details

- **Architecture:** `core/src/compact_strategy.rs` defines `CompactStrategy` trait
- **Strategies:** `core/src/compact_strategies/{simple,file_recovery}.rs`
- **Dispatch:** `core/src/compact.rs` selects strategy based on `compact_prompt`
- **Templates:** `core/templates/compact/{prompt,file_recovery}.md`

## Future Extensions

To add a custom strategy:

1. Implement `CompactStrategy` trait in new file
2. Register in `CompactStrategyRegistry::new()`
3. Add prompt template in `templates/compact/`
4. Use via `compact_prompt = "strategy:your-name"`

See `CLAUDE.md` for full development guide.
