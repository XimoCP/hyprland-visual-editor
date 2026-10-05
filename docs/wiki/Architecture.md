# Architecture

HVE is built for Hyprland and nothing replaces it. This page explains the
seams that keep the pieces independent; the fragment/assembly, colour, preset,
background, IPC, tray and watchdog systems are described on their own pages.

## Hyprland is the base, never a backend

HVE is built for Hyprland and nothing replaces it. Compositor-specific
knowledge — the `hyprctl` calls, the reload, the workspace queries — lives
behind the `Composer` seam (`src/composer/mod.rs`), whose only implementation
is `HyprlandComposer` (`src/composer/hyprland.rs`). The rest of the code talks
to `Composer`, never to Hyprland directly.

## Shell agnosticism

HVE is agnostic of the **desktop shell** — the layer that sits on top of
Hyprland (Noctalia, DMS, Calestia, …). It is **not** agnostic of Hyprland:
Hyprland is the base, hence the name. The shell is a separate axis from the
compositor.

This is exactly what is true today, and the tests that keep it true.

**The core never names a shell in its production code.** A core file's
production code never spells a shell name, a shell path, a shell command line
or a shell plugin id (the check strips comments and test fixtures). The core
files are `src/main.rs`, `src/callbacks.rs`, `src/theme_manager.rs`,
`src/config.rs`, `src/watcher.rs`, `src/ipc.rs`, `src/tray.rs`,
`src/engine.rs` and `src/shell/**`.

**Everything the core needs is a capability.** The operations the core needs
from the running shell are methods of `ShellCapabilities`
(`src/providers/shell_capabilities.rs`): the live wallpaper, a theme's saved
wallpaper, whether an apply arms the late palette re-assert, and
Do-Not-Disturb status and set. Every method has a declined default, so a shell
that cannot provide one keeps the neutral path and nothing breaks. The core
resolves the running shell through `providers::active_shell()` and calls only
these methods.

**Adding another shell is one entry.** The registration router
(`src/providers/mod.rs`) carries a data-driven backend list
(`id / detect / fallback / make / declare`). Adding a shipped shell is one
entry plus its provider module, never an `if` in the core.

**The proof is a test.** `src/shell_agnosticism_tests.rs` drives the core end
to end through a test-only fake shell adapter and a fake theme provider:
backfill, palette re-assert arming, DND engage and restore, and
list/save/apply/preview/facts. If adding a new shell ever forces an edit to
those tests, the claim is false.

**The guard is in the build.** `src/architecture_contract.rs` fails the build
when any shell name (`noctalia`, `quickshell`, `calestia`, `dms`) appears in a
core file, and a tripwire fails when a new `src/shell/` module joins the tree
unpoliced.

### What is pluggable

- The colour sources are genuinely pluggable: each source is one module under
  `assets/scripts/color_sources.d/`, discovered from that directory, so adding
  a source is one new file there — `assets/scripts/colors.sh` is the loader
  and is never edited to add one.
- A theme provider can be implemented against `ThemeProvider`
  (`src/theme_manager.rs`) without touching the core.
- The compositor sits behind `Composer` (`src/composer/mod.rs`).
