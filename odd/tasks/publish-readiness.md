# Publish readiness

- **Branch**: `hve2-visual-rewrite`
- **Opened**: 2026-10-05
- **Source**: `odd/audit-2026-10-04.md` (internal audit, read-only)
- **Rule**: one item at a time; the keeper approves each before the next starts.

## Objective

Close the internal audit's publication blockers and its load-bearing findings, in the
audit's own suggested order, so the repository can be published without known defects
hiding behind green tests.

## Tasks

### Blockers (small; three of them are one-line fixes)

- [x] **P1 — the MIT licence text exists** (2026-10-05). `LICENSE` added, MIT,
      `Copyright (c) 2026 XimoCP` (the name `Cargo.toml` already declares). If the
      keeper wants a legal name instead of the handle, it is a one-line edit.
- [x] **P2 — `hve.desktop` stops naming one machine's home** (2026-10-05).
      `Exec=sh -c "exec \$HOME/.local/bin/hve"`, validated with
      `desktop-file-validate` (exit 0; the only message left is a pre-existing
      `Categories` hint). Chosen over plain `Exec=hve` on evidence: the installer
      puts the binary at `$HOME/.local/bin/hve` and only offers a `/usr/local/bin`
      symlink for `hve-ipc`, never for `hve` — and `install.sh` itself warns that
      `~/.local/bin` may not be in `PATH`, so a bare `hve` would break for exactly
      the users the installer already knows about. `install.sh` copies the shipped
      file verbatim, so the fix reaches every install.
- [x] **P3 — a git remote exists** (closed 2026-10-05). `origin` =
      `git@github.com:XimoCP/hyprland-visual-editor.git` (SSH). The repo held the
      keeper's first, obsolete HVE (Quickshell/Noctalia v4); with his
      authorisation its `main` was replaced by the current HVE (preserved locally
      as tag `legacy-hve1`), and `hve2-visual-rewrite` was pushed as a branch.
- [x] **P4 — README and WIKI, each with its own job, plus Spanish copies**
      (closed 2026-10-05).
      - `README.md` — the simple explanation of the project.
      - `WIKI.md` — a structured manual: an index, then one topic per section
        explained from A to Z before moving to the next.
      - `readme_es.md` and `wiki_es.md` — **1:1 Spanish counterparts for the keeper
        only; never uploaded** (kept out of git). They must stay faithful to their
        English source: same headings, same order, same code and tables.
      - Step 1 is reading both files and mapping how they are built before any
        rewrite.
      - The drift measured by the audit (33 139 B vs 34 119 B) is expected once the
        roles differ; what matters now is that each file is complete for its own
        role and that each Spanish copy mirrors its English source.
      - **Step 1 done (2026-10-05), mapping findings**: both files are the SAME
        document, and both are written in SPANISH today (not English). README 663
        lines, WIKI 687. Four drift hunks: README has the `show` IPC row, the
        second-instance note and the `hve-ipc show` line (newer on user-facing
        behaviour); WIKI has a 28-line English `Gallery image pipeline performance`
        section (newer on that one internal topic). Neither has a licence,
        contributing/development or troubleshooting section, no images, and both
        carry the placeholder clone URL `https://github.com/tu-usuario/hve.git`.
        WIKI's own index does not list its unique section. 12 code blocks and 12
        tables per file; 35 TOC anchors generated from Spanish headings; the app
        opens `WIKI.md` (else `README.md`) from `src/callbacks.rs:270-281`, and the
        in-app About tree lists both names in `i18n/{en,es}.json`.
      - **Consequence**: the publishable pair must be ENGLISH (repo rule), so the
        Spanish files become the keeper-only copies, and the "1:1" is between each
        English file and its Spanish counterpart. Translating headings regenerates
        every TOC anchor; the two ASCII diagrams need re-padding if their comments
        are translated; the preset metadata block and all commands/paths stay
        verbatim.
      - **Proposed skeleton (approved 2026-10-05, minus the troubleshooting page)**: README = what it is,
        philosophy, install, quick start, where the manual lives, licence. WIKI =
        index, then one topic per section from A to Z: installation, uninstallation,
        architecture, project structure, usage and keybinds, IPC, configuration
        reference, FAQ, development.
      - The two Spanish copies live at the repo root as `readme_es.md` and
        `wiki_es.md`, excluded from git, and must never be picked up by the app's
        About link.
      - **Format decided 2026-10-05: a GitHub wiki**, after checking GitHub's own
        documentation. Facts that shape the work: the wiki is a SEPARATE git
        repository (`REPO.wiki.git`); one Markdown file per page and the FILENAME
        is the page title; titles may not contain `\ / : * ? " < > |`, so there are
        no nested pages and the hierarchy comes from `_Sidebar.md` (shown on every
        page) with `Home.md` as the landing page and an optional `_Footer.md`;
        editing happens on the web or by cloning the wiki, and only the default
        branch is published.
      - **Workflow the keeper asked for**: everything is prepared and reviewed
        LOCALLY first; nothing is uploaded until the keeper says so. The wiki
        source therefore lives in this repository under `docs/wiki/` (English,
        reviewable in normal commits), the Spanish mirror lives under
        `docs/wiki-es/` and is gitignored together with `readme_es.md`, and a
        publish step (copy into a clone of the wiki repository and push) waits for
        P3 to provide the remote.
      - **Page list (flat names, English, one topic each)**: `Home.md`,
        `Installation.md`, `Uninstallation.md`, `Architecture.md`,
        `Themes-and-Colours.md`, `Presets.md`, `Backgrounds.md`, `IPC.md`,
        `Tray-and-Automation.md`, `Project-Structure.md`, `Usage.md`,
        `Configuration.md`, `FAQ.md`, `Development.md`, plus
        `_Sidebar.md` and `_Footer.md` (no `Troubleshooting` page — see P8).
      - **Open decision for the keeper**: the app's About link opens `WIKI.md` (else
        `README.md`) from `src/callbacks.rs:270-281`. Once the manual is a
        multi-page wiki, that single file no longer represents it: either the link
        moves to the wiki URL (needs P3) or it keeps opening a local page. Until
        that is decided, `WIKI.md` stays in place. **Decided 2026-10-05: the
        link moves to the wiki URL** — `on_open_docs` now opens
        `https://github.com/XimoCP/hyprland-visual-editor/wiki`.
      - **Progress (2026-10-05, complete)**:
        the wiki is real under `docs/wiki/` — `Home.md`, `_Sidebar.md`,
        `_Footer.md` plus `Installation`, `Uninstallation`, `Architecture`
        (with the verified shell-agnosticism contract), `Themes-and-Colours`,
        `Presets`, `Backgrounds`, `IPC`, `Tray-and-Automation`,
        `Project-Structure`, `Usage`, `Configuration`, `FAQ` and `Development`.
        `README.md` is rewritten in English to the approved skeleton (what it
        is, philosophy, Hyprland-base/shell-axis, install, quick start, manual,
        licence). The Spanish mirror is done too: `docs/wiki-es/` carries all
        sixteen pages plus `readme_es.md` at the root, a 1:1 counterpart of the
        English source (same headings, order, code and tables; neutral
        professional Spanish), stored locally and gitignored — it is never
        uploaded and the app's About link never sees it. The porting writer cross-checked every
        claim against the code and corrected the source doc: `config_version` 8
        (not 7), the real `Config::default()` values, the 11 IPC commands (not
        9), the single shipped keybind `SUPER + H`, the marker block spellings,
        the colour-source chain (6 steps, `color_sources.d/`), 19 animations
        (not 18), Lua-only assets, the gallery/panel navigation model, and the
        "two lines" myth (it is one small block). **Residuals**: `WIKI.md` is
        now a superseded single-file Spanish manual with known stale claims and
        still the app's About target — the open decision above must be taken
        before publishing; the `hve_watchdog.sh` HVE_DIR resolution looks like a
        real defect (batch 3, item 7) — **fixed 2026-10-05** in
        `assets/scripts/hve_watchdog.sh`: the deployed copy derived its HVE
        home as `$HOME` (always present) so cleanup never fired; "installed" is
        now the binary `$HOME/.local/bin/hve`, with two contract tests.
- [x] **P5 — `Cargo.toml` carries `readme` and `repository`** (closed
      2026-10-05). `readme = "README.md"` and
      `repository = "https://github.com/XimoCP/hyprland-visual-editor"`.

### Installer and the agnosticism claim (keeper's questions, 2026-10-05)

- [x] **P6 — `install.sh` fixes before publishing** (read-only review done; **closed
      2026-10-05**). Commits: `d4140d2` (XDG, python3, `config_version` 8, autostart
      removal, root-guard i18n, dead keys, Void names), `f7dc35b` (a relative XDG value
      is ignored like `dirs` does; the autostart update no longer clobbers an existing
      config), `5b0a5e4` (the runtime scripts resolve their paths through XDG too),
      `741e884` (the shared helpers reject a relative XDG value as well).
      A second cross-model pass then found two CRITICALs the same family had left
      behind — `hve_watchdog.sh` handed an unchecked relative `XDG_CACHE_HOME` to
      `rm -rf`, and `colors.sh` resolved its config root the unchecked way while
      documented as standalone — both fixed and probed (the watchdog's delete target
      now falls back to `$HOME/.cache/hve` in the relative case). A follow-up
      verification passed with no blocking findings; its two small residuals (an
      exported relative `HVE_CACHE_DIR`, a relative `$HOME`) are closed by a guard at
      the delete site, which now only removes an absolute `.../hve` path. Evidence: the
      first pass was verified cross-model; every behaviour was probed by running the
      real snippets — four XDG cases (unset / absolute / empty / relative), the
      autostart update over an existing config with other keys, its idempotency, its
      failure path leaving the file untouched, and the runtime scripts matching the old
      literals when XDG is unset. The orchestrator's own probe caught and fixed a
      `set -u` hazard the first attempt introduced in `utils.sh`. Remaining, tracked
      elsewhere: five distro branches still unexercised (M4) and no `shellcheck` gate.
      Verified good: no personal paths, prompts never hang, idempotent, refuses root,
      every path matches the code.
- [x] **P7 — the documentation must not overclaim shell agnosticism** (closed
      2026-10-05 by A7 of `odd/tasks/shell-agnosticism.md`, commit `270102d`).
      The overclaim is gone from `README.md:7`, `WIKI.md:7` and the FAQ of both;
      the wording now states what the code proves (the seam, the data-driven
      registry, the fake-adapter proof, the build guard) and `docs/wiki/Architecture.md`
      documents the contract. Verified cross-model (GLM), which caught and forced
      the fix of a surviving FAQ claim, a wrong colour-source location and an
      overstated guard scope.
- [x] **P8 — no troubleshooting section anywhere** (keeper's decision, 2026-10-05): closed
      2026-10-24. Verified: `grep -rn -i troubleshoot docs README.md WIKI.md` returns nothing, and
      `docs/wiki/` holds no troubleshooting page;
      people report issues in the repository instead. Remove `Troubleshooting` from the
      wiki page list and keep it out of the README.

### Load-bearing findings (the audit's items 6-7)
- [x] **H1 — the forced close and the poisoned locks.** Closed 2026-10-24 (`85ff9e8`): the bare
      `THEME_*.lock().unwrap()` pattern is gone from the ten process-wide theme statics, replaced by
      `lock_recover` (`main.rs`), which recovers a poisoned guard instead of cascading the panic.
      Verified cross-model (glm-5.3-flash), which caught the one multi-line call site a single-line
      rewrite had missed; `grep` for the bare pattern on those statics is now empty. The forced close
      itself stays open: its log capture is still the only path to a cause.
- [ ] **H2 — the source-text tests.** PARTIAL (2026-10-24): both named cases are resolved. The
      construction guard that passed on comments was already fixed (`9cf66c1`); the preset-style
      validator that echoed the generator is gone (`3e483f8`) — its table `allowed_styles_for` and the
      test that consumed it were deleted, the `four canonical leaves` assert moved into the independent
      corpus validator, and only `every_emitted_family_is_used_by_the_shipped_corpus_on_the_same_leaf`
      (which reads `assets/animations/`) remains. The PATTERN is still open: 51 tests in
      `src/shell/ui_tests.rs` read repository source and assert on text.

### Sweeps (own changes, with the audit's counts as the checklist)

- [x] **M1 — dead code** left by the removed UI. Closed 2026-10-24 (`434402a`): 68 leaf i18n keys
      removed from both languages (276 → 278 after restoring the one the check wrongly flagged), the
      three orphan `.slint` components deleted, the test that only read `FilterBar.slint` deleted, and
      two stale comments naming `FilterBar` updated. Cross-model verification caught the trap this
      item hides: `19_stylized2.5D.lua` becomes the key `19_stylized2_5D` through `scan.sh`'s
      dot-to-underscore sanitising, so a stem-based scan calls it dead — the key was restored.
      NOT done, deliberately: `SystemSection.settings-title` is a four-file property chain (not an
      isolated dead property) and `auto-minimize` IS read in nine places; both need visual surgery,
      not a dead-code sweep.
- [x] **M2 — dependency chain** (`ksni` → `dbus-codegen` → `clap 2.34` / `bitflags 1.3.2`): the `ksni` 0.2 → 0.3 bump
      removed it (`odd/tasks/ksni-0.3-bump.md`). Measured after the bump: `bitflags 1.3.2` is NOT part of that chain
      — it arrives via `png` ← `tiny-skia` ← `hve`/`resvg`, so it needs a `tiny-skia`/`png` decision, not a ksni one.
- [x] **M3 — clippy debt** (the audit's counts) plus a release-time gate. Closed 2026-10-24
      (`896271f`): 66 warnings → 0, `scripts/clippy-gate.sh` runs
      `cargo clippy --all-targets -- -D warnings` from any cwd. The 15 `neg_cmp_op_on_partial_ord`
      sites route through `not_positive_or_nan`, which keeps the NaN-safe negation instead of flipping
      the operator. Verified cross-model (glm-5.3-flash): behaviour-identical.
- [ ] **M4 — `install.sh`**: five of its six distro branches have never run. OPEN, and it needs a
      decision before work starts: the detection reads `/etc/os-release` by absolute path, so proving
      the other five branches means either real environments or extracting the detection into a
      sourceable function with an injectable path. The second is a refactor of the installer (Tier 3)
      and it changes a script whose contract tests read it by string — it is not a last-minute edit.

## Acceptance criteria

- Each item closes with its own commit and, where behaviour is involved, its own test.
- Nothing is marked done without evidence a reader can check (file, command, output).
- No item changes scope silently: a decision only the keeper can make stops the item and
  is asked for, not guessed.

## Progress and evidence

- 2026-10-05: tracker created; P1 done (LICENSE).
- 2026-10-05: P2 done (the launcher no longer names one machine).
- 2026-10-05: P6 and P7 closed (installer family, honest shell claims).
- 2026-10-05: **P4 closed** — the English manual under `docs/wiki/` (sixteen
  pages), the English README, and the keeper-only Spanish mirror under
  `docs/wiki-es/` + `readme_es.md` (gitignored).
- 2026-10-05: **P3 and P5 closed** — `origin` set to the keeper's repo (his
  obsolete first HVE replaced on `main` by the current one) and `Cargo.toml`
  carries `readme` + `repository`.
- 2026-10-05: **the wiki is published** — all sixteen pages pushed to
  `hyprland-visual-editor.wiki.git` (live at
  https://github.com/XimoCP/hyprland-visual-editor/wiki).
- 2026-10-05: **the About link opens the wiki** — `on_open_docs` spawns
  `xdg-open https://github.com/XimoCP/hyprland-visual-editor/wiki`
  (`src/callbacks.rs`, pure `docs_open_command` + a contract test), replacing
  the local `WIKI.md`/`README.md` fallback.
- 2026-10-05: **the README became the richer landing page** at the keeper's
  request (hero banner, tagline, features table, manual pointer). It slightly
  overlaps the wiki; the keeper will personalise it later. The banner is the
  neon art in `assets/branding/`.
- Pending: nothing blocking. `WIKI.md` stays as the historical single-file
  manual; the app no longer opens it.

## Progress and evidence (2026-10-24)

- **P8 closed** — no troubleshooting section anywhere (grep-verified).
- **H1 closed** (`85ff9e8`) — poison recovery instead of a panic cascade; the forced close itself is
  still unexplained and keeps its log capture.
- **M3 closed** (`896271f`) — clippy 66 → 0 plus a gate; cross-model verified behaviour-identical.
- **M1 closed** (`434402a`) — 68 keys, 3 components, 1 test; the `19_stylized2_5D` false positive was
  caught by cross-model verification and restored.
- **H2 partial** (`3e483f8`) — both named cases resolved; the 51-test pattern remains.
- **P8/H1/M1/M3/H2 commits** are on `hve2-visual-rewrite`; the suite stands at 1845 passed / 3 failed,
  those 3 being the pre-existing colour-ownership and watcher set.

## Next step

M4 needs the keeper's call (real environments vs. extracting the detection). Nothing else in this
tracker blocks publication.
