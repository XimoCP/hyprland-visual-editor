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
- [ ] **P2 — `hve.desktop` stops naming one machine's home.** Today
      `Exec=/home/ximo/.local/bin/hve` (line 5). Needs a decision: `Exec=hve` (relies on
      `PATH`, and the installer already offers a `/usr/local/bin` symlink) or a `sh -c`
      wrapper. Also check `install.sh` writes the same content it ships.
- [ ] **P3 — a git remote exists.** Nothing can be published while
      `git remote -v` is empty. Needs the keeper's destination (host + repository);
      the orchestrator cannot invent it.
- [ ] **P4 — `README.md` and `WIKI.md` agree again.** 33 139 B vs 34 119 B today, while
      the README's own note says they are kept identical. Needs a decision on which one
      is the source of truth, then a sync.
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
- Pending: P2 onward.

## Next step

P2 — the `hve.desktop` decision.
