#!/usr/bin/env bash
# Full regeneration pipeline: Rust → JSON Schema → Python types.
# Run from the repo root: ./cocode-sdk/scripts/generate_all.sh

set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"

echo "=== Step 1: Generate JSON Schema from Rust ==="
bash "$DIR/generate_schemas.sh"

echo ""
echo "=== Step 2: Generate Python types ==="
bash "$DIR/generate_python.sh"

echo ""
echo "=== Done ==="
