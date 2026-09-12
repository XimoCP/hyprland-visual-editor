#!/bin/bash

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Lua-only fragment target (config format is never detected at runtime).
mkdir -p "$HVE_FRAGMENTS_DIR"
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/shader.lua"

# The preset is the filename (e.g., 02_monocromo.frag)
PRESET=$1

# --- SHADER LOGIC ---

# Case 1: Disable (None, empty, or 'clean' shader)
if [ "$PRESET" == "none" ] || [ -z "$PRESET" ] || [ "$PRESET" == "00_limpio.frag" ]; then

    # Delete the internal fragment to avoid ghost configs
    rm -f "$TARGET_FRAGMENT"

    # PRO TIP: Force Hyprland to clear the shader in memory immediately.
    # (Hyprland 0.55+ Lua routes the keyword through the Lua shim.)
    hyprctl keyword decoration:screen_shader "" 2>/dev/null || hyprctl keyword decoration.screen_shader ""

    echo "Syncing: Shaders disabled."

# Case 2: Enable a specific filter
else
    # SECURITY: the preset name comes from theme state / the scan UI and is
    # embedded into Hyprland config and Lua. Reject anything outside
    # [A-Za-z0-9._-] plus spaces so quotes, backslashes, newlines and
    # shell/script metacharacters can never leak into the generated config.
    # (Spaces are mapped to '_' before matching: a literal space inside a
    # case bracket class is a bash parse error.)
    testpreset=$(printf '%s' "$PRESET" | tr ' ' '_')
    case "$testpreset" in
        ''|*[!A-Za-z0-9._-]*)
            notify-send "HVE Error" "Invalid shader preset name" -i dialog-error
            exit 1
            ;;
    esac

    SHADER_PATH="$HVE_SHADERS_DIR/$PRESET"

    # Security check in the internal path
    if [ ! -f "$SHADER_PATH" ]; then
        notify-send "HVE Error" "Shader not found: $PRESET" -i dialog-error
        exit 1
    fi

    # 💾 GENERATE THE LUA WRAPPER FRAGMENT for the master overlay.lua
    echo "hl.config({ decoration = { [\"screen_shader\"] = \"$SHADER_PATH\" } })" > "$TARGET_FRAGMENT"

    echo "Syncing: Applying shader $PRESET (lua wrapper)"
fi

# --- CALL THE MASTER ASSEMBLER ---
if [ -f "$HVE_SCRIPTS_DIR/assemble.sh" ]; then
    bash "$HVE_SCRIPTS_DIR/assemble.sh"
else
    # Fallback in case assemble.sh is missing for some reason
    hyprctl reload
fi