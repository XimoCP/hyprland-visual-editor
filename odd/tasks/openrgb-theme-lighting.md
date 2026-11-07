# Feature: the OpenRGB lighting travels with the theme

**Locator**: `odd/tasks/openrgb-theme-lighting.md`
**Engram mirror**: topic `odd/openrgb-theme-lighting/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Requested**: 2026-11-07 by the keeper: "cuando guarde un tema o re-escriba o re-guarde … se
quede guardado también el perfil de color de openrgb y que se cargue con ese tema".
**Status**: IN PROGRESS
**TDD**: strict — runner `cargo test`; every external command is replaced by a stub binary on a
temporary `PATH` (`tempfile` + `serial_test`), the pattern `src/providers/noctalia.rs` already
uses for `noctalia`.
**Route**: direct inline (no writer trigger: one behaviour, one file + one registration line).

## Specs

- **S1 — Availability gates registration.** HVE registers the `openrgb` provider only when the
  `openrgb` binary answers `--version` AND its SDK server accepts a TCP connection on the port
  OpenRGB itself records (`Server.default_port` in `~/.config/OpenRGB/OpenRGB.json`, 6742 when
  absent). On any machine without both, nothing is registered and no other behaviour changes,
  so themes saved elsewhere never see it and every existing feature behaves exactly as today.

- **S2 — A save captures the live lighting.** Saving a theme writes the current OpenRGB state as
  an unchanged OpenRGB profile document at `{theme_dir}/providers/openrgb/profile.json`.
  Re-saving overwrites it with the state that is live at that moment (the keeper's "o re-guarde").

- **S3 — An apply restores it.** Applying a theme that holds `profile.json` loads that profile by
  absolute path (`openrgb --profile <abs>`), never by name, so the theme carries its own bytes
  and no name collision with the keeper's own profiles is possible.

- **S4 — A theme without a profile applies exactly as before.** Old themes, and themes saved while
  OpenRGB was unavailable, apply with no lighting step and no new error.

- **S5 — Lighting never costs the theme.** A failure talking to OpenRGB (server gone, timeout,
  non-zero exit, unreadable profile) is logged and the theme save/apply continues. Rationale: the
  provider runs inside `ThemeManager::save`/`apply`, where a returned error aborts the whole
  theme — losing presets and colours because the fan controller hiccuped is collateral damage.

- **S6 — Every external command is bounded.** `openrgb` runs under a deadline (5 s) and is killed
  when it expires; a hung server never freezes the UI thread.

- **S7 — No user input reaches a command line.** The capture profile name is a constant
  (`hve-capture`), the loaded path is a file HVE itself resolved inside the theme directory, and
  the theme name never reaches the CLI as an option.

- **S8 — The gallery shows what it captured.** The info panel lists the captured device count and
  up to six distinct colours from the profile as swatches, read through the bounded
  `theme_media::read_theme_record` path. A missing or unreadable record shows nothing.

- **S9 — Documented as optional.** `WIKI.md` lists OpenRGB among the providers and states that it
  is optional and auto-detected.

## Tasks

| ID | Specs | Route | Status |
|----|-------|-------|--------|
| T1 | S1 | inline | DONE — `detected()` = binary answers `--version` AND the recorded port accepts TCP; `server_port_from` is pure and tested |
| T2 | S2, S6, S7 | inline | DONE — `save()` captures into `providers/openrgb/profile.json` (atomic temp+rename), removes the transient `hve-capture` profile, bounded by a 5 s deadline |
| T3 | S3, S4, S5 | inline | DONE — `apply()` loads by absolute path; no record or any failure logs and continues |
| T4 | S1 | inline | DONE — `registers_openrgb` (pure policy) + `register_default_providers`; live probe kept out of the suite by `cfg!(test)` |
| T5 | S8 | inline | DONE — `theme_facts` reports device count and up to six distinct swatches from the profile |
| T6 | S9 | inline | DONE — WIKI section + module table row; `src/providers/openrgb.rs` joins `SCANNED_FILES` at 0 measured cross-backend tokens |


## Log

- **L1 (2026-11-07, verbatim)**: "lo que quiero es que cuando guarde un un tema o re-escriba o
  re-guarde ... se quede guardado también el perfil de color de openrgb y que se cargue con ese
  tema... como lo haríamos?"
- **L2 (2026-11-07, verbatim)**: "vale, pero podemos hacer que se publique para que no haya
  fricciones pero que sea algo que no tengamos que mecionar ya que solo me funcionaría en mi pc,
  no??" — answered: publishing removes rebase friction; what makes it invisible is auto-detection
  (it only registers where OpenRGB answers), not secrecy.
- **L3 (2026-11-07)**: approval to build it with auto-registration only when OpenRGB responds.
- **L4 (2026-11-08, verbatim)**: "deja marcado que los temas actuales no se reempaqueten... que
  solo se guarde o publiquen los cambios de código, pero no se alteren los temas ya empaquetado y
  luego haces commit y push". Honoured as a FREEZE of `assets/themes/`: the policy is stated in
  `assets/themes/README.md` and enforced by
  `providers::openrgb::tests::the_shipped_themes_carry_no_lighting` (plus a fixture test proving
  the guard bites). No shipped theme was modified.


### Measured evidence (2026-11-07, this machine)

- `openrgb --profile /abs/path.json` → `Profile loaded successfully`, exit 0. Loading by absolute
  path works, so the profile can live inside the theme directory.
- `openrgb --save-profile hve-probe` → wrote `~/.config/OpenRGB/profiles/hve-probe.json` with the
  live colours (server forwarded the command; captured while `--profile joker` was applied).
- `openrgb --save-profile /abs/path/name` → sanitised the path into a flat name inside the
  standard profiles directory (`tmpopencodergbprobehve-theme.json`), so the capture must read the
  standard location and then move the bytes. The probe artifacts were removed afterwards.
- `~/.config/OpenRGB/OpenRGB.json` carries `Server.default_port = 6742`.
- `openrgb --version` exits 0 (used as the liveness probe; it never touches a device).
- The running instance is `openrgb --profile joker --startminimized --server`.

### Verification evidence (2026-11-07)

- `cargo test providers::openrgb` → 20 passed, 1 ignored (0.12 s).
- Mutation check (the honest substitute for RED-first in this unit: tests and implementation
  landed in the same write, so "green" alone would prove nothing): forcing `apply` to run without
  a record failed `apply_is_a_quiet_noop_without_a_profile`, and deleting the transient-profile
  removal failed `save_removes_the_transient_profile_it_asked_for`. Both reverted; suite green.
  A gap in the strict TDD rule for this unit is ACCEPTED and recorded here rather than claimed as
  observed RED.
- `cargo test -- --ignored live_openrgb_round_trip` → PASS against the real running OpenRGB:
  detection, capture into a temporary theme, facts with a device count > 0, and a reload of the
  just-captured state (non-destructive).
- `the_theme_manager_records_the_provider_and_restores_the_lighting` → PASS through the real
  `ThemeManager::save`/`apply`, so `meta.json` records the id and the apply reaches OpenRGB.
- `cargo test architecture_contract` → 10 passed with the new file in `SCANNED_FILES`.

### Verification round 1 (independent model, 2026-11-07) — FAIL, all findings closed

One BLOCKER: `WIKI.md` promised the transient `hve-capture` profile is deleted ALWAYS while the
early returns skipped the cleanup (a failed capture that still wrote the file left it behind).
Closed by extracting `capture()` and removing the scratch on every path after the attempt, with a
new test (`save_removes_the_transient_profile_even_when_the_command_fails`).

Advisories taken: the deadline test now proves the child is killed and reaped (pid file + `/proc`,
no more `sleep 30` grandchild); the `/tmp` fallback for a missing config home was REMOVED (the
provider skips the capture instead of reading bytes from a world-writable directory);
`detected()` is memoized in a `OnceLock` because the router asks more than once; the WIKI line
count was corrected.


### Consequence worth knowing

A theme saved with lighting records the `openrgb` id, and `ThemeManager::list()` hides a theme
whose providers are not all registered — the same rule every provider follows today. With the
OpenRGB server stopped (the keeper's autostart runs `--server`, so it is up today), such a theme
is not listed in the gallery. Stated in `WIKI.md` rather than worked around: the provider is
either available or it is not.

