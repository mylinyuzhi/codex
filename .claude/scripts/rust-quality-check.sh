#!/usr/bin/env bash
# Rust quality gate — runs when Claude stops after editing Rust files.
# Exit 0 = pass (stop proceeds), Exit 2 = fail (stop blocked, stderr fed to agent).

# Ensure cargo/rustup are on PATH (hooks run without .bashrc)
[ -f "/usr/local/cargo/env" ] && . "/usr/local/cargo/env"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

cd "${CLAUDE_PROJECT_DIR:?CLAUDE_PROJECT_DIR not set}/cocode-rs"

# Check if any .rs files were modified (staged or unstaged)
if ! git diff --name-only HEAD 2>/dev/null | grep -q '\.rs$'; then
  exit 0  # No Rust changes, skip checks
fi

echo "Rust changes detected — running quality checks..." >&2

# Step 1: fmt (auto-fix, should not fail)
just fmt 2>&1

# Step 2: check — must pass
CHECK_OUTPUT=$(just check 2>&1) || {
  echo "Compilation failed. Fix these errors:" >&2
  echo "$CHECK_OUTPUT" >&2
  exit 2
}

# Step 3: clippy on lib code — block on warnings
LIB_OUTPUT=$(cargo clippy --all-features 2>&1)
LIB_EXIT=$?
if [ $LIB_EXIT -ne 0 ] || echo "$LIB_OUTPUT" | grep -q '^warning:'; then
  WARNINGS=$(echo "$LIB_OUTPUT" | grep -E '^(warning|error)' | grep -v 'generated')
  if [ -n "$WARNINGS" ]; then
    echo "Clippy lib warnings/errors detected. Fix them:" >&2
    echo "$WARNINGS" >&2
    exit 2
  fi
fi

# Step 4: clippy on tests — only block on errors, warnings are tolerated
TEST_OUTPUT=$(cargo clippy --all-features --tests 2>&1)
if [ $? -ne 0 ]; then
  ERRORS=$(echo "$TEST_OUTPUT" | grep '^error' | grep -v 'generated\|aborting')
  if [ -n "$ERRORS" ]; then
    echo "Clippy test errors detected. Fix them:" >&2
    echo "$ERRORS" >&2
    exit 2
  fi
fi

echo "Quality checks passed." >&2
exit 0
