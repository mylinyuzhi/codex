# coco-file-ignore

Unified `.gitignore` / `.ignore` / custom-exclude filtering wrapped over the `ignore` crate.

## Key Types

| Type | Purpose |
|------|---------|
| `IgnoreService` | Main entry point; `new(config)` / `with_defaults()` → `create_walk_builder` |
| `IgnoreConfig` | Toggles: `respect_gitignore`, `respect_ignore`, `include_hidden`, `follow_links`, `custom_excludes` |
| `PatternMatcher`, `PathChecker` | Lower-level matcher primitives |
| `IGNORE_FILES`, `find_ignore_files` | Ignore-file discovery helpers |
| `BINARY_FILE_PATTERNS`, `COMMON_IGNORE_PATTERNS`, `SYSTEM_FILE_EXCLUDES`, `COMMON_DIRECTORY_EXCLUDES`, `get_all_default_excludes` | Shared pattern constants |

Re-exports `ignore::WalkBuilder` so consumers don't need to depend on `ignore` directly.

## Note

Direct edits to existing files are preferred over `*_ext.rs` extension modules for this crate.
