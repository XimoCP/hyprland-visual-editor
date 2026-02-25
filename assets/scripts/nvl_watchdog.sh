#!/bin/bash
# nvl_watchdog.sh - Vigila que el plugin siga instalado y limpia si no es así

# 1. Definimos las rutas (ahora apuntando a la nueva carpeta NVL)
PLUGIN_DIR="$HOME/.config/noctalia/plugins/noctalia-visual-layer"
HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
NVL_SAFE_DIR="$HOME/.config/noctalia/NVL"

# 2. Comprobamos si la carpeta original del plugin ha sido borrada por la Shell
if [ ! -d "$PLUGIN_DIR" ]; then

    # Eliminamos la línea conflictiva del hyprland.conf del usuario silenciosamente
    sed -i '/source = ~\/.config\/noctalia\/NVL\/overlay.conf/d' "$HYPR_CONF"

    # Borramos toda la carpeta de refugio NVL (se lleva el overlay y el propio script por delante)
    rm -rf "$NVL_SAFE_DIR"
fi
