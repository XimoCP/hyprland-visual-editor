#!/bin/bash

# geometry.sh - Controls physical size dynamically (Lua/Conf)

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format BEFORE resolving preset so we pick the right extension
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"

# 1. SEPARATE DECLARATION
export HVE_FORMAT

if [ -f "$FORMAT_CACHE" ]; then
    # 2. CLEAN ASSIGNMENT (Cache)
    HVE_FORMAT=$(cat "$FORMAT_CACHE")
else
    source "$HVE_SCRIPTS_DIR/detect_format.sh"
    # 2. CLEAN ASSIGNMENT (Dynamic detection)
    HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
fi

# 🎛️ DYNAMIC EXTENSION ASSIGNMENT
if [ "$HVE_FORMAT" = "lua" ]; then
    EXT="lua"
    ALT_EXT="conf"  # Alternative extension for cleanup
else
    EXT="conf"
    ALT_EXT="lua"   # Alternative extension for cleanup
fi

mkdir -p "$HVE_FRAGMENTS_DIR"
BORDER_SIZE=$1

# Basic validation (Fallback to 2 if QML fails)
if [ -z "$BORDER_SIZE" ]; then
    BORDER_SIZE=2
fi

# Define fragment paths dynamically
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/geometry.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/geometry.${ALT_EXT}"

# Preventive cleanup: remove the fragment of the opposite format
rm -f "$OLD_FRAGMENT"

# 1. INTERNAL FRAGMENT GENERATION (Respecting strict line-breaks for .conf)
if [ "$EXT" = "lua" ]; then
    echo "hl.config({ general = { border_size = $BORDER_SIZE } })" > "$TARGET_FRAGMENT"
else
    cat <<EOF > "$TARGET_FRAGMENT"
general {
    border_size = $BORDER_SIZE
}
EOF
fi

echo "Geometry: Applied border size $BORDER_SIZE ($EXT mode)"

# 2. RECONSTRUCTION WITH INTERNAL ASSEMBLER
if [ -f "$HVE_SCRIPTS_DIR/assemble.sh" ]; then
    bash "$HVE_SCRIPTS_DIR/assemble.sh"
else
    echo "Error: Assembler script not found in $HVE_SCRIPTS_DIR"
    exit 1
fi