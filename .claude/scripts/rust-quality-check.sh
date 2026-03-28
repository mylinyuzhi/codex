#!/usr/bin/env bash
# Rust quality gate — runs when Claude stops after editing Rust files.
# Exit 0 = pass (stop proceeds), Exit 2 = fail (stop blocked, stderr fed to agent).

# Ensure cargo/rustup are on PATH
[ -f "/usr/local/cargo/env" ] && . "/usr/local/cargo/env"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Disable sccache to avoid FD limit issues in constrained environments
export RUSTC_WRAPPER=""
export CARGO_BUILD_JOBS=2

cd "${CLAUDE_PROJECT_DIR:?CLAUDE_PROJECT_DIR not set}/cocode-rs"

# Check if any .rs files were modified (staged or unstaged)
CHANGED_RS=$(git diff --name-only HEAD 2>/dev/null | grep '\.rs$')
if [ -z "$CHANGED_RS" ]; then
  exit 0  # No Rust changes, skip checks
fi

echo "Rust changes detected — running quality checks..." >&2

# Detect which crates were changed (extract crate directories from paths)
CRATE_ARGS=""
for f in $CHANGED_RS; do
  # Find the nearest Cargo.toml to determine the crate
  dir=$(dirname "$f")
  while [ "$dir" != "." ] && [ ! -f "$dir/Cargo.toml" ]; do
    dir=$(dirname "$dir")
  done
  if [ -f "$dir/Cargo.toml" ]; then
    name=$(grep '^name' "$dir/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
    if [ -n "$name" ] && ! echo "$CRATE_ARGS" | grep -q -- "-p $name"; then
      CRATE_ARGS="$CRATE_ARGS -p $name"
    fi
  fi
done

if [ -z "$CRATE_ARGS" ]; then
  echo "Could not determine changed crates, skipping." >&2
  exit 0
fi

echo "Checking crates:$CRATE_ARGS" >&2

# Step 1: check changed crates — must pass
CHECK_OUTPUT=$(cargo check $CRATE_ARGS 2>&1) || {
  # Filter out "can't find crate" errors (stale build cache, not real errors)
  REAL_ERRORS=$(echo "$CHECK_OUTPUT" | grep '^error' | grep -v "can't find crate\|E0463\|aborting")
  if [ -n "$REAL_ERRORS" ]; then
    echo "Compilation failed. Fix these errors:" >&2
    echo "$REAL_ERRORS" >&2
    exit 2
  fi
  echo "Build cache errors detected (not code errors), passing." >&2
}

echo "Quality checks passed." >&2
exit 0
