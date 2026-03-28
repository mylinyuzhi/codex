#!/usr/bin/env bash
# Generate Python Pydantic models from JSON Schema.
#
# Uses a custom generator (postprocess_python.py) that reads the Rust-generated
# JSON schemas directly and produces proper Pydantic models with tagged-union
# support, accessor methods, and ClientRequest wrappers.
#
# Run from the repo root: ./cocode-sdk/scripts/generate_python.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCHEMA_DIR="$REPO_ROOT/cocode-sdk/schemas/json"
SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT_FILE="$REPO_ROOT/cocode-sdk/python/src/cocode_sdk/generated/protocol.py"

if [ ! -d "$SCHEMA_DIR" ]; then
    echo "Schema directory not found. Run generate_schemas.sh first."
    exit 1
fi

# Check that schema files exist
if [ ! -f "$SCHEMA_DIR/server_notification.json" ]; then
    echo "Schema files not found in $SCHEMA_DIR. Run generate_schemas.sh first."
    exit 1
fi

echo "==> Generating Python types from $SCHEMA_DIR..."
python3 "$SCRIPTS_DIR/postprocess_python.py" "$SCHEMA_DIR" "$OUTPUT_FILE"

# Format with ruff if available
if command -v ruff &>/dev/null; then
    echo "==> Formatting with ruff..."
    ruff format "$OUTPUT_FILE" 2>/dev/null || true
fi

echo "==> Done. Generated types in: $OUTPUT_FILE"
