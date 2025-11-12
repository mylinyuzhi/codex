#!/bin/bash
# Session start hook

set -euo pipefail

INPUT=$(cat)

if command -v jq &> /dev/null; then
    SESSION_ID=$(echo "$INPUT" | jq -r '.session_id')
    echo "Session started: $SESSION_ID" >&2
fi

cat <<EOF
{
  "continue": true,
  "systemMessage": "Session initialized"
}
EOF
exit 0
