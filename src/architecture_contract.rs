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
//! whole-phase audit found — was invisible). The pin test fails when any
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
const TOKENS: &[&str] = &[
    "hyprctl",
    "hyprland",
    "noctalia",
    "skwd",
    "mpvpaper",
    "quickshell",
    "matugen",
    "gsettings",
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
    // (a router calling backends is the sanctioned shape).
    ("src/providers/background.rs", "skwd"),
    ("src/providers/background.rs", "mpvpaper"),
    ("src/providers/background.rs", "skwd_engine"),
    ("src/providers/background.rs", "providers::mpvpaper"),
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
/// is pinned at its real count (6) so it can only ratchet down. **A phase
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
    ("src/theme_manager.rs", "noctalia", 3),
    ("src/shell/gallery/thumbs.rs", "noctalia", 4),
    ("src/shell/gallery/thumbs.rs", "mpvpaper", 1),
    ("src/theme.rs", "gsettings", 1),
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
    // noctalia.rs: the audited violation. `skwd_policy` 6x = the module
    // configuring the skwd backend through `skwd_policy.rs` (config_path /
    // yield / restore); `skwd` 15x counts every skwd mention including the
    // config-path strings. Fixing the chain lowers both pins.
    ("src/providers/noctalia.rs", "skwd", 15),
    ("src/providers/noctalia.rs", "skwd_policy", 6),
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
];

/// Production code only: strip full-line and trailing `//` comments, then
/// drop everything from the test module to EOF.
///
/// Cut-heuristic verification (2026-09-29, by grep, all 20 scanned files):
/// every scanned Rust file that HAS a test module has exactly ONE line
/// containing `mod tests`, and it opens the trailing test module (no
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

/// Phase-2 blind spot: the scanner must COVER the provider modules and keep
/// the cross-backend tokens in its vocabulary. Without this tripwire, removing
/// a file from `SCANNED_FILES` (or a token from `TOKENS`) silently unpins
/// every coupling in it — the pin test would stay green while going blind.
#[test]
fn scanner_covers_provider_modules_and_tokens() {
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
    ] {
        assert!(
            SCANNED_FILES.contains(&file),
            "the scanner no longer covers {file}: every provider-level coupling in it \
             becomes invisible"
        );
    }
    for token in [
        "skwd_policy",
        "skwd_engine",
        "bg_info",
        "providers::mpvpaper",
        "providers::background",
    ] {
        assert!(
            TOKENS.contains(&token),
            "the cross-backend token `{token}` left the vocabulary: a new reference \
             to it would go unpoliced"
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
