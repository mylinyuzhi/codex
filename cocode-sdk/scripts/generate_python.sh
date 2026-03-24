#!/usr/bin/env bash
# Generate Python Pydantic models from JSON Schema.
# Requires: pip install datamodel-code-generator
#
# Run from the repo root: ./cocode-sdk/scripts/generate_python.sh
#
# If datamodel-code-generator is not installed, this script will print
# instructions and exit. The hand-written types in
# cocode-sdk/python/src/cocode_sdk/generated/protocol.py serve as
# the baseline and can be used without code generation.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCHEMA_DIR="$REPO_ROOT/cocode-rs/app-server-protocol/schema/json"
OUT_FILE="$REPO_ROOT/cocode-sdk/python/src/cocode_sdk/generated/protocol_gen.py"

if ! command -v datamodel-codegen &>/dev/null; then
    echo "datamodel-code-generator not found."
    echo "Install with: pip install datamodel-code-generator"
    echo ""
    echo "Using hand-written protocol.py instead."
    exit 0
fi

BUNDLE="$SCHEMA_DIR/cocode_app_server_protocol.schemas.json"

if [ ! -f "$BUNDLE" ]; then
    echo "Schema bundle not found. Run generate_schemas.sh first."
    exit 1
fi

echo "==> Generating Python types from $BUNDLE..."
datamodel-codegen \
    --input "$BUNDLE" \
    --output "$OUT_FILE" \
    --input-file-type jsonschema \
    --output-model-type pydantic_v2.BaseModel \
    --use-annotated \
    --field-constraints \
    --target-python-version 3.10

echo "==> Generated: $OUT_FILE"
