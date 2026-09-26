#!/usr/bin/env python3
"""hve-sentinel - the "bar above HVE" catcher.

WHY THIS EXISTS
---------------
The theme transition is a cascade of timers (T=0 mask, T=1900 fullscreen,
T=2470 restore...). When the compositor is slow (a fractional-scale monitor, a
page of reloads, a loaded machine) a timer fires before the compositor is
ready, HVE does not end up immersive, and the shell bar stays ABOVE the window.
It is intermittent, so eyeballing it has been the only tool. This is the
instrument that finally catches it.

THE LESSON IT IS BUILT ON (the v2 probe failed here)
----------------------------------------------------
The previous probe polled `hyprctl` 4x/second from a visible terminal. Each
poll made the terminal the active window, HVE lost focus and hid itself
(its contract: focus lost -> special:minimized), so the act of measuring
removed the thing being measured. This sentinel therefore:

  1. LISTENS to Hyprland's event socket. Push, not poll. Listening takes no
     focus away: the compositor tells us, we never ask.
  2. ASKS ONLY WHEN AN EVENT JUSTIFIES IT - one hidden `hyprctl clients -j`
     when a relevant event lands, never on a timer.
  3. HIDES nothing and opens NO window. It runs detached, so its own process
     never competes for focus.

WHAT IT RECORDS
---------------
Only around relevant events (theme transition, config reload, workspace and
fullscreen changes). Between events: silence, zero cost. The log is bounded:
it rotates and keeps the most recent lines only.

READING IT
----------
    python3 hve-sentinel.py --report        # the suspicious moments, summarised

The KEEPER is the sensor: apply a theme, and when the bar stays above HVE, say
so. This file holds the compositor's own state at that moment, which HVE itself
cannot know (HVE does not know whether the bar is above it - the compositor
does).
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import threading
import time
from collections import deque

# ── Configuration ──────────────────────────────────────────────────────────
LOG_PATH = os.environ.get("HVE_SENTINEL_LOG", os.path.expanduser("~/.cache/hve/sentinel.log"))
MAX_LOG_LINES = 4000          # bounded: rotate at this size, keep the tail
CHECK_QUIET_MS = 1200         # after an event, wait this long before the ONE check
HVE_TITLE = "Hyprland Visual Editor"

# Event names worth reacting to. Everything else is ignored (zero cost).
INTERESTING = {
    "configreloaded",
    "workspace", "workspacev2",
    "fullscreen",
    "activewindow", "activewindowv2",
    "activespecial", "activespecialv2",
    "openwindow", "closewindow",
    "focusedmon", "focusedmonv2",
    "changefloatingmode",
}


def hyprland_signature():
    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    if sig:
        return sig
    root = "/run/user/%d/hypr" % os.getuid()
    try:
        for name in sorted(os.listdir(root)):
            if os.path.exists(os.path.join(root, name, ".socket2.sock")):
                return name
    except OSError:
        pass
    return None


SIG = hyprland_signature()
if not SIG:
    sys.stderr.write("hve-sentinel: no Hyprland instance found\n")
    sys.exit(2)
ENV = dict(os.environ, HYPRLAND_INSTANCE_SIGNATURE=SIG)
SOCK_PATH = "/run/user/%d/hypr/%s/.socket2.sock" % (os.getuid(), SIG)

log_lock = threading.Lock()


def log(line):
    with log_lock:
        with open(LOG_PATH, "a", buffering=1) as fh:
            fh.write(line.rstrip("\n") + "\n")
        rotate_if_needed()


def rotate_if_needed():
    """Keep the log bounded: never let it grow without limit."""
    try:
        with open(LOG_PATH, "r") as fh:
            lines = fh.readlines()
        if len(lines) > MAX_LOG_LINES:
            keep = lines[-MAX_LOG_LINES // 2:]
            with open(LOG_PATH + ".tmp", "w") as fh:
                fh.write("# rotated at %s\n" % time.strftime("%H:%M:%S"))
                fh.writelines(keep)
            os.replace(LOG_PATH + ".tmp", LOG_PATH)
    except OSError:
        pass


def hypr(*args):
    """One hidden compositor query. Called ONLY when an event justifies it."""
    try:
        out = subprocess.run(
            ["hyprctl", *args], capture_output=True, text=True, env=ENV, timeout=3
        )
        return out.stdout
    except Exception:
        return ""


def snapshot_hve():
    """The state of the HVE window as the compositor sees it right now."""
    try:
        clients = json.loads(hypr("clients", "-j") or "[]")
    except Exception:
        clients = []
    for c in clients:
        if (c.get("title") or "") == HVE_TITLE:
            ws = (c.get("workspace") or {}).get("name", "?")
            return {
                "found": True,
                "fullscreen": c.get("fullscreen", -1),
                "ws": ws,
                "size": c.get("size"),
                "at": c.get("at"),
                "monitor": c.get("monitor"),
                # The probe's definition: HVE visible but NOT immersive.
                "bar_above": (not ws.startswith("special")) and c.get("fullscreen", -1) != 2,
            }
    return {"found": False}


def monitor_of_hve():
    """Which monitor HVE is on, with its scale - the fractional-scale clue."""
    try:
        for m in json.loads(hypr("monitors", "-j") or "[]"):
            yield m
    except Exception:
        return


def describe_monitors():
    parts = []
    for m in monitor_of_hve():
        parts.append(
            "%s %sx%s@%s scale=%s pos=%s,%s"
            % (m.get("name"), m.get("width"), m.get("height"),
               m.get("refreshRate"), m.get("scale"), m.get("x"), m.get("y"))
        )
    return "; ".join(parts) if parts else "?"


# ── The single deferred check ──────────────────────────────────────────────
pending = threading.Timer(0, lambda: None)
pending_lock = threading.Lock()


def schedule_check(reason):
    """After the quiet window, take ONE snapshot and judge it.

    One timer, always replaced: a burst of events collapses into one check.
    """
    global pending

    def run():
        snap = snapshot_hve()
        monitors = describe_monitors()
        if snap.get("bar_above"):
            log("SUSPECT  reason=%s  HVE fullscreen=%s ws=%s monitor=%s size=%s at=%s  | monitors: %s"
                % (reason, snap.get("fullscreen"), snap.get("ws"), snap.get("monitor"),
                   snap.get("size"), snap.get("at"), monitors))
        else:
            log("ok        reason=%-16s HVE found=%s fullscreen=%s ws=%s"
                % (reason, snap.get("found"), snap.get("fullscreen"), snap.get("ws")))

    with pending_lock:
        try:
            pending.cancel()
        except Exception:
            pass
        pending = threading.Timer(CHECK_QUIET_MS / 1000.0, run)
        pending.daemon = True
        pending.start()


def event_reader():
    while True:
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.connect(SOCK_PATH)
        except OSError:
            time.sleep(0.5)
            continue
        log("# connected %s" % time.strftime("%H:%M:%S"))
        f = s.makefile("rb")
        try:
            while True:
                line = f.readline()
                if not line:
                    break
                text = line.decode("utf-8", "replace").strip()
                if not text:
                    continue
                name, _, payload = text.partition(">>")
                if name not in INTERESTING:
                    continue
                log("%s\t%s\t%s" % (time.strftime("%H:%M:%S"), name, payload))
                schedule_check(name)
        except OSError:
            pass
        finally:
            try:
                f.close()
                s.close()
            except OSError:
                pass
        time.sleep(0.2)


# ── Report ─────────────────────────────────────────────────────────────────
def report():
    if not os.path.exists(LOG_PATH):
        print("no sentinel log at %s" % LOG_PATH)
        return
    suspects = []
    total = 0
    with open(LOG_PATH) as fh:
        for line in fh:
            if line.startswith("SUSPECT"):
                suspects.append(line.rstrip())
            elif line.startswith("ok ") or line.startswith("ok\t"):
                total += 1
    print("=== hve-sentinel report ===")
    print("log:        %s" % LOG_PATH)
    print("checks:     %d" % (total + len(suspects)))
    print("suspects:   %d" % len(suspects))
    for s in suspects[-20:]:
        print("  " + s)
    if not suspects:
        print("  (none - HVE ended immersive at every check)")


def main():
    ap = argparse.ArgumentParser(description="HVE bar-above sentinel")
    ap.add_argument("--report", action="store_true", help="print the suspects and exit")
    ap.add_argument("--tail", type=int, default=0, help="print the last N log lines and exit")
    args = ap.parse_args()

    if args.report:
        report()
        return
    if args.tail:
        with open(LOG_PATH) as fh:
            for line in deque(fh, args.tail):
                print(line.rstrip())
        return

    os.makedirs(os.path.dirname(LOG_PATH), exist_ok=True)
    log("# hve-sentinel started pid=%d sig=%s" % (os.getpid(), SIG))
    sys.stderr.write("hve-sentinel: listening on %s (log %s)\n" % (SOCK_PATH, LOG_PATH))
    event_reader()


if __name__ == "__main__":
    main()
