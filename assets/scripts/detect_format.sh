#!/bin/bash

# --- HVE Format Detection Script ---
# Detects whether user uses hyprland.lua OR hyprland.conf
# Prioritizes lua (newer) over conf (legacy)
# Returns: "lua" or "conf"

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Cache file
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"

# Lua config file (priority 1 - NEW)
LUA_CONFIG="$HVE_HYPR_DIR/hyprland.lua"

# Conf config file (priority 2 - LEGACY)
CONF_CONFIG="$HVE_HYPR_DIR/hyprland.conf"

#############################################################################
# detect_format() - Main function
# Usage: source "$(dirname "$0")/detect_format.sh" && detect_format
# Returns: "lua" or "conf" via stdout, caches to ~/.cache/noctalia/HVE/hve_format
#############################################################################

detect_format() {
    local detected_format=""

    # Check for Lua config first (Hyprland 0.55+)
    if [ -f "$LUA_CONFIG" ]; then
        # Validate it's actually a valid Lua file by checking for Lua API patterns:
        # hl.config, hl.animation, hl.curve, hl.exec, hl.keybind, or require()
        if grep -qE '^\s*(hl\.|require\()' "$LUA_CONFIG" 2>/dev/null; then
            detected_format="lua"
        fi
    fi

    # If no valid Lua found, check for conf (legacy)
    if [ -z "$detected_format" ]; then
        if [ -f "$CONF_CONFIG" ]; then
            detected_format="conf"
        else
            # Neither file exists - default to conf for legacy compatibility
            detected_format="conf"
        fi
    fi

    # Cache the result
    mkdir -p "$HVE_SAFE_DIR"
    local cached=""
    [ -f "$FORMAT_CACHE" ] && cached=$(cat "$FORMAT_CACHE" 2>/dev/null)
    if [ "$detected_format" != "$cached" ]; then
        echo "$detected_format" > "$FORMAT_CACHE" 2>/dev/null || true
    fi

    echo "$detected_format"
}

# --- If executed directly (not sourced), auto-detect ---
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    detect_format
fi
