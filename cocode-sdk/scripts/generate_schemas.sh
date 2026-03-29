#!/usr/bin/env bash
# Generate JSON Schema files from Rust protocol types.
# Run from the repo root: ./cocode-sdk/scripts/generate_schemas.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "==> Generating JSON Schema from Rust types..."
cd "$REPO_ROOT/cocode-rs"
cargo run --bin export-app-server-schema

echo "==> Copying schemas to cocode-sdk/schemas/json/..."
mkdir -p "$REPO_ROOT/cocode-sdk/schemas/json"
cp "$REPO_ROOT/cocode-rs/app-server-protocol/schema/json/"*.json "$REPO_ROOT/cocode-sdk/schemas/json/"

echo "==> Schemas written to cocode-sdk/schemas/json/"
ls -la "$REPO_ROOT/cocode-sdk/schemas/json/"
