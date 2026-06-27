#!/bin/bash
#shellcheck disable=SC1091,SC2155
# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format BEFORE resolving preset so we pick the right extension
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"
if [ -f "$FORMAT_CACHE" ]; then
    export HVE_FORMAT=$(cat "$FORMAT_CACHE")
else
    source "$HVE_SCRIPTS_DIR/detect_format.sh"
    export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
fi

# 🎛️ DYNAMIC EXTENSION ASSIGNMENT
if [ "$HVE_FORMAT" = "lua" ]; then
    EXT="lua"
    ALT_EXT="conf"  # Alternative extension for cleanup
else
    EXT="conf"
    ALT_EXT="lua"   # Alternative extension for cleanup
fi

mkdir -p "$HVE_FRAGMENTS_DIR"
PRESET_NAME=$1

# Define fragment paths dynamically
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/border.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/border.${ALT_EXT}"

# 1. SHUTDOWN LOGIC (None or empty)
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$TARGET_FRAGMENT" "$OLD_FRAGMENT"
    echo "Border disabled."
else
    # Preventative cleanup: remove fragment of opposite format
    rm -f "$OLD_FRAGMENT"

    # 2. DYNAMIC LOADING - Respects HVE_FORMAT for extension preference
    TARGET_FILE=$(hve_resolve_preset "$HVE_BORDERS_DIR" "$PRESET_NAME")

    if [ $? -eq 0 ] && [ -n "$TARGET_FILE" ]; then
        # Copy the preset content to the dynamic fragment
        cat "$TARGET_FILE" > "$TARGET_FRAGMENT"
        echo "Border preset applied: $PRESET_NAME ($EXT mode)"
    else
        # Security fallback: adapts structure based on format
        if [ "$EXT" = "lua" ]; then
            echo 'hl.config({ general = { ["col.active_border"] = primary } })' > "$TARGET_FRAGMENT"
        else
            echo "general { col.active_border = \$primary }" > "$TARGET_FRAGMENT"
        fi
        echo "Warning: Preset $PRESET_NAME not found. Using safe fallback."
    fi
fi

# 3. ASSEMBLY
bash "$HVE_SCRIPTS_DIR/assemble.sh"