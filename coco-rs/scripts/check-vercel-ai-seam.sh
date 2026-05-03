#!/usr/bin/env bash
# Enforce the coco-inference seam.
#
# Rule: only `services/inference/Cargo.toml` (the seam crate) may declare
# a `vercel-ai*` dependency. All other crates must reach AI SDK types via
# `coco_inference::*` aliases — see `services/inference/src/lib.rs` for
# the canonical version-agnostic re-export list.
#
# `coco-messages` consumes `LanguageModelMessage` (etc.) via `coco_inference`,
# never via `vercel_ai_provider`. `coco-types` is intentionally LLM-free.
#
# Wired into `just pre-commit` (justfile) and the Stop hook
# (.claude/scripts/rust-quality-check.sh). Safe to run standalone:
#   ./scripts/check-vercel-ai-seam.sh
#
# Exits with non-zero status (and prints offending Cargo.toml paths) when
# violations are found; silent + status 0 when clean.

set -euo pipefail

# Run from this script's parent directory so relative paths line up
# regardless of where the user invokes it.
cd "$(dirname "$0")/.."

# Allowed Cargo.toml owners of any `vercel-ai*` dep:
#   - the workspace root (`Cargo.toml`) where workspace deps are declared
#   - the `vercel-ai/` SDK crates themselves (they reference each other internally)
#   - `services/inference/Cargo.toml` — the single seam crate
allow_re='^(Cargo\.toml$|vercel-ai/|services/inference/Cargo\.toml$)'

# A line in a Cargo.toml that declares a dep on any `vercel-ai*` crate.
# Matches both forms:
#   vercel-ai-anthropic = { workspace = true }
#   vercel-ai-anthropic.workspace = true
# Lines like `vercel-ai-stuff` in comments don't begin with the dep name,
# so the leading anchor is safe.
forbidden_dep_re='^[[:space:]]*vercel-ai[A-Za-z0-9_-]*([.][A-Za-z0-9_-]+)?[[:space:]]*='

violations=$(
    while IFS= read -r path; do
        if grep -qE "$forbidden_dep_re" "$path" 2>/dev/null; then
            printf '%s\n' "$path"
        fi
    done < <(find . -name Cargo.toml -not -path './target/*' -print)
)
violations=$(printf '%s' "$violations" | sed 's|^\./||' | grep -Ev "$allow_re" || true)

if [ -n "$violations" ]; then
    echo "✗ Cargo.toml depends on vercel-ai* outside the seam:" >&2
    echo "$violations" | sed 's/^/    /' >&2
    echo "  → only services/inference may declare vercel-ai* deps." >&2
    echo "  → other crates must use coco_inference::* aliases." >&2
    echo "  → see services/inference/src/lib.rs for the canonical re-export list." >&2
    exit 1
fi
