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

# ── Paths ──────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HVE_BIN="$HOME/.local/bin/hve"
HVE_SCRIPTS="$HOME/.local/bin/assets/scripts"
HVE_DESKTOP="$HOME/.local/share/applications/hve.desktop"
HVE_AUTOSTART_DIR="$HOME/.config/autostart"
HVE_AUTOSTART="$HVE_AUTOSTART_DIR/hve.desktop"
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
# 1. DETECT DISTRO AND PACKAGE MANAGER
# ============================================================================
echo ""
info "Detecting operating system..."

DISTRO=""
if [ -f /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    case "$ID" in
        arch|manjaro|endeavouros|artix)        DISTRO="arch"    ;;
        debian|ubuntu|pop|linuxmint|elementary) DISTRO="debian"  ;;
        fedora)                                 DISTRO="fedora"  ;;
        opensuse*|suse)                         DISTRO="opensuse" ;;
        void)                                   DISTRO="void"    ;;
        gentoo)                                 DISTRO="gentoo"  ;;
        *)                                      DISTRO="unknown" ;;
    esac
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

ok "Detected: $DISTRO"

# ── Distro-specific configuration ──────────────────────────────────────────
REQUIRED_PKGS=()
NETCAT_PKG=""
INSTALL_CMD=""

case "$DISTRO" in
    arch)
        INSTALL_CMD="pacman -S --noconfirm"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3 glib2 cairo pango)
        NETCAT_PKG="openbsd-netcat"
        ;;
    debian)
        INSTALL_CMD="apt install -y"
        REQUIRED_PKGS=(inotify-tools pkg-config libgtk-3-dev libglib2.0-dev libcairo2-dev libpango1.0-dev)
        NETCAT_PKG="netcat-openbsd"
        ;;
    fedora)
        INSTALL_CMD="dnf install -y"
        REQUIRED_PKGS=(inotify-tools pkgconfig gtk3-devel glib2-devel cairo-devel pango-devel)
        NETCAT_PKG="nc"
        ;;
    opensuse)
        INSTALL_CMD="zypper install -y"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel)
        NETCAT_PKG="netcat"
        ;;
    void)
        INSTALL_CMD="xbps-install -y"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel)
        NETCAT_PKG="netcat"
        ;;
    gentoo)
        INSTALL_CMD="emerge -qv"
        REQUIRED_PKGS=(inotify-tools pkg-config gtk3 glib2 cairo pango)
        NETCAT_PKG="netcat"
        ;;
    *)
        fail "Unsupported distro ($DISTRO). Install dependencies manually and re-run."
        ;;
esac

# ============================================================================
# 2. INSTALL SYSTEM DEPENDENCIES
# ============================================================================
echo ""
info "Installing system dependencies..."

if ! sudo $INSTALL_CMD "${REQUIRED_PKGS[@]}" 2>/dev/null; then
    warn "Batch install failed — trying individually..."
    for pkg in "${REQUIRED_PKGS[@]}"; do
        if ! sudo $INSTALL_CMD "$pkg" 2>/dev/null; then
            warn "Could not install: $pkg (you may need to install it manually)"
        fi
    done
fi

# Optional: netcat (for hve-ipc)
if [ -n "$NETCAT_PKG" ] && ! command -v nc &>/dev/null; then
    sudo $INSTALL_CMD "$NETCAT_PKG" 2>/dev/null && ok "netcat installed" || \
        warn "netcat not available — hve-ipc needs nc installed separately"
fi

# Verify key build/run dependencies
for cmd in inotifywait pkg-config; do
    if command -v "$cmd" &>/dev/null; then
        ok "$cmd found"
    else
        warn "$cmd not found — some features may not work"
    fi
done

# ============================================================================
# 3. VERIFY / INSTALL RUST
# ============================================================================
echo ""
info "Checking Rust toolchain..."

if command -v rustc &>/dev/null; then
    ok "Rust $(rustc --version)"
else
    warn "Rust not found — installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
    if command -v rustc &>/dev/null; then
        ok "Rust installed: $(rustc --version)"
    else
        fail "Rust installation failed. Try manually: https://rustup.rs"
    fi
fi

# ============================================================================
# 4. BUILD
# ============================================================================
echo ""
info "Building HVE (this may take a while)..."
cd "$SCRIPT_DIR"
cargo build --release
ok "Build complete"

# ============================================================================
# 5. INSTALL BINARY + ASSETS + DESKTOP
# ============================================================================
echo ""
info "Installing HVE..."

# Binary
mkdir -p "$HOME/.local/bin"
cp "$SCRIPT_DIR/target/release/hve" "$HVE_BIN"
chmod +x "$HVE_BIN"
ok "Binary → $HVE_BIN"

# Assets
if [ -d "$SCRIPT_DIR/assets" ]; then
    mkdir -p "$HOME/.local/bin/assets"
    for dir in scripts borders shaders fragments animations; do
        if [ -d "$SCRIPT_DIR/assets/$dir" ]; then
            cp -r "$SCRIPT_DIR/assets/$dir" "$HOME/.local/bin/assets/"
        fi
    done
    ok "Assets → ~/.local/bin/assets/"
fi

# Desktop entry
if [ -f "$SCRIPT_DIR/hve.desktop" ]; then
    mkdir -p "$HOME/.local/share/applications"
    cp "$SCRIPT_DIR/hve.desktop" "$HVE_DESKTOP"
    ok "Desktop entry → $HVE_DESKTOP"
fi

# ============================================================================
# 6. DETECT COLOR TOOL (cosmetic — runtime detection is in color_watcher.sh)
# ============================================================================
echo ""
info "Detecting color tool..."

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

ok "Color tool: $DETECTED_TOOL"
info "Watching:    $WATCH_TARGET"

# ============================================================================
# 7. INSTALL IPC SCRIPT (optional)
# ============================================================================
echo ""
IPC_INSTALLED=false
if prompt_yes_no "¿Instalar script hve-ipc para atajos de teclado? [Y/n]" "yes"; then
    mkdir -p "$(dirname "$HVE_IPC")"
    cat > "$HVE_IPC" << 'IPC_EOF'
#!/bin/bash
# HVE IPC — sends commands to the HVE UNIX socket
SOCKET="${XDG_RUNTIME_DIR:-/tmp}/hve.sock"
if [ ! -S "$SOCKET" ]; then
    echo "HVE no está corriendo" >&2
    exit 1
fi
echo "$@" | nc -U "$SOCKET"
IPC_EOF
    chmod +x "$HVE_IPC"
    ok "IPC script → $HVE_IPC"
    IPC_INSTALLED=true

    if ! command -v nc &>/dev/null; then
        warn "nc not found — hve-ipc needs netcat (nc) to work"
    fi
fi

# ============================================================================
# 8. AUTOSTART (optional)
# ============================================================================
echo ""
AUTOSTART_INSTALLED=false
if prompt_yes_no "¿Agregar al autostart (inicia con --tray)? [y/N]" "no"; then
    mkdir -p "$HVE_AUTOSTART_DIR"
    cat > "$HVE_AUTOSTART" << 'AUTOSTART_EOF'
[Desktop Entry]
Type=Application
Name=HVE (System Tray)
Exec=hve --tray
Hidden=false
X-GNOME-Autostart-enabled=true
AUTOSTART_EOF
    ok "Autostart → $HVE_AUTOSTART"
    AUTOSTART_INSTALLED=true
fi

# ============================================================================
# 9. LAUNCH NOW (optional)
# ============================================================================
echo ""
if prompt_yes_no "¿Iniciar HVE ahora? [y/N]" "no"; then
    if [ -x "$HVE_BIN" ]; then
        "$HVE_BIN" &
        ok "HVE iniciado"
    else
        warn "Binary not found at $HVE_BIN"
    fi
fi

# ============================================================================
# 10. SUMMARY
# ============================================================================
echo ""
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  ✅ HVE installed!${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo ""
echo "   Binary:    $HVE_BIN"
echo "   Assets:    ~/.local/bin/assets/"
echo "   Desktop:   $HVE_DESKTOP"
if [ "$AUTOSTART_INSTALLED" = true ]; then
    echo "   Autostart: $HVE_AUTOSTART"
fi
if [ "$IPC_INSTALLED" = true ]; then
    echo "   IPC:       $HVE_IPC"
fi
echo ""
echo "   Run:       hve"
if [ "$IPC_INSTALLED" = true ]; then
    echo "   Atajos:    hve-ipc toggle-system"
fi
echo ""

# Warn if ~/.local/bin not in PATH
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    warn '~/.local/bin is not in PATH. Add this to your shell config:'
    echo '  export PATH="$HOME/.local/bin:$PATH"'
fi
