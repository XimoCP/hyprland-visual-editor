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
    MSG_CACHE="¿Eliminar datos de runtime (~/.cache/hve)? [y/N]"
    MSG_CACHE_HINT="Son archivos temporales (logs, backups). Se recrean al ejecutar HVE de nuevo."
    MSG_CONFIG="¿Eliminar configuración de usuario (~/.config/hve/)? [y/N]"
    MSG_CONFIG_WARN="⚠️  Esto borra tus ajustes de HVE (tema, animaciones, binds, presets)."
    MSG_SUDO_CLEAN="Limpiando symlink del sistema..."
    MSG_SUDO_DONE="Symlink del sistema eliminado"
    MSG_SUDO_SKIP="Symlink del sistema no encontrado"
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
    MSG_CACHE="Remove runtime data (~/.cache/hve)? [y/N]"
    MSG_CACHE_HINT="Temporary files (logs, backups). They'll be recreated if you run HVE again."
    MSG_CONFIG="Remove user configuration (~/.config/hve/)? [y/N]"
    MSG_CONFIG_WARN="⚠️  This deletes your HVE settings (theme, animations, keybinds, presets)."
    MSG_SUDO_CLEAN="Cleaning system symlink..."
    MSG_SUDO_DONE="System symlink removed"
    MSG_SUDO_SKIP="System symlink not found"
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
HVE_ASSETS="$HOME/.local/bin/assets"
HVE_DESKTOP="$HOME/.local/share/applications/hve.desktop"
HVE_AUTOSTART="$HOME/.config/autostart/hve.desktop"
HVE_IPC="$HOME/.local/bin/hve-ipc"
HVE_IPC_SYMLINK="/usr/local/bin/hve-ipc"
HVE_CACHE="$HOME/.cache/hve"
HVE_CONFIG="$HOME/.config/hve"

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

# ── 2. Assets (scripts, borders, shaders, etc.) ────────────────────────────
remove_dir "$HVE_ASSETS" "Assets (~/.local/bin/assets/)"

# ── 3. Desktop entry ───────────────────────────────────────────────────────
remove_file "$HVE_DESKTOP" "Acceso directo (.local/share/applications/hve.desktop)"

# ── 4. Autostart ───────────────────────────────────────────────────────────
remove_file "$HVE_AUTOSTART" "Autostart (.config/autostart/hve.desktop)"

# ── 5. IPC script ──────────────────────────────────────────────────────────
remove_file "$HVE_IPC" "IPC script (~/.local/bin/hve-ipc)"

# ── 6. System symlink (needs sudo) ─────────────────────────────────────────
echo ""
info "$MSG_SUDO_CLEAN"
if [ -L "$HVE_IPC_SYMLINK" ] || [ -f "$HVE_IPC_SYMLINK" ]; then
    if sudo rm -f "$HVE_IPC_SYMLINK" 2>/dev/null; then
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
if prompt_yes_no "$MSG_CACHE" "no"; then
    remove_dir "$HVE_CACHE" "Cache (~/.cache/hve/)"
else
    info "Cache conservado"
fi

# ── 8. User config (ask!) ──────────────────────────────────────────────────
echo ""
warn "$MSG_CONFIG_WARN"
if prompt_yes_no "$MSG_CONFIG" "no"; then
    remove_dir "$HVE_CONFIG" "Configuración (~/.config/hve/)"
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
