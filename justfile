set working-directory := "codex-rs"
set positional-arguments

# Display help
help:
    just -l

# `codex`
alias c := codex
codex *args:
    cargo run --bin codex -- "$@"

# `codex exec`
exec *args:
    cargo run --bin codex -- exec "$@"

# Run the CLI version of the file-search crate.
file-search *args:
    cargo run --bin codex-file-search -- "$@"

# Build the CLI and run the app-server test client
app-server-test-client *args:
    cargo build -p codex-cli
    cargo run -p codex-app-server-test-client -- --codex-bin ./target/debug/codex "$@"

# format code
fmt:
    cargo fmt -- --config imports_granularity=Item 2>/dev/null

fix *args:
    cargo clippy --fix --all-features --tests --allow-dirty "$@"

clippy:
    cargo clippy --all-features --tests "$@"

install:
    rustup show active-toolchain
    cargo fetch

# Run `cargo nextest` since it's faster than `cargo test`, though including
# --no-fail-fast is important to ensure all tests are run.
#
# Run `cargo install cargo-nextest` if you don't have it installed.
test:
    cargo nextest run --no-fail-fast

# Run the MCP server
mcp-server-run *args:
    cargo run -p codex-mcp-server -- "$@"

# ============================================================================
# Live Integration Tests (codex-api)
# ============================================================================

# Run all codex-api live integration tests
live-test:
    cargo test -p codex-api --ignored -- --test-threads=1

# Run live tests for a specific provider
live-test-genai:
    cargo test -p codex-api --ignored genai -- --test-threads=1

live-test-anthropic:
    cargo test -p codex-api --ignored anthropic -- --test-threads=1

live-test-openai:
    cargo test -p codex-api --ignored openai -- --test-threads=1

live-test-volc-ark:
    cargo test -p codex-api --ignored volc_ark -- --test-threads=1

live-test-zai:
    cargo test -p codex-api --ignored zai -- --test-threads=1

# Run live tests for a specific feature category
live-test-feature feature:
    cargo test -p codex-api --ignored {{feature}} -- --test-threads=1
