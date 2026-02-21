#!/bin/bash

# --- RUTAS ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
PRESETS_DIR="$PLUGIN_DIR/assets/animations"   # Carpeta de presets
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"
SCRIPTS_DIR="$PLUGIN_DIR/assets/scripts"

mkdir -p "$FRAGMENTS_DIR"
PRESET_NAME=$1

# 1. LÓGICA DE APAGADO
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$FRAGMENTS_DIR/animation.conf"
    echo "Animaciones desactivadas."
else
    # 2. CARGA DINÁMICA
    TARGET_FILE="$PRESETS_DIR/$PRESET_NAME"

    if [ -f "$TARGET_FILE" ]; then
        cat "$TARGET_FILE" > "$FRAGMENTS_DIR/animation.conf"
        echo "Aplicada animación: $PRESET_NAME"
    else
        # Si no existe, no aplicamos nada para no romper Hyprland
        rm -f "$FRAGMENTS_DIR/animation.conf"
        echo "Error: Animación $PRESET_NAME no encontrada."
    fi
fi

# 3. ENSAMBLAJE
bash "$SCRIPTS_DIR/assemble.sh"
