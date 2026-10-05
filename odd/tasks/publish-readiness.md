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
- [ ] **P3 — a git remote exists.** Nothing can be published while
      `git remote -v` is empty. Needs the keeper's destination (host + repository);
      the orchestrator cannot invent it.
- [ ] **P4 — README and WIKI, each with its own job, plus Spanish copies**
      (keeper's scope, 2026-10-05; **format changed to a GitHub wiki** the same day). Roles are now different, so the old "kept
      identical" invariant is retired:
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
      - **Proposed skeleton (awaiting the keeper's approval)**: README = what it is,
        philosophy, install, quick start, where the manual lives, licence. WIKI =
        index, then one topic per section from A to Z: installation, uninstallation,
        architecture, project structure, usage and keybinds, IPC, configuration
        reference, troubleshooting, FAQ, development.
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
        `Configuration.md`, `Troubleshooting.md`, `FAQ.md`, `Development.md`, plus
        `_Sidebar.md` and `_Footer.md`.
      - **Open decision for the keeper**: the app's About link opens `WIKI.md` (else
        `README.md`) from `src/callbacks.rs:270-281`. Once the manual is a
        multi-page wiki, that single file no longer represents it: either the link
        moves to the wiki URL (needs P3) or it keeps opening a local page. Until
        that is decided, `WIKI.md` stays in place.
- [ ] **P5 — `Cargo.toml` carries `readme` and `repository`.** `readme = "README.md"` is
      free; `repository` waits on P3.

### Load-bearing findings (the audit's items 6-7)

- [ ] **H1 — the forced close and the poisoned locks.** Keep the log capture ready for
      the next occurrence, and remove the bare `THEME_*.lock().unwrap()` pattern that
      turns one panic into an immediate cascade.
- [ ] **H2 — the source-text tests.** Convert the assertions that guard real behaviour
      into binding or behaviour assertions, starting with the two that hid defects.

### Sweeps (own changes, with the audit's counts as the checklist)

- [ ] **M1 — dead code** left by the removed UI (76 i18n keys, `HexDelegate.slint`,
      `PickTuneBlock.slint`, `FilterBar.slint`, and the smaller leftovers).
- [ ] **M2 — dependency chain** (`ksni` → `dbus-codegen` → `clap 2.34` / `bitflags 1.3.2`).
- [ ] **M3 — clippy debt** (the audit's counts) plus a release-time gate.
- [ ] **M4 — `install.sh`**: five of its six distro branches have never run.

## Acceptance criteria

- Each item closes with its own commit and, where behaviour is involved, its own test.
- Nothing is marked done without evidence a reader can check (file, command, output).
- No item changes scope silently: a decision only the keeper can make stops the item and
  is asked for, not guessed.

## Progress and evidence

- 2026-10-05: tracker created; P1 done (LICENSE).
- 2026-10-05: P2 done (the launcher no longer names one machine).
- Pending: P3 (needs the remote URL), P4, P5.

## Next step

P4 — decide which of README.md / WIKI.md is the source of truth and sync the other.
