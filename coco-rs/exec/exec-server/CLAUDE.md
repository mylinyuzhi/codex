# coco-exec-server

Minimal filesystem abstraction ported from codex-rs so `coco-apply-patch`
can consume the same `ExecutorFileSystem` trait and `LOCAL_FS` static that
codex-rs uses.

Only the file-system subset of codex-rs `codex-exec-server` is ported here:

- `ExecutorFileSystem` trait (async, tokio-backed)
- `LocalFileSystem` + `LOCAL_FS` static
- Companion types (`CreateDirectoryOptions`, `RemoveOptions`, `CopyOptions`,
  `FileMetadata`, `ReadDirectoryEntry`, `FileSystemResult`)

The process-execution RPC and sandbox-policy-aware variants that codex-rs
provides are intentionally not ported — coco-rs has its own sandbox stack
(`coco-sandbox`) and the only current consumer is `coco-apply-patch`, which
exclusively uses the non-sandbox file-system methods.
