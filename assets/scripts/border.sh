#!/bin/bash

# --- RUTAS ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
PRESETS_DIR="$PLUGIN_DIR/assets/borders"      # Donde el usuario guarda sus .conf
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"  # Donde se genera el temporal
SCRIPTS_DIR="$PLUGIN_DIR/assets/scripts"

mkdir -p "$FRAGMENTS_DIR"
PRESET_NAME=$1

# 1. LÓGICA DE APAGADO (None o vacío)
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$FRAGMENTS_DIR/border.conf"
    echo "Borde desactivado."
else
    # 2. CARGA DINÁMICA
    # Buscamos el archivo .conf en la carpeta assets/borders/
    TARGET_FILE="$PRESETS_DIR/$PRESET_NAME"

    if [ -f "$TARGET_FILE" ]; then
        # Copiamos el contenido del preset al fragmento
        cat "$TARGET_FILE" > "$FRAGMENTS_DIR/border.conf"
        echo "Aplicado preset de borde: $PRESET_NAME"
    else
        # Fallback de seguridad: si el archivo no existe, usa el color base
        echo "general { col.active_border = \$primary }" > "$FRAGMENTS_DIR/border.conf"
        echo "Advertencia: Preset $PRESET_NAME no encontrado."
    fi
fi

# 3. ENSAMBLAJE
bash "$SCRIPTS_DIR/assemble.sh"
