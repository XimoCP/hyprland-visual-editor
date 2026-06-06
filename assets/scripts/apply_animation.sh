#!/bin/bash
#shellcheck disable=SC1091,SC2155
# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format BEFORE resolving preset so we pick the right extension
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"
if [ -f "$FORMAT_CACHE" ]; then
    export HVE_FORMAT=$(cat "$FORMAT_CACHE")
else
    source "$HVE_SCRIPTS_DIR/detect_format.sh"
    export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
fi

# 🎛️ ASIGNACIÓN DINÁMICA DE EXTENSIÓN
if [ "$HVE_FORMAT" = "lua" ]; then
    EXT="lua"
    ALT_EXT="conf"  # Extensión alternativa para limpieza
else
    EXT="conf"
    ALT_EXT="lua"   # Extensión alternativa para limpieza
fi

mkdir -p "$HVE_FRAGMENTS_DIR"
PRESET_NAME=$1

# Definimos las rutas de los fragmentos de forma dinámica
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/animation.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/animation.${ALT_EXT}"

# 1. SHUTDOWN LOGIC (None or empty)
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$TARGET_FRAGMENT" "$OLD_FRAGMENT"
    echo "Border disabled."
else
    # Limpieza preventiva: eliminamos el fragmento del formato opuesto
    rm -f "$OLD_FRAGMENT"

    # 2. DYNAMIC LOADING - Respects HVE_FORMAT for extension preference
    TARGET_FILE=$(hve_resolve_preset "$HVE_ANIMATIONS_DIR" "$PRESET_NAME")
#!/bin/bash

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format BEFORE resolving preset so we pick the right extension
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"
if [ -f "$FORMAT_CACHE" ]; then
    export HVE_FORMAT=$(cat "$FORMAT_CACHE")
else
    source "$HVE_SCRIPTS_DIR/detect_format.sh"
    export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
fi

# 🎛️ ASIGNACIÓN DINÁMICA DE EXTENSIÓN
if [ "$HVE_FORMAT" = "lua" ]; then
    EXT="lua"
    ALT_EXT="conf"  # Extensión alternativa para limpieza
else
    EXT="conf"
    ALT_EXT="lua"   # Extensión alternativa para limpieza
fi

mkdir -p "$HVE_FRAGMENTS_DIR"
PRESET_NAME=$1

# Definimos las rutas de los fragmentos de forma dinámica
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/animation.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/animation.${ALT_EXT}"

# 1. SHUTDOWN LOGIC (None or empty)
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$TARGET_FRAGMENT" "$OLD_FRAGMENT"
    echo "Animations disabled."
else
    # Limpieza preventiva: eliminamos el fragmento del formato opuesto
    rm -f "$OLD_FRAGMENT"

    # 2. DYNAMIC LOADING - Respects HVE_FORMAT for extension preference
    TARGET_FILE=$(hve_resolve_preset "$HVE_ANIMATIONS_DIR" "$PRESET_NAME")

    if [ $? -eq 0 ] && [ -n "$TARGET_FILE" ]; then
        # Copy the preset content to the dynamic fragment
        cat "$TARGET_FILE" > "$TARGET_FRAGMENT"
        echo "Animation preset applied: $PRESET_NAME ($EXT mode)"
    else

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format BEFORE resolving preset so we pick the right extension
FORMAT_CACHE="$HVE_SAFE_DIR/hve_format"
if [ -f "$FORMAT_CACHE" ]; then
    export HVE_FORMAT=$(cat "$FORMAT_CACHE")
else
    source "$HVE_SCRIPTS_DIR/detect_format.sh"
    export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")
fi

# 🎛️ ASIGNACIÓN DINÁMICA DE EXTENSIÓN
if [ "$HVE_FORMAT" = "lua" ]; then
    EXT="lua"
    ALT_EXT="conf"  # Extensión alternativa para limpieza
else
    EXT="conf"
    ALT_EXT="lua"   # Extensión alternativa para limpieza
fi

mkdir -p "$HVE_FRAGMENTS_DIR"
PRESET_NAME=$1

# Definimos las rutas de los fragmentos de forma dinámica
TARGET_FRAGMENT="$HVE_FRAGMENTS_DIR/animation.${EXT}"
OLD_FRAGMENT="$HVE_FRAGMENTS_DIR/animation.${ALT_EXT}"

# 1. SHUTDOWN LOGIC (None or empty)
if [ "$PRESET_NAME" == "none" ] || [ -z "$PRESET_NAME" ]; then
    rm -f "$TARGET_FRAGMENT" "$OLD_FRAGMENT"
    echo "Animations disabled."
else
    # Limpieza preventiva: eliminamos el fragmento del formato opuesto
    rm -f "$OLD_FRAGMENT"

    # 2. DYNAMIC LOADING - Respects HVE_FORMAT for extension preference
    TARGET_FILE=$(hve_resolve_preset "$HVE_ANIMATIONS_DIR" "$PRESET_NAME")

    if [ $? -eq 0 ] && [ -n "$TARGET_FILE" ]; then
        # Copy the preset content to the dynamic fragment
        cat "$TARGET_FILE" > "$TARGET_FRAGMENT"
        echo "Animation preset applied: $PRESET_NAME ($EXT mode)"
    else
        # 🚨 SECURITY FALLBACK: Corregido con sintaxis real de animaciones
        if [ "$EXT" = "lua" ]; then
            # Activa animaciones en la API de Lua de Hyprland
            echo 'hl.config({ animations = { enabled = true } })' > "$TARGET_FRAGMENT"
        else
            # Activa animaciones en la sintaxis clásica de Hyprland
            echo "animations { enabled = true }" > "$TARGET_FRAGMENT"
        fi
        echo "Warning: Preset $PRESET_NAME not found. Using safe animation fallback."
    fi
fi

# 3. ASSEMBLY
bash "$HVE_SCRIPTS_DIR/assemble.sh"
        # 🚨 SECURITY FALLBACK: Corregido con sintaxis real de animaciones
        if [ "$EXT" = "lua" ]; then
            # Activa animaciones en la API de Lua de Hyprland
            echo 'hl.config({ animations = { enabled = true } })' > "$TARGET_FRAGMENT"
        else
            # Activa animaciones en la sintaxis clásica de Hyprland
            echo "animations { enabled = true }" > "$TARGET_FRAGMENT"
        fi
        echo "Warning: Preset $PRESET_NAME not found. Using safe animation fallback."
    fi
fi

# 3. ASSEMBLY
bash "$HVE_SCRIPTS_DIR/assemble.sh"
    if [ $? -eq 0 ] && [ -n "$TARGET_FILE" ]; then
        # Copy the preset content to the dynamic fragment
        cat "$TARGET_FILE" > "$TARGET_FRAGMENT"
        echo "Border preset applied: $PRESET_NAME ($EXT mode)"
    else
        # Security fallback: adapts structure based on format
        if [ "$EXT" = "lua" ]; then
            echo 'hl.config({ general = { ["col.active_border"] = primary } })' > "$TARGET_FRAGMENT"
        else
            echo "general { col.active_border = \$primary }" > "$TARGET_FRAGMENT"
        fi
        echo "Warning: Preset $PRESET_NAME not found. Using safe fallback."
    fi
fi

# 3. ASSEMBLY
bash "$HVE_SCRIPTS_DIR/assemble.sh"