#!/bin/bash
# ============================================================================
# HVE Installer — Hyprland Visual Editor
# Multi-distro installer with smart dependency management
#
# Supports: Arch/Manjaro, Debian/Ubuntu/Pop, Fedora, OpenSUSE, Void, Gentoo
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
    # ── Generales ──
    MSG_DETECTING_OS="Detectando sistema operativo..."
    MSG_DETECTED="Detectado:"
    MSG_UNSUPPORTED="Distribución no soportada (%s). Instala las dependencias manualmente y vuelve a ejecutar."
    MSG_INSTALLING_DEPS="Instalando dependencias del sistema..."
    MSG_BATCH_FAIL="Fallo la instalación por lote — probando una por una..."
    MSG_COULD_NOT_INSTALL="No se pudo instalar %s (puede que necesites instalarlo manualmente)"
    MSG_FOUND="encontrado"
    MSG_NOT_FOUND="no encontrado — algunas funciones pueden no funcionar"
    MSG_CHECKING_RUST="Verificando Rust..."
    MSG_RUST_OK="Rust %s"
    MSG_RUST_NOT_FOUND="Rust no encontrado — instalando rustup..."
    MSG_RUST_INSTALLED="Rust instalado: %s"
    MSG_RUST_FAIL="Fallo la instalación de Rust. Intenta manualmente: https://rustup.rs"
    MSG_PREFLIGHT_LUA="Configuración Lua de Hyprland detectada — HVE gestionará hyprland.lua"
    MSG_PREFLIGHT_CONF="Hyprland usa un hyprland.conf heredado y HVE 2 solo gestiona hyprland.lua. Migre su configuración antes de habilitar HVE."
    MSG_PREFLIGHT_NONE="Aún no se encontró configuración de Hyprland — HVE generará los ajustes Lua en la primera ejecución."
    MSG_BUILDING="Compilando HVE (esto puede llevar un rato)..."
    MSG_BUILD_DONE="Compilación completada"
    MSG_INSTALLING="Instalando HVE..."
    MSG_BINARY_TO="Binario → %s"
    MSG_ASSETS_TO="Assets → ~/.local/bin/assets/"
    MSG_DESKTOP_TO="Acceso directo → %s (%s)"
    MSG_DETECTING_COLOR="Detectando herramienta de colores..."
    MSG_COLOR_TOOL="Herramienta de colores: %s"
    MSG_WATCHING="Vigilando:  %s"
    MSG_IPC_TITLE="Instalando script hve-ipc..."
    MSG_IPC_TO="Script IPC → %s"
    MSG_IPC_PROMPT="¿Instalar hve-ipc? Te permite controlar HVE desde atajos de teclado de Hyprland (toggle system tray, pause, cambiar animaciones, etc.). Sin esto, los atajos no funcionan. [Y/n]"
    MSG_SYMLINK_PROMPT="¿Crear symlink en /usr/local/bin/? Si Hyprland/Noctalia ejecuta hve-ipc desde una ruta fija, el symlink evita tener que configurar el PATH. (necesita sudo) [y/N]"
    MSG_SYMLINK_OK="Symlink → /usr/local/bin/hve-ipc"
    MSG_SYMLINK_FAIL="No se pudo crear el symlink — créalo manualmente:"
    MSG_FIRST_TOGGLE_TO="Script workaround → %s"
    MSG_AUTOSTART_PROMPT="¿Iniciar HVE con el sistema (bandeja)? HVE se ejecuta en segundo plano como icono en la bandeja del sistema para cambiar temas, animaciones, bordes al instante. Se activa via exec-once en hve-settings (Hyprland nativo). Sin autostart, tenés que ejecutar 'hve --tray' manualmente cada vez. [y/N]"
    MSG_AUTOSTART_TO="Autostart → exec-once en hve-settings (Hyprland nativo)"
    MSG_LAUNCH_PROMPT="¿Iniciar HVE ahora? [y/N]"
    MSG_LAUNCHED="HVE iniciado"
    MSG_NOT_FOUND_AT="Binario no encontrado en %s"
    MSG_INSTALLED="HVE instalado!"
    MSG_PATH_WARN="~/.local/bin no está en tu PATH. Agrega esto a tu configuración del shell:"
    MSG_RUN="Ejecutar:   hve"
    MSG_SHORTCUTS="Atajos:    hve-ipc toggle-system"
    MSG_YES="sí"
    MSG_NO="no"
else
    # ── General ──
    MSG_DETECTING_OS="Detecting operating system..."
    MSG_DETECTED="Detected:"
    MSG_UNSUPPORTED="Unsupported distro (%s). Install dependencies manually and re-run."
    MSG_INSTALLING_DEPS="Installing system dependencies..."
    MSG_BATCH_FAIL="Batch install failed — trying individually..."
    MSG_COULD_NOT_INSTALL="Could not install: %s (you may need to install it manually)"
    MSG_FOUND="found"
    MSG_NOT_FOUND="not found — some features may not work"
    MSG_CHECKING_RUST="Checking Rust toolchain..."
    MSG_RUST_OK="Rust %s"
    MSG_RUST_NOT_FOUND="Rust not found — installing rustup..."
    MSG_RUST_INSTALLED="Rust installed: %s"
    MSG_RUST_FAIL="Rust installation failed. Try manually: https://rustup.rs"
    MSG_PREFLIGHT_LUA="Hyprland Lua config detected — HVE will manage hyprland.lua"
    MSG_PREFLIGHT_CONF="Hyprland runs a legacy hyprland.conf and HVE 2 only manages hyprland.lua. Migrate your config before enabling HVE."
    MSG_PREFLIGHT_NONE="No Hyprland config found yet — HVE will generate Lua settings on first run."
    MSG_BUILDING="Building HVE (this may take a while)..."
    MSG_BUILD_DONE="Build complete"
    MSG_INSTALLING="Installing HVE..."
    MSG_BINARY_TO="Binary → %s"
    MSG_ASSETS_TO="Assets → ~/.local/bin/assets/"
    MSG_DESKTOP_TO="Desktop entry → %s (%s)"
    MSG_DETECTING_COLOR="Detecting color tool..."
    MSG_COLOR_TOOL="Color tool: %s"
    MSG_WATCHING="Watching:    %s"
    MSG_IPC_TITLE="Installing IPC script..."
    MSG_IPC_TO="IPC script → %s"
    MSG_IPC_PROMPT="Install hve-ipc? Enables Hyprland keyboard shortcuts to control HVE (toggle system tray, pause, switch animations, etc.). Without this, keybinds won't work. [Y/n]"
    MSG_SYMLINK_PROMPT="Create symlink in /usr/local/bin/? If Hyprland/Noctalia calls hve-ipc from a fixed path, the symlink ensures keybinds work without PATH config. (requires sudo) [y/N]"
    MSG_SYMLINK_OK="Symlink → /usr/local/bin/hve-ipc"
    MSG_SYMLINK_FAIL="Could not create symlink — create it manually:"
    MSG_FIRST_TOGGLE_TO="Toggle workaround script → %s"
    MSG_AUTOSTART_PROMPT="Start HVE on login (system tray)? HVE runs in the background as a tray icon for quick theme, animation, border, and shader switching. Uses exec-once in hve-settings (Hyprland-native). Without autostart, you'll need to run 'hve --tray' manually each session. [y/N]"
    MSG_AUTOSTART_TO="Autostart → exec-once in hve-settings (Hyprland-native)"
    MSG_LAUNCH_PROMPT="Launch HVE now? [y/N]"
    MSG_LAUNCHED="HVE launched"
    MSG_NOT_FOUND_AT="Binary not found at %s"
    MSG_INSTALLED="HVE installed!"
    MSG_PATH_WARN="~/.local/bin is not in your PATH. Add this to your shell config:"
    MSG_RUN="Run:        hve"
    MSG_SHORTCUTS="Shortcuts:  hve-ipc toggle-system"
    MSG_YES="yes"
    MSG_NO="no"
fi

# sprintf helper for messages with arguments
msg_fmt() {
    local fmt="$1"; shift
    # shellcheck disable=SC2059
    printf "$fmt" "$@"
}

# ── Sudo guard ─────────────────────────────────────────────────────────────
check_sudo() {
    if [ "$(id -u)" -eq 0 ]; then
        echo -e "${RED}❌ No ejecutes este script con sudo o como root.${NC}"
        echo "   El script pedirá sudo cuando sea necesario."
        exit 1
    fi
}

# ── Paths ──────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HVE_BIN="$HOME/.local/bin/hve"
HVE_SCRIPTS="$HOME/.local/bin/assets/scripts"
HVE_DESKTOP="$HOME/.local/share/applications/hve.desktop"
HVE_CONFIG_DIR="$HOME/.config/hve"
HVE_CONFIG_JSON="$HVE_CONFIG_DIR/config.json"
HVE_IPC="$HOME/.local/bin/hve-ipc"
HYPR_DIR="$HOME/.config/hypr"

# ── Helpers ────────────────────────────────────────────────────────────────
ok()   { echo -e "${GREEN}✅ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠️  $1${NC}"; }
fail() { echo -e "${RED}❌ $1${NC}"; exit 1; }
info() { echo -e "${CYAN}ℹ️  $1${NC}"; }

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

# ============================================================================
# 0. SUDO GUARD
# ============================================================================
check_sudo

# ============================================================================
# 1. DETECT DISTRO AND PACKAGE MANAGER
# ============================================================================
echo ""
info "$MSG_DETECTING_OS"

DISTRO=""
if [ -f /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    case "$ID" in
        arch|manjaro|endeavouros|artix|cachyos)       DISTRO="arch"    ;;
        debian|ubuntu|pop|linuxmint|elementary)        DISTRO="debian"  ;;
        fedora)                                         DISTRO="fedora"  ;;
        opensuse*|suse)                                 DISTRO="opensuse" ;;
        void)                                           DISTRO="void"    ;;
        gentoo)                                         DISTRO="gentoo"  ;;
        *)                                              DISTRO="unknown" ;;
    esac
    # Fallback: check ID_LIKE for derivatives not in the explicit list
    if [ "$DISTRO" = "unknown" ] && [ -n "${ID_LIKE:-}" ]; then
        case "$ID_LIKE" in
            *arch*)   DISTRO="arch"    ;;
            *debian*) DISTRO="debian"  ;;
            *fedora*) DISTRO="fedora"  ;;
            *suse*)   DISTRO="opensuse" ;;
        esac
    fi
elif [ -f /etc/arch-release ]; then
    DISTRO="arch"
elif [ -f /etc/debian_version ]; then
    DISTRO="debian"
elif [ -f /etc/fedora-release ]; then
    DISTRO="fedora"
elif [ -f /etc/gentoo-release ]; then
    DISTRO="gentoo"
else
    DISTRO="unknown"
fi

ok "$MSG_DETECTED $DISTRO"

# ── Distro-specific configuration ──────────────────────────────────────────
REQUIRED_PKGS=()
INSTALL_CMD=""

case "$DISTRO" in
    arch)
        INSTALL_CMD="pacman -S --noconfirm"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3 glib2 cairo pango)
        ;;
    debian)
        INSTALL_CMD="apt install -y"
        REQUIRED_PKGS=(inotify-tools pkg-config libgtk-3-dev libglib2.0-dev libcairo2-dev libpango1.0-dev)
        ;;
    fedora)
        INSTALL_CMD="dnf install -y"
        REQUIRED_PKGS=(inotify-tools pkgconfig gtk3-devel glib2-devel cairo-devel pango-devel)
        ;;
    opensuse)
        INSTALL_CMD="zypper install -y"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel)
        ;;
    void)
        INSTALL_CMD="xbps-install -y"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel)
        ;;
    gentoo)
        INSTALL_CMD="emerge -qv"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3 glib2 cairo pango)
        ;;
    *)
        fail "$(msg_fmt "$MSG_UNSUPPORTED" "$DISTRO")"
        ;;
esac

# ============================================================================
# 2. INSTALL SYSTEM DEPENDENCIES
# ============================================================================
echo ""
info "$MSG_INSTALLING_DEPS"

if ! sudo $INSTALL_CMD "${REQUIRED_PKGS[@]}" 2>/dev/null; then
    warn "$MSG_BATCH_FAIL"
    for pkg in "${REQUIRED_PKGS[@]}"; do
        if ! sudo $INSTALL_CMD "$pkg" 2>/dev/null; then
            warn "$(msg_fmt "$MSG_COULD_NOT_INSTALL" "$pkg")"
        fi
    done
fi

# Verify key build/run dependencies
for cmd in inotifywait pkg-config; do
    if command -v "$cmd" &>/dev/null; then
        ok "$cmd $MSG_FOUND"
    else
        warn "$cmd $MSG_NOT_FOUND"
    fi
done

# ============================================================================
# 3. VERIFY / INSTALL RUST
# ============================================================================
echo ""
info "$MSG_CHECKING_RUST"

if command -v rustc &>/dev/null; then
    ok "$(msg_fmt "$MSG_RUST_OK" "$(rustc --version)")"
else
    warn "$MSG_RUST_NOT_FOUND"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
    if command -v rustc &>/dev/null; then
        ok "$(msg_fmt "$MSG_RUST_INSTALLED" "$(rustc --version)")"
    else
        fail "$MSG_RUST_FAIL"
    fi
fi

# ============================================================================
# 3b. HYPRLAND LUA PREFLIGHT (HVE 2 is Lua-only)
# ============================================================================
echo ""
if [ -f "$HYPR_DIR/hyprland.lua" ] && grep -qE 'hl\.|require\(' "$HYPR_DIR/hyprland.lua" 2>/dev/null; then
    ok "$MSG_PREFLIGHT_LUA"
elif [ -f "$HYPR_DIR/hyprland.conf" ]; then
    warn "$MSG_PREFLIGHT_CONF"
else
    info "$MSG_PREFLIGHT_NONE"
fi

# ============================================================================
# 4. BUILD
# ============================================================================
echo ""
info "$MSG_BUILDING"
cd "$SCRIPT_DIR"
cargo build --release
ok "$MSG_BUILD_DONE"

# ============================================================================
# 5. INSTALL BINARY + ASSETS + DESKTOP
# ============================================================================
echo ""
info "$MSG_INSTALLING"

# Binary
mkdir -p "$HOME/.local/bin"
cp "$SCRIPT_DIR/target/release/hve" "$HVE_BIN"
chmod +x "$HVE_BIN"
ok "$(msg_fmt "$MSG_BINARY_TO" "$HVE_BIN")"

# Assets
if [ -d "$SCRIPT_DIR/assets" ]; then
    mkdir -p "$HOME/.local/bin/assets"
    for dir in scripts borders shaders fragments animations; do
        if [ -d "$SCRIPT_DIR/assets/$dir" ]; then
            cp -r "$SCRIPT_DIR/assets/$dir" "$HOME/.local/bin/assets/"
        fi
    done
    ok "$MSG_ASSETS_TO"
fi

# Desktop entry
if [ -f "$SCRIPT_DIR/hve.desktop" ]; then
    mkdir -p "$HOME/.local/share/applications"
    cp "$SCRIPT_DIR/hve.desktop" "$HVE_DESKTOP"
    # Use the actual binary path so the launcher doesn't depend on PATH
    sed -i "s|Exec=.*hve|Exec=$HVE_BIN|" "$HVE_DESKTOP"
    ok "$(msg_fmt "$MSG_DESKTOP_TO" "$HVE_DESKTOP" "$HVE_BIN")"
fi

# ============================================================================
# 6. DETECT COLOR TOOL (cosmetic — runtime detection is in color_watcher.sh)
# ============================================================================
echo ""
info "$MSG_DETECTING_COLOR"

DETECTED_TOOL=""
WATCH_TARGET=""

if [ -f "$HYPR_DIR/noctalia/noctalia-colors.lua" ]; then
    DETECTED_TOOL="Noctalia (lua)"
    WATCH_TARGET="$HYPR_DIR/noctalia/noctalia-colors.lua"
elif [ -f "$HOME/.cache/wal/colors.json" ]; then
    DETECTED_TOOL="pywal"
    WATCH_TARGET="$HOME/.cache/wal/colors.json"
elif [ -f "$HOME/.config/matugen/config.toml" ]; then
    DETECTED_TOOL="matugen"
    MATUGEN_OUT=$(grep -A5 'hyprland' "$HOME/.config/matugen/config.toml" 2>/dev/null \
        | grep 'output' | head -1 | sed 's/.*= *//' | tr -d '"' | sed "s|~|$HOME|")
    if [ -n "$MATUGEN_OUT" ] && [ -f "$MATUGEN_OUT" ]; then
        WATCH_TARGET="$MATUGEN_OUT"
    else
        WATCH_TARGET="$HOME/.config/matugen"
    fi
else
    DETECTED_TOOL="manual"
    FOUND=$(grep -rl 'primary\s*=' "$HYPR_DIR" 2>/dev/null | head -1 || true)
    if [ -n "$FOUND" ]; then
        WATCH_TARGET="$FOUND"
    else
        WATCH_TARGET="$HYPR_DIR"
    fi
fi

ok "$(msg_fmt "$MSG_COLOR_TOOL" "$DETECTED_TOOL")"
info "$(msg_fmt "$MSG_WATCHING" "$WATCH_TARGET")"

# ============================================================================
# 7. INSTALL IPC SCRIPT (optional)
# ============================================================================
echo ""
IPC_INSTALLED=false
if prompt_yes_no "$MSG_IPC_PROMPT" "yes"; then
    mkdir -p "$(dirname "$HVE_IPC")"
    cp "$SCRIPT_DIR/assets/scripts/hve-ipc" "$HVE_IPC"
    chmod +x "$HVE_IPC"
    ok "$(msg_fmt "$MSG_IPC_TO" "$HVE_IPC")"
    IPC_INSTALLED=true
fi

# If hve-ipc was installed, offer global symlink (for Hyprland/Noctalia)
if [ "$IPC_INSTALLED" = true ]; then
    if prompt_yes_no "$MSG_SYMLINK_PROMPT" "no"; then
        if sudo ln -sf "$HVE_IPC" /usr/local/bin/hve-ipc 2>/dev/null; then
            ok "$MSG_SYMLINK_OK"
        else
            warn "$MSG_SYMLINK_FAIL"
            info "  sudo ln -sf $HVE_IPC /usr/local/bin/hve-ipc"
        fi
    fi
fi

# ============================================================================
# NOTE: HVE now handles Wayland first-toggle priming internally
#       (show + xdg-shell minimize via winit). No external script needed.
# ============================================================================
# 8. AUTOSTART (optional)
# ============================================================================
echo ""
AUTOSTART_INSTALLED=false
if prompt_yes_no "$MSG_AUTOSTART_PROMPT" "no"; then
    # HVE's set_autostart() writes the Lua autostart block into
    # hve-settings.lua as hl.on("hyprland.start", ...) for `hve --tray`.
    #
    # We write auto_start:true to config.json. On next launch, HVE reads it
    # and calls set_autostart(true), which handles the hve-settings injection.
    mkdir -p "$HVE_CONFIG_DIR"
    if [ -f "$HVE_CONFIG_JSON" ]; then
        # Replace auto_start value (HVE-generated JSON always has it)
        sed -i 's/"auto_start"\s*:\s*\(true\|false\)/"auto_start": true/' "$HVE_CONFIG_JSON" 2>/dev/null || {
            echo '{"config_version":3,"auto_start":true}' > "$HVE_CONFIG_JSON"
        }
    else
        echo '{"config_version":3,"auto_start":true}' > "$HVE_CONFIG_JSON"
    fi
    ok "$MSG_AUTOSTART_TO"
    AUTOSTART_INSTALLED=true
fi

# ============================================================================
# 9. LAUNCH NOW (optional)
# ============================================================================
echo ""
if prompt_yes_no "$MSG_LAUNCH_PROMPT" "no"; then
    if [ -x "$HVE_BIN" ]; then
        "$HVE_BIN" &
        ok "$MSG_LAUNCHED"
    else
        warn "$(msg_fmt "$MSG_NOT_FOUND_AT" "$HVE_BIN")"
    fi
fi

# ============================================================================
# 10. SUMMARY
# ============================================================================
echo ""
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  ✅ $MSG_INSTALLED${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo ""

# Build summary lines from translated templates
echo "   $(msg_fmt "$MSG_BINARY_TO" "$HVE_BIN")"
echo "   $(msg_fmt "$MSG_ASSETS_TO")"
echo "   $(msg_fmt "$MSG_DESKTOP_TO" "$HVE_DESKTOP" "$HVE_BIN")"
if [ "$AUTOSTART_INSTALLED" = true ]; then
    echo "   $MSG_AUTOSTART_TO"
fi
if [ "$IPC_INSTALLED" = true ]; then
    echo "   $(msg_fmt "$MSG_IPC_TO" "$HVE_IPC")"
fi
echo ""
echo "   $MSG_RUN"
if [ "$IPC_INSTALLED" = true ]; then
    echo "   $MSG_SHORTCUTS"
fi
echo ""

# Warn if ~/.local/bin not in PATH
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    warn "$MSG_PATH_WARN"
    echo '  export PATH="$HOME/.local/bin:$PATH"'
fi
