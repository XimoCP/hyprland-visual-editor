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
BORDER_SIZE=${1:-2}
BORDER_RADIUS=${2:-0}
GAPS_IN=${3:-5}
GAPS_OUT=${4:-5}

# Basic validation (Fallback to 2 if QML fails)
if [ -z "$BORDER_SIZE" ] || ! [[ "$BORDER_SIZE" =~ ^-?[0-9]+$ ]]; then
    echo "[HVE] Warning: invalid BORDER_SIZE '$BORDER_SIZE', falling back to 2" >&2
    BORDER_SIZE=2
fi

# Validate radius (decoration.rounding). Fallback to 0 = no rounding.
if [ -z "$BORDER_RADIUS" ] || ! [[ "$BORDER_RADIUS" =~ ^-?[0-9]+$ ]] || [ "$BORDER_RADIUS" -lt 0 ]; then
    echo "[HVE] Warning: invalid BORDER_RADIUS '$BORDER_RADIUS', falling back to 0" >&2
    BORDER_RADIUS=0
fi

# Validate gaps (general.gaps_in / gaps_out). Fallback to 5.
for VAR in GAPS_IN GAPS_OUT; do
    VAL="${!VAR}"
    if [ -z "$VAL" ] || ! [[ "$VAL" =~ ^-?[0-9]+$ ]] || [ "$VAL" -lt 0 ]; then
        echo "[HVE] Warning: invalid $VAR '$VAL', falling back to 5" >&2
        eval "$VAR=5"
    fi
done

# Define fragment paths dynamically
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/geometry.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/geometry.${ALT_EXT}"

# Preventive cleanup: remove the fragment of the opposite format
rm -f "$OLD_FRAGMENT"

# 1. INTERNAL FRAGMENT GENERATION (Respecting strict line-breaks for .conf)
if [ "$EXT" = "lua" ]; then
    if [ "$BORDER_RADIUS" -gt 0 ]; then
        cat <<EOF > "$TARGET_FRAGMENT"
hl.config({
  general = {
    border_size = $BORDER_SIZE,
    gaps_in     = $GAPS_IN,
    gaps_out    = $GAPS_OUT,
  },
  decoration = {
    rounding       = $BORDER_RADIUS,
    rounding_power = 2,
  },
})
EOF
    else
        cat <<EOF > "$TARGET_FRAGMENT"
hl.config({
  general = {
    border_size = $BORDER_SIZE,
    gaps_in     = $GAPS_IN,
    gaps_out    = $GAPS_OUT,
  },
})
EOF
    fi
else
    if [ "$BORDER_RADIUS" -gt 0 ]; then
        cat <<EOF > "$TARGET_FRAGMENT"
general {
    border_size = $BORDER_SIZE
    gaps_in     = $GAPS_IN
    gaps_out    = $GAPS_OUT
}

decoration {
    rounding       = $BORDER_RADIUS
    rounding_power = 2
}
EOF
    else
        cat <<EOF > "$TARGET_FRAGMENT"
general {
    border_size = $BORDER_SIZE
    gaps_in     = $GAPS_IN
    gaps_out    = $GAPS_OUT
}
EOF
    fi
fi

echo "Geometry: border=$BORDER_SIZE radius=$BORDER_RADIUS gaps=${GAPS_IN}/${GAPS_OUT} ($EXT mode)"

# 2. RECONSTRUCTION WITH INTERNAL ASSEMBLER
if [ -f "$HVE_SCRIPTS_DIR/assemble.sh" ]; then
    bash "$HVE_SCRIPTS_DIR/assemble.sh"
else
    echo "Error: Assembler script not found in $HVE_SCRIPTS_DIR"
    exit 1
fi