#!/bin/bash
# ============================================================================
# hve-first-toggle.sh — Workaround for Wayland toggle-tray first-show bug
#
# Bug: When HVE starts in --tray mode under Wayland, the first
# slint::Window::show() does NOT complete the xdg_toplevel roundtrip
# with the compositor. The window() call returns immediately, but the
# surface isn't mapped yet. Result: the first SUPER+H appears to do
# nothing, and the window only appears on the 3rd press.
#
# This script waits for HVE to be ready, then sends 2 extra toggle-tray
# commands to "prime" the Wayland connection:
#   1st toggle: show() → Wayland roundtrip happens now
#   2nd toggle: hide() → back to hidden, ready for real user input
#
# Usage (automatic):
#   HVE's autostart runs this automatically after hve --tray.
#
# Usage (manual, if not using autostart):
#   Add to Hyprland config AFTER hve --tray:
#     exec-once = hve-first-toggle
#   Or add to your window manager's autostart.
# ============================================================================
set -euo pipefail

SOCKET="${XDG_RUNTIME_DIR:-/tmp}/hve.sock"
MAX_WAIT=10
IPC_SCRIPT="hve-ipc"

# ── Wait for HVE to be ready ──
for i in $(seq 1 "$MAX_WAIT"); do
    if [ -S "$SOCKET" ]; then
        break
    fi
    sleep 1
done

if [ ! -S "$SOCKET" ]; then
    echo "[hve-first-toggle] HVE socket not found after ${MAX_WAIT}s — is HVE running?" >&2
    exit 1
fi

# ── Let HVE settle (Wayland roundtrip after socket bind) ──
sleep 0.5

# ── Helper: send toggle-tray ──
send_toggle() {
    if command -v "$IPC_SCRIPT" &>/dev/null; then
        "$IPC_SCRIPT" toggle-tray 2>/dev/null || true
    elif command -v socat &>/dev/null; then
        echo "toggle-tray" | socat - "UNIX-CONNECT:$SOCKET" 2>/dev/null || true
    elif command -v nc &>/dev/null; then
        echo "toggle-tray" | nc -U "$SOCKET" 2>/dev/null || true
    else
        echo "[hve-first-toggle] No IPC method found (hve-ipc/socat/nc)" >&2
        exit 1
    fi
}

# ── Prime the Wayland connection (2 toggles with a small gap) ──
send_toggle
sleep 0.3
send_toggle

echo "[hve-first-toggle] Wayland connection primed — toggle-tray ready" >&2
