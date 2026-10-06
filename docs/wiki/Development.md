# 💻 Development

**Build HVE from source, test it, and work on its code.**

This page is for developers. If you installed HVE and just want to use it, you do not need it — see [Usage](Usage).

## 🏗️ Build

Prerequisites are the Rust toolchain (`rustup` installs it if missing) and the system packages listed in [Installation](Installation).

```bash
cargo build --release
```

The binary lands in `target/release/hve`; the installer runs exactly this command. The Slint UI is compiled from `build.rs` (`slint_build::compile("ui/main.slint")`), so a build always carries the current `ui/` tree with it.

## 🧪 Test

```bash
cargo test
```

The suite is the gate: a module is done when its tests are green, and a change is written test-first — the failing test comes before the implementation.

Three families of tests are worth knowing about:

| File | What it guards |
|------|----------------|
| `src/scripts_contract.rs` | Runs the **real** scripts in `assets/scripts/` inside hermetic temporary homes. A stub `bin/` is put first on `PATH` so `hyprctl`, `pgrep`, `noctalia` and `inotifywait` can never touch your actual compositor during a test run. |
| `src/architecture_contract.rs` | A measurement instrument, not production code. It pins today's integration-token counts per core and provider file and fails when a count **grows** (a coupling leaked in) or when a pin is **higher** than the measured count (a stale pin). It also carries the shell-name guard: a core production file that spells `noctalia`, `quickshell`, `calestia` or `dms` fails the build with `SHELL NAME IN CORE: … move it behind the adapter that owns it (src/providers/shell_capabilities.rs)`. Lower a pin when you move a coupling; never raise it to hide one. |
| `src/shell_agnosticism_tests.rs` | Drives the core end to end with a **fake** shell adapter and a **fake** theme provider the core has never heard of. If a new shell ever forces an edit here to keep it passing, the agnosticism claim is false. |

## 👀 Visual verification for UI changes

A UI change that only shows "tests green + build clean" is not verified. Run the headless render and read the images it produced:

```bash
HVE_RENDER_DIR=/tmp/opencode/render-verify-<unique> cargo test slice_focus_flow_renders -- --nocapture
```

The test prints a `render artifacts for this run: <dir>` line; open the PNGs in **that** directory and check the geometry yourself. Each run writes its own directory, so a frame can never come from another run — anything flat in `/tmp/opencode/*.png` is a leftover and never evidence. Sibling render tests cover other states (`ctrl_drawer_deploy_renders`, `mosaic_curtain_phase_driven_renders_mid_and_settled`, `panel_morph_midflight_renders`, `theme_info_panel_renders_closed_and_open`, …); extend them when you touch a new visual state.

## 📏 Working rules

Taken from the repository's own `AGENTS.md`:

- **Strict TDD** — write the failing test first, then implement. Test runner: `cargo test`.
- **English artifacts** — code, specs, docs, comments and UI copy are written in English.
- **One module at a time** — each one closes with tests green before the next starts.
- **Keep the engine intact** — `src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs` and all of `src/providers/` are reused, not rewritten.
- **One mutating window** — never stack windows.
- **No GPL copying** — never copy code from hyprmod or skwd-wall (both GPL-3.0); translate visual ideas only, with credit.

## ⚡ Gallery image pipeline performance

Single decode → four artifacts: the worker decodes the source once and
derives thumb (≤400×720 cover), hero (≤1600×900 contain), and both
parallelogram slats (collapsed/expanded). All decode/resize/bake work
runs off the Slint thread; the UI callback only wraps `Image::from_rgba8`
handles and updates row data.

Content-keyed cache: artifacts are `<len>-<hash>-thumb.png`,
`<len>-<hash>-hero.png`, `<len>-<hash>-slat.png`, and
`<len>-<hash>-slat-exp.png` under `$XDG_CACHE_HOME/hve/thumbs`. Same
bytes share one key regardless of path/mtime; warm hits load slats from
small PNGs and skip the full-size decode. `prune_stale(cache_dir, 512)`
bounded sweep keeps the newest 512 artifacts by mtime and deletes the
oldest beyond the cap (off UI thread, best-effort, once per gallery open)
— prevents unbounded stale accumulation without reading every source.

In-place cards model: `sync_cards(&VecModel, new_rows)` diffs by name,
removes stale rows descending, updates changed rows via `set_row_data`,
and pushes additions—no `ModelRc` swap on theme apply, so delegates
are preserved.

Curtain: a single parent `curtain-phase` (0→1) driven by one 16 ms
Timer is passed to mosaic cells; each cell computes its wipe from pure
math `clamp((phase*600 - delay)/300)` and renders an opaque cover
Rectangle. No Timer lives inside repeater delegates; reduced-motion
falls back to the existing crossfade.
