# Wiki voice — a manual a newcomer can actually start with

- **Branch**: `hve2-visual-rewrite`
- **Opened**: 2026-10-06
- **Keeper's mandate**: "dale a fuego… haz que no sea sosa, ponle iconitos y
  títulos molones". The diagnosis that triggered it (an external review, agreed
  with): the wiki is well built and technically serious, but it is written for
  someone who already knows Linux and Hyprland. `Home` explains fragments,
  overlay, markers and atomic writes before saying what the reader gains, and
  `Usage` opens with an architecture rationale instead of how to use the app.

## Objective

Keep every fact exactly as it is and change **who the text talks to**: a
newcomer first, the engineer after.

## The shape of every page

1. A short plain-language promise at the top (what this page gets you).
2. Short, plain sections with icons in the headings.
3. Then the technical depth, under a clearly marked heading, unchanged in
   substance. Technical pages keep their rigour — they only gain a plain
   opening line and heading icons.

**One text at two depths, never two parallel explanations.** The easy part
stays short (two to four lines); if it grows into a second full explanation it
will drift out of sync with the technical one.

## Hard rules

- **No fact may change.** Friendlier wording may not invent, soften or drop a
  technical claim. If a rewrite is unsure, the original wording wins.
- **English** (the repository's wiki source). The keeper's Spanish mirror in
  `docs/wiki-es/` is local and gitignored: out of scope here.
- **No new pages and no renames.** Page names are URLs and the sidebar links to
  them.
- **Icons and titles**: one emoji per heading, from a small consistent set, and
  inviting H1s/section titles. Never a wall of emoji.
- Explain a term the first time in plain words; do not lecture.
- Publishing to the GitHub wiki is a separate, keeper-authorized push.

## Tasks

One work unit each: a delegated writer, a cross-model verifier that checks every
claim against the code, one commit.

- [ ] **W1 — Home, _Sidebar, _Footer.** The landing page answers "what is this
  and why would I want it" in ten lines, then the depth. The sidebar groups
  *getting started* apart from *technical reference* and gains icons.
- [ ] **W2 — Installation, Usage.** "I want to install it, tell me exactly what
  to do" and "I have it installed, how do I use it". The first ten minutes,
  end to end, with the technical detail after.
- [ ] **W3 — Themes and colours, Presets, Backgrounds.** "I want to change the
  colours, what does all this mean", "what is a preset and how do I use one",
  "what happens with wallpapers and videos".
- [ ] **W4 — FAQ, Uninstallation.** Plain answers, plain removal.
- [ ] **W5 — Technical pages, light touch.** Architecture, IPC,
  Project-Structure, Configuration, Development, Tray and automation: a plain
  opening line, heading icons, and no loss of depth.

## Acceptance criteria

- Someone who has never heard of Hyprland can read Home, then Installation,
  then Usage, and get HVE running and used, without meeting an unexplained term.
- Every technical statement in the wiki still matches the code.
- No page was renamed or added; the sidebar still reaches all of them.
- The technical pages are still usable as a reference.

## Progress and evidence

- 2026-10-06: contract written from the keeper's mandate and the external review
  he agreed with. Pending: W1 onward.
