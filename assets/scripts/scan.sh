#!/bin/bash

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

TARGET_FOLDER="$1"
SEARCH_DIR="$HVE_ASSETS_DIR/$TARGET_FOLDER"

# Escape a value for safe inclusion in a JSON string literal: backslashes
# first (so they are not interpreted as JSON escapes), then quotes, then the
# control characters that would otherwise produce invalid JSON.
escape_json() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/\t/\\t/g' -e 's/\r/\\r/g'
}

# --- Lua-only scan ---
# Preset twins live as `*.lua`; shaders are `*.frag`. There is no format
# detection, no `.conf` globbing, and no per-base dedup.
if [ ! -d "$SEARCH_DIR" ]; then echo "[]"; exit 0; fi

declare -a SELECTED_FILES

while IFS= read -r filepath; do
    filename=$(basename "$filepath")

    # Skip system files
    if [[ "$filename" == *"store"* ]]; then continue; fi

    # Skip geometry files (handled separately)
    if [[ "${filename%.*}" == "geometry" ]]; then continue; fi

    SELECTED_FILES+=("$filepath")
done < <(find "$SEARCH_DIR" -maxdepth 1 -type f \( -name "*.lua" -o -name "*.frag" \) | sort)

echo "["
FIRST=true

for filepath in "${SELECTED_FILES[@]}"; do
    filename=$(basename "$filepath")
    ID_NAME="${filename%.*}"

    # 1. Translation keys
    # Sanitize the id for the i18n key path: dots inside a file name would
    # break the dot-notation lookup (tr.rs splits keys on '.'), so any '.'
    # becomes '_'. The file itself keeps its original name.
    KEY_ID="${ID_NAME//./_}"
    KEY_T="${TARGET_FOLDER}.presets.${KEY_ID}.title"
    KEY_D="${TARGET_FOLDER}.presets.${KEY_ID}.desc"

    # 2. Metadata extraction (both # @Title and -- @Title styles)
    # Note: values are NOT escaped here — escaping happens at JSON output
    # time via escape_json so every field gets identical treatment.
    function get_meta() {
        grep -m1 -E "^[ \t]*(#|--)[ \t]*@$1:" "$filepath" 2>/dev/null | sed 's/^[ \t]*\(#\|--\)[ \t]*@[^:]*:[ \t]*//' | sed 's/^[ \t]*//;s/[ \t]*$//' | tr -d '\r'
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

    # 3. JSON Output — every interpolated value is escaped (backslashes,
    #    quotes, tabs, CR) so filenames/metadata cannot break the JSON or
    #    inject fields. Filenames come from the filesystem scan; metadata
    #    comes from file contents.
    cat <<EOF
    {
        "file": "$(escape_json "$filename")",
        "title": "$(escape_json "$KEY_T")",
        "desc": "$(escape_json "$KEY_D")",
        "rawTitle": "$(escape_json "$RAW_T")",
        "rawDesc": "$(escape_json "$RAW_D")",
        "icon": "$(escape_json "$ICON")",
        "color": "$(escape_json "$COLOR")",
        "tag": "$(escape_json "$TAG")"
    }
EOF

done

echo "]"
