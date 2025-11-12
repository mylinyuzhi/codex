#!/bin/bash
# Audit logging hook
#
# This hook logs all tool executions to an audit trail.

set -euo pipefail

# Read JSON input
INPUT=$(cat)

# Create audit log directory
AUDIT_DIR="${HOME}/.codex/audit"
mkdir -p "$AUDIT_DIR"

# Log file with date
LOG_FILE="${AUDIT_DIR}/$(date +%Y-%m-%d).log"

# Extract relevant fields
if command -v jq &> /dev/null; then
    TIMESTAMP=$(echo "$INPUT" | jq -r '.timestamp')
    TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // "unknown"')
    SESSION_ID=$(echo "$INPUT" | jq -r '.session_id')

    # Log the event
    echo "[${TIMESTAMP}] Session: ${SESSION_ID} | Tool: ${TOOL_NAME}" >> "$LOG_FILE"
else
    # Simple fallback
    echo "$INPUT" >> "$LOG_FILE"
fi

# Always continue (non-blocking)
cat <<EOF
{
  "continue": true,
  "systemMessage": "Audit log recorded"
}
EOF
exit 0
