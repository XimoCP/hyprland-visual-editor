#!/bin/bash
# --- HVE Get Colors (JSON output) ---
# Sources colors.sh and outputs the detected colors as JSON.
# Used by the Rust engine to get machine-readable color data.

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/colors.sh"

# Output only JSON — colors.sh diagnostics go to stderr
printf '{"primary":"%s","secondary":"%s","tertiary":"%s","surface":"%s","surface_lowest":"%s","accent":"%s"}' \
    "$HVE_PRIMARY" "$HVE_SECONDARY" "$HVE_TERTIARY" "$HVE_SURFACE" "$HVE_SURFACE_LOWEST" "$HVE_ACCENT"
