#!/bin/bash

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
# shellcheck source=/dev/null
source "$SCRIPT_DIR/utils.sh"
# shellcheck source=/dev/null
source "$SCRIPT_DIR/reload_coalescer.sh"

# Lua-only assembler: the config format is never detected at runtime.
FINAL_FILE="$HVE_SAFE_DIR/overlay.lua"
TEMP_FILE="$HVE_SAFE_DIR/overlay.tmp"
COMMENT="--"

# Ensure directories exist
mkdir -p "$HVE_FRAGMENTS_DIR"
mkdir -p "$HVE_SAFE_DIR"

# --- INITIALIZE TEMP FILE ---
{
    echo "#!/usr/bin/env hyprland"
    echo "${COMMENT} HYPRLAND VISUAL EDITOR - MASTER OVERLAY"
    echo "${COMMENT} Automatically generated (Lua mode)"
    echo ""
} > "$TEMP_FILE"

# --- SYSTEM: COLORS (from Hyprland IPC) ---
# shellcheck source=/dev/null
source "$HVE_SCRIPTS_DIR/colors.sh"

{
    echo "${COMMENT} [SYSTEM: COLORS]"
    echo "${COMMENT} Source: auto-detected (Noctalia/pywal/matugen/manual)"
    echo "primary = \"${HVE_PRIMARY}\""
    echo "secondary = \"${HVE_SECONDARY}\""
    echo "surface = \"${HVE_SURFACE}\""
    echo "surface_lowest = \"${HVE_SURFACE_LOWEST}\""
    echo "accent = \"${HVE_ACCENT}\""
    echo ""
} >> "$TEMP_FILE"
# --- IMMORTAL CURVE ---
{
    echo "${COMMENT} [SYSTEM: CURVES]"
    echo "hl.curve(\"linear\", {type = \"bezier\", points = {{0,0},{1,1}}})"
    echo "${COMMENT} ----------------------------------------------------"
    echo ""
} >> "$TEMP_FILE"

# --- ORDERED ASSEMBLY (NATIVE FILES) ---
MODULES=("animation" "border" "shader" "geometry")

for MOD in "${MODULES[@]}"; do
    NATIVE_FRAGMENT="$HVE_FRAGMENTS_DIR/${MOD}.lua"

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

if grep -qE '^[[:space:]]*(general|decoration|animations)[[:space:]]*\{' "$TEMP_FILE"; then
    echo "❌ [HVE ERROR] Validation failed! Expected Lua but detected classic syntax (.conf)."
    VALID=false
fi

# If the test fails, we abort safely
if [ "$VALID" = false ]; then
    echo "⚠️ [HVE WARNING] Operation aborted. The active overlay was not modified."
    rm -f "$TEMP_FILE"
    exit 1
fi

# --- MASTER MOVE - Atomic Replacement ---
mv "$TEMP_FILE" "$FINAL_FILE"

# ============================================
# 🔗 UNIVERSAL SYMLINK FOR NOCTALIA MANIFESTO
# ============================================
UNIVERSAL_OVERLAY="$HVE_SAFE_DIR/overlay.current"

rm -f "$UNIVERSAL_OVERLAY"
ln -s "$FINAL_FILE" "$UNIVERSAL_OVERLAY"

# --- APPLICATION ---
# One reload per change-burst, never lost: hve_reload_queue coalesces via
# reload_coalescer.sh (marker + drainer). Disable with HVE_RELOAD_COALESCE=0.
if pgrep -x "Hyprland" > /dev/null; then
    hve_reload_queue
fi

echo "✅ [HVE SUCCESS] Overlay verified. Universal 'overlay.current' symlink updated."
