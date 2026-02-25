#!/bin/bash

# --- RUTAS PRINCIPALES ---
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
FRAGMENTS_DIR="$PLUGIN_DIR/assets/fragments"

# 🌟 NUEVA RUTA SEGURA (El Refugio) 🌟
NVL_SAFE_DIR="$HOME/.config/noctalia/NVL"
OVERLAY_FILE="$NVL_SAFE_DIR/overlay.conf" # El overlay ahora vive fuera del plugin
WATCHDOG_FILE="$NVL_SAFE_DIR/nvl_watchdog.sh"

# Mantenemos la ruta de tus colores
COLORS_FILE="$HOME/.config/hypr/noctalia/noctalia-colors.conf"
HYPR_CONF="$HOME/.config/hypr/hyprland.conf"

# Ruta interna del ensamblador
ASSEMBLE_SCRIPT="$PLUGIN_DIR/assets/scripts/assemble.sh"

# --- MARCADORES PARA HYPRLAND ---
MARKER_START="# >>> NOCTALIA VISUAL LAYER START <<<"
MARKER_END="# >>> NOCTALIA VISUAL LAYER END <<<"

# --- CONTENIDO A INYECTAR ---
LINE_WATCHDOG="exec-once = $WATCHDOG_FILE" # Inyección del guardián
LINE_COLORS="source = $COLORS_FILE"
LINE_OVERLAY="source = $OVERLAY_FILE"

ACTION=$1

# --- FUNCIÓN DE LIMPIEZA ---
clean_hyprland_conf() {
    # 1. Borrar todo el bloque entre los marcadores
    sed -i "/$MARKER_START/,/$MARKER_END/d" "$HYPR_CONF"

    # 2. Limpieza de seguridad por si quedaron líneas sueltas viejas
    sed -i "\|source = .*noctalia-visual-layer/overlay.conf|d" "$HYPR_CONF"
    sed -i "\|$LINE_OVERLAY|d" "$HYPR_CONF"
    sed -i "\|$LINE_COLORS|d" "$HYPR_CONF"

    # 3. Eliminar líneas vacías extra al final
    sed -i '${/^$/d;}' "$HYPR_CONF"
}

# --- FUNCIÓN DE PREPARACIÓN ---
setup_files() {
    echo "Preparando entorno seguro y guardián de Noctalia..."

    # Crear carpeta de fragmentos y el nuevo refugio NVL
    mkdir -p "$FRAGMENTS_DIR"
    mkdir -p "$NVL_SAFE_DIR"

    # Dar permisos de ejecución a los scripts internos
    chmod +x "$PLUGIN_DIR/assets/scripts/"*.sh

    # 🛡️ Desplegar el script guardián
    cp "$PLUGIN_DIR/assets/scripts/nvl_watchdog.sh" "$WATCHDOG_FILE"
    chmod +x "$WATCHDOG_FILE"

    # Crear fragmentos vacíos internos
    touch "$FRAGMENTS_DIR/animation.conf"
    touch "$FRAGMENTS_DIR/geometry.conf"
    touch "$FRAGMENTS_DIR/border.conf"
    touch "$FRAGMENTS_DIR/shader.conf"

    # Ejecutar el ensamblador interno
    # Exportamos la variable para que assemble.sh sepa dónde debe guardar el archivo final
    export OVERLAY_FILE="$NVL_SAFE_DIR/overlay.conf"

    if [ -f "$ASSEMBLE_SCRIPT" ]; then
        bash "$ASSEMBLE_SCRIPT"

        # PARCHE DE SEGURIDAD: Por si assemble.sh tiene la ruta vieja escrita "a fuego" en su código
        if [ -f "$PLUGIN_DIR/overlay.conf" ]; then
            mv "$PLUGIN_DIR/overlay.conf" "$NVL_SAFE_DIR/overlay.conf"
        fi
    else
        echo "# Noctalia Overlay Base" > "$OVERLAY_FILE"
    fi
}

# --- LÓGICA PRINCIPAL ---

if [ "$ACTION" == "enable" ]; then
    setup_files
    clean_hyprland_conf # Limpiar duplicados

    # Inyectamos el BLOQUE COMPLETO apuntando al refugio seguro
    echo "" >> "$HYPR_CONF"
    echo "$MARKER_START" >> "$HYPR_CONF"
    echo "# 1. Guardián de Desinstalación Activo" >> "$HYPR_CONF"
    echo "$LINE_WATCHDOG" >> "$HYPR_CONF"
    echo "# 2. Definición de Variables (Paleta de Colores)" >> "$HYPR_CONF"
    echo "$LINE_COLORS" >> "$HYPR_CONF"
    echo "# 3. Aplicación de Efectos (Visual Layer)" >> "$HYPR_CONF"
    echo "$LINE_OVERLAY" >> "$HYPR_CONF"
    echo "$MARKER_END" >> "$HYPR_CONF"

    # Recarga final
    hyprctl reload
    notify-send "Noctalia Visual" "Sistema ACTIVADO (Entorno Seguro)" -i system-software-update

elif [ "$ACTION" == "disable" ]; then
    clean_hyprland_conf # Borra el bloque de hyprland.conf

    # 🧹 Limpieza total: borramos la carpeta refugio con el overlay y el guardián
    rm -rf "$NVL_SAFE_DIR"

    hyprctl reload
    notify-send "Noctalia Visual" "Sistema DESACTIVADO" -i system-shutdown
fi
