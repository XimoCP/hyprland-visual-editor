//! Capability-routing architecture contract (`odd/tasks/hve-capability-routing.md`).
//!
//! The keeper's two inviolable rules: (1) Hyprland is the base, never a
//! backend — Hyprland-specific knowledge lives in `src/composer/hyprland.rs`
//! (plus the Hyprland-owned guard/markers), never scattered through core
//! files; (2) backends are siblings, never a chain — a backend never calls,
//! waits for, or configures another backend.
//!
//! This module pins TODAY's per-file integration-token counts for the core
//! files named in the plan's definition of done **and for every provider
//! module** (Phase-2 measurement: the scanner used to stop at `shell.rs`, so
//! every provider-level chain — the `noctalia → skwd_policy` coupling the
//! whole-phase audit found — was invisible). The Phase-4 visibility pass
//! (2026-09-29) then found the same class of blind spot one directory over:
//! the `read-desktop-preference` probes moved into
//! `src/theme/desktop_preference/` and the dead `pgrep hyprmod` scaffolding
//! lived in `src/shell/gallery/slot.rs`, files no token policed — the
//! scanner reported clean zeros over couplings it simply could not see.
//! The pin test fails when any
//! count GROWS (a new coupling leaked into a core or provider file) and also
//! when a pin is HIGHER than the measured count (a stale pin — lower it,
//! never raise it without moving the coupling into its backend module). Each
//! phase of the plan lowers these pins toward zero. This file is a
//! measurement instrument: it never changes production behaviour, and known
//! debt is PINNED at its measured count, never deleted or hidden.
#![cfg(test)]

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Files the plan forbids editing when adding a backend. Shell scripts are
/// scanned whole (they have no `mod tests`); see `production_body`.
///
/// Phase-2 extension (2026-09-29): the provider modules that can carry
/// integration knowledge join the core files. `hve_presets.rs` is scanned
/// whole too — grep verified it has NO test module at all, so the `mod tests`
/// cut never triggers for it. `src/providers/mod.rs` is deliberately NOT
/// scanned: it is the registration router whose job is to name every backend,
/// so its mentions are legitimate registration, not coupling.
///
/// Phase-4 visibility extension (2026-09-29): a file list that cannot see a
/// coupling reports a clean zero while the coupling lives one directory over —
/// worse than no instrument. Two such blind spots were found by audit:
/// (1) the `gsettings`/`darkman` probes MOVED from `src/theme.rs` into
/// `src/theme/desktop_preference/`, so the `("src/theme.rs", "gsettings")`
/// pin fell to 0 without the probe dying — the capability directory and its
/// two backends now join; (2) the dead `pgrep hyprmod` scaffolding lived in
/// `src/shell/gallery/slot.rs`, a file the `hyprmod` token never policed at
/// all — it joins too. Also added: `src/composer/hyprland.rs`, the largest
/// ungated `Command::new` host left in `src/` — its OWN Hyprland tokens are
/// exempt (sanctioned home, see `EXEMPT_TOKENS`), so what it adds is the 12
/// cross-backend tokens at pin 0: the base module can never reach a backend
/// unseen.
const SCANNED_FILES: &[&str] = &[
    "src/main.rs",
    "src/settings.rs",
    "src/theme_manager.rs",
    "src/shell/gallery/thumbs.rs",
    "src/theme.rs",
    "src/config_guard.rs",
    "src/hypr_ipc.rs",
    "src/providers/shell.rs",
    "assets/scripts/colors.sh",
    "assets/scripts/color_watcher.sh",
    // Phase-4 visibility: the read-desktop-preference capability directory.
    "src/theme/desktop_preference/mod.rs",
    "src/theme/desktop_preference/gsettings.rs",
    "src/theme/desktop_preference/darkman.rs",
    // Phase-4 visibility: the plan names this file for `pgrep hyprmod`.
    "src/shell/gallery/slot.rs",
    // Phase-4 visibility: the Hyprland base module (own tokens exempt).
    "src/composer/hyprland.rs",
    "src/providers/noctalia.rs",
    "src/providers/noctalia_runtime.rs",
    "src/providers/mpvpaper.rs",
    "src/providers/skwd_engine.rs",
    "src/providers/skwd_policy.rs",
    "src/providers/wallpaper_authority.rs",
    "src/providers/background.rs",
    "src/providers/bg_info.rs",
    "src/providers/hve_presets.rs",
    "src/providers/wallpaper.rs",
];

/// Integration names that must stay out of core files — and cross-backend
/// module names that must stay out of each OTHER's files. Matching is
/// case-insensitive substring on the token; paths are never lowercased
/// before matching (only the scanned body is).
///
/// Phase-2 additions, chosen so a NEW cross-backend call is caught while a
/// legitimate dependency is not:
/// - `skwd_policy`, `skwd_engine`, `bg_info` are the bare MODULE names. They
///   match the qualified path (`crate::providers::skwd_policy`) AND the
///   `use`-aliased / `super::` forms a sibling file would realistically
///   write, so they are strictly stronger than the `providers::`-qualified
///   spelling for the same single-token model.
/// - `providers::mpvpaper` exists because the bare `mpvpaper` token is
///   exempt in `noctalia.rs` (see `EXEMPT_TOKENS`): there it names
///   Noctalia's OWN plugin id (`noctalia/mpvpaper`), while a real module
///   dependency must stay policed.
/// - `providers::background` is qualified because the bare word
///   `background` is ordinary English ("the background engine owns the
///   scheme") and would force absurd, churning pins.
/// - Phase-4 visibility (2026-09-29): `darkman` joins so a NEW reach into
///   the `read-desktop-preference` probes is caught from ANY file: the
///   `gsettings` side was already policed (the token predates the move),
///   but nothing named the darkman backend. Smallest set that works: a core
///   file calling `Command::new("darkman")`, `...::darkman::DarkmanBackend`
///   or reading `~/.cache/darkman/mode` itself now fails at pin 0, while the
///   capability's sanctioned registration point and each backend's own
///   identity stay exempt (see `EXEMPT_TOKENS`). A `desktop_preference`
///   token was deliberately NOT added: the core calling the capability
///   entry point (`theme.rs -> read_desktop_preference()`) is the
///   architecture working as designed, and `gsettings`/`darkman` already
///   catch every probe-naming form.
const TOKENS: &[&str] = &[
    "hyprctl",
    "hyprland",
    "noctalia",
    "skwd",
    "mpvpaper",
    "quickshell",
    "matugen",
    "gsettings",
    "darkman",
    "hyprmod",
    "skwd_policy",
    "skwd_engine",
    "bg_info",
    "providers::mpvpaper",
    "providers::background",
];

/// (file, token) pairs the checker SKIPS: the token is the file's own
/// identity or domain, or the reference belongs to a category the contract
/// allows. Skipped pairs are still MEASURED and printed in the report (pin
/// column reads `exempt`); they simply never fail. Every entry below was
/// verified by reading the actual occurrences on 2026-09-29.
///
/// Why an exemption instead of a pin: pinning these would force absurd,
/// brittle numbers — 135 self-mentions of `noctalia` inside `noctalia.rs`
/// (its own log prefixes and plugin paths) would fail on any log-line edit —
/// and the failure message would give wrong advice ("move it into its
/// backend module") for references that are the architecture working as
/// designed.
const EXEMPT_TOKENS: &[(&str, &str)] = &[
    // A module's own identity: its log prefixes, ids, config paths and its
    // own neutral seam (noctalia <-> noctalia_runtime are one family).
    ("src/providers/noctalia.rs", "noctalia"),
    ("src/providers/noctalia_runtime.rs", "noctalia"),
    ("src/providers/mpvpaper.rs", "mpvpaper"),
    ("src/providers/skwd_engine.rs", "skwd"),
    ("src/providers/skwd_policy.rs", "skwd"),
    // Noctalia's OWN plugin id (`noctalia/mpvpaper`) and the local helpers
    // around it — not a reference to the mpvpaper MODULE; that dependency is
    // policed by the `providers::mpvpaper` pin (0).
    ("src/providers/noctalia.rs", "mpvpaper"),
    ("src/providers/noctalia_runtime.rs", "mpvpaper"),
    // The `apply-background` ROUTER: naming its backends is its whole job
    // (a router calling backends is the sanctioned shape). `skwd_policy`
    // joins `skwd_engine` there: the router now also routes the
    // colour-authority hold/release to the engine's OWN policy module —
    // the mechanism stays in the engine, only the caller changed.
    ("src/providers/background.rs", "skwd"),
    ("src/providers/background.rs", "mpvpaper"),
    ("src/providers/background.rs", "skwd_engine"),
    ("src/providers/background.rs", "skwd_policy"),
    ("src/providers/background.rs", "providers::mpvpaper"),
    // The ENGINE's own config schema: `skwd-wall-v2/config.json` carries a
    // `noctalia` section (`noctalia.themeMode`, one of the two ownership
    // signals), so the ownership read in `skwd_policy.rs` mentions the
    // substring while parsing the engine's OWN file format — a config key,
    // not a reference to the Noctalia backend. Measured: 3x (the
    // `get("noctalia")` string and its two reader bindings), all inside
    // `engine_owns_color_scheme`. A real reach (running the noctalia CLI,
    // naming a noctalia path) from this file stays policed at pin 0 via
    // the other tokens.
    ("src/providers/skwd_policy.rs", "noctalia"),
    // The SHARED info module names its sources; the three consumers read
    // shared manifest info, which is not a backend-to-backend chain.
    ("src/providers/bg_info.rs", "noctalia"),
    ("src/providers/bg_info.rs", "mpvpaper"),
    ("src/providers/noctalia.rs", "bg_info"),
    ("src/providers/mpvpaper.rs", "bg_info"),
    ("src/providers/background.rs", "bg_info"),
    // The read-only layer CLASSIFIER: painter names (skwd suite, mpvpaper)
    // are the markers it must recognise to classify layers. Its `hyprctl`
    // count stays PINNED instead — that is base-compositor knowledge whose
    // sanctioned home is `composer/hyprland.rs`.
    ("src/providers/wallpaper_authority.rs", "skwd"),
    ("src/providers/wallpaper_authority.rs", "mpvpaper"),
    // --- Phase-4 visibility (2026-09-29), verified by reading every
    // occurrence on the current tree ---
    // The `read-desktop-preference` capability directory: a backend's OWN
    // probe identity (same shape as `noctalia.rs` -> `noctalia`) and the
    // capability's REGISTRATION ROUTER naming the backends it registers
    // (same sanctioned-registration answer as `providers/mod.rs` and the
    // `apply-background` router above). Measured: `gsettings.rs` `gsettings`
    // 4x (struct + impl ids, argv), `darkman.rs` `darkman` 4x (ids, cache
    // path), `mod.rs` 3x each (`pub mod` + the `Default` registration).
    // A REAL probe reach from elsewhere — a core file or a sibling backend
    // naming either probe — is still policed at pin 0.
    ("src/theme/desktop_preference/gsettings.rs", "gsettings"),
    ("src/theme/desktop_preference/darkman.rs", "darkman"),
    ("src/theme/desktop_preference/mod.rs", "gsettings"),
    ("src/theme/desktop_preference/mod.rs", "darkman"),
    // `composer/hyprland.rs` is the SANCTIONED HOME of Hyprland knowledge
    // (spec: "Hyprland Is the Base, Never a Backend"), so `hyprctl` 18x and
    // `hyprland` 6x there are the architecture working as designed — and
    // pinning them would make the plan's OWN moves fail as growth in the
    // sanctioned destination (Phase 3 sends the raw `hyprctl` sites INTO
    // `Composer`; Phase 4 moves `hypr_ipc.rs` into the module). `hyprmod`
    // 36x: grep-verified 2026-09-29, ALL 36 matches are this file's own
    // `HyprMode` V4/V5 enum (`hypr_mode`, `HyprMode::V5`…) and ZERO are the
    // hyprmod TOOL — the substring is unusable here without an absurd,
    // wrong-advice pin. The 12 cross-backend tokens stay policed at pin 0.
    ("src/composer/hyprland.rs", "hyprctl"),
    ("src/composer/hyprland.rs", "hyprland"),
    ("src/composer/hyprland.rs", "hyprmod"),
];

fn is_exempt(file: &str, token: &str) -> bool {
    EXEMPT_TOKENS
        .iter()
        .any(|(f, t)| *f == file && *t == token)
}

/// Pinned (file, token, count) triples measured on 2026-09-28 by running
/// this scanner. A missing (file, token) pair pins zero: ANY occurrence is
/// a new coupling and fails. Each plan phase lowers these pins toward zero.
///
/// ## Phase-2 measured baseline (2026-09-29) — read before touching a pin
///
/// The provider modules joined `SCANNED_FILES` and the cross-backend tokens
/// joined `TOKENS`, so counts that were never measured before are now pinned
/// below. These entries are TODAY'S DEBT, measured on the current tree, not
/// approvals: the `noctalia → skwd_policy` chain the whole-phase audit found
/// was pinned at its real count (6) until the colour-authority re-owning
/// closed it (measured 0). **A phase
/// that lowers a coupling must lower its pin in the same commit; a pin is
/// never raised to make a new coupling pass** — raising it requires the plan
/// to re-home the reference deliberately (the failure message says so).
/// Exempt pairs (see `EXEMPT_TOKENS`) carry no pin by design.
const KNOWN_LEAKS: &[(&str, &str, usize)] = &[
    // Phase 3 (2026-09-29): the command path (`eval`, `dispatch` ×2) moved
    // behind `Composer`; measured 0 by the scanner — lowered 6 → 0.
    ("src/main.rs", "hyprctl", 0),
    ("src/main.rs", "hyprland", 12),
    ("src/main.rs", "noctalia", 13),
    ("src/main.rs", "skwd", 1),
    ("src/main.rs", "mpvpaper", 1),
    ("src/main.rs", "skwd_policy", 1),
    // Phase 3 (2026-09-29): `reload_hyprland` routes through the composer's
    // `reload-config` verb; measured 0 — lowered 2 → 0.
    ("src/settings.rs", "hyprctl", 0),
    ("src/settings.rs", "hyprland", 5),
    ("src/theme_manager.rs", "hyprland", 1),
    // Phase 3 (2026-09-29): the shared-palette cleanup moved behind the
    // provider's `deletable_artifacts` declaration; measured 0 — lowered 3 → 0.
    ("src/theme_manager.rs", "noctalia", 0),
    ("src/shell/gallery/thumbs.rs", "noctalia", 4),
    ("src/shell/gallery/thumbs.rs", "mpvpaper", 1),
    // Phase 4 (2026-09-29): the `read-desktop-preference` capability took
    // the probe out of the core (see `src/theme/desktop_preference/`);
    // measured 0 — lowered 1 → 0.
    ("src/theme.rs", "gsettings", 0),
    ("src/config_guard.rs", "hyprland", 2),
    ("src/hypr_ipc.rs", "hyprland", 9),
    ("src/providers/shell.rs", "hyprctl", 1),
    ("src/providers/shell.rs", "noctalia", 35),
    ("src/providers/shell.rs", "quickshell", 6),
    ("assets/scripts/colors.sh", "noctalia", 4),
    ("assets/scripts/colors.sh", "matugen", 2),
    ("assets/scripts/color_watcher.sh", "hyprland", 1),
    ("assets/scripts/color_watcher.sh", "noctalia", 1),
    ("assets/scripts/color_watcher.sh", "matugen", 1),
    // --- Phase-2 measured baseline (2026-09-29), provider modules ---
    // noctalia.rs: the audited violation, CLOSED by the colour-authority
    // re-owning (2026-09-29): the yield/restore, the config resolver, the
    // marker and the guard are reached through the `apply-background`
    // router now, so `skwd_policy` measures 0 (lowered 6 -> 0) and `skwd`
    // counts only the four log lines that still NAME the engine for the
    // user (measured 4; lowered 15 -> 4).
    ("src/providers/noctalia.rs", "skwd", 4),
    ("src/providers/noctalia.rs", "skwd_policy", 0),
    // noctalia.rs delegating apply-background to the ROUTER; Phase 2 may
    // move that selection to the core, which lowers this pin to 0.
    ("src/providers/noctalia.rs", "providers::background", 1),
    // mpvpaper.rs: the bidirectional `noctalia ↔ mpvpaper` chain the plan
    // breaks in Phase 2 (state dir + plugin helpers from noctalia_runtime).
    ("src/providers/mpvpaper.rs", "noctalia", 5),
    // wallpaper_authority.rs: raw `hyprctl` outside `composer/hyprland.rs`.
    ("src/providers/wallpaper_authority.rs", "hyprctl", 2),
    // hve_presets.rs declares its one integration claim: `shell() -> "hyprland"`.
    ("src/providers/hve_presets.rs", "hyprland", 1),
    // wallpaper.rs (inert: no `mod wallpaper;` today, but it ships and is
    // pending re-wiring) hardcodes Noctalia's cache paths for wallpapers.json.
    ("src/providers/wallpaper.rs", "noctalia", 4),
    // --- Phase-4 visibility baseline (2026-09-29), newly-scanned files ---
    // Read before touching: the desktop-preference capability directory,
    // `src/shell/gallery/slot.rs` and `src/composer/hyprland.rs` joined
    // `SCANNED_FILES`, so their (file, token) counts are measured here for
    // the FIRST time. Measured TODAY by running this scanner on the current
    // tree: every NON-EXEMPT pair in those five files is 0 — an absent
    // entry below pins 0 by the `pin_for` rule, so 0 is the baseline for
    // all of them (new coupling of any token fails immediately). The
    // non-zero pairs are identity/registration and carry `exempt` in the
    // report instead of a pin: `gsettings.rs` `gsettings` 4,
    // `darkman.rs` `darkman` 4, `mod.rs` `gsettings` 3 / `darkman` 3,
    // `composer/hyprland.rs` `hyprctl` 18 / `hyprland` 6 / `hyprmod` 36
    // (see `EXEMPT_TOKENS` for why each is measured-and-exempt, not pinned).
    // These are measured debt, never approvals: a later phase lowers any
    // count that drops — and only the plan's own re-homing may change them.
    // The single explicit pin below is the coupling the plan names for
    // slot.rs (`odd/tasks/hve-capability-routing.md`, Phase 4):
    ("src/shell/gallery/slot.rs", "hyprmod", 0),
];

/// Production code only: strip full-line and trailing `//` comments, then
/// drop everything from the test module to EOF.
///
/// Cut-heuristic verification (2026-09-29, by grep, all 25 scanned files):
/// every scanned Rust file that HAS a test module has exactly ONE line
/// containing `mod tests` — modulo the Phase-4 exceptions documented
/// below — and it opens the trailing test module (no
/// column-0 item follows it before the final `}`), so cutting at the first
/// line containing `mod tests` keeps all production code and no tests.
/// `src/providers/hve_presets.rs` has ZERO occurrences — no test module at
/// all — so it is scanned whole, like the
/// shell scripts. A literal "cut at the first `#[cfg(test)]`" would be
/// WRONG here: `src/main.rs:25-29` registers test-only modules
/// (`#[cfg(test)] mod scripts_contract;` …) at the top of the file, and
/// `src/theme_manager.rs:28` plus `src/providers/shell.rs:12,157` mention
/// `#[cfg(test)]` inside `//` comments (gone after comment stripping). No
/// scanned file interleaves tests with production code, so no file is
/// excluded.
///
/// Phase-4 additions, verified the same way on 2026-09-29 (three shapes
/// that are NOT the plain "exactly one trailing module" case, each checked
/// by listing every line containing `mod tests` AND every column-0 line
/// after the cut):
/// - `darkman.rs` — one `mod tests` (line 46), only the final `}` at
///   column 0 after it. `mod.rs` — one (line 118), final `}` at 180.
/// - `gsettings.rs` — one (line 50); the column-0 shell lines after it
///   (74-77) are content of the test module's `ARGV_GUARD` raw string,
///   well past the cut.
/// - `slot.rs` — TWO lines contain `mod tests`: 459 (the real module) and
///   793 (`.take_while(|line| !line.contains("mod tests"))` inside that
///   very module). The cut takes the FIRST — correct — and no column-0
///   non-brace line follows 459.
/// - `composer/hyprland.rs` — one `mod tests` (899); a second TEST module
///   (`#[cfg(test)] mod targeting_tests`, lines 1171-1265) follows it.
///   Column-0 lines after 899 are only `}` (1169) and that second test
///   module: no production item lives past the cut.
fn production_body(source: &str) -> String {
    let mut kept = Vec::new();
    for line in source.lines() {
        if line.contains("mod tests") {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        kept.push(code);
    }
    kept.join("\n")
}

/// Case-insensitive occurrence count of one token in an already-lowered body.
fn count_token(lowered_body: &str, token: &str) -> usize {
    lowered_body.matches(token).count()
}

fn pin_for(file: &str, token: &str) -> usize {
    KNOWN_LEAKS
        .iter()
        .find(|(f, t, _)| *f == file && *t == token)
        .map(|(_, _, n)| *n)
        .unwrap_or(0)
}

/// Check one production body against the pins; returns one message per
/// violated pin (empty = clean). Exempt (file, token) pairs — legitimate
/// dependencies documented in `EXEMPT_TOKENS` — are skipped here and shown
/// as `exempt` in the report.
fn check_body_against_pins(file: &str, lowered_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for token in TOKENS {
        if is_exempt(file, token) {
            continue;
        }
        let actual = count_token(lowered_body, token);
        let pin = pin_for(file, token);
        if actual > pin {
            failures.push(format!(
                "NEW COUPLING: {file} mentions `{token}` {actual}x (pin {pin}). \
                 Move it into its backend module, or — if the plan re-homed it \
                 deliberately — update the pin."
            ));
        } else if pin > actual {
            failures.push(format!(
                "STALE PIN: {file} mentions `{token}` {actual}x but the pin says {pin}. \
                 A phase of odd/tasks/hve-capability-routing.md removed a coupling: \
                 lower the pin to {actual}, never raise it."
            ));
        }
    }
    failures
}

fn read_production_body(file: &str) -> String {
    let source = std::fs::read_to_string(repo_root().join(file))
        .unwrap_or_else(|_| panic!("{file} must exist"));
    production_body(&source).to_lowercase()
}

#[test]
fn architecture_pins_hold_or_improve() {
    let mut failures = Vec::new();
    let mut report = String::from("file | token | actual | pin\n");
    for file in SCANNED_FILES {
        let body = read_production_body(file);
        failures.extend(check_body_against_pins(file, &body));
        for token in TOKENS {
            let actual = count_token(&body, token);
            let pin = if is_exempt(file, token) {
                String::from("exempt")
            } else {
                pin_for(file, token).to_string()
            };
            report.push_str(&format!("{file} | {token} | {actual} | {pin}\n"));
        }
    }
    println!("{report}");
    assert!(
        failures.is_empty(),
        "capability-routing contract violated (odd/tasks/hve-capability-routing.md):\n{}",
        failures.join("\n")
    );
}

/// The pin mechanism itself, over real production text: a leak added to a
/// TEMPORARY copy of a scanned file (the repo file is never touched) must be
/// reported as a new coupling.
#[test]
fn scanner_reports_a_new_leak_in_a_temp_copy() {
    let file = "src/settings.rs";
    let source = std::fs::read_to_string(repo_root().join(file)).unwrap();
    let clean = production_body(&source).to_lowercase();
    assert!(
        check_body_against_pins(file, &clean).is_empty(),
        "the test's baseline must be clean against the pins"
    );

    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("settings.rs");
    // Inject BEFORE the trailing test module: appending at EOF would land
    // past the `mod tests` cut and the scanner must (correctly) ignore it.
    let cut = source
        .find("mod tests")
        .expect("a scanned file must have a trailing test module");
    let mut tampered_source = source.clone();
    tampered_source.insert_str(
        cut,
        "// temporary probe, never committed\nfn probe_leak() { let _ = \"hyprctl reload\"; }\n",
    );
    std::fs::write(&copy, &tampered_source).unwrap();
    let tampered = std::fs::read_to_string(&copy).unwrap();
    let tampered_body = production_body(&tampered).to_lowercase();

    assert_eq!(
        count_token(&tampered_body, "hyprctl"),
        count_token(&clean, "hyprctl") + 1,
        "the scanner must see the injected leak"
    );
    let failures = check_body_against_pins(file, &tampered_body);
    assert!(
        failures.iter().any(|m| m.contains("NEW COUPLING") && m.contains("hyprctl")),
        "the pin check must flag the injected leak, got: {failures:?}"
    );
}

/// Coverage tripwire: the scanner must COVER every policed file and keep the
/// cross-backend tokens in its vocabulary. Without this tripwire, removing a
/// file from `SCANNED_FILES` (or a token from `TOKENS`) silently unpins every
/// coupling in it — the pin test would stay green while going blind.
#[test]
fn scanner_covers_every_policed_file_and_token() {
    for file in [
        "src/providers/noctalia.rs",
        "src/providers/noctalia_runtime.rs",
        "src/providers/mpvpaper.rs",
        "src/providers/skwd_engine.rs",
        "src/providers/skwd_policy.rs",
        "src/providers/wallpaper_authority.rs",
        "src/providers/background.rs",
        "src/providers/bg_info.rs",
        "src/providers/hve_presets.rs",
        "src/providers/wallpaper.rs",
        // Phase-4 blind spots (2026-09-29): the coupling moved out of sight
        // of the old file list — see the two injection tests below.
        "src/theme/desktop_preference/mod.rs",
        "src/theme/desktop_preference/gsettings.rs",
        "src/theme/desktop_preference/darkman.rs",
        "src/shell/gallery/slot.rs",
        "src/composer/hyprland.rs",
    ] {
        assert!(
            SCANNED_FILES.contains(&file),
            "the scanner no longer covers {file}: every coupling in it \
             becomes invisible"
        );
    }
    for token in [
        "skwd_policy",
        "skwd_engine",
        "bg_info",
        "providers::mpvpaper",
        "providers::background",
        "darkman",
    ] {
        assert!(
            TOKENS.contains(&token),
            "the cross-backend token `{token}` left the vocabulary: a new reference \
             to it would go unpoliced"
        );
    }
}

/// Phase-4 blind spot: the dead `pgrep hyprmod` scaffolding was deleted from
/// `src/shell/gallery/slot.rs`, but the `hyprmod` token never policed this
/// file, so the scanner could never have caught it. The file must be COVERED,
/// and a re-injected leak — into a TEMPORARY copy (the repo file is never
/// touched) — must be reported as a new coupling.
#[test]
fn scanner_covers_gallery_slot_and_reports_an_injected_hyprmod_leak() {
    let file = "src/shell/gallery/slot.rs";
    assert!(
        SCANNED_FILES.contains(&file),
        "the scanner no longer covers {file}: the `pgrep hyprmod` coupling the plan \
         names for this file (odd/tasks/hve-capability-routing.md, Phase 4) becomes \
         invisible"
    );

    let source = std::fs::read_to_string(repo_root().join(file)).unwrap();
    let clean = production_body(&source).to_lowercase();
    assert!(
        check_body_against_pins(file, &clean).is_empty(),
        "today's measured baseline for slot.rs must be clean against the pins"
    );

    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("slot.rs");
    // Inject BEFORE the trailing test module: appending at EOF would land
    // past the `mod tests` cut and the scanner must (correctly) ignore it.
    let cut = source
        .find("mod tests")
        .expect("slot.rs must have a trailing test module");
    let mut tampered_source = source.clone();
    tampered_source.insert_str(
        cut,
        "// temporary probe, never committed\n\
         fn probe_leak() { let _ = \"pgrep -x hyprmod\"; }\n",
    );
    std::fs::write(&copy, &tampered_source).unwrap();
    let tampered = std::fs::read_to_string(&copy).unwrap();
    let tampered_body = production_body(&tampered).to_lowercase();

    assert_eq!(
        count_token(&tampered_body, "hyprmod"),
        count_token(&clean, "hyprmod") + 1,
        "the scanner must see the injected leak"
    );
    let failures = check_body_against_pins(file, &tampered_body);
    assert!(
        failures
            .iter()
            .any(|m| m.contains("NEW COUPLING") && m.contains("hyprmod")),
        "the pin check must flag the injected slot.rs leak, got: {failures:?}"
    );
}

/// Phase-4 blind spot #2: the `gsettings`/`darkman` probes moved out of the
/// core into `src/theme/desktop_preference/`, so the core pin fell to 0 while
/// the coupling only moved one directory over — unpoliced. The scanner must
/// COVER the capability directory, must NOT flag its sanctioned registration
/// point or a backend's own identity, and must catch a NEW sibling reach
/// injected into a TEMPORARY copy (the repo file is never touched).
#[test]
fn scanner_covers_desktop_preference_backends_and_catches_a_sibling_reach() {
    let backend_file = "src/theme/desktop_preference/gsettings.rs";
    let source = std::fs::read_to_string(repo_root().join(backend_file)).unwrap();
    let clean = production_body(&source).to_lowercase();

    // A backend reaching its SIBLING backend — here `gsettings.rs` spawning
    // the darkman probe itself (spec: "Backends Are Siblings, Never a
    // Chain") — must be reported, never silently unpoliced.
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("gsettings.rs");
    let cut = source
        .find("mod tests")
        .expect("gsettings.rs must have a trailing test module");
    let mut tampered_source = source.clone();
    tampered_source.insert_str(
        cut,
        "// temporary probe, never committed\n\
         fn probe_leak() { let _ = std::process::Command::new(\"darkman\").arg(\"mode\"); }\n",
    );
    std::fs::write(&copy, &tampered_source).unwrap();
    let tampered = std::fs::read_to_string(&copy).unwrap();
    let tampered_body = production_body(&tampered).to_lowercase();

    assert_eq!(
        count_token(&tampered_body, "darkman"),
        count_token(&clean, "darkman") + 1,
        "the scanner must see the injected sibling reach"
    );
    let failures = check_body_against_pins(backend_file, &tampered_body);
    assert!(
        failures
            .iter()
            .any(|m| m.contains("NEW COUPLING") && m.contains("darkman")),
        "the pin check must flag the injected gsettings -> darkman reach, got: \
         {failures:?}"
    );

    // Coverage: the pin test only reads files listed here — and the
    // sanctioned registration point plus each backend's own identity must
    // stay CLEAN, because `EXEMPT_TOKENS` carries their probe mentions.
    for file in [
        "src/theme/desktop_preference/mod.rs",
        "src/theme/desktop_preference/gsettings.rs",
        "src/theme/desktop_preference/darkman.rs",
    ] {
        assert!(
            SCANNED_FILES.contains(&file),
            "the scanner no longer covers {file}: the probe coupling it hosts \
             becomes invisible"
        );
        let body =
            production_body(&std::fs::read_to_string(repo_root().join(file)).unwrap())
                .to_lowercase();
        let failures = check_body_against_pins(file, &body);
        assert!(
            failures.is_empty(),
            "the legitimate registration/identity mentions in {file} must stay clean \
             (see EXEMPT_TOKENS), got: {failures:?}"
        );
    }
}

/// The scanner must see the REAL violation the whole-phase audit found —
/// `noctalia.rs` configuring the skwd backend through `skwd_policy.rs` —
/// injected into a TEMPORARY copy of the provider file (the repo file is
/// never touched), and must NOT call today's pinned debt a new coupling.
#[test]
fn scanner_reports_the_known_violation_in_a_provider_temp_copy() {
    let file = "src/providers/noctalia.rs";
    let source = std::fs::read_to_string(repo_root().join(file)).unwrap();
    let clean = production_body(&source).to_lowercase();
    let baseline = check_body_against_pins(file, &clean);
    assert!(
        baseline.is_empty(),
        "today's measured debt must be PINNED, not reported as new: {baseline:?}"
    );

    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("noctalia.rs");
    // Inject BEFORE the trailing test module: appending at EOF would land
    // past the `mod tests` cut and the scanner must (correctly) ignore it.
    let cut = source
        .find("mod tests")
        .expect("noctalia.rs must have a trailing test module");
    let mut tampered_source = source.clone();
    tampered_source.insert_str(
        cut,
        "// temporary probe, never committed\n\
         fn probe_leak() { let _ = crate::providers::skwd_policy::config_path(); }\n",
    );
    std::fs::write(&copy, &tampered_source).unwrap();
    let tampered = std::fs::read_to_string(&copy).unwrap();
    let tampered_body = production_body(&tampered).to_lowercase();

    assert_eq!(
        count_token(&tampered_body, "skwd_policy"),
        count_token(&clean, "skwd_policy") + 1,
        "the scanner must see the injected noctalia -> skwd_policy reference"
    );
    let failures = check_body_against_pins(file, &tampered_body);
    assert!(
        failures
            .iter()
            .any(|m| m.contains("NEW COUPLING") && m.contains("skwd_policy")),
        "the pin check must flag the injected violation, got: {failures:?}"
    );
}

/// Both failure directions of the pin check, over synthetic bodies.
#[test]
fn pin_check_fails_both_directions() {
    let pins: &[(&str, &str, usize)] = &[("probe.rs", "noctalia", 2)];
    let pin = |token: &str| {
        pins
            .iter()
            .find(|(f, t, _)| *f == "probe.rs" && *t == token)
            .map(|(_, _, n)| *n)
            .unwrap_or(0)
    };
    let check = |body: &str| {
        let mut out = Vec::new();
        for token in ["noctalia", "hyprctl"] {
            let actual = count_token(body, token);
            let p = pin(token);
            if actual > p {
                out.push(format!("NEW COUPLING {token}"));
            } else if p > actual {
                out.push(format!("STALE PIN {token}"));
            }
        }
        out
    };

    let grown = check("noctalia noctalia noctalia");
    assert!(
        grown.iter().any(|m| m.contains("NEW COUPLING")),
        "a count above its pin must fail: {grown:?}"
    );
    let shrunk = check("noctalia");
    assert!(
        shrunk.iter().any(|m| m.contains("STALE PIN")),
        "a pin above its count must fail so pins get lowered: {shrunk:?}"
    );
}
