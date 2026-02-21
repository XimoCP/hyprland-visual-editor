#!/bin/bash

# --- CONFIGURACIÓN DE RUTAS AUTOCONTENIDAS ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"
FINAL_FILE="$PLUGIN_DIR/overlay.conf"
TEMP_FILE="$PLUGIN_DIR/overlay.tmp"
# [NUEVO] Ruta de los colores oficiales de Noctalia
COLORS_FILE="$HOME/.config/hypr/noctalia/noctalia-colors.conf"

# Aseguramos que la carpeta de fragmentos exista dentro del plugin
mkdir -p "$FRAGMENTS_DIR"

# 1. CREACIÓN DEL ARCHIVO TEMPORAL
echo "# NOCTALIA VISUAL LAYER - OVERLAY MAESTRO" > "$TEMP_FILE"
echo "# Generado automáticamente el: $(date)" >> "$TEMP_FILE"
echo "" >> "$TEMP_FILE"

# --- [CRÍTICO: COLORES PRIMERO] ---
# Cargamos las variables ($primary, $secondary...) antes que nada.
if [ -f "$COLORS_FILE" ]; then
    echo "# [SISTEMA: COLORES]" >> "$TEMP_FILE"
    echo "source = $COLORS_FILE" >> "$TEMP_FILE"
    echo "" >> "$TEMP_FILE"
else
    echo "# [ADVERTENCIA] Archivo de colores no encontrado: $COLORS_FILE" >> "$TEMP_FILE"
fi

# --- [FIX CRÍTICO: CURVA INMORTAL] ---
# Inyectamos la curva linear GLOBALMENTE aquí.
echo "bezier = linear, 0, 0, 1, 1" >> "$TEMP_FILE"
echo "# ----------------------------------------------------" >> "$TEMP_FILE"
echo "" >> "$TEMP_FILE"


# 2. ENSAMBLAJE ORDENADO (JERARQUÍA DE PODER)

# -- A) ANIMACIONES --
if [ -f "$FRAGMENTS_DIR/animation.conf" ]; then
    echo "# [MÓDULO: ANIMACIONES]" >> "$TEMP_FILE"
    cat "$FRAGMENTS_DIR/animation.conf" >> "$TEMP_FILE"
    echo "" >> "$TEMP_FILE"
fi

# -- B) BORDES (Estilo y Color) --
if [ -f "$FRAGMENTS_DIR/border.conf" ]; then
    echo "# [MÓDULO: BORDES]" >> "$TEMP_FILE"
    cat "$FRAGMENTS_DIR/border.conf" >> "$TEMP_FILE"
    echo "" >> "$TEMP_FILE"
fi

# -- C) SHADERS --
if [ -f "$FRAGMENTS_DIR/shader.conf" ]; then
    echo "# [MÓDULO: SHADERS]" >> "$TEMP_FILE"
    cat "$FRAGMENTS_DIR/shader.conf" >> "$TEMP_FILE"
    echo "" >> "$TEMP_FILE"
fi

# -- D) GEOMETRÍA (El Jefe Supremo) --
# Lo ponemos AL FINAL para que el slider siempre mande sobre el tamaño,
# sobrescribiendo cualquier error que venga de los bordes anteriores.
if [ -f "$FRAGMENTS_DIR/geometry.conf" ]; then
    echo "# [MÓDULO: GEOMETRÍA]" >> "$TEMP_FILE"
    cat "$FRAGMENTS_DIR/geometry.conf" >> "$TEMP_FILE"
    echo "" >> "$TEMP_FILE"
fi

# 3. MOVIMIENTO MAESTRO
mv "$TEMP_FILE" "$FINAL_FILE"

# 4. APLICACIÓN
if pgrep -x "Hyprland" > /dev/null; then
    # Usamos reload para aplicar cambios sin reiniciar la sesión
    hyprctl reload
fi
