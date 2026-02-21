#!/bin/bash

# --- RUTAS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
ASSETS_DIR="$SCRIPT_DIR/.."
TARGET_FOLDER="$1"
SEARCH_DIR="$ASSETS_DIR/$TARGET_FOLDER"

if [ ! -d "$SEARCH_DIR" ]; then echo "[]"; exit 0; fi

echo "["
FIRST=true

while read -r filepath; do
    filename=$(basename "$filepath")

    # Saltamos archivos de sistema
    if [[ "$filename" == *"store"* || "$filename" == "geometry.conf" ]]; then continue; fi

    ID_NAME="${filename%.*}"

    # 1. Claves para traducción (Lo que ya tenías)
    KEY_T="${TARGET_FOLDER}.presets.${ID_NAME}.title"
    KEY_D="${TARGET_FOLDER}.presets.${ID_NAME}.desc"

    # 2. LECTURA REAL DEL ARCHIVO (¡ESTO ES LO QUE FALTABA!)
    function get_meta() {
        # El 2>/dev/null evita errores si el archivo es raro
        grep -m1 -E "^[ \t]*(#|//) @$1:" "$filepath" 2>/dev/null | cut -d: -f2- | sed 's/^[ \t]*//;s/[ \t]*$//;s/"/\\"/g' | tr -d '\r'
    }

    RAW_T=$(get_meta "Title")
    RAW_D=$(get_meta "Desc")
    ICON=$(get_meta "Icon")
    COLOR=$(get_meta "Color")
    TAG=$(get_meta "Tag")

    # Valores por defecto de seguridad
    [ -z "$RAW_T" ] && RAW_T="$ID_NAME"
    [ -z "$ICON" ] && ICON="help"
    [ -z "$COLOR" ] && COLOR="#888888"
    [ -z "$TAG" ] && TAG="USER"

    if [ "$FIRST" = true ]; then FIRST=false; else echo ","; fi

    # 3. Salida JSON con TODO (Claves + Texto Real)
    cat <<EOF
    {
        "file": "$filename",
        "title": "$KEY_T",
        "desc": "$KEY_D",
        "rawTitle": "$RAW_T",
        "rawDesc": "$RAW_D",
        "icon": "$ICON",
        "color": "$COLOR",
        "tag": "$TAG"
    }
EOF

done < <(find "$SEARCH_DIR" -maxdepth 1 -type f \( -name "*.conf" -o -name "*.frag" \) | sort)

echo "]"
