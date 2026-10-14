# Lane C — code health (HVE)

Branch `hve2-visual-rewrite`. Test runner `cargo test` (single binary `hve`; UI renders need
`HVE_RENDER_DIR=<dir> cargo test --bin hve <test> -- --nocapture` and the frames are read by eye).
Delivery: one work-unit commit per task, `cargo fmt` and the mechanical passes kept apart from any
behaviour change; the release binary is installed only after the tree is green and a return point exists.

Opened 2026-10-14 on the keeper's instruction to finish the `theme-package-trust` lane properly before the app
is handed to him for deep testing. Lane C is the LAST lane of that contract and it must run SERIAL and ALONE:
it deletes modules, formats 23k lines and bumps a dependency, so nothing else may be in flight.

## Specs

- **S1 — Formatting is a dedicated, mechanical commit.** `cargo fmt` may go FIRST or LAST, never mixed with a
  behaviour change, and nothing else may be in that commit. No `rustfmt.toml` is added unless a change is
  justified in writing (the repo has none today).
- **S2 — The ~70 autofixable clippy warnings are fixed, and the float-comparison class is NEVER autofixed.** The
  audit measured 194 warnings (81 in the bin target, 194 with the test target, 81 of them duplicated) and named the
  unsafe class explicitly: `!(a < b)` and `a >= b` differ on NaN, and the mosaic/slice layout depends on the
  current semantics. Those stay as they are, with the reason written down, not silenced with an `allow`.
- **S3 — `src/tray.rs` stops being able to panic on a non-multiple-of-4 buffer.** Both conversion sites
  (`chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]])`, lines 25-28 and 75-78) index `p[2]`/`p[3]` on a last chunk
  that can be shorter. Both are guarded, and a test pins that a truncated buffer yields `floor(len/4)` pixels
  instead of panicking.
- **S4 — A dead-code `allow` that silences a whole MODULE is either removed with the code it hides, or kept with
  the reason written down.** Suppressing a module-wide `dead_code` is how dead code survives review; the compile
  is the authority on what is unused, so the removal is proved by the build, not by reading.
- **S5 — `ksni` 0.2 → 0.3 only if the tray can be verified.** The bump removes a duplicate dependency tree (slint
  already links `ksni` 0.3.5 internally) and takes `clap v2.34` + `dbus-codegen` out of the build graph. It is a
  USER-VISIBLE surface (the tray icon and menu), so it ships only if the API maps without inventing behaviour and
  the result can be checked; otherwise it is deferred with the reason and the cost recorded.
- **S6 — The corroborated `reload_coalescer` flake is FIXED, not waived.** Two different tests failed under full
  suite load and both passed in isolation: the assertions rest on a wall-clock window (`sleep` then "is it still
  inside the window"), and a loaded machine stretches a `sleep` past its window. The fix makes the tests wait for
  the CONDITION they need instead of assuming a duration.
- **S7 — The source-text assertions are a decision, not a pile.** ~63 sites read `src/**` or `include_str!` a
  `.slint` file to assert its text. They caught real regressions during this contract (a font floor, a wiring pin),
  so they are NOT deleted; the ones that only restate a rename (the `skwd_policy` guard-order slicing of its own
  source is the worst) are replaced by a behavioural check or narrowed, and the rule is written down.

## Tasks

| ID  | Spec | Task                                                                 | Route          | Status |
|-----|------|----------------------------------------------------------------------|----------------|--------|
| T0  | —    | Return point: tag + copy of the installed binary, HEAD recorded      | inline         | todo   |
| T1  | S1   | `cargo fmt` alone in its own commit                                  | inline         | todo   |
| T2  | S3   | `tray.rs` chunk guard + truncation test                              | inline         | todo   |
| T3  | S4   | Dead-code: remove ONLY `bg_info::notify_plugin_required` + its compat pin (recon below); every other site stays with its reason | inline | todo |
| T4  | S2   | Clippy autofixes, float class excluded and recorded                  | inline + suite | todo   |
| T5  | S5   | `ksni` 0.2 → 0.3: DEFERRED with the reason (see recon below)         | inline, decide | done (deferred) |
| T6  | S6   | `reload_coalescer`/`watcher` flake: reproduce ON DEMAND under load, fix the mechanism, re-run under the same load | inline + suite | todo |
| T7  | S7   | Source-text assertions: narrow/behavioural where rename-fragile only | inline         | todo   |
| T8  | all  | Full suite + `cargo check --all-targets` + release build, then install | inline       | todo   |

### T3 recon (read, 2026-10-14) — what the module-wide allows actually hide

- `src/shell/gallery/views/mod.rs:13,15,17` (`hexagon`, `mosaic`, `slice`): the module doc already states the
  reason — "parts of these engines are intentionally dead in the binary while remaining test-covered". The Rust
  engines own the geometry the Slint delegates present, and the unit tests are what verify it. Deleting them
  deletes the verification of the views. **KEPT, reason already in the file.**
- `src/providers/noctalia_paths.rs:22-45` and `shell_capabilities.rs:57`: the seam's vocabulary (a `ShellProvider`
  and a declined `NoShell`) that production reaches only through `config_dir`; the in-file comment already explains
  that rustc's `dead_code` does not count `#[cfg(test)]` usage in a plain `check`. This is the seam the
  shell-agnosticism lane built for OTHER shells. **KEPT, reason already in the file.**
- `src/providers/bg_info.rs:291` — the ONE real candidate: `notify_plugin_required` exists, by its own docstring,
  only to keep a "historical API-compat path" RESOLVING, and nothing in the binary calls it; the only thing that
  consumes it is the compatibility test that pins its existence. A function kept alive by the test that pins it is
  the self-licking ice cream this lane exists to remove. Its `mpvpaper` twin is named in the same docstring and
  must go with it, together with the pin.

### T6 plan (reproduce first — a flake "fixed" by loosening an assertion would hide a real loss of coalescing)

1. Reproduce ON DEMAND: run the module's tests (and the full suite) while the machine is under artificial CPU load
   (`nproc` busy loops), and capture the failing assertion verbatim.
2. Identify the MECHANISM from the failure: a bounded deadline that a loaded machine misses, or a wall-clock quiet
   window that a late-but-legitimate event trips.
3. Fix the mechanism, not the symptom; then repeat the same artificial load and require clean runs.
4. Record what could NOT be proved (an intermittent flake that never reproduced is not "fixed" by a green run).

### T5 recon (measured read of the two crates, 2026-10-14) — why the bump is DEFERRED

**The win is real, and measured from `Cargo.lock`**: `ksni 0.2.2` (the direct dep) drags `dbus 0.9.11`,
`dbus-tree 0.9.2`, `thiserror 1.0.69` and the build-time `dbus-codegen 0.9.1`, which is the ONLY consumer of
`clap 2.34.0` (+ `ansi_term`, `atty`, `strsim 0.8`, `textwrap`, `vec_map`). `ksni 0.3.5` is ALREADY in the graph
as slint's own dependency, on `zbus 5.16.0` + `async-io`/`futures-*`, so the bump would delete a whole duplicated
DBus stack without adding anything: `tokio` is not in the lock at all, so 0.3 must be taken with
`default-features = false, features = ["blocking", "async-io"]`.

**The API maps, with two real differences**: `TrayService::new(tray)` + `.handle()` + `.spawn() -> ()` becomes
`use ksni::blocking::TrayMethods; let handle = tray.spawn() -> Result<Handle<T>, Error>`, and `Handle::update` now
returns `Option<R>`. `Icon { width, height, data }`, `menu::StandardItem` (used with `..Default::default()`),
`MenuItem::Separator` and the whole `Tray` trait are unchanged.

**Why it does not ship today**: the FAILURE SEMANTICS change in the user-visible path. 0.2's `spawn()` runs the
service inside a spawned thread and `.unwrap()`s there, so a failure is a thread panic nobody sees; 0.3 resolves
the service SYNCHRONOUSLY and hands the caller a `Result` — meaning HVE must decide what "no tray" means, change
`start_tray`'s signature and handle it at every caller (main.rs and the IPC refresh path). That is new behaviour in
the notification area, and it depends on a live `StatusNotifierHost`: this session has a session bus
(`dbus-run-session` is installed) but no host to register against, and launching the GUI unattended to smoke-test
the tray would APPLY THEMES to the keeper's live desktop. Shipping a change to a user-visible surface that cannot
be checked is what S5 forbids; the cost of waiting is build time, not function. Done in ONE commit with the keeper
present: features + `spawn()` result + the "no tray" path + a `dbus-run-session` smoke probe for that run only.

## Evidence to record per task

- The command run and its OBSERVED output (counts, not adjectives).
- For the flake: the failing assertion reproduced, and the number of consecutive clean full-suite runs after.
- For the dependency bump: the `cargo tree` before/after and what could NOT be verified.
- Anything deferred, with the reason, in this file — never silently.

## Log

- **L1 (2026-10-14, the keeper, verbatim)**: `vale me voy a ir a trabajar... me gustaría que trabajaras de forma
  autonoma en mi ausencía y que tomes las mejores decisiones sin mí... tienes permiso para leer fuera del proyecto,
  pero aun necesitas permiso para escribir fuera de el. Dalé duro... me gustaría que cuando volviera ya este todo
  terminado y con el binario instalado y listo para probar a fondo... antes de instalar binario ni nada un punto de
  retorno siempre es bienvenido.` — this is the authorization for Lane C, for the serial order, and for the install,
  with a return point first.
- **L2 (2026-10-14, the parent, recon from the 2026-10-12 audit at `7f43ccc`)**: clippy 194 warnings; `cargo fmt
  --check` 1,548 chunks / 23,362 lines; `ksni` 0.2 duplicates slint's internal 0.3.5 and drags `clap v2.34` +
  `dbus-codegen`; zero TODO/FIXME and zero `#[ignore]`d tests; 94 production `panic`-capable sites of which 84 are
  `Mutex::lock().unwrap()` and the ONLY real candidate is the tray chunk index; ~63 source-text assertion sites;
  ~70 test-side `thread::sleep` calls. The audit's own warning about the float class is what S2 encodes.
