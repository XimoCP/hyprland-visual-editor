#!/bin/bash

# --- RUTAS AUTOCONTENIDAS ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"
SCRIPTS_DIR="$PLUGIN_DIR/assets/scripts"
SHADERS_DIR="$PLUGIN_DIR/assets/shaders"

# Asegurar que la carpeta interna de fragmentos existe
mkdir -p "$FRAGMENTS_DIR"

# El preset es el nombre del archivo (ej: 02_monocromo.frag)
PRESET=$1

# --- LÓGICA DE SHADERS ---

# Caso 1: Desactivar (None, vacío o el shader 'limpio')
if [ "$PRESET" == "none" ] || [ -z "$PRESET" ] || [ "$PRESET" == "00_limpio.frag" ]; then

    # Borramos el fragmento interno
    rm -f "$FRAGMENTS_DIR/shader.conf"

    # TRUCO PRO: Forzamos a Hyprland a vaciar el shader en memoria inmediatamente
    # para que el cambio sea instantáneo al hacer clic.
    hyprctl keyword decoration:screen_shader ""

    echo "Sincronizando: Shaders desactivados."

# Caso 2: Activar un filtro específico
else
    SHADER_PATH="$SHADERS_DIR/$PRESET"

    # Verificación de seguridad en la ruta interna
    if [ ! -f "$SHADER_PATH" ]; then
        notify-send "Noctalia Error" "No se encuentra el shader: $PRESET" -i dialog-error
        exit 1
    fi

    # Escribimos el fragmento en la nueva ruta interna
    echo "decoration {
    screen_shader = $SHADER_PATH
}" > "$FRAGMENTS_DIR/shader.conf"

    echo "Sincronizando: Aplicando shader $PRESET"
fi

# --- LLAMADA AL MAESTRO ENSAMBLADOR ---
if [ -f "$SCRIPTS_DIR/assemble.sh" ]; then
    bash "$SCRIPTS_DIR/assemble.sh"
else
    # Fallback si por algún motivo assemble.sh no está
    hyprctl reload
fi
