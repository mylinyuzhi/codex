#!/bin/bash
# Session end hook

set -euo pipefail

INPUT=$(cat)

if command -v jq &> /dev/null; then
    SESSION_ID=$(echo "$INPUT" | jq -r '.session_id')
    echo "Session ended: $SESSION_ID" >&2
fi

cat <<EOF
{
  "continue": true,
  "systemMessage": "Session cleanup completed"
}
EOF
exit 0
