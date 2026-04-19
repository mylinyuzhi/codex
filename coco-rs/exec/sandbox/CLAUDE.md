# coco-sandbox

Sandbox configuration and enforcement. Rust-native: seccomp filtering, Seatbelt (macOS), bubblewrap (Linux), violation ring buffer. TS has no real equivalent (thin adapter around npm sandboxing).

## Key Types

- `SandboxMode` (None / ReadOnly / Strict) — defined in `coco-types`
- `SandboxConfig`, `SandboxSettings`
- `PermissionChecker`, `SandboxPlatform` trait (Unix / Windows stubs)
