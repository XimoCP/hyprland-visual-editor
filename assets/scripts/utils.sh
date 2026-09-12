#!/bin/bash
# shellcheck disable=SC2034
# --- HVE Unified Path Handler ---
# All scripts should source this file instead of hardcoding paths.
# Usage: source "$(dirname "${BASH_SOURCE[0]}")/utils.sh"

# Resolve plugin directory from script location, not hardcoded $HOME
# Works whether called directly, sourced, or via bash <script>
_HVE_SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"

# If we're in assets/scripts/, go up one level to get assets/
if [[ "$_HVE_SCRIPT_DIR" == */assets/scripts ]]; then
    HVE_ASSETS_DIR="$(dirname "$_HVE_SCRIPT_DIR")"
else
    HVE_ASSETS_DIR="$_HVE_SCRIPT_DIR"
fi

# Plugin root is one level above assets/
HVE_PLUGIN_DIR="$(dirname "$HVE_ASSETS_DIR")"

# Subdirectories
HVE_SCRIPTS_DIR="$HVE_ASSETS_DIR/scripts"
HVE_FRAGMENTS_DIR="$HVE_ASSETS_DIR/fragments"
HVE_BORDERS_DIR="$HVE_ASSETS_DIR/borders"
HVE_ANIMATIONS_DIR="$HVE_ASSETS_DIR/animations"
HVE_SHADERS_DIR="$HVE_ASSETS_DIR/shaders"

# Safe cache directory (outside plugin, survives plugin deletion for cleanup)
HVE_SAFE_DIR="${HVE_CACHE_DIR:-$HOME/.cache/hve}"

# Hyprland config directory
HVE_HYPR_DIR="$HOME/.config/hypr"

# Resolve a preset file. Lua-only: HVE never detects config format at runtime,
# so a preset name always resolves to `$dir/$name.lua`.
# Usage: _resolved_file=$(hve_resolve_preset "$PRESETS_DIR" "$PRESET_NAME")
hve_resolve_preset() {
    local dir="$1"
    local name="$2"

    # If the name already carries an extension, honour it only if it exists.
    if [[ "$name" == *.* ]]; then
        if [ -f "$dir/$name" ]; then
            echo "$dir/$name"
            return 0
        fi
        return 1
    fi

    if [ -f "$dir/$name.lua" ]; then
        echo "$dir/$name.lua"
        return 0
    fi

    return 1
}

# Trim leading/trailing whitespace from a string
# Usage: trimmed=$(hve_trim "  hello  ")
hve_trim() {
    local var="$1"
    var="${var#"${var%%[![:space:]]*}"}"
    var="${var%"${var##*[![:space:]]}"}"
    echo "$var"
}

# Strip quotes (single and double) from a string
# Usage: unquoted=$(hve_unquote '"hello"')
hve_unquote() {
    local var="$1"
    var="${var#\"}"
    var="${var%\"}"
    var="${var#\'}"
    var="${var%\'}"
    echo "$var"
}

# Clean up internal variable
unset _HVE_SCRIPT_DIR
