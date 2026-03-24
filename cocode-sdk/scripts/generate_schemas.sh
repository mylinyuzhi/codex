#!/usr/bin/env bash
# Generate JSON Schema files from Rust protocol types.
# Run from the repo root: ./cocode-sdk/scripts/generate_schemas.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "==> Generating JSON Schema from Rust types..."
cd "$REPO_ROOT/cocode-rs"
cargo run --bin export-app-server-schema

echo "==> Schemas written to cocode-rs/app-server-protocol/schema/json/"
ls -la "$REPO_ROOT/cocode-rs/app-server-protocol/schema/json/"
