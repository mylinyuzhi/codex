#!/usr/bin/env bash
# Generate JSON Schema files from coco-rs protocol types.
#
# Builds the `export_schema` example under the `schema` feature in
# coco-types, then runs the resulting binary. Output lands in
# `coco-sdk/schemas/json/`.
#
# Usage:
#   ./coco-sdk/scripts/generate_schemas.sh              # regenerate (skip if up-to-date)
#   ./coco-sdk/scripts/generate_schemas.sh --check      # exit 1 if regen would change anything
#   ./coco-sdk/scripts/generate_schemas.sh --force      # regenerate unconditionally
#   ./coco-sdk/scripts/generate_schemas.sh --quiet      # suppress per-file progress
#
# `--check` is the CI mode: it runs the generator into a tempdir and
# diffs against the committed bundle. Exits 0 if identical, 1 + a diff
# summary if not. Does NOT modify the working tree.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CARGO_MANIFEST="$REPO_ROOT/coco-rs/common/types/Cargo.toml"
SCHEMA_DIR="$REPO_ROOT/coco-sdk/schemas/json"
BUNDLE="$SCHEMA_DIR/coco_app_server_protocol.schemas.json"

CHECK_MODE=false
FORCE_MODE=false
QUIET_MODE=false

for arg in "$@"; do
    case "$arg" in
        --check) CHECK_MODE=true ;;
        --force) FORCE_MODE=true ;;
        --quiet|-q) QUIET_MODE=true ;;
        -h|--help)
            sed -n '2,17p' "$0"
            exit 0
            ;;
        *)
            echo "error: unknown flag '$arg' (use --help)" >&2
            exit 2
            ;;
    esac
done

log() {
    if ! $QUIET_MODE; then
        echo "$@"
    fi
}

# ---------------------------------------------------------------------------
# Skip regen when nothing has changed
# ---------------------------------------------------------------------------
#
# The schema is fully derived from `coco-types` source + the example
# itself. If every input is older than the bundle, the cargo invocation
# would be wasted work — skip unless `--force` or `--check`.
needs_regen() {
    [[ ! -f "$BUNDLE" ]] && return 0
    local newer
    newer=$(find \
        "$REPO_ROOT/coco-rs/common/types/src" \
        "$REPO_ROOT/coco-rs/common/types/examples" \
        "$REPO_ROOT/coco-rs/common/types/Cargo.toml" \
        -newer "$BUNDLE" -print -quit 2>/dev/null || true)
    [[ -n "$newer" ]]
}

if ! $FORCE_MODE && ! $CHECK_MODE && ! needs_regen; then
    log "==> Schemas already up-to-date (use --force to regenerate)."
    exit 0
fi

# ---------------------------------------------------------------------------
# Build the example once, then exec. Splits build vs. run so warm-cache
# invocations don't reprint the cargo build banner. `--quiet` suppresses
# the "Compiling …" lines on cold builds; errors still surface.
# ---------------------------------------------------------------------------
log "==> Building export_schema (release-equivalent debug profile)..."
BUILD_FLAGS=(--manifest-path "$CARGO_MANIFEST" -p coco-types --features schema --example export_schema)
if $QUIET_MODE; then
    cargo build "${BUILD_FLAGS[@]}" --quiet
else
    cargo build "${BUILD_FLAGS[@]}"
fi

EXAMPLE_BIN="$(cargo metadata --manifest-path "$CARGO_MANIFEST" --format-version 1 \
    | python3 -c 'import json,sys;m=json.load(sys.stdin);print(m["target_directory"])')/debug/examples/export_schema"

if [[ ! -x "$EXAMPLE_BIN" ]]; then
    echo "error: built binary not found at $EXAMPLE_BIN" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# --check: run into a tempdir, diff the bundle, never touch the tree.
# ---------------------------------------------------------------------------
if $CHECK_MODE; then
    TMP_OUT="$(mktemp -d)"
    trap 'rm -rf "$TMP_OUT"' EXIT
    log "==> Running export_schema into $TMP_OUT..."
    if $QUIET_MODE; then
        "$EXAMPLE_BIN" "$TMP_OUT" > /dev/null
    else
        "$EXAMPLE_BIN" "$TMP_OUT"
    fi
    if diff -q "$BUNDLE" "$TMP_OUT/coco_app_server_protocol.schemas.json" > /dev/null 2>&1; then
        log "==> OK: schemas are up-to-date."
        exit 0
    fi
    echo "ERROR: schemas are out of date. Run:" >&2
    echo "    ./coco-sdk/scripts/generate_schemas.sh" >&2
    echo "    ./coco-sdk/scripts/generate_python.sh" >&2
    diff -u "$BUNDLE" "$TMP_OUT/coco_app_server_protocol.schemas.json" | head -60
    exit 1
fi

# ---------------------------------------------------------------------------
# Regenerate in place
# ---------------------------------------------------------------------------
log "==> Running export_schema → $SCHEMA_DIR..."
if $QUIET_MODE; then
    "$EXAMPLE_BIN" "$SCHEMA_DIR" > /dev/null
else
    "$EXAMPLE_BIN" "$SCHEMA_DIR"
fi

# ---------------------------------------------------------------------------
# End-of-run summary — replaces the noisy `ls -la`.
# ---------------------------------------------------------------------------
SCHEMA_COUNT=$(find "$SCHEMA_DIR" -maxdepth 1 -name '*.json' | wc -l | tr -d ' ')
BUNDLE_KB=$(du -k "$BUNDLE" | cut -f1)
DEF_COUNT=$(python3 -c "import json;s=json.load(open('$BUNDLE'));d=s.get('\$defs') or s.get('definitions') or {};print(len(d))")
log ""
log "==> Wrote $SCHEMA_COUNT schema file(s); bundle is ${BUNDLE_KB}K with $DEF_COUNT type definitions."
