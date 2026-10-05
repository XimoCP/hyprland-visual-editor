# Shell agnosticism — the contract

- **Branch**: `hve2-visual-rewrite`
- **Opened**: 2026-10-05
- **Keeper's mandate**: HVE is agnostic of the DESKTOP SHELL, never of Hyprland.
  Hyprland is the base (hence the name); the layer that sits on top of it —
  Noctalia, DMS, Calestia, anything else — is what agnosticism is about. The
  keeper authorised the whole route, no further questions, one morning of work.
- **Backup before any of it**: branch `backup/hve2-2026-10-05` at `c6b2a55`,
  bundle + tarball in `~/Proyectos/hve-backups/hve-20261005-053105.*`.

## The contract (what must be true when this is done)

1. **The core never names a shell.** No shell name, path, CLI, file format or
   plugin id appears in the core files (`src/main.rs`, `src/callbacks.rs`,
   `src/theme_manager.rs`, `src/config.rs`, `src/watcher.rs`, `src/ipc.rs`,
   `src/tray.rs`, `src/shell/**`, `src/engine.rs`). Every shell-specific fact
   lives inside the adapter that owns it.
2. **Every shell-specific operation the core needs is a capability of the
   adapter interface**, and a shell that cannot provide one declines it: the
   core asks "give me the live wallpaper", never "run `noctalia msg
   wallpaper-get`".
3. **The adapter registry is data-driven**: registering another shell is an
   entry in a list plus its detector, not an `if` inside the core.
4. **A proof exists**: a test-only FAKE shell adapter that the core drives
   end-to-end (list, save, apply, preview, facts, colour re-assert) with no
   core change. That test is the definition of agnostic; until it exists, the
   claim is unproven.
5. **A guard exists**: the architecture contract fails the build when a shell
   name appears in a core file, including files it does not list today.
6. **Behaviour is preserved for the shipped adapters.** The keeper's own
   Noctalia setup must work exactly as before: the whole suite green, the
   existing colour/preview/apply behaviour untouched.
7. **The documentation states exactly this** — no more, no less (see P7 in
   `odd/tasks/publish-readiness.md`).

## Verified starting point (2026-10-05, read-only report)

**Real seams that already work**

- Colour sources: `assets/scripts/colors.sh:413-580` — a module contract plus
  directory discovery; shipped modules `noctalia_lua.sh`, `noctalia_palette.sh`,
  `pywal.sh`, `matugen.sh`, `manual_hypr.sh`. Adding a shell's colour source is
  one file. This part of the claim is already true.
- Theme state: `ThemeProvider` (`src/theme_manager.rs:109-225`) with `save` /
  `apply` mandatory and `post_apply`, `reassert_colours`,
  `derive_colour_authority`, `deletable_artifacts`, `preview_sources`,
  `theme_facts` optional. `ThemeManager` gives orchestration, two-pass apply,
  shared-artifact cleanup and info aggregation for free.
- Compositor: `Composer` (`src/composer/mod.rs:106-192`) with
  `HyprlandComposer` as the only implementation. Separate axis, cleaner; not
  part of this contract.

**The leaks to close** (file:line, all verified)

| # | Where | What it is |
|---|-------|-----------|
| L1 | `src/providers/mod.rs:27` | `register_default_providers` hardcodes the Noctalia detection + `hve-presets` |
| L2 | `src/providers/mod.rs:88` | `declaration_provider` maps ids with a hardcoded match |
| L3 | `src/main.rs:680-681` | gallery backfill reads `providers/noctalia-v5/wallpaper.txt` by name |
| L4 | `src/main.rs:695` | core startup calls `noctalia_runtime::noctalia_msg(["msg","wallpaper-get"])` |
| L5 | `src/main.rs:1052-1063` | core inspects `providers/noctalia-v5/source.txt` + `palette.json` to decide the palette re-assert |
| L6 | `src/main.rs:2582-2598` | core sends `noctalia msg notification-dnd-set/status` during the theme swap |
| L7 | `src/providers/noctalia_runtime.rs:1-11` | documented as "neutral shared piece"; it spawns `noctalia`, knows its config/state dirs and the `noctalia/mpvpaper` plugin id |
| L8 | `src/providers/bg_info.rs:1-18` | documented as "neutral shared background information"; imports `noctalia_state_dir` and names the plugin |
| L9 | `src/providers/shell.rs:36-40` | `NOCTALIA_FILES` / `NOCTALIA_RENDERED_FILES` live in the nominally generic shell module |
| L10 | `src/providers/shell.rs:155-301` | `ShellProvider` / `ShellDetector` / `ShellRegistry`: half-built seam, only Noctalia impls, `ShellRegistry` dead in production, `NoctaliaV5Provider` ignores it |
| L11 | `src/architecture_contract.rs:57-135` | pins tokens per file only for a fixed file list; a leak in an unlisted core file is invisible |

**Explicitly out of scope**

- Building DMS, Calestia or any other shell adapter. The contract makes them
  possible and cheap; nobody is writing them.
- Hyprland agnosticism: `Composer` stays, the Hyprland scripts stay.
- Rewriting the engine (`engine.rs`, `config.rs`, `settings.rs`,
  `theme_manager.rs`, `app_state.rs`, `watcher.rs`, `utils.rs`): they are reused,
  never rewritten. This work touches them only to move a shell-specific call
  behind a seam, never to change their behaviour.
- The colour-source script contract: already agnostic, only guarded.

## Route (chosen; cheapest-first, proof early)

Each step is one work unit: a delegated writer (paid model, never a free one),
then a cross-model verification (different family), then one commit. The suite
must be green and behaviour preserved at every step; a step that cannot keep
both is stopped and reported, never forced.

- **A1 — the proof first.** A test-only fake shell adapter plus the
  end-to-end test that drives the core through it (list, save, apply, preview,
  facts). It will fail exactly at the leaks L3-L6; that failure is the
  specification for A2. No production change in this step.
- **A2 — the capability seam.** Give the adapter interface the capabilities the
  core needs and move L3-L6 behind them, with a no-op/declined default so a
  shell that cannot provide one still works. Behaviour identical for Noctalia.
- **A3 — module honesty.** L7, L8, L9: either rename so the coupling is
  explicit, or split the genuinely neutral part out and leave the Noctalia part
  in the provider. No behaviour change.
- **A4 — the registry.** L1, L2: a data-driven list (id, detector, constructor)
  so adding a shell is one entry. Behaviour identical.
- **A5 — the half-built seam.** L10: promote `ShellProvider` into the real
  shell-adapter interface (used by every provider) or delete it. Decide from
  what the code needs after A2; do not leave a dead seam behind.
- **A6 — the guard.** L11: extend `architecture_contract.rs` to cover the core
  files it does not list, with shell-name pins at zero, so a new leak fails the
  build.
- **A7 — the words.** The wiki page for the contract plus the honest README
  wording, once the code is true.

## Acceptance criteria

- The A1 fake-adapter test passes with the core untouched by shell knowledge.
- `cargo test` green (baseline: 1346 passed) at every commit; no behaviour
  change for the shipped Noctalia adapters.
- `cargo build --release` warning-free.
- No shell name remains in a core file; the extended architecture contract
  proves it and fails on a new one.
- Every step has its cross-model verification recorded, with the model named.

## Progress and evidence

- 2026-10-05: contract written from the verified read-only report; backup taken.
- Pending: A1 onward.

## Next step

A1 — the fake-adapter proof (delegated writer, then cross-model verification).
