# FileTracker Implementation Status

This document tracks the FileTracker implementation status and integration points
with compaction, system-reminder, and rewind systems.

## Overview

FileTracker is a core component that tracks file read state for:
- Read-before-edit validation
- Already-read detection (cache hits)
- Change detection (file modified since last read)
- Compaction restoration (re-read files after compact)

## Implementation Location

| Component | Crate | File |
|-----------|-------|------|
| FileTracker struct | cocode-tools | `core/tools/src/context.rs` |
| File read tracking policy | cocode-message | `core/message/src/file_read_tracking_policy.rs` |
| File context resolver | cocode-system-reminder | `core/system-reminder/src/file_context_resolver.rs` |
| File restoration | cocode-loop | `core/loop/src/compaction.rs` |

## Key Constants (Claude Code v2.1.38 Alignment)

```rust
// Token limits for file restoration
pub const MAX_TOKENS_PER_FILE: usize = 5_000;   // 5k tokens per file
pub const MAX_TOTAL_TOKENS: usize = 50_000;     // 50k total tokens
pub const MAX_FILES_TO_RESTORE: usize = 5;      // Max 5 files to restore

// LRU cache limits
pub const LRU_MAX_ENTRIES: usize = 100;         // 100 file entries
pub const LRU_MAX_SIZE_BYTES: usize = 26_214_400; // ~25MB total
```

## Integration Points

### 1. Tool Execution (Read, Edit, Write)

When a file is read:
1. `Read` tool records file state via `FileTracker::record_read_with_state()`
2. State includes: content, timestamp, mtime, hash, read kind
3. Edit tool validates file was read before allowing modifications

### 2. System Reminder (Changed Files)

`ChangedFilesGenerator` uses FileTracker to:
1. Check `has_file_changed()` for each tracked file
2. Generate diff for modified files
3. Include in system reminder for model awareness

### 3. @Mentioned Files (Cache Detection)

`AtMentionedFilesGenerator` uses FileTracker for:
1. `is_already_read_unchanged()` - check if file can be skipped
2. Only full content reads are cacheable
3. Partial reads always re-read the file

### 4. Compaction (File Restoration)

Two approaches for file restoration:

#### `build_file_restoration_from_tracker()` - Uses cached content
- Gets files from tracker's cached content
- Faster but may have stale content
- Good for quick restoration

#### `collect_files_to_restore()` - Re-reads fresh content (Claude Code alignment)
- Re-reads files from disk
- Ensures fresh content after compaction
- Applies token limits (5k per file, 50k total, max 5 files)

### 5. Rewind (State Preservation)

FileTracker should survive rewind operations:
- `snapshot()` - capture current state
- `replace_snapshot()` - restore state after rewind
- File read state persists across conversation slicing

## File Read Kinds

```rust
pub enum FileReadKind {
    FullContent,    // Complete file read (cacheable)
    PartialContent, // Partial read with offset/limit (not cacheable)
    MetadataOnly,   // Glob/Grep results (not cacheable)
}
```

## Internal Files (Excluded from Tracking)

Files that should NOT be tracked or restored:
- Session memory files (`session-memory/summary.md`)
- Plan files (`~/.cocode/plans/`)
- Auto memory files (`MEMORY.md`, `memory-*.md`)
- Tool result persistence files (`tool-results/`)

Use `is_internal_file()` to check for exclusion.

## Known Gaps vs Claude Code

### Implemented ✅
- [x] LRU cache with size limits
- [x] Token estimation for content
- [x] File change detection (mtime-based)
- [x] Already-read cache detection
- [x] Internal file filtering
- [x] File restoration with token limits
- [x] File read tracking policy module

### Partially Implemented ⚠️
- [ ] Token-based LRU eviction (currently byte-based)
- [ ] `CompactedLargeFileRef` for truncated file tracking

### Not Yet Implemented ❌
- [ ] Rewind integration (needs testing)
- [ ] Session memory compaction Tier 1 integration
- [ ] Background file state cleanup

## Testing

Unit tests are located in:
- `core/tools/src/context.test.rs` - FileTracker tests
- `core/message/src/file_read_tracking_policy.rs` - Policy tests
- `core/system-reminder/src/file_context_resolver.rs` - Resolver tests
- `core/loop/src/compaction.test.rs` - Restoration tests

## Future Work

1. **Token-based eviction**: Consider switching from byte-based to token-based LRU eviction
2. **CompactedLargeFileRef**: Add tracking for truncated files during restoration
3. **Rewind integration**: Verify FileTracker state preservation during rewind
4. **Performance**: Profile file restoration during compaction

## References

- Claude Code analysis: `/analyze/cc/analysis_claude_code/claude_code_v_2.1.38/`
- File tracker doc: `07_compact/file_tracker.md`
- Rewind doc: `35_rewind/implementation.md`
- System reminder: `04_system_reminder/file_read_tracking.md`