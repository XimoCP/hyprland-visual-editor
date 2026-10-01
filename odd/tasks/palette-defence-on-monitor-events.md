# Palette Defence Anchored on Monitor Events

**Locator**: `odd/tasks/palette-defence-on-monitor-events.md`
**Repo**: `hve` — branch `hve2-visual-rewrite`
**Opened**: 2026-10-01
**Surface**: `src/hypr_ipc.rs` (the Hyprland socket2 listener), `src/color_authority.rs`,
`src/providers/noctalia.rs` (`reassert_colours`), `assets/scripts/color_watcher.sh`

## Objective

While an HVE theme is the active colour authority, an AUTOMATIC palette change (a
monitor coming or going, which makes skwd re-apply the same wallpaper and recompute
the theme) must be corrected back to HVE's palette. A MANUAL change must never be
touched.

## Problem (measured, 2026-10-01)

`skwd-walld` re-applies the theme on every monitor hotplug even when the wallpaper has
not changed (`hotplug.rs:369 start_output_watch` → `restore_last` → `PostCommitStep::Theme`
→ `theme_worker.rs:80`). Its noctalia backend shells out to `noctalia theme <image>`, and
the same image yields a DIFFERENT palette on different runs (measured: `#67abe4` at 21:26
against `#e4aa67` at 04:13 for the same video). So the desktop palette jumps on display
events.

HVE already reacts to palette changes, but it watches the palette FILE and re-asserts on
ANY change (`assets/scripts/color_watcher.sh` → `hve-ipc assert-color-authority` →
`reassert_colours`). That is the wrong anchor: a change the keeper makes on purpose (a new
wallpaper in skwd, a theme setting) rewrites the same file and HVE would revert it.

## Decision (keeper, 2026-10-01)

- **Do NOT touch skwd's code.** The visible jump (skwd applies its palette, HVE puts its
  own back a moment later) is accepted: these are bounded events, not a constant fight.
- The fix lives in HVE only.
- **The file stays the detector; the monitor event becomes the permission.** A palette
  change with no monitor event is presumed legitimate and HVE does nothing.

## Design

1. The Hyprland socket2 listener (`src/hypr_ipc.rs`) gains an arm for `monitoradded` and
   `monitorremoved` (the file already has `spawn_listener` and `spawn_focus_listener` to
   follow, and `connect_and_listen` already branches on `configreloaded`/`fullscreen`).
2. On such an event, arm a short window (skwd's theme step is asynchronous: the theme
   worker runs on its own task, so the palette arrives a moment later).
3. Inside that window, the existing drift check decides: compare the live palette with
   HVE's applied snapshot. Drifted → re-assert ONCE. Not drifted → nothing.
4. Outside that window, a palette change never triggers a re-assert.
5. Agnosticism: the monitor knowledge stays in the Hyprland IPC layer, the palette
   knowledge in the provider. No sealed engine file is touched
   (`engine.rs`, `config.rs`, `settings.rs`, `theme_manager.rs`, `app_state.rs`,
   `watcher.rs`, `utils.rs`).

## Known limits (honest, recorded at open)

- **HVE closed ⇒ no defence.** The listener lives in the app, so an event while HVE is
  closed leaves skwd's palette in place; opening HVE (which applies the active theme)
  restores it. Accepted for this slice.
- The window is a heuristic. A monitor event that produces no palette change is a no-op;
  a palette change caused by something else automatic (rotation, schedule) is NOT covered
  by this slice — that is the follow-up that needs an origin field in skwd's event stream.
- Whether a suspend/resume emits `monitoradded` is unverified. If it does not, that case
  is only covered by opening HVE.

## Tasks

- [x] T1 — Tests first (RED). DONE — 5 tests in `src/hypr_ipc.rs`, RED proven then GREEN,
      and every assertion falsified by a throwaway probe that was reverted.
- [x] T2 — The monitor arm in `src/hypr_ipc.rs`. DONE — `MONITOR_REASSERT_WINDOW = 5 s`,
      one-shot, armed only by `monitoradded`/`monitorremoved` (prefix match covers the `v2`
      spellings). Length justified from skwd's own log (same-second applies; 1198 ms
      slowest measured chain) and matched to the watcher's existing 5 s cooldown.
- [x] T3 — Gate the existing re-assert on the window. DONE — the file watch stays the
      detector; `assert-color-authority` re-asserts only while the window is armed and
      unused. The startup repair keeps its own UNGATED `repair-color-authority` verb, and
      verification proved that verb is reachable ONLY at HVE start (one call site in
      `color_watcher.sh`, never from the change loop), so the gate is not decorative.
- [ ] T4 — **The keeper's live test. STILL PENDING — nothing outside unit tests has ever
      armed that window.** `cargo run` from the repo is enough. Steps: apply an HVE theme,
      tail `~/.cache/hve/color_watcher.log`, unplug then replug a monitor (expect the
      palette to jump and HVE's to return within ~2 s), then change the wallpaper manually
      in skwd (expect HVE to print NO "Colours re-asserted" line and leave it alone).

Commit: `7becc1521e82da0ef2298f78161a5021b0cfe5c1` — 3 files, +203/−6. Full suite
`1206 passed / 0 failed`, build warning-free, `architecture_contract` 7 passed,
`ipc::` 34 passed, `scripts_contract` 28 passed. Cross-model verification: PASS, with the
three limits below.

## Verified limits (cross-model verification, 2026-10-01)

- **A manual change made WITHIN ~5 s of a monitor event WILL be reverted.** The wall-clock
  guarantee is "safe when made more than 5 s after the last monitor event". Inherent to any
  time-window permission; the narrow case is a hotplug whose burst writes no watched file.
- **A multi-pass burst can consume the window without correcting anything**: if a non-palette
  write lands in the watcher's first pass and skwd's palette write lands during that pass,
  the permit is spent and the hijack persists until the next event. Failure direction is
  safe (no manual revert). Measured evidence says bursts are same-second.
- **Suspend/resume is WORSE than this document first claimed.** It said the resume case was
  "covered by opening HVE" — but HVE is ALREADY open when the machine resumes, so if resume
  emits no `monitoradded`, there is no correction path at all for the rest of that session.
  Unverified: whether resume emits it. This is the first thing to check in the live test.
- HVE closed ⇒ no defence (accepted). A slow apply beyond the window is missed (safe
  direction). The wiring line that calls `consume_monitor_permit` has no test of its own
  (3 lines, no `ipc.rs` test drives the gated verb).

## Acceptance criteria

- A monitor event with an HVE theme active restores HVE's palette.
- A manual palette/wallpaper change is never reverted.
- With no HVE theme active (snapshot absent), the defence is a no-op.
- No ping-pong: one re-assert per observed event, never a loop.
- Nothing in skwd is modified.
