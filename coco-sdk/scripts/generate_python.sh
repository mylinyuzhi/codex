#!/usr/bin/env bash
# Generate Python Pydantic models from JSON Schema.
#
# Uses a custom generator (postprocess_python.py) that reads the Rust-generated
# JSON schemas directly and produces proper Pydantic models with tagged-union
# support, accessor methods, and ClientRequest wrappers.
#
# Run from the repo root: ./coco-sdk/scripts/generate_python.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCHEMA_DIR="$REPO_ROOT/coco-sdk/schemas/json"
SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT_FILE="$REPO_ROOT/coco-sdk/python/src/coco_sdk/generated/protocol.py"

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

# Append compatibility stubs for names referenced by client.py/tests but not
# yet emitted by the Rust schema generator. Stubs are loose BaseModels with
# `model_config = {"extra": "allow"}` so downstream imports work while the
# Rust side catches up.
echo "==> Appending compatibility stubs..."
python3 "$SCRIPTS_DIR/append_stubs.py" "$REPO_ROOT/coco-sdk/python" "$OUTPUT_FILE"

# Format with ruff if available
if command -v ruff &>/dev/null; then
    echo "==> Formatting with ruff..."
    ruff format "$OUTPUT_FILE" 2>/dev/null || true
fi

# Regenerate __init__.py with the actual set of protocol class names.
echo "==> Regenerating __init__.py..."
python3 "$SCRIPTS_DIR/regen_init.py" "$OUTPUT_FILE"

echo "==> Done. Generated types in: $OUTPUT_FILE"
