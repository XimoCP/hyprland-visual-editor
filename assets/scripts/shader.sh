#!/bin/bash

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

# 🎛️ ASIGNACIÓN DINÁMICA DE EXTENSIÓN PARA EL FRAGMENTO
if [ "$HVE_FORMAT" = "lua" ]; then
    EXT="lua"
    ALT_EXT="conf"
else
    EXT="conf"
    ALT_EXT="lua"
fi

# Ensure the internal fragments folder exists
mkdir -p "$HVE_FRAGMENTS_DIR"

# Definimos las rutas de los fragmentos de forma dinámica
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/shader.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/shader.${ALT_EXT}"

# The preset is the filename (e.g., 02_monocromo.frag)
PRESET=$1

# --- SHADER LOGIC ---

# Case 1: Disable (None, empty, or 'clean' shader)
if [ "$PRESET" == "none" ] || [ -z "$PRESET" ] || [ "$PRESET" == "00_limpio.frag" ]; then

    # Delete both internal fragments to avoid ghost configs
    rm -f "$TARGET_FRAGMENT" "$OLD_FRAGMENT"

    # PRO TIP: Force Hyprland to clear the shader in memory immediately.
    # Evaluamos la sintaxis de hyprctl según la versión (en v0.55+ Lua cambia el keyword)
    if [ "$EXT" = "lua" ]; then
        hyprctl keyword decoration:screen_shader "" 2>/dev/null || hyprctl keyword decoration.screen_shader ""
    else
        hyprctl keyword decoration:screen_shader ""
    fi

    echo "Syncing: Shaders disabled."

# Case 2: Enable a specific filter
else
    # Limpieza preventiva del formato opuesto
    rm -f "$OLD_FRAGMENT"

    SHADER_PATH="$HVE_SHADERS_DIR/$PRESET"

    # Security check in the internal path
    if [ ! -f "$SHADER_PATH" ]; then
        notify-send "HVE Error" "Shader not found: $PRESET" -i dialog-error
        exit 1
    fi

    # 💾 GENERACIÓN DEL FRAGMENTO ENVOLTORIO SEGÚN FORMATO
    if [ "$EXT" = "lua" ]; then
        # Sintaxis nativa para el master overlay.lua
        echo "hl.config({ decoration = { [\"screen_shader\"] = \"$SHADER_PATH\" } })" > "$TARGET_FRAGMENT"
    else
        # Sintaxis clásica para el master overlay.conf
        echo "decoration {
    screen_shader = $SHADER_PATH
}" > "$TARGET_FRAGMENT"
    fi

    echo "Syncing: Applying shader $PRESET ($EXT wrapper)"
fi

# --- CALL THE MASTER ASSEMBLER ---
if [ -f "$HVE_SCRIPTS_DIR/assemble.sh" ]; then
    bash "$HVE_SCRIPTS_DIR/assemble.sh"
else
    # Fallback in case assemble.sh is missing for some reason
    hyprctl reload
fi