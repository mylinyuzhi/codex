#!/usr/bin/env bash
# Rust quality gate — runs when Claude stops after editing Rust files.
# Exit 0 = pass (stop proceeds), Exit 2 = fail (stop blocked, stderr fed to agent).
#
# Targets `coco-rs/` because that is the active-development workspace
# (per coco-rs/CLAUDE.md and the parent CLAUDE.md). `cocode-rs/` is the
# read-only reference implementation and is intentionally not gated here.

# Ensure cargo/rustup are on PATH (hooks run without .bashrc).
[ -f "/usr/local/cargo/env" ] && . "/usr/local/cargo/env"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

PROJECT_DIR="${CLAUDE_PROJECT_DIR:?CLAUDE_PROJECT_DIR not set}"
WORKSPACE="$PROJECT_DIR/coco-rs"

# Skip when nothing in coco-rs changed (Rust source OR Cargo manifest —
# manifest changes catch a vercel-ai-provider dep being re-introduced
# without touching any .rs file).
changed=$(git -C "$PROJECT_DIR" diff --name-only HEAD 2>/dev/null \
            | grep -E '^coco-rs/.*(\.rs|/Cargo\.toml)$' || true)
if [ -z "$changed" ]; then
  exit 0
fi

cd "$WORKSPACE"

# Workaround: virtiofs (macOS Podman) caps open FDs at ~700, too low for a
# full-workspace cargo build.  Place target dir on the container-local overlay
# filesystem, keyed by workspace path so concurrent worktrees stay isolated.
export CARGO_TARGET_DIR="/tmp/cargo-target-$(echo "$PWD" | md5sum | cut -c1-12)"

echo "Rust changes detected — running quality checks..." >&2

# Step 0: vercel-ai seam guard — no direct vercel_ai_provider:: imports
# outside services/inference, no V4-suffixed type names leaking, no
# Cargo.toml depending on the seam crate outside the seam itself.
# Runs first because it's <1s and catches dep-graph violations that
# subsequent cargo steps would otherwise mask as "compiles fine".
if [ -x "./scripts/check-vercel-ai-seam.sh" ]; then
  SEAM_OUTPUT=$(./scripts/check-vercel-ai-seam.sh 2>&1) || {
    echo "vercel-ai seam violation:" >&2
    echo "$SEAM_OUTPUT" >&2
    echo "  → see services/inference/src/lib.rs for the canonical re-export list." >&2
    exit 2
  }
fi

# Step 1: fmt (auto-fix, should not fail).
just fmt 2>&1

# Step 2: check — must pass.
CHECK_OUTPUT=$(just check 2>&1) || {
  echo "Compilation failed. Fix these errors:" >&2
  echo "$CHECK_OUTPUT" >&2
  exit 2
}

# Step 3: clippy on lib code — block on warnings AND errors.
#
# Workspace baseline is zero warnings (re-established 2026-04-29). The
# git pre-commit hook (`coco-rs/scripts/git-hooks/pre-commit`) enforces
# the same gate so warnings can't slip in via `git commit` either.
LIB_OUTPUT=$(cargo clippy --all-features 2>&1)
LIB_RC=$?
LIB_WARNS=$(echo "$LIB_OUTPUT" | grep '^warning:' | grep -v 'generated\|warnings emitted')
if [ $LIB_RC -ne 0 ] || [ -n "$LIB_WARNS" ]; then
  echo "Clippy lib warnings/errors detected. Fix them:" >&2
  echo "$LIB_OUTPUT" >&2
  exit 2
fi

# Step 4: clippy on tests — same policy as Step 3.
TEST_OUTPUT=$(cargo clippy --all-features --tests 2>&1)
TEST_RC=$?
TEST_WARNS=$(echo "$TEST_OUTPUT" | grep '^warning:' | grep -v 'generated\|warnings emitted')
if [ $TEST_RC -ne 0 ] || [ -n "$TEST_WARNS" ]; then
  echo "Clippy test warnings/errors detected. Fix them:" >&2
  echo "$TEST_OUTPUT" >&2
  exit 2
fi

echo "Quality checks passed." >&2
exit 0
