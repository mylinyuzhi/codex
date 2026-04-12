# coco-process-hardening

Process hardening via OS-level security primitives. Copied from cocode-rs (Rust-only).

## Source
Copied from cocode-rs `exec/process-hardening`. Rust-only: `prctl`, `ptrace(PT_DENY_ATTACH)`, env sanitization via libc. No TS equivalent.

## Key Types
Process hardening functions (libc FFI wrappers)

## Dependencies
None (libc only).
