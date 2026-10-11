# Feature: remove the legacy Noctalia v4 backend

**Locator**: `odd/tasks/remove-noctalia-v4-backend.md`
**Engram mirror**: topic `odd/remove-noctalia-v4-backend/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Restore point**: tag `restore-point/pre-v4-removal-2026-10-11` (`b02b4c2`) + themes tarball
`~/Proyectos/hve-backups/hve-themes-20261011-010841.tar.gz`
**Status**: DONE (2026-10-11) — v4 backend and its retired companion provider removed,
cross-model verification passed, coverage gap closed. Keeper authorized 2026-10-11:
"luego quitame v4... v5 es la base como dejar hyprland en lua... nadie mira atras
después de actualizar cuando no es mantenido".
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~300-420 authored changed lines, dominated by deletions → single PR, no chaining.
**Route**: delegated writer (2+ non-trivial files), cross-model verification (HVE AGENTS.md Tier 3: deletes code).

## Objective

Delete the Noctalia v4 backend and its retired companion provider from HVE, so the
product has exactly one shipped shell backend: `noctalia-v5`.

## Problem / Why

v5 is the base and v4 is unmaintained. Keeping it costs a second provider
implementation, a fallback registration path, a `wallpaper` id alias that exists
only to read old themes, and a `wallpapers.json` parser used by nothing else.
Every one of those is a surface that must be kept in step with the live backend.

## Verified inventory (read-only report, 2026-10-11)

v4-only — `BORRAR`:

- `NoctaliaV4Provider` in full: `src/providers/noctalia.rs:693-1046` (id `noctalia`
  :714-715, `preview_sources` :740-750, `save` :752-790, `apply` :792-837,
  `post_apply` :839-1045) plus its `use` import at `:6`.
- `wallpapers.json` parsing: `WallpaperEntry` / `NestedWallpapersFormat` /
  `ScreenWallpaperFormat` / `ScreenEntry` :21-46 and `parse_theme_entries` :50-104
  (single caller :821, inside the v4 apply).
- Colour helpers called only by the v4 `post_apply`: `parse_hex_rgb` :162,
  `rgb_to_hex` :176, `darken_hex` :181, `lighten_hex` :188, `blend_hex` :195,
  `is_dark_hex` :208, `generate_terminal_section` :218-296 (callers :239-296, :953-956).
- `NoctaliaV4Paths` — `src/providers/noctalia_paths.rs:57-177` (`is_active` :169-176),
  `NOCTALIA_FILES` :44, `NOCTALIA_RENDERED_FILES` :48 (only :99/:103).
- `WallpaperProvider` — `src/providers/wallpaper.rs:58-232`. The module is NOT
  compiled today (no `pub mod wallpaper;` in `mod.rs:1-11`); the file is inert.
- v4 path methods on the shared `ShellProvider` trait: `template_processor`
  `noctalia_paths.rs:106-123`, `reload_command` :125-127, `apply_wallpaper` :130-143,
  `get_wallpaper` :146-159.

Shared — `MANTENER` (v5 call sites proved):

- `remove_stale_artifact` `noctalia.rs:302` → :1790, :1843, :1999, :2258
- `mpvpaper_enabled_in_settings` :324 → :622; `mpvpaper_state_in_settings` :391 → :659
- `disable_wallpaper_automation` :464, `has_wallpaper_automation_section` :516,
  `neutralize_noctalia_wallpaper_rotation` :532 → :2313
- `PluginReassert` :581, `plugin_reassert_action` :591 → :2042,
  `should_capture_video_manifest` :610 → :1984, `live_mpvpaper_plugin_enabled` :619 → :2027,
  `mpvpaper_can_paint_for_capture` :655 → :1980
- `palette_swatches` :1055 / `theme_facts_from_records` :1080 → :1601-1602,
  `parse_custom_palette_name` :1121 → :2638, `find_palette_path` :1141 → :1629, :2551
- colour re-assert cluster :1185-1381 → :2504, :2558
- `NoctaliaShell` :1406-1474 / `dnd_bool_arg` :1478 → `active_shell()` `mod.rs:117`
- `NoctaliaV5Provider` :1505-2670, `NoctaliaV5Paths` `noctalia_paths.rs:184-284`
- The `ShellProvider` trait itself (`noctalia_paths.rs:16-41`): shared by `NoctaliaV5Paths`

Registry — `src/providers/mod.rs`:

- `shell_backends` :51-68, v4 entry with `fallback: true` :60-66
- `register_default_providers` :81-111, `None =>` warn arm :101-103 (unreachable today)
- `declaration_provider` alias `wallpaper` → `noctalia` :137-150 (alias :145)

Tests and pins that move with the deletion:

- v4 unit tests: `noctalia_paths.rs:292,298,304,315,326,333,342,350,366`;
  `noctalia.rs:2693, 2972, 2979, 2986, 2996, 3004, 3012, 3020, 3065, 3088, 3110, 3131, 3149, 3162`;
  `wallpaper.rs:238-351` (goes with the file)
- Registry/fallback/alias pins: `mod.rs:301, 340, 355, 375, 498`;
  gallery pins relying on the alias: `thumbs.rs:1010, 1028, 1044, 1147, 1260, 2215, 2255, 2353`
- `src/architecture_contract.rs`: `SCANNED_FILES` :85 and :608, pin :402, comment :396-401 —
  the scanner PANICS when a listed file is missing (:520-522), so `wallpaper.rs` must
  leave the list in the same commit that deletes the file.
- Provider-agnostic, stays: all `shell_agnosticism_tests.rs`; `theme_manager.rs:2035`
  (incidental `StubProvider { id: "noctalia" }` :2063); `theme_manager.rs:1411`.

## Decisions recorded (keeper stance: nobody looks back)

- **D1 — no fallback.** Remove the v4 registry entry. On a machine without v5 no shell
  backend registers: `wanted = None` takes the `None =>` arm (`mod.rs:101-103`), HVE
  starts with `hve-presets` only (presets and geometry, no theme save/apply). The
  warning stays as the visible signal. `active_shell()` keeps returning `NoctaliaShell`.
- **D2 — no data migration.** User theme dirs that declare the v4 id stay on disk and
  stay hidden, exactly as they are hidden today (`theme_manager.rs:504-506`);
  `REMOVED_PROVIDER_IDS` (`theme_manager.rs:258`) is NOT touched, so no migration
  rewrites anybody's `meta.json`.
- **D3 — the `wallpaper` alias goes with v4.** `preview_wallpaper_candidates`
  (`noctalia.rs:115-156`) and the wallpapers.json parser are deleted with their only
  consumers; legacy dirs lose their declarer and their preview. Accepted: those dirs
  are already invisible.
- **D4 — i18n keys stay as they are** (`themes.provider` map in `i18n/en.json:588-591`
  and `i18n/es.json:588-591` has no `wallpaper` key and `display_name_key` has no
  production consumer).

## Out of scope

- Any behavioural change to `NoctaliaV5Provider`, the gallery, the colour pipeline,
  the background router, or `skwd` paths.
- `install.sh:425-427` (watches the v4 path) and
  `assets/scripts/color_sources.d/noctalia_lua.sh` (separate colour mechanism, still
  functional) — left alone; a follow-up may revisit them.
- `openspec/specs/capability-routing/spec.md:34,134` mentions `reload_command`: specs
  are contracts, so they are NOT edited here. Flag as a follow-up.
- No deletion of any directory under `~/.config/hve/themes/`.

## Acceptance criteria

1. `cargo test` green and `cargo check --all-targets` clean.
2. No surviving reference to the v4 provider in `src/` (grep for the id and for the
   deleted helpers returns only intentional historical docs).
3. v5 behaviour untouched: no v5-owned test is modified, and `NoctaliaV5Provider`
   keeps calling the same shared helpers listed above.
4. `architecture_contract` pins updated and green with `wallpaper.rs` gone.
5. Nothing under `~/.config/hve/themes/` or `~/.config/hve/config.json` is modified
   by the build or the tests (tests must sandbox `HOME`).

## Checks

- `cargo test`
- `cargo check --all-targets`
- grep sweep: the v4 provider id and `wallpaper` alias absent from `src/` code paths

## Work units

- [x] **U1** — delete the v4 provider code: `NoctaliaV4Provider` and its v4-only
  helpers (wallpapers.json parsing, the v4 colour helpers), `NoctaliaV4Paths` with
  `NOCTALIA_FILES` / `NOCTALIA_RENDERED_FILES`, the v4 `ShellProvider` method impls,
  and the inert `src/providers/wallpaper.rs`. Route: delegated writer. — `a9697ec`.
  **Boundary absorbed:** the registry's v4 entry could not be separated from this unit
  (`shell_backends` constructs `NoctaliaV4Provider`, so the tree did not compile without
  dropping it), so U1 took the entry and its registration pins; U2 kept the alias.
- [x] **U2** — registry and routing: drop the v4 entry from `shell_backends`, drop the
  `wallpaper` alias from `declaration_provider`, and leave the `None =>` arm as the
  deliberate no-backend path. Route: delegated writer. — `6ee7bce`.
- [x] **U3** — tests and pins: delete the v4 unit tests, rewrite the registry/fallback/
  alias and gallery pins that assumed v4, update `architecture_contract`'s
  `SCANNED_FILES` and its pin in the same commit as U1's file deletion. Route: delegated writer. — `bc5da92`.
- [x] **U4** — docs: update the wiki and `WIKI.md` lines that describe the v4 fallback
  (`docs/wiki/Project-Structure.md:88-93`, `docs/wiki/Themes-and-Colours.md:86-91`,
  `docs/wiki/Configuration.md:59`, `docs/wiki/Backgrounds.md:132-134`, `WIKI.md:465,515`).
  Route: parent (already-read, mechanical line edits). — `29c2e79`, `78c49f2`.
- [x] **U6** — restore the coverage the verification found missing: the retired v4 pins
  were the only test of the rule that decides between two declarers of the SAME preview
  role (first one in provider-directory order wins). Added a discriminating pair over
  identical fixtures that flips only the incoming order, plus a pin of the storage
  contract behind it (an unresolvable `providers/{id}/` declares nothing and is dropped).
  Route: parent (test-only, one file). — `cfb684b`.
- [x] **U5** — verification: `cargo test`, `cargo check --all-targets`, the grep sweep,
  and confirmation that no keeper data file changed. Route: parent + cross-model verifier
  (`opencode-go/glm-5.3-flash`, a different model family from the writer's).

## Verification result (U5)

Independent run by a different model family, over `b02b4c2..HEAD`:

- Nothing shared was deleted: every "must be kept" symbol still exists AND still has its
  v5 call site (checked by grep, not by trusting the diff).
- v5 untouched: every hunk in `noctalia.rs` / `noctalia_paths.rs` / `mod.rs` is a `use`
  line, a v4 or test-only deletion. One comment-only change landed inside
  `NoctaliaV5Paths::is_active` (`noctalia_paths.rs:140-142`) — text, not behaviour.
- `cargo check --all-targets`: 0 errors, 0 messages. `cargo test` (independent run):
  **1497 passed, 0 failed, 0 ignored**, 145.48 s.
- `architecture_contract` coherent: `wallpaper.rs` out of `SCANNED_FILES`, file-level pin
  updated, panic-on-missing-file confirmed real.
- No user data touched by the commits (`git diff --name-only` stays inside `src/` and docs).
  Every `Config::save()` call site in tests is sandboxed by `TempEnv`, so the keeper's
  `~/.config/hve/config.json` write at 01:42 is NOT attributable to the tests; a live
  `hve --tray` instance is running. Left as an open doubt, not a defect.
- One coverage gap found and closed in U6 (`cfb684b`).
- Pre-existing docs rot, NOT ours and NOT fixed: `WIKI.md` still lists `providers/shell.rs`
  (the file is `noctalia_paths.rs` today) with stale line counts in its module tables.
  Follow-up, different topic.

## Progress log

- 2026-10-11: restore point taken (tag `restore-point/pre-v4-removal-2026-10-11` at
  `b02b4c2` + themes tarball); 12 `(test k)` gallery clones removed with
  `scripts/test-themes.sh remove` (the leak source was that helper script, not
  `cargo test`; see `odd/tasks/gallery-video-preview-and-delete-linkage.md` S2);
  read-only inventory completed; this contract written.
- 2026-10-11: U1-U3 by the delegated writer (`a9697ec`, `6ee7bce`, `bc5da92`);
  U4 by the parent (`29c2e79`, `78c49f2`); cross-model verification passed with one
  coverage gap; U6 closed it (`cfb684b`). Net: 1756 lines deleted, 149 added in the
  removal, plus the restored tests.
