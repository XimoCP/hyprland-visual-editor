#!/bin/bash

# --- RUTAS AUTOCONTENIDAS (ENCAPSULADAS) ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"

# Archivo maestro generado (ahora vive en la raíz del plugin)
OVERLAY_FILE="$PLUGIN_DIR/overlay.conf"

# Mantenemos la ruta de tus colores (externa a Noctalia para compatibilidad)
COLORS_FILE="$HOME/.config/hypr/noctalia/noctalia-colors.conf"
HYPR_CONF="$HOME/.config/hypr/hyprland.conf"

# Ruta interna del ensamblador
ASSEMBLE_SCRIPT="$PLUGIN_DIR/assets/scripts/assemble.sh"

# --- MARCADORES PARA HYPRLAND ---
MARKER_START="# >>> NOCTALIA VISUAL LAYER START <<<"
MARKER_END="# >>> NOCTALIA VISUAL LAYER END <<<"

# --- CONTENIDO A INYECTAR ---
LINE_COLORS="source = $COLORS_FILE"
LINE_OVERLAY="source = $OVERLAY_FILE"

ACTION=$1

# --- FUNCIÓN DE LIMPIEZA ---
clean_hyprland_conf() {
    # 1. Borrar todo el bloque entre los marcadores
    sed -i "/$MARKER_START/,/$MARKER_END/d" "$HYPR_CONF"

    # 2. Limpieza de seguridad por si quedaron líneas sueltas
    sed -i "\|$LINE_OVERLAY|d" "$HYPR_CONF"
    sed -i "\|$LINE_COLORS|d" "$HYPR_CONF"

    # 3. Eliminar líneas vacías extra al final
    sed -i '${/^$/d;}' "$HYPR_CONF"
}

# --- FUNCIÓN DE PREPARACIÓN ---
setup_files() {
    echo "Preparando entorno encapsulado de Noctalia..."
    # Crear carpeta de fragmentos dentro del plugin
    mkdir -p "$FRAGMENTS_DIR"

    # Dar permisos de ejecución a los scripts internos
    chmod +x "$PLUGIN_DIR/assets/scripts/"*.sh

    # Crear fragmentos vacíos internos
    touch "$FRAGMENTS_DIR/animation.conf"
    touch "$FRAGMENTS_DIR/geometry.conf"
    touch "$FRAGMENTS_DIR/border.conf"
    touch "$FRAGMENTS_DIR/shader.conf"

    # Ejecutar el ensamblador interno
    if [ -f "$ASSEMBLE_SCRIPT" ]; then
        bash "$ASSEMBLE_SCRIPT"
    else
        echo "# Noctalia Overlay Base" > "$OVERLAY_FILE"
    fi
}

# --- LÓGICA PRINCIPAL ---

if [ "$ACTION" == "enable" ]; then
    setup_files
    clean_hyprland_conf # Limpiar duplicados

    # Inyectamos el BLOQUE COMPLETO apuntando al interior del plugin
    echo "" >> "$HYPR_CONF"
    echo "$MARKER_START" >> "$HYPR_CONF"
    echo "# 1. Definición de Variables (Paleta de Colores)" >> "$HYPR_CONF"
    echo "$LINE_COLORS" >> "$HYPR_CONF"
    echo "# 2. Aplicación de Efectos (Visual Layer)" >> "$HYPR_CONF"
    echo "$LINE_OVERLAY" >> "$HYPR_CONF"
    echo "$MARKER_END" >> "$HYPR_CONF"

    # Recarga final
    hyprctl reload
    notify-send "Noctalia Visual" "Sistema ACTIVADO (Entorno Encapsulado)" -i system-software-update

elif [ "$ACTION" == "disable" ]; then
    clean_hyprland_conf # Borra el bloque y desvincula el plugin

    hyprctl reload
    notify-send "Noctalia Visual" "Sistema DESACTIVADO" -i system-shutdown
fi
