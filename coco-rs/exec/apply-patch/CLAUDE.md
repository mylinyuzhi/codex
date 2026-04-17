# coco-apply-patch

Unified diff/patch application with fuzzy matching. Source ported from
`codex-rs/apply-patch` — treat codex-rs as the upstream source of truth for
this crate.

Depends on `coco-exec-server` (local `ExecutorFileSystem`) because the
public API is async and takes a `cwd: &AbsolutePathBuf` + `fs: &dyn
ExecutorFileSystem`. Relative paths in patches resolve against that cwd.

Lives under `exec/` (not `utils/`) because it performs side-effecting
filesystem operations, alongside `coco-shell`, `coco-sandbox`, and
`coco-exec-server`.
