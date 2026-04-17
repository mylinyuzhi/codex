#!/usr/bin/env bash
# Generate JSON Schema files from coco-rs protocol types.
#
# Runs the `export_schema` example under the `schema` feature in coco-types.
# The example writes directly to `coco-sdk/schemas/json/` by default, so
# there's no copy step.
#
# Usage: ./coco-sdk/scripts/generate_schemas.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "==> Generating JSON Schema from coco-rs types..."
cd "$REPO_ROOT/coco-rs"
cargo run -p coco-types --features schema --example export_schema

echo
echo "==> Schemas written to coco-sdk/schemas/json/"
ls -la "$REPO_ROOT/coco-sdk/schemas/json/"
