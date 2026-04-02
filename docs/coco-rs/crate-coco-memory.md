# coco-memory — Crate Plan

TS source: `src/memdir/` (507 LOC), `src/services/extractMemories/`, `src/services/SessionMemory/`, `src/services/autoDream/`

## Dependencies

```
coco-memory depends on:
  - coco-types (Message), coco-inference (ApiClient — LLM for auto-extraction)
  - utils/frontmatter (YAML frontmatter parsing)

coco-memory does NOT depend on:
  - coco-tools, coco-query, any app/ crate
```

## Data Definitions

```rust
/// 4-type taxonomy with scope rules (from memdir/memoryTypes.ts 271 LOC):
///   User: role, preferences, knowledge → tailor behavior
///   Feedback: corrections AND confirmations → avoid repeating mistakes
///   Project: goals, decisions, deadlines → understand context (convert relative dates)
///   Reference: pointers to external systems → know where to look
pub enum MemoryEntryType { User, Feedback, Project, Reference }

pub struct MemoryEntry {
    pub name: String,
    pub description: String,        // One-line, used for relevance matching
    pub entry_type: MemoryEntryType,
    pub content: String,
    pub file_path: PathBuf,
}

/// MEMORY.md index: one-line pointers, max 200 lines.
/// Lines after 200 truncated with warning.
/// Max 25KB total size for MEMORY.md.
pub struct MemoryIndex {
    pub entries: Vec<MemoryIndexEntry>,
}

pub struct MemoryIndexEntry {
    pub title: String,
    pub file: String,     // Relative path to .md file
    pub hook: String,     // One-line description (<150 chars)
}
```

## Core Logic

```rust
pub struct MemoryManager {
    pub memory_dir: PathBuf,  // ~/.coco/projects/<hash>/memory/
    pub index: MemoryIndex,
}

impl MemoryManager {
    /// Load MEMORY.md index + all referenced memory files.
    /// Truncation: If MEMORY.md exceeds 200 lines or 25KB, truncate with warning.
    pub fn load(project_dir: &Path) -> Self;

    /// Two-step save process:
    /// 1. Write memory content to its own file (e.g., feedback_testing.md)
    /// 2. Add one-line pointer to MEMORY.md index
    /// Dedup: check existing memories before creating new one.
    pub fn save(&mut self, entry: MemoryEntry) -> Result<(), MemoryError>;
    pub fn delete(&mut self, name: &str) -> Result<(), MemoryError>;

    /// Auto-extract memories from conversation (LLM call via ApiClient).
    pub async fn auto_extract(&mut self, messages: &[Message], api: &ApiClient) -> Vec<MemoryEntry>;

    /// Staleness detection (from memdir/memoryAge.ts 53 LOC):
    /// Human-readable age ("47 days ago").
    /// Caveat text for memories >1 day old: "this memory may be stale".
    pub fn memory_age(entry: &MemoryEntry) -> String;
    pub fn memory_freshness_text(entry: &MemoryEntry) -> Option<String>;

    /// Sonnet-based recall selector (from memdir/findRelevantMemories.ts 141 LOC):
    /// Filters 5 most relevant memory files based on current context.
    pub async fn find_relevant_memories(
        &self,
        context: &str,
        api: &ApiClient,
    ) -> Vec<MemoryEntry>;
}
```
