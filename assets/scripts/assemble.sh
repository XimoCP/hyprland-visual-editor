#!/bin/bash

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
# shellcheck source=/dev/null
source "$SCRIPT_DIR/utils.sh"

# Output files in safe directory
FINAL_FILE_LUA="$HVE_SAFE_DIR/overlay.lua"
FINAL_FILE_CONF="$HVE_SAFE_DIR/overlay.conf"
TEMP_FILE="$HVE_SAFE_DIR/overlay.tmp"

# Format detection
FORMAT_DETECTION_SCRIPT="$HVE_SCRIPTS_DIR/detect_format.sh"
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"

# Ensure directories exist
mkdir -p "$HVE_FRAGMENTS_DIR"
mkdir -p "$HVE_SAFE_DIR"

# Detect format fresh
if [ -f "$FORMAT_DETECTION_SCRIPT" ]; then
    # shellcheck source=/dev/null
    source "$FORMAT_DETECTION_SCRIPT"
fi

if [ ! -f "$FORMAT_CACHE" ]; then
    HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
else
    HVE_FORMAT=$(cat "$FORMAT_CACHE")
fi
export HVE_FORMAT

# --- CONFIGURATION BASED ON FORMAT ---
if [ "$HVE_FORMAT" = "lua" ]; then
    COMMENT="--"
    EXT="lua"
    FINAL_TARGET="$FINAL_FILE_LUA"
else
    COMMENT="#"
    EXT="conf"
    FINAL_TARGET="$FINAL_FILE_CONF"

    fi

# --- INITIALIZE TEMP FILE ---
{
    if [ "$HVE_FORMAT" = "lua" ]; then
        echo "#!/usr/bin/env hyprland"
    fi
    echo "${COMMENT} HYPRLAND VISUAL EDITOR - MASTER OVERLAY"
    echo "${COMMENT} Automatically generated (Native $HVE_FORMAT mode)"
    echo ""
} > "$TEMP_FILE"

# --- SYSTEM: COLORS ---
if [ "$HVE_FORMAT" = "lua" ] && [ -f "${HVE_COLORS_BASE}.conf" ]; then
    awk -F'=' '/^[[:space:]]*\$/ {
        var = $1; gsub(/[[:space:]]|\$/, "", var);
        val = $2; sub(/^[[:space:]]*/, "", val); sub(/[[:space:]]*$/, "", val);
        print var " = \"" val "\""
    }' "${HVE_COLORS_BASE}.conf" > "${HVE_COLORS_BASE}.lua"
fi

HVE_COLORS_FILE="${HVE_COLORS_BASE}.${EXT}"

if [ -f "$HVE_COLORS_FILE" ]; then
    {
        echo "${COMMENT} [SYSTEM: COLORS]"
        if [ "$HVE_FORMAT" = "lua" ]; then
            echo "dofile(os.getenv(\"HOME\") .. \"/.config/hypr/noctalia/noctalia-colors.lua\")"
        else
            echo "source = $HVE_COLORS_FILE"
        fi
        echo ""
    } >> "$TEMP_FILE"
else
    echo "${COMMENT} [WARNING] Colors file not found: $HVE_COLORS_FILE" >> "$TEMP_FILE"
fi
# --- IMMORTAL CURVE ---
{
    echo "${COMMENT} [SYSTEM: CURVES]"
    if [ "$HVE_FORMAT" = "lua" ]; then
        echo "hl.curve(\"linear\", {type = \"bezier\", points = {{0,0},{1,1}}})"
    else
        echo "bezier = linear, 0, 0, 1, 1"
    fi
    echo "${COMMENT} ----------------------------------------------------"
    echo ""
} >> "$TEMP_FILE"

# --- ORDERED ASSEMBLY (NATIVE FILES) ---
MODULES=("animation" "border" "shader" "geometry")

for MOD in "${MODULES[@]}"; do
    NATIVE_FRAGMENT="$HVE_FRAGMENTS_DIR/${MOD}.${EXT}"

    if [ -f "$NATIVE_FRAGMENT" ]; then
        {
            echo "${COMMENT} [MODULE: ${MOD^^}]"
            cat "$NATIVE_FRAGMENT"
            echo ""
            echo ""
        } >> "$TEMP_FILE"
    fi
done

# ============================================
# 🔍 SYNTAX RECOGNITION AND VALIDATION (LIVE TEST)
# ============================================
VALID=true

if [ "$HVE_FORMAT" = "lua" ]; then
    if grep -qE '^[[:space:]]*(general|decoration|animations)[[:space:]]*\{' "$TEMP_FILE"; then
        echo "❌ [HVE ERROR] Validation failed! Expected Lua but detected classic syntax (.conf)."
        VALID=false
    fi
else
    if grep -qE 'hl\.(config|animation|curve)|require\(' "$TEMP_FILE"; then
        echo "❌ [HVE ERROR] Validation failed! The builder expects .conf but the file contains Lua code."
        VALID=false
    fi
fi

# If the test fails, we abort safely
if [ "$VALID" = false ]; then
    echo "⚠️ [HVE WARNING] Operation aborted. The active overlay was not modified."
    rm -f "$TEMP_FILE"
    exit 1
fi

# --- MASTER MOVE - Atomic Replacement & Cleanup ---
mv "$TEMP_FILE" "$FINAL_TARGET"

if [ "$HVE_FORMAT" = "lua" ]; then
    rm -f "$FINAL_FILE_CONF"
else
    rm -f "$FINAL_FILE_LUA"
fi

# ============================================
# 🔗 UNIVERSAL SYMLINK FOR NOCTALIA MANIFESTO
# ============================================
UNIVERSAL_OVERLAY="$HVE_SAFE_DIR/overlay.current"

rm -f "$UNIVERSAL_OVERLAY"
ln -s "$FINAL_TARGET" "$UNIVERSAL_OVERLAY"

# --- APPLICATION ---
if pgrep -x "Hyprland" > /dev/null; then
    hyprctl reload > /dev/null 2>&1
fi

echo "✅ [HVE SUCCESS] Overlay verified. Universal 'overlay.current' symlink updated."
