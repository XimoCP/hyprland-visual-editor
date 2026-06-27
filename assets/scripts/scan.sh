#!/bin/bash

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

TARGET_FOLDER="$1"
SEARCH_DIR="$HVE_ASSETS_DIR/$TARGET_FOLDER"

# --- FORMAT DETECTION ---
HVE_FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"
FORMAT_DETECTION_SCRIPT="$HVE_SCRIPTS_DIR/detect_format.sh"

# Detect format (always detect, don't skip if cache exists - user may have changed)
if [ -f "$FORMAT_DETECTION_SCRIPT" ]; then
    source "$FORMAT_DETECTION_SCRIPT" 2>/dev/null
    export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
else
    export HVE_FORMAT="conf"
fi
# Fallback if detection returned empty
[ -z "$HVE_FORMAT" ] && HVE_FORMAT="conf"

if [ ! -d "$SEARCH_DIR" ]; then echo "[]"; exit 0; fi

# Collect all preset files and group by base name
# For each base name, pick the file matching HVE_FORMAT, fallback to the other
declare -A SEEN_BASE
declare -a SELECTED_FILES

while read -r filepath; do
    filename=$(basename "$filepath")
    ext="${filename##*.}"
    base="${filename%.*}"

    # Skip system files
    if [[ "$filename" == *"store"* ]]; then continue; fi

    # Skip geometry files (handled separately)
    if [[ "$base" == "geometry" ]]; then continue; fi

    # If we haven't seen this base name, tentatively select it
    if [ -z "${SEEN_BASE[$base]}" ]; then
        SEEN_BASE[$base]="$filepath"
    else
        # Already seen this base - pick the one matching HVE_FORMAT
        existing_ext="${SEEN_BASE[$base]##*.}"
        if [ "$ext" = "$HVE_FORMAT" ]; then
            # New file matches format, prefer it
            SEEN_BASE[$base]="$filepath"
        elif [ "$existing_ext" != "$HVE_FORMAT" ] && [ "$ext" != "$HVE_FORMAT" ]; then
            # Neither matches format, keep the first one
            :
        fi
        # If existing already matches format, keep it (do nothing)
    fi
done < <(find "$SEARCH_DIR" -maxdepth 1 -type f \( -name "*.conf" -o -name "*.lua" -o -name "*.frag" \) | sort)

# Build selected files list
for base in $(echo "${!SEEN_BASE[@]}" | tr ' ' '\n' | sort); do
    SELECTED_FILES+=("${SEEN_BASE[$base]}")
done

echo "["
FIRST=true

for filepath in "${SELECTED_FILES[@]}"; do
    filename=$(basename "$filepath")
    ID_NAME="${filename%.*}"

    # 1. Translation keys
    KEY_T="${TARGET_FOLDER}.presets.${ID_NAME}.title"
    KEY_D="${TARGET_FOLDER}.presets.${ID_NAME}.desc"

    # 2. Metadata extraction (both # @Title and -- @Title styles)
    function get_meta() {
        grep -m1 -E "^[ \t]*(#|--)[ \t]*@$1:" "$filepath" 2>/dev/null | sed 's/^[ \t]*\(#\|--\)[ \t]*@[^:]*:[ \t]*//' | sed 's/^[ \t]*//;s/[ \t]*$//;s/"/\\"/g' | tr -d '\r'
    }

    RAW_T=$(get_meta "Title")
    RAW_D=$(get_meta "Desc")
    ICON=$(get_meta "Icon")
    COLOR=$(get_meta "Color")
    TAG=$(get_meta "Tag")

    # Safe default values
    [ -z "$RAW_T" ] && RAW_T="$ID_NAME"
    [ -z "$ICON" ] && ICON="help"
    [ -z "$COLOR" ] && COLOR="#888888"
    [ -z "$TAG" ] && TAG="USER"

    if [ "$FIRST" = true ]; then FIRST=false; else echo ","; fi

    # 3. JSON Output
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

done

echo "]"
