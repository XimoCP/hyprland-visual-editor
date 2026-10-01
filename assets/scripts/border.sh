#!/bin/bash
#shellcheck disable=SC1091,SC2155
# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Lua-only fragment target (config format is never detected at runtime).
mkdir -p "$HVE_FRAGMENTS_DIR"
PRESET_NAME=$1
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/border.lua"

# 1. SHUTDOWN LOGIC (None or empty)
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$TARGET_FRAGMENT"
    echo "Border disabled."
else
    # 2. LOADING - built-ins from `$HVE_BORDERS_DIR`, saved presets from the
    # user directory (built-in first; see hve_resolve_preset).
    TARGET_FILE=$(hve_resolve_preset "$HVE_BORDERS_DIR" "$PRESET_NAME" "$HVE_USER_PRESETS_DIR/borders")

    if [ $? -eq 0 ] && [ -n "$TARGET_FILE" ]; then
        # Copy the preset content to the dynamic fragment
        cat "$TARGET_FILE" > "$TARGET_FRAGMENT"
        echo "Border preset applied: $PRESET_NAME (lua mode)"
    else
        # Security fallback: a valid Lua border fragment.
        echo 'hl.config({ general = { ["col.active_border"] = primary } })' > "$TARGET_FRAGMENT"
        echo "Warning: Preset $PRESET_NAME not found. Using safe fallback."
    fi
fi

# 3. ASSEMBLY
bash "$HVE_SCRIPTS_DIR/assemble.sh"