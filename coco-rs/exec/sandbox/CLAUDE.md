# coco-sandbox

Sandbox configuration and enforcement. Copied from cocode-rs (Rust superior).

## Source
Copied from cocode-rs `exec/sandbox`. Rust superior: seccomp filtering, Seatbelt/bubblewrap enforcement, violation ring buffer. TS equivalent is just an adapter around npm package.

## Key Types
- `SandboxMode` (None/ReadOnly/Strict) — defined in coco-types
- `SandboxConfig`, `SandboxSettings`
- `PermissionChecker`, `SandboxPlatform` trait (Unix/Windows stubs)

## Dependencies
coco-types (SandboxMode)
