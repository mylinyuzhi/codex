#!/usr/bin/env bash
# Rust quality gate — runs when Claude stops after editing Rust files.
# Exit 0 = pass (stop proceeds), Exit 2 = fail (stop blocked, stderr fed to agent).

cd "${CLAUDE_PROJECT_DIR:?CLAUDE_PROJECT_DIR not set}/cocode-rs"

# Check if any .rs files were modified (staged or unstaged)
if ! git diff --name-only HEAD 2>/dev/null | grep -q '\.rs$'; then
  exit 0  # No Rust changes, skip checks
fi

echo "Rust changes detected — running quality checks..." >&2

# Step 1: fmt (auto-fix, should not fail)
just fmt 2>&1

# Step 2: check + clippy — capture output, report errors
OUTPUT=$(just pre-commit 2>&1) || {
  echo "Quality checks failed. Fix these errors:" >&2
  echo "$OUTPUT" >&2
  exit 2  # Block stop, feed errors back to agent
}

echo "Quality checks passed." >&2
exit 0
