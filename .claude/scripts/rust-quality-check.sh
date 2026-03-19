#!/usr/bin/env bash
# Rust quality gate — runs when Claude stops after editing Rust files.
set -euo pipefail

cd /lyz/codespace/codex/cocode-rs

# Check if any .rs files were modified (staged or unstaged)
if ! git diff --name-only HEAD 2>/dev/null | grep -q '\.rs$'; then
  exit 0  # No Rust changes, skip checks
fi

echo "🔍 Rust changes detected — running quality checks..."

echo "── fmt ──"
just fmt

echo "── pre-commit (fmt + check + clippy) ──"
just pre-commit

echo "✅ Quality checks passed."
