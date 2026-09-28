# Feature: HVE routes capabilities to independent backends

**Locator**: `odd/tasks/hve-capability-routing.md`
**Engram mirror**: topic `odd/hve-capability-routing/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Measured**: 2026-09-28 by two independent read-only audits (Rust core; scripts + installer + config).
**Status**: IN PROGRESS — Phase 1 done (every unit committed and green); the whole-phase
adversarial verification was running when this was written. Phases 2-4 pending.
**Night-shift configuration (decided by the keeper)**: writer `opencode-go/mimo-v2.6-flash`
(paid, $0.14/$0.28 per M tokens — no free-tier quota); verifier `opencode-go/glm-5.3-flash`;
relay on a provider error `ollama-cloud/deepseek-v4.1-flash` (one retry, recorded). Phase 0-1 were
written by the free model (`opencode/muse-spark-1.3-contributor-free`) with GLM verification.

## Run ledger — unit, commit, state

| Unit | What landed | Commit | State |
| --- | --- | --- | --- |
| pre | The applied theme owns the palette: declarative descriptor + scripts + provider re-assert (the fix that started this) | `9f93082` | verified PASS; its six findings closed in 1e |
| 0 | The capability contract (`openspec/specs/capability-routing/spec.md`) + the pinned architecture test | `7193c03` | done |
| 1a | Colour-authority descriptor: created, updated, cleared (nothing read it yet) | `b8e16a7` | done |
| 1b | The central scripts read the descriptor; the backend layout leaves `colors.sh` | `cd32c8a` | done |
| 1c1 | Provider seam `reassert_colours` + the `assert-color-authority` verb | `899b69d` | done |
| 1c2 | Noctalia implements the re-assert (pure drift decision + existing helpers) | `caa171a` | done |
| 1c3 | The watcher asks the app; bash stops driving the backend | `939073c` | done |
| 1d1 | Colour sources become modules behind a loader (pywal first) | `b5ad3bc` | done |
| 1d2 | Noctalia's two colour steps become modules | `beae4d1` | done |
| 1d3 | matugen and the manual scan become modules | `fb78098` | done |
| 1d4 | Modules declare their refresh; the watcher only routes it | `e1379c4` | done |
| 1e | Verification findings: F3/F4/F5 closed, F1 pinned, F2/F6 moot | `84b1e53` | done |
| 1f | No integration fills another one's gap (tertiary hook, kitty path deleted) | `0f0ee56` | done |
| — | Whole-phase adversarial verification (GLM-5.3-Flash) | `0f0ee56` | **PASS** — 1085/0, 0 warnings, no blocker or critical; 6 warnings + 4 suggestions recorded below as residuals |

**Phase verification, in one paragraph**: every claim was confirmed adversarially — the migrated
sources behave identically to the base (order, fall-through, watch set and log lines, proven by
side-by-side sandboxes), the descriptor resisted every attack tried (absolute, relative, `..`,
directory, missing, malformed, stale theme, symlinked file and directory, hostile palette names)
with nothing read or written outside the intended tree, the IPC verb is idempotent and panic-free,
the pins recount exactly, the kitty cross-fill is gone and the v4 fill still works through the hook.

**Residuals — close R1-R3 before Phase 2, the rest with the phase that owns them**:

- **R1 (warning, availability).** `assert-color-authority` runs up to three `noctalia msg` calls with
  no timeout while holding the IPC accept thread, so a wedged CLI stalls the whole IPC surface *and*
  the watcher loop. The pattern pre-exists (`refresh-theme` does the same with `get_colors.sh`), but
  this verb extends it to a third-party CLI: move the re-assert off the accept path and bound the child.
- **R2 (warning, upgrade window).** A theme applied *before* this phase has no descriptor, so it has
  no declared colour authority until it is applied again. **This affects the keeper's live test:
  after reinstalling, apply the theme once.** A small startup fix (write the descriptor from the
  existing applied-theme state) closes the window for good.
- **R3 (doc).** `openspec/specs/capability-routing/spec.md` still carries the pre-Phase-1 known-debt
  paragraph (the five-source chain, the hardcoded watch paths) which is now false. Correct it.
- **R4 (warning).** The bash descriptor reader accepts unknown fields (a 50 MB extra field was
  honoured); the Rust reader rejects them. Make both readers agree, and cap the read.
- **R5 (warning).** The listing probes spawn `bash -c` without closing fd 9. Short-lived, but a hung
  probe would hold the singleton lock past the parent's death — the defect class the guard comment
  describes.
- **R6 (warning, test blind spots).** The pin test's tokens lack `kitty`, `pywal`, `wal`, `dank`, and
  its file list omits `get_colors.sh`, `assemble.sh`, `src/ipc.rs`, `src/watcher.rs`. Worse: a
  *file-format* coupling is invisible to any name scan.
- **R7 (the honest finish-line caveat).** Phase 1 achieved "no integration *names*", not "no
  integration *knowledge*": `colors.sh` still hardcodes the Noctalia M3 palette shape for the
  priority-10 snapshot step (`dark` / `mPrimary`…), and `color_watcher.sh` names `manual_hypr.sh` for
  the empty-list fallback. Both are candidates for the loader/contract work in Phase 2.
- **R8 (suggestions).** A module whose `try` calls `exit` aborts `get_colors.sh`; the watcher logs
  "Asked HVE to re-assert" even when the ask failed and stamps the cooldown before the `-x` guard;
  an apply failing between the yield and the descriptor write leaves the theme with no declared
  authority; renaming the applied theme clears the descriptor instead of rewriting it.

**Architecture pins after Phase 1** (the test fails on growth and on stale pins, so the debt can
only ratchet down): `colors.sh` noctalia 43→4, matugen 11→2, hyprland 3→0, quickshell 2→0,
skwd 2→0; `color_watcher.sh` noctalia 54→1, matugen 7→1, hyprland 2→1. Every remaining mention in
both scripts is a comment.

**Intended behaviour change of Phase 1** (the only one): a scheme whose palette brings no tertiary
no longer borrows kitty's colour; it gets HVE's documented fallback unless its own backend supplies
one through the new hook.

**Pending when this was written**: (1) the phase verdict — record it here, then commit this
document; (2) the keeper's live test, which needs a reinstall (the repo does not touch the running
desktop); (3) `hve_watchdog.sh` still uses the old cache-dir default (flagged by 1e, follow-up);
(4) Phases 2-4, starting with turning `disabled_providers` into a real switch and then breaking the
`noctalia → skwd` and `noctalia ↔ mpvpaper` chains one short stretch at a time.
**Rule (the keeper's contract)**: HVE adapts capabilities, it does not force-unify configuration
systems. The core says *what* it wants; a router sends each capability to an independent backend.
A backend never calls another backend. **Hyprland is the base, not a backend**: HVE is the
Hyprland Visual Editor and speaks Hyprland natively; what must stay interchangeable are the pieces
mounted on top of it — the shells (Noctalia, Dank Material, …), the colour tools and the wallpaper
engines.

## Verdict — how agnostic HVE is today (measured, not guessed)

Two good seams already exist and are worth keeping:

- `ThemeProvider` + `ProviderCapabilities` — `src/theme_manager.rs:7-16,23-64`, registered at
  `src/providers/mod.rs:19-34`.
- `Composer` — `src/composer/mod.rs:75-117`, one implementation `HyprlandComposer`
  (`src/composer/hyprland.rs`), selected by hand at `src/main.rs:4707`.

But **only one capability actually flows through its seam end to end**: window state
(show/hide/focus/fullscreen/tray) via `Composer`. Everything else either hardcodes a concrete
integration in a core file, or is a chain of adapters where A calls B:

- `hyprctl` outside `Composer`: **8 production sites** — `src/settings.rs:23`, `src/main.rs:1179,
  1412, 2125, 2138, 2189, 2204`, `src/providers/shell.rs:118`. The completed compositor change's
  own invariant ("`grep -r hyprctl src/` must match only `src/composer/hyprland.rs`", its
  `design.md:117,128`) is already violated by this tree.
- `noctalia` in core: **~14 production references** — `src/main.rs:535,699,2232,2240`,
  `src/theme_manager.rs:650,661`, `src/shell/gallery/thumbs.rs:325,356,365,371`.
- Chains (the forbidden pattern): `providers/noctalia.rs:179-225` calls the skwd engine;
  `noctalia.rs:1600,1608` drives mpvpaper; `providers/mpvpaper.rs:207-208,519` calls back into
  Noctalia (**bidirectional**); `noctalia.rs:1052-1060,1430-1458` reaches back into Noctalia from
  the skwd flow. In bash: `color_watcher.sh:239-265` runs `noctalia templates-apply` → `assemble.sh`
  → `hve_reload_queue` → `hyprctl reload`, four adapters in sequence.
- The colour pipeline is the worst single case: `colors.sh:558-563` chains five sources
  (`theme snapshot → noctalia palette → noctalia lua → pywal → matugen → manual lua`) and even
  lets one integration fill another's gap (Noctalia v4 → v5 tertiary, `colors.sh:400-419`; kitty
  → Noctalia palette, `colors.sh:421-434`). `color_watcher.sh:31-94` hardcodes every watch path.
  A new shell today = editing `colors.sh` + `color_watcher.sh`.
- `disabled_providers` (`src/config.rs:45-46`) is persisted and **has no consumer anywhere** —
  the one "switch a backend off" knob is dead.
- Other independent leaks: `src/theme.rs:73,83` (gsettings / darkman for the system theme),
  `src/hypr_ipc.rs:34,41` (Hyprland event socket) and `src/config_guard.rs:80-98` (reads
  `hyprland.lua` to gate every mutation — Hyprland-native by nature, but unnamed).
- The repo already *declares* the intent — `README.md:7,645`, `WIKI.md:7,669`
  ("agnóstico al escritorio") — but no capability/router contract exists anywhere, and
  `openspec/specs/composer/spec.md:5,9-13` still frames Composer as "abstracts the compositor
  implementation", which contradicts the corrected contract.

**One-line verdict**: the seams exist; the wiring ignores them. HVE is agnostic in its README,
not in its structure.

## Definition of done (measurable)

A new shell/colour tool can be supported by **adding files under its own backend directory** —
never by editing `src/main.rs`, `src/settings.rs`, `src/theme_manager.rs`, `colors.sh` or
`color_watcher.sh`. Pinned by an architecture test that fails when a known integration name
appears in a core file outside its backend module.

## Phases (each phase approved by the keeper; small units; TDD; cross-model verification)

### Phase 0 — write the contract down (no code)
- The capability vocabulary (one verb per capability the core may want): `window-state`,
  `reload-config`, `read-config-option`/`write-config-option`, `apply-border|animation|shader|
  geometry`, `set-colours`, `apply-background`, `read-desktop-preference`, `yield-to-another-app`,
  `read-preview-source`.
- The backend contract: id, capabilities it declares, the files it owns, its watch paths, its
  re-assert/reload action. No backend may call another backend.
- Correct the Composer wording: Hyprland is encapsulated, not interchangeable.

### Phase 1 — colour authority, as the proof of pattern (the case that is biting now)
- Re-home the just-landed palette fix: the applied theme's provider **writes a declarative
  descriptor** (which palette file, which name, which backend) and the central scripts only read
  it; the re-assert is executed by the provider (Rust, where `noctalia.rs:1653-1684` already does
  copy + `color-scheme-set` + `templates-apply`), never by `noctalia` CLI inside `colors.sh`.
- Convert `colors.sh`'s chain into a loader over backend colour modules (`colors.d/<id>.sh`), and
  let each backend declare its own watch paths so `color_watcher.sh` stops naming Noctalia/pywal/
  matugen. Keep the existing order as the *declared* priority, not as hardcoded control flow.
- Remove the cross-fill paths (v4→v5, kitty→Noctalia) or move them inside the backend that owns
  the gap.
- Fix the six findings from the palette verification while re-homing (palette-name whitelist
  asymmetry, cooldown stamped before the copy, followed theme symlink, `XDG_CACHE_HOME` divergence,
  the misleading "Colors from" line printed before parsing, and the test that cannot tell the
  loop-side re-assert from the start-side one).

### Phase 2 — break the chains between integrations
- `apply-background` becomes one capability with independent backends (skwd engine, mpvpaper,
  Noctalia), selected by the core; delete the in-provider delegation and the circular calls
  (`noctalia ↔ mpvpaper`, `noctalia → skwd`).
- Make `disabled_providers` real: the registry consumes it.

### Phase 3 — finish the seam that already exists
- Move the 8 raw `hyprctl` sites behind `Composer` (queries included: `getoption`, `workspaces`,
  `activeworkspace`, `dispatch`, `eval`), and make the old invariant true again.
- Move `theme_manager.rs:650-673`'s hardcoded `noctalia-v5` layout and `noctalia_config_dir()` into
  the Noctalia provider (the provider answers "what do I own; delete my artifacts").
- Move `shell/gallery/thumbs.rs:324-427`'s hardcoded provider layouts behind a provider-declared
  "preview source" capability, so the gallery can preview a theme from any backend.
- `settings.rs:23`'s reload → the `reload-config` capability.

### Phase 4 — the remaining leaks
- `theme.rs:73,83` (gsettings/darkman) → a `read-desktop-preference` capability with backends.
- `hypr_ipc.rs` event transport → inside the Hyprland module, with capability-named events.
- `config_guard.rs` → the Hyprland module's own named guard (Hyprland-native; not a backend).
- `shell/gallery/slot.rs:408-419` (`pgrep hyprmod`, currently dead code) → a real
  `yield-to-another-app` capability, or delete it.

## Order rationale

Phase 1 first: it is the live defect, and it exercises the whole pattern (descriptor + loader +
backend-owned action) on a capability with real tests already in place. Phase 2 removes the
chains that turn every future backend into a dependency hell. Phase 3 fixes the leak with the
best existing seam (and cheap tests). Phase 4 is cleanup that must not block the rest.

## Out of scope

- Rewriting the engine: `AGENTS.md` stands — `src/engine.rs`, `config.rs`, `settings.rs`,
  `theme_manager.rs`, `app_state.rs`, `watcher.rs`, `utils.rs` and `src/providers/` are reused;
  this plan re-homes knowledge and adds seams, it does not reimplement them.
- Monitors/HDR/display layout: that belongs to hyprmod.
- Making HVE compositor-agnostic. Hyprland is the base by design.

## Risks

- Phase 1 changes the scripts that colour the whole desktop: the sandbox harnesses
  (`src/scripts_contract.rs`, `src/watcher.rs`, `src/reload_coalescer.rs`) are the safety net, plus
  the keeper's live test — the only valid check for what the screen shows.
- Phase 3 touches `main.rs` theme transitions and hot geometry: keep the paid-for behaviour
  (masks, fullscreen survival, geometry performance) pinned by their existing tests before moving
  anything.
- A loader convention in bash must stay `set -u`-safe, validated and never `eval` file content;
  the current scripts' security posture is the bar to preserve.
