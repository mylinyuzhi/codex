#!/bin/bash
# Validate shell command hook
#
# This hook checks for dangerous shell commands and blocks them.

set -euo pipefail

# Read JSON input from stdin
INPUT=$(cat)

# Extract command using jq (if available)
if command -v jq &> /dev/null; then
    COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
else
    # Fallback: simple grep
    COMMAND=$(echo "$INPUT" | grep -o '"command":"[^"]*"' | cut -d'"' -f4 || echo "")
fi

# List of dangerous command patterns
DANGEROUS_PATTERNS=(
    "rm -rf /"
    "dd if="
    ":(){ :|:& };:"  # fork bomb
    "> /dev/sda"
    "mkfs"
    "chmod -R 777 /"
)

# Check each dangerous pattern
for pattern in "${DANGEROUS_PATTERNS[@]}"; do
    if [[ "$COMMAND" == *"$pattern"* ]]; then
        # Block the command with JSON output
        cat <<EOF
{
  "continue": false,
  "decision": "block",
  "reason": "Dangerous command detected: $pattern",
  "systemMessage": "Security hook blocked potentially dangerous command"
}
EOF
        exit 0
    fi
done

# Approve the command
cat <<EOF
{
  "continue": true,
  "decision": "approve",
  "systemMessage": "Command validated successfully"
}
EOF
exit 0
