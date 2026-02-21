#!/bin/bash

# geometry.sh - Controla solo el tamaño físico (Encapsulado)

# --- RUTAS AUTOCONTENIDAS ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"
SCRIPTS_DIR="$PLUGIN_DIR/assets/scripts"

# Aseguramos que la carpeta interna exista
mkdir -p "$FRAGMENTS_DIR"

SIZE=$1

# Validación básica
if [ -z "$SIZE" ]; then
    SIZE=2
fi

# 1. GENERACIÓN DEL FRAGMENTO INTERNO
# Mantenemos el FIX de no usar 'no_border_on_floating'
echo "general {
    border_size = $SIZE
}" > "$FRAGMENTS_DIR/geometry.conf"

# 2. RECONSTRUCCIÓN CON EL ENSAMBLADOR INTERNO
if [ -f "$SCRIPTS_DIR/assemble.sh" ]; then
    bash "$SCRIPTS_DIR/assemble.sh"
else
    echo "Error: No se encuentra el script ensamblador en $SCRIPTS_DIR"
    exit 1
fi
