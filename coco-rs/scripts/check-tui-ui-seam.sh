#!/usr/bin/env bash
# Enforce the coco-tui-ui presentational seam.
#
# Rule: `coco-tui-ui` is a pure, domain-free, i18n-free presentational
# crate ("view-models in, ratatui out"). Its [dependencies] must stay within a
# small allowlist (ratatui + crossterm + unicode-* + leaf utils). It MUST NOT
# depend on any coco domain/app crate (config, messages, types, state, query,
# context, keybindings, …) or on rust-i18n — any of those would let AppState,
# settings, or translation leak into the render layer.
#
# Wired into `just check-seam` (and thus quick-check / pre-commit). Run alone:
#   ./scripts/check-tui-ui-seam.sh
#
# Non-zero exit + offending dep names on violation; silent + status 0 when clean.

set -euo pipefail

cd "$(dirname "$0")/.."

manifest="tui-ui/Cargo.toml"
allow_re='^(ratatui|crossterm|unicode-width|unicode-segmentation|tracing|base64|tokio|coco-utils-string|coco-utils-common)$'

# Extract crate names from the [dependencies] block (handles both
# `name.workspace = true` and `name = { … }` forms).
deps=$(awk '
    /^\[dependencies\]/ { inblock = 1; next }
    /^\[/              { inblock = 0 }
    inblock && /^[A-Za-z0-9_-]+/ {
        name = $0
        sub(/[[:space:]]*[.=].*$/, "", name)
        print name
    }
' "$manifest")

violations=$(printf '%s\n' "$deps" | grep -Ev "$allow_re" | grep -v '^$' || true)

if [ -n "$violations" ]; then
    echo "✗ coco-tui-ui depends on crates outside the presentational allowlist:" >&2
    echo "$violations" | sed 's/^/    /' >&2
    echo "  → the render crate must stay domain-free and i18n-free." >&2
    echo "  → allowed: ratatui, crossterm, unicode-width, unicode-segmentation," >&2
    echo "             coco-utils-string, coco-utils-common." >&2
    echo "  → presentation that needs AppState/messages/config/i18n belongs in app/tui." >&2
    exit 1
fi
