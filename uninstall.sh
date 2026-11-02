#!/bin/bash
# ============================================================================
# HVE Uninstaller — Hyprland Visual Editor
# Removes everything the installer created, plus runtime data.
# ============================================================================
set -euo pipefail

# ── Colors ─────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ── Language detection ──────────────────────────────────────────────────────
detect_lang() {
    local lang="${LANG:-en}"
    lang="${lang:0:2}"
    case "$lang" in
        es) echo "es" ;;
        *)  echo "en" ;;
    esac
}
LANG_CODE=$(detect_lang)

# ── i18n messages ──────────────────────────────────────────────────────────
if [ "$LANG_CODE" = "es" ]; then
    MSG_TITLE="Desinstalando HVE..."
    MSG_DONE="HVE desinstalado"
    MSG_REMOVING="Eliminando"
    MSG_OK="eliminado"
    MSG_NOT_FOUND="no encontrado — ignorado"
    MSG_CACHE="¿Eliminar datos de runtime (%s)? [y/N]"
    MSG_CACHE_HINT="Son archivos temporales (logs, backups). Se recrean al ejecutar HVE de nuevo."
    MSG_CONFIG="¿Eliminar configuración de usuario (%s/)? [y/N]"
    MSG_CONFIG_WARN="⚠️  Esto borra tus ajustes de HVE (tema, animaciones, binds, presets)."
    MSG_SUDO_CLEAN="Limpiando symlink del sistema..."
    MSG_SUDO_DONE="Symlink del sistema eliminado"
    MSG_SUDO_SKIP="Symlink del sistema no encontrado"
    MSG_HYPR_STEP="Quitando el bloque de HVE de la configuración de Hyprland"
    MSG_HYPR_OK="Bloque de HVE quitado de %s"
    MSG_HYPR_NONE="Sin bloque de HVE en la configuración de Hyprland — ignorado"
    MSG_HYPR_UNTERMINATED="Bloque de HVE sin cierre en %s — no se toca (revisa el archivo a mano)"
    MSG_MUTE_STEP="Devolviendo el mando al motor de skwd-wall"
    MSG_MUTE_OK="Motor de skwd-wall devuelto a '%s'"
    MSG_MUTE_NONE="Sin mute pendiente del motor — ignorado"
    MSG_MUTE_WARN="No se pudo devolver el mando del motor; ajusta 'theme.policy' a mano en %s"
    MSG_SUMMARY="Resumen de desinstalación"
    MSG_ITEMS_REMOVED="Elementos eliminados"
    MSG_NOT_REMOVED="No se encontraron"
    MSG_CONFIG_KEPT="Configuración de usuario conservada"
    MSG_RECOVER="Para reinstalar: ./install.sh"
    MSG_PATH_WARN="(todavía tenés ~/.local/bin en el PATH, pero ya no hay nada de HVE)"
    MSG_YES="sí"
    MSG_NO="no"
else
    MSG_TITLE="Uninstalling HVE..."
    MSG_DONE="HVE uninstalled"
    MSG_REMOVING="Removing"
    MSG_OK="removed"
    MSG_NOT_FOUND="not found — skipped"
    MSG_CACHE="Remove runtime data (%s)? [y/N]"
    MSG_CACHE_HINT="Temporary files (logs, backups). They'll be recreated if you run HVE again."
    MSG_CONFIG="Remove user configuration (%s/)? [y/N]"
    MSG_CONFIG_WARN="⚠️  This deletes your HVE settings (theme, animations, keybinds, presets)."
    MSG_SUDO_CLEAN="Cleaning system symlink..."
    MSG_SUDO_DONE="System symlink removed"
    MSG_SUDO_SKIP="System symlink not found"
    MSG_HYPR_STEP="Removing HVE's block from the Hyprland configuration"
    MSG_HYPR_OK="HVE block removed from %s"
    MSG_HYPR_NONE="No HVE block in the Hyprland configuration — skipped"
    MSG_HYPR_UNTERMINATED="HVE block in %s has no closing marker — left untouched (check it by hand)"
    MSG_MUTE_STEP="Handing the engine back to skwd-wall"
    MSG_MUTE_OK="skwd-wall engine handed back to '%s'"
    MSG_MUTE_NONE="No pending engine mute — skipped"
    MSG_MUTE_WARN="Could not hand the engine back; set 'theme.policy' by hand in %s"
    MSG_SUMMARY="Uninstall summary"
    MSG_ITEMS_REMOVED="Items removed"
    MSG_NOT_REMOVED="Not found"
    MSG_CONFIG_KEPT="User configuration kept"
    MSG_RECOVER="To reinstall: ./install.sh"
    MSG_PATH_WARN="(~/.local/bin is still in your PATH, but HVE files are gone)"
fi

# ── Helpers ─────────────────────────────────────────────────────────────────
ok()   { echo -e "${GREEN}✅ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠️  $1${NC}"; }
info() { echo -e "${CYAN}ℹ️  $1${NC}"; }
fail() { echo -e "${RED}❌ $1${NC}"; }

# sprintf helper for messages with arguments
msg_fmt() {
    local fmt="$1"; shift
    # shellcheck disable=SC2059
    printf "$fmt" "$@"
}

# Resolve one XDG base directory the way the Rust side does. `dirs` uses the
# variable only when it is an ABSOLUTE path and otherwise falls back; a shell
# `${VAR:-fallback}` would instead accept a RELATIVE value and resolve it
# against the caller's CWD, removing a directory the app never wrote. Unset
# and empty keep the fallback, exactly as before.
xdg_dir() {
    local value="${1:-}" fallback="$2"
    case "$value" in
        /*) printf '%s\n' "$value" ;;
        *)  printf '%s\n' "$fallback" ;;
    esac
}

prompt_yes_no() {
    local prompt="$1" default="$2"
    local answer
    read -p "$prompt " -n 1 -r answer || true
    echo
    if [[ -z "${answer:-}" ]]; then
        [[ "$default" == "yes" ]] && return 0 || return 1
    fi
    [[ "$answer" =~ [Yy] ]] && return 0 || return 1
}

# ── Paths ───────────────────────────────────────────────────────────────────
HVE_BIN="$HOME/.local/bin/hve"
# The app dir holds the real binary and its assets. `HVE_LEGACY_ASSETS` is the
# loose `assets/` an older installer left in ~/.local/bin; it is still removed
# so an install that never ran the new installer is cleaned completely too.
HVE_APP_DIR="$HOME/.local/opt/hve"
HVE_LEGACY_ASSETS="$HOME/.local/bin/assets"
HVE_DESKTOP="$HOME/.local/share/applications/hve.desktop"
HVE_ICON_DIR="$HOME/.local/share/icons/hicolor"
HVE_IPC="$HOME/.local/bin/hve-ipc"
# HVE_FIRST_TOGGLE removed — priming is now internal via winit/xdg-shell
# `/usr/local/bin` normally needs root, so removal falls back to sudo. The path
# is overridable so the uninstaller test can plant a throwaway symlink instead
# of touching the machine's own.
HVE_IPC_SYMLINK="${HVE_IPC_SYMLINK_PATH:-/usr/local/bin/hve-ipc}"
# Mirror the app's `dirs::cache_dir()` / `dirs::config_dir()` resolution
# (src/config.rs): honour XDG_*_HOME only when it is an absolute path, else
# fall back to $HOME/.cache|.config. A relative XDG value is ignored exactly
# as `dirs` ignores it, so we never act on a stray directory.
HVE_CACHE="$(xdg_dir "${XDG_CACHE_HOME:-}" "$HOME/.cache")/hve"
HVE_CONFIG="$(xdg_dir "${XDG_CONFIG_HOME:-}" "$HOME/.config")/hve"
# Hyprland's own config dir, resolved the same way (it follows XDG too).
HVE_LOCAL_CONFIG="$(xdg_dir "${XDG_CONFIG_HOME:-}" "$HOME/.config")"
HVE_HYPR_LUA="$HVE_LOCAL_CONFIG/hypr/hyprland.lua"
HVE_HYPR_CONF="$HVE_LOCAL_CONFIG/hypr/hyprland.conf"

REMOVED=()
NOT_FOUND=()

remove_file() {
    local path="$1" label="$2"
    if [ -f "$path" ] || [ -L "$path" ]; then
        rm -f "$path"
        ok "$label → $MSG_OK"
        REMOVED+=("$label")
    else
        warn "$label → $MSG_NOT_FOUND"
        NOT_FOUND+=("$label")
    fi
}

remove_dir() {
    local path="$1" label="$2"
    if [ -d "$path" ]; then
        rm -rf "$path"
        ok "$label → $MSG_OK"
        REMOVED+=("$label")
    else
        warn "$label → $MSG_NOT_FOUND"
        NOT_FOUND+=("$label")
    fi
}

# Remove only the HVE autostart block from hve-settings.lua, leaving keybinds
# and window rules in place. Mirrors src/config_markers.rs:
#   LUA_AUTOSTART_START = "-- >>> HVE AUTOSTART <<<"
#   LUA_AUTOSTART_END   = "-- >>> HVE AUTOSTART END <<<"
# Runs even when the user keeps the cache, so no `hve --tray` entry survives
# pointing at a binary that is being removed.
remove_autostart_block() {
    local path="$HVE_CACHE/hve-settings.lua"
    local label="Autostart entry (hve-settings.lua)"
    if [ ! -f "$path" ] \
        || ! grep -qF -- '-- >>> HVE AUTOSTART <<<' "$path" \
        || ! grep -qF -- '-- >>> HVE AUTOSTART END <<<' "$path"; then
        warn "$label → $MSG_NOT_FOUND"
        NOT_FOUND+=("$label")
        return
    fi
    # Delete from start marker through end marker, inclusive. Leading/trailing
    # whitespace is tolerated, matching the trimmed compare in settings.rs.
    sed -i '/^[[:space:]]*-- >>> HVE AUTOSTART <<<[[:space:]]*$/,/^[[:space:]]*-- >>> HVE AUTOSTART END <<<[[:space:]]*$/d' "$path"
    ok "$label → $MSG_OK"
    REMOVED+=("$label")
}

# Remove HVE's injected block from the Hyprland configuration. The uninstaller
# used to leave this to the watchdog, but the watchdog lives in the cache — and
# the cache is deleted right here when the keeper accepts it. The block then
# survived with two `dofile`s pointing at files that no longer exist, and
# Hyprland failed on the next start. Only the delimited block is removed, so the
# keeper's own lines around it are never touched.
remove_hyprland_block() {
    local file removed=0
    info "$MSG_HYPR_STEP"
    for file in "$HVE_HYPR_LUA" "$HVE_HYPR_CONF"; do
        [ -f "$file" ] || continue
        grep -qF '>>> HYPRLAND VISUAL EDITOR START <<<' "$file" || continue
        # BOTH markers or nothing: `sed '/START/,/END/d'` with a missing END
        # deletes everything to the end of the file, which would take the
        # keeper's own configuration with it.
        if ! grep -qF '>>> HYPRLAND VISUAL EDITOR END <<<' "$file"; then
            warn "$(msg_fmt "$MSG_HYPR_UNTERMINATED" "$file")"
            continue
        fi
        sed -i '/>>> HYPRLAND VISUAL EDITOR START <<</,/>>> HYPRLAND VISUAL EDITOR END <<</d' "$file"
        ok "$(msg_fmt "$MSG_HYPR_OK" "$file")"
        removed=1
    done
    if [ "$removed" -eq 1 ]; then
        REMOVED+=("$MSG_HYPR_STEP")
    else
        warn "$MSG_HYPR_NONE"
        NOT_FOUND+=("$MSG_HYPR_STEP")
    fi
}

# Hand the skwd-wall engine back before it is forgotten. Applying a theme mutes
# the engine (`theme.policy` -> "off") and the app restores it on its next
# start — a start that never happens once HVE is gone, which would leave the
# keeper's wallpaper engine muted forever. The pending value lives in the yield
# marker. Only HVE's own mute is undone: if the keeper changed the policy by
# hand, or the file holds more than one bare `policy` key, nothing is written.
release_engine_mute() {
    local marker="$HVE_CACHE/skwd-policy-yield.json"
    local restored="" status=0
    info "$MSG_MUTE_STEP"
    if [ ! -f "$marker" ]; then
        warn "$MSG_MUTE_NONE"
        NOT_FOUND+=("$MSG_MUTE_STEP")
        return
    fi
    if ! command -v python3 >/dev/null 2>&1; then
        warn "$(msg_fmt "$MSG_MUTE_WARN" "$marker")"
        NOT_FOUND+=("$MSG_MUTE_STEP")
        return
    fi
    restored="$(python3 - "$marker" <<'PY'
import json
import re
import sys

marker = sys.argv[1]
try:
    with open(marker, encoding="utf-8") as fh:
        record = json.load(fh)
    config_path = record["config_path"]
    previous = record["previous_value"]
except Exception:
    sys.exit(2)

try:
    with open(config_path, encoding="utf-8") as fh:
        text = fh.read()
except Exception:
    sys.exit(3)

# The bare `policy` key, never the dotted `theme.policy` entries: the pattern
# anchors on the opening quote, which the dotted key does not have there.
policy = re.compile(r'"policy"(\s*:\s*)"([^"]*)"')
found = policy.findall(text)
if len(found) != 1 or found[0][1] != "off":
    # Not our mute, or an ambiguous file: leave it exactly as it is.
    sys.exit(4)

written = policy.sub(lambda m: '"policy"%s"%s"' % (m.group(1), previous), text, count=1)
try:
    with open(config_path, "w", encoding="utf-8") as fh:
        fh.write(written)
except Exception:
    sys.exit(5)
print(previous)
PY
)" || status=$?
    case "${status:-0}" in
        0)
            ok "$(msg_fmt "$MSG_MUTE_OK" "$restored")"
            rm -f "$marker"
            REMOVED+=("$MSG_MUTE_STEP")
            ;;
        4)
            info "$MSG_MUTE_NONE"
            NOT_FOUND+=("$MSG_MUTE_STEP")
            ;;
        *)
            warn "$(msg_fmt "$MSG_MUTE_WARN" "$marker")"
            NOT_FOUND+=("$MSG_MUTE_STEP")
            ;;
    esac
}

# ============================================================================
# MAIN
# ============================================================================
echo ""
echo -e "${CYAN}═══════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  $MSG_TITLE${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════${NC}"
echo ""

# ── 1. Binary ──────────────────────────────────────────────────────────────
info "$MSG_REMOVING: binario y assets"
remove_file "$HVE_BIN" "Binario (~/.local/bin/hve)"

# ── 2. App dir + assets ────────────────────────────────────────────────────
remove_dir "$HVE_APP_DIR" "App (~/.local/opt/hve/)"
remove_dir "$HVE_LEGACY_ASSETS" "Assets antiguos (~/.local/bin/assets/)"

# ── 3. Desktop entry + launcher icon ───────────────────────────────────────
remove_file "$HVE_DESKTOP" "Acceso directo (.local/share/applications/hve.desktop)"
# The icons live in the hicolor theme, not next to the binary. Leaving them
# behind keeps a familiar face in the launcher for an application that is gone,
# and a stale copy in any size directory would shadow a future install.
REMOVED_ICONS=0
for icon in "$HVE_ICON_DIR"/*/apps/hve.png; do
    [ -e "$icon" ] || continue
    rm -f "$icon"
    REMOVED_ICONS=$((REMOVED_ICONS + 1))
done
if [ "$REMOVED_ICONS" -gt 0 ]; then
    ok "Iconos del lanzador (.local/share/icons/hicolor/*/apps/hve.png) → $MSG_OK"
    REMOVED+=("Iconos del lanzador")
else
    warn "Iconos del lanzador (.local/share/icons/hicolor/*/apps/hve.png) → $MSG_NOT_FOUND"
    NOT_FOUND+=("Iconos del lanzador")
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -d "$HVE_ICON_DIR" ]; then
    gtk-update-icon-cache -f -t "$HVE_ICON_DIR" >/dev/null 2>&1 || true
fi

# ── 4. Autostart ───────────────────────────────────────────────────────────
# Install never writes ~/.config/autostart/hve.desktop; the real autostart is
# the `hl.on("hyprland.start", ...)` block inside hve-settings.lua. Remove just
# that block, even if the cache is kept below.
remove_autostart_block

# ── 4b. Hyprland block ─────────────────────────────────────────────────────
# Must run before the cache prompt: with the cache kept, the watchdog would
# clean the block at the next login, but with the cache deleted nothing else
# ever removes it.
echo ""
remove_hyprland_block

# ── 4c. skwd-wall engine mute ──────────────────────────────────────────────
# The yield marker lives in the cache, so this runs before that prompt too.
echo ""
release_engine_mute

# ── 5. IPC script ──────────────────────────────────────────────────────────
remove_file "$HVE_IPC" "IPC script (~/.local/bin/hve-ipc)"

# ── NOTE: First-toggle script removed — priming is internal via winit/xdg-shell

# ── 6. System symlink (needs sudo) ─────────────────────────────────────────
echo ""
info "$MSG_SUDO_CLEAN"
if [ -L "$HVE_IPC_SYMLINK" ] || [ -f "$HVE_IPC_SYMLINK" ]; then
    # A user-writable destination (a sandbox, or a /usr/local owned by the
    # user) needs no password; only a root-owned one falls back to sudo.
    if rm -f "$HVE_IPC_SYMLINK" 2>/dev/null || sudo rm -f "$HVE_IPC_SYMLINK" 2>/dev/null; then
        ok "$MSG_SUDO_DONE"
        REMOVED+=("Symlink del sistema (/usr/local/bin/hve-ipc)")
    else
        warn "No se pudo eliminar $HVE_IPC_SYMLINK"
    fi
else
    warn "$MSG_SUDO_SKIP"
fi

# ── 7. Cache (temporary runtime data) ──────────────────────────────────────
echo ""
info "$MSG_CACHE_HINT"
if prompt_yes_no "$(msg_fmt "$MSG_CACHE" "$HVE_CACHE")" "no"; then
    remove_dir "$HVE_CACHE" "$(msg_fmt 'Cache (%s/)' "$HVE_CACHE")"
else
    info "Cache conservado"
fi

# ── 8. User config (ask!) ──────────────────────────────────────────────────
echo ""
warn "$MSG_CONFIG_WARN"
if prompt_yes_no "$(msg_fmt "$MSG_CONFIG" "$HVE_CONFIG")" "no"; then
    remove_dir "$HVE_CONFIG" "$(msg_fmt 'Configuración (%s/)' "$HVE_CONFIG")"
else
    info "$MSG_CONFIG_KEPT"
fi

# ============================================================================
# SUMMARY
# ============================================================================
echo ""
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  ✅ $MSG_DONE${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo ""
echo "   $MSG_SUMMARY:"
echo "   $MSG_ITEMS_REMOVED (${#REMOVED[@]}):"
for item in "${REMOVED[@]}"; do
    echo "     • $item"
done
if [ ${#NOT_FOUND[@]} -gt 0 ]; then
    echo "   $MSG_NOT_REMOVED (${#NOT_FOUND[@]}):"
    for item in "${NOT_FOUND[@]}"; do
        echo "     • $item"
    done
fi
echo ""
echo "   $MSG_RECOVER"
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    warn "$MSG_PATH_WARN"
fi
echo ""
