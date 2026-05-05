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

# Step 2: clippy via shared script. `--incremental --head` lints
# {changed crates ∪ reverse-dep closure}; falls back to workspace clippy
# when Cargo.toml/Cargo.lock/toolchain change or affected ≥ 70%.
# Workspace policy is zero warnings — script exits non-zero on any warning.
# `just check` is intentionally NOT run here: clippy is a strict superset of
# check, and running both means rustc + clippy-driver compile every dep
# twice (different cache keys).
CLIPPY_OUTPUT=$(bash "$PROJECT_DIR/.claude/scripts/clippy.sh" --incremental --head 2>&1)
CLIPPY_RC=$?
if [ $CLIPPY_RC -ne 0 ]; then
  echo "Clippy warnings/errors detected. Fix them:" >&2
  echo "$CLIPPY_OUTPUT" >&2
  exit 2
fi

echo "Quality checks passed." >&2
exit 0
