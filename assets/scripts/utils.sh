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

# User preset root, mirroring `PresetStore::new` (src/preset_store.rs):
# `dirs::config_dir()` → `$XDG_CONFIG_HOME` when set, else `$HOME/.config`,
# plus `hve/presets`. Saved presets live in `<root>/borders` and
# `<root>/animations`. An explicit `HVE_USER_PRESETS_DIR` (sandbox/test
# override) wins, same rule as `HVE_CACHE_DIR`.
HVE_USER_PRESETS_DIR="${HVE_USER_PRESETS_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/hve/presets}"

# Safe cache directory (outside plugin, survives plugin deletion for cleanup).
# Cache-dir convention (capability-routing unit 1e, F5): the default mirrors
# Rust's `src/config.rs::hve_cache_dir()` — `$XDG_CACHE_HOME` when set, else
# `$HOME/.cache`, plus `hve` — so both sides find `color-authority.json`.
# Rule: an explicit `HVE_CACHE_DIR` (sandbox/test override) wins; otherwise
# the XDG rule above. In production neither `HVE_*` var is set, so both agree.
HVE_SAFE_DIR="${HVE_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/hve}"

# Hyprland config directory
HVE_HYPR_DIR="$HOME/.config/hypr"

# A preset name is UNSAFE when it is empty, contains a path separator (`/`
# or `\`), equals `.` or `..`, or starts with `.`. Interior dots are ALLOWED:
# the built-in `19_stylized2.5D.lua` and user preset stems such as
# `keeper_saved` may carry them. Returns 0 (true) when the name is safe.
# A NUL byte cannot travel through a bash variable, so it is impossible here;
# the Rust mirror (`PresetStore::validate_name`) rejects it explicitly.
_hve_preset_name_is_safe() {
    local name="$1"
    [ -n "$name" ] || return 1
    case "$name" in
        */*) return 1 ;;
        *\\*) return 1 ;;
        .|..) return 1 ;;
        .*) return 1 ;;
    esac
    return 0
}

# Confirm `$dir/$name` stays inside `$dir` after normalization. `realpath -m`
# canonicalizes without requiring the file to exist, so `..` components and
# symlinks that escape the directory are refused. `--` ends option parsing so
# a hostile name can never be read as a flag. Returns 0 (true) when contained.
_hve_preset_path_is_contained() {
    local dir="$1"
    local name="$2"
    local resolved_dir resolved_path
    resolved_dir="$(realpath -m -- "$dir" 2>/dev/null)" || return 1
    resolved_path="$(realpath -m -- "$dir/$name" 2>/dev/null)" || return 1
    case "$resolved_path" in
        "$resolved_dir"/*) return 0 ;;
    esac
    return 1
}

# Look for a preset inside ONE directory. Lua-only: HVE never detects config
# format at runtime, so a preset name always resolves to `$dir/$name.lua`.
_hve_preset_in_dir() {
    local dir="$1"
    local name="$2"

    # Reject a hostile name before touching the filesystem: the name arrives
    # unsanitized from the UI and a `..`/absolute component would otherwise
    # `cat` an arbitrary local file into the assembled Hyprland fragment.
    _hve_preset_name_is_safe "$name" || return 1
    _hve_preset_path_is_contained "$dir" "$name" || return 1

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

# Resolve a preset file. The first argument is the BUILT-IN directory and is
# searched FIRST: a user preset must never shadow a built-in of the same name.
# The optional third argument is the USER preset directory (a saved preset at
# `$XDG_CONFIG_HOME/hve/presets/<category>/<name>.lua`), searched only when
# the built-in directory has no match — the same order as
# `PresetStore::read_border_file` (src/preset_store.rs), so shell and Rust
# agree. Returns 1 when neither directory holds the name: the caller's safe
# fallback depends on that.
# Usage: _resolved_file=$(hve_resolve_preset "$PRESETS_DIR" "$PRESET_NAME" "$HVE_USER_PRESETS_DIR/borders")
hve_resolve_preset() {
    local dir="$1"
    local name="$2"
    local user_dir="${3:-}"

    # Built-in directory first.
    if _hve_preset_in_dir "$dir" "$name"; then
        return 0
    fi

    # Then the documented user directory.
    if [ -n "$user_dir" ] && [ "$user_dir" != "$dir" ]; then
        if _hve_preset_in_dir "$user_dir" "$name"; then
            return 0
        fi
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
