#!/bin/sh
# detect-tools.sh — inventories installed system tools as JSON.
# Used during Docker build to generate /etc/ozzie/system-tools.json.
set -e
echo "["
first=true
for cmd in git curl jq bash ssh docker age; do
    if command -v "$cmd" >/dev/null 2>&1; then
        ver=$("$cmd" --version 2>/dev/null | head -1 || echo "unknown")
        [ "$first" = true ] && first=false || printf ",\n"
        printf '  {"name":"%s","version":"%s"}' "$cmd" "$ver"
    fi
done
echo ""
echo "]"
