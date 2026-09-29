# Closure Record: HVE Capability Routing

**Locator**: `odd/tasks/hve-capability-routing-closure.md`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Closed**: 2026-09-29, by the closing operator of the autonomous night session
**Plan**: `odd/tasks/hve-capability-routing.md`
**Head at closure**: `33c109a` (the plan's last commit; this record is the commit right
after it)
**Checkpoint**: tag `checkpoint/night-2026-09-29-final`, branch `backup/night-2026-09-29-final`

This document records what the plan actually delivered, what it did NOT deliver, and what
only the keeper can check. Nothing here is claimed as verified unless it was verified in
this session by a command whose output is quoted.

## Final evidence (run by the closing operator, 2026-09-29)

```
$ cargo test --quiet 2>&1 | tail -4
....................................................................................... 1131/1182
...................................................
test result: ok. 1182 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 64.43s

$ cargo test --quiet architecture_contract -- --nocapture 2>&1 | grep -E "result:|STALE|NEW COUPLING"
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 1175 filtered out; finished in 0.04s
```

- **Tests**: 1182 passed, 0 failed, 0 ignored.
- **Architecture scanner**: 7 passed, 0 failed. No `STALE PIN` line, no `NEW COUPLING` line.
  The ratchet held: known couplings can only go down, and a stale pin fails the build.
- **Tree**: clean of `src/` edits at both ends of the run (only the pre-existing `.atl/`
  cache and the untracked `odd/consult/` were dirty, both deliberately left alone).

## Commit chain `a5d7d5e` → `33c109a` (15 commits, read from `git log`)

| Commit | One line |
| --- | --- |
| `a5d7d5e` | fix(color): bound the noctalia CLI (10 s deadline) and repair the missing colour authority on startup (Phase 1 residuals R1 + R2) |
| `0e781b8` | feat(providers): the disabled-providers switch becomes real — the registration seam consumes `Config::disabled_providers` |
| `2db3471` | refactor(providers): background info becomes shared; the noctalia → skwd / noctalia → mpvpaper chains route through `background.rs` |
| `5f12434` | refactor(providers): the plugin supervisor (probe, bounce, service clear) moves into the neutral runtime seam, out of `mpvpaper.rs` |
| `00cc76b` | refactor(providers): one plugin probe in the neutral seam — the duplicate `noctalia msg plugins list` parse is deleted |
| `16e5e61` | docs(contract): close the Phase 2 audit findings (spec wording, stale `wallpaper_authority` doc claim, scanner now scans the providers) |
| `da7c548` | refactor(composer): the read-only compositor queries (`query_json`, `read_option`) move behind the `Composer` trait |
| `2c28a0d` | refactor(composer): the compositor commands (`dispatch_script`, `eval_lua`, `reload_config`) move behind the trait; every `hyprctl` spawn in `src/` is now in `composer/hyprland.rs` |
| `7cdd5a1` | refactor(gallery): the dead hyprmod yield scaffolding in `slot.rs` is deleted, with a source guard so it cannot return silently |
| `3e0fcc1` | refactor(theme): Phase 3+4 — deletable-artifact cleanup hook, `preview_sources`, and the `src/theme/desktop_preference/` capability (gsettings/darkman behind a router) |
| `5136c5b` | refactor(providers): the skwd engine config write routes behind the apply-background router; scanner blind spots (providers, gallery, composer module) close |
| `cbd3ec9` | refactor(gallery): the preview source becomes a declared capability (`ThemeProvider::preview_sources`), gallery pins for noctalia/mpvpaper fall to 0 |
| `1bef896` | test(shell): the CLI-spawning shell tests take the shared env lock — fixes a real flake (0 failures in 10 runs after) |
| `ce96355` | docs(contract): the reload path is one pipeline, not a chain; re-homes the last chain item with the spec argument, and fixes the bare `hyprctl reload` in `shader.sh` when `assemble.sh` is missing |
| `33c109a` | docs(odd): close every phase of the capability-routing plan — the plan document records what closed and what was deliberately re-scoped |

## What closed, per phase (one line each)

- **Phase 0 — contract written down** (`7193c03`): the capability vocabulary and backend
  contract live in `openspec/specs/capability-routing/spec.md`, pinned by a test that fails
  on growth and on stale pins.
- **Phase 1 — colour authority** (`9f93082`, `b8e16a7`..`0f0ee56`, plus `a5d7d5e`): the
  applied theme owns its palette through a declarative descriptor, the central scripts only
  read it, the re-assert is executed by the provider, the five-source chain became a loader
  over declared colour modules, the cross-fill paths are gone, and residuals R1 (unbounded
  CLI) and R2 (missing descriptor on upgrade) are fixed.
- **Phase 2 — no backend calls another backend** (`0e781b8`..`5136c5b`): the disabled-providers
  switch is real, `apply-background` routes through `background.rs`, the skwd adapter, the
  shared background info and the Noctalia plugin supervisor each live with their owner, the
  colour-authority yield moved behind the same router, and the scanner learned to see the
  providers so the debt ratchets down.
- **Phase 3 — the Composer seam finished** (`da7c548`, `2c28a0d`, `cbd3ec9`, part of `3e0fcc1`):
  compositor queries and commands both go through the trait, `settings.rs` reloads through
  `Composer::reload_config`, the gallery previews through `ThemeProvider::preview_sources`,
  and every `Command::new("hyprctl")` in `src/` is inside `composer/hyprland.rs`.
- **Phase 4 — remaining leaks** (`3e0fcc1`, `7cdd5a1`): gsettings/darkman moved behind
  `read-desktop-preference`, `config_guard.rs` was read and found to be a pure predicate over
  file content (named honestly, not re-homed), and the dead `hyprmod` scaffolding was deleted.
- **Close-out docs** (`16e5e61`, `ce96355`, `33c109a`): the audit findings, the reload-pipeline
  argument and the plan's own status were brought in line with the code.

## What did NOT close (named plainly)

1. **`src/hypr_ipc.rs` event renaming** — the events are not renamed to capability vocabulary.
   The transport is already encapsulated and its `hyprland` tokens are env/log strings and
   comments (pin `hypr_ipc.rs/hyprland = 9`). This is polish, not a leak.
2. **`init.sh` one-shot reloads** — `assets/scripts/init.sh` still issues its own
   `hyprctl reload` at lines 146 and 162 instead of going through the coalescer.
3. **Uncalled `reload_command` in `src/providers/shell.rs`** — the trait method is declared
   (line 25) and implemented (lines 117, 228) but has no production caller; only tests call it.
   Pinned as known debt (`shell.rs/hyprctl = 1`).
4. **`assemble.sh` / `reload_coalescer.sh` are not in the scanner's file list** —
   `SCANNED_FILES` in `src/architecture_contract.rs` contains `assets/scripts/colors.sh` and
   `assets/scripts/color_watcher.sh` only. A new coupling introduced in those two scripts
   would not be caught by the architecture test.
5. **The `wallpaper_authority.rs` description still reads as an open `Composer` violation** —
   the spec's "Known debt" paragraph (`openspec/specs/capability-routing/spec.md:33-38`) and
   the scanner comment (`src/architecture_contract.rs:298`) still say "raw `hyprctl` outside
   `Composer`", while the scanner pins it at a measured 2: it is accepted pinned debt, and the
   wording has not been updated to say so.
6. **No live-desktop verification in this session** — nothing on the running desktop was
   exercised here. The suite, the scanner and the headless renders are all this session ran.

## What only the keeper can verify

- **The live desktop**: the palette surviving suspend was verified by the keeper earlier in
  the work; **nothing after that point has been checked live**. This session ran no desktop
  session, no `hyprctl` round-trip against a real compositor, and no reinstall.
- **The visual look**: no render inspection of the current head belongs to this session.
- **Real-engine interaction**: every path that talks to Hyprland, Noctalia, skwd or mpvpaper
  was exercised only through tests and stubs. The real compositor, the real layer stack and
  the real backend CLIs are unverified here.
- After reinstall, the keeper must apply the theme once (Phase 1 residual R2's upgrade
  window is fixed at startup, but only a live run proves it).

## Safety net

| What | Where |
| --- | --- |
| Checkpoint tag (start of the night) | `checkpoint/night-2026-09-29` → `backup/night-2026-09-29` |
| Checkpoint tag (final, this closure) | `checkpoint/night-2026-09-29-final` → `backup/night-2026-09-29-final` → `33c109a` |
| Bundle, start of the night | `/tmp/opencode/hve-night-2026-09-29.bundle` (9 050 472 bytes, 05:06) |
| Bundle, final | `/tmp/opencode/hve-night-2026-09-29-final.bundle` — created from `hve2-visual-rewrite` after this record was committed and verified: complete history (`git bundle verify` OK) |

The final tag points at `33c109a` (the plan's own closing commit, the last commit before
this record was written); this record is the commit after it. Both are inside the final
bundle.

Earlier standing safety tags (`backup-stable`, `pre-cleanup`, `pre-hy3-snapshot`,
`pre-winit-minimize`, `v1.0.1`) and backup branches (`backup/hve2-stable`,
`backup/panel-engagement-ux`, `backup/pre-settings-two-col`) were not touched.

Nothing was pushed: this repo has no remote.
