# coco-error

Unified error handling with StatusCode classification. Copied from cocode-rs.

## Source
Copied from cocode-rs `common/error`. Rust-only: snafu + proc-macro error infrastructure with no TS equivalent.

## Key Types
- `StatusCode` (5-digit `XX_YYY` classification)
- `ErrorExt` trait (`status_code()`, `is_retryable()`, `retry_after()`, `output_msg()`)
- snafu + snafu-virtstack for virtual stack traces

## Dependencies
None internal. Foundation crate at L1.
