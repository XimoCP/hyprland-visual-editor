//! Capability-routing architecture contract (`odd/tasks/hve-capability-routing.md`).
//!
//! The keeper's two inviolable rules: (1) Hyprland is the base, never a
//! backend — Hyprland-specific knowledge lives in `src/composer/hyprland.rs`
//! (plus the Hyprland-owned guard/markers), never scattered through core
//! files; (2) backends are siblings, never a chain — a backend never calls,
//! waits for, or configures another backend.
//!
//! This module pins TODAY's per-file integration-token counts for the core
//! files named in the plan's definition of done. The pin test fails when any
//! count GROWS (a new coupling leaked into a core file) and also when a pin
//! is HIGHER than the measured count (a stale pin — lower it, never raise
//! it without moving the coupling into its backend module). Each phase of
//! the plan lowers these pins toward zero.
#![cfg(test)]

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Core files the plan forbids editing when adding a backend. Shell scripts
/// are scanned whole (they have no `mod tests`); see `production_body`.
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
];

/// Integration names that must stay out of core files. Matching is
/// case-insensitive substring on the token; paths are never lowercased
/// before matching (only the scanned body is).
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
];

/// Pinned (file, token, count) triples measured on 2026-09-28 by running
/// this scanner. A missing (file, token) pair pins zero: ANY occurrence is
/// a new coupling and fails. Each plan phase lowers these pins toward zero.
const KNOWN_LEAKS: &[(&str, &str, usize)] = &[
    ("src/main.rs", "hyprctl", 9),
    ("src/main.rs", "hyprland", 12),
    ("src/main.rs", "noctalia", 13),
    ("src/main.rs", "skwd", 1),
    ("src/main.rs", "mpvpaper", 1),
    ("src/settings.rs", "hyprctl", 2),
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
    ("assets/scripts/colors.sh", "hyprland", 2),
    ("assets/scripts/colors.sh", "noctalia", 12),
    ("assets/scripts/colors.sh", "matugen", 11),
    ("assets/scripts/color_watcher.sh", "hyprland", 2),
    ("assets/scripts/color_watcher.sh", "noctalia", 32),
    ("assets/scripts/color_watcher.sh", "matugen", 7),
];

/// Production code only: strip full-line and trailing `//` comments, then
/// drop everything from the test module to EOF.
///
/// Cut-heuristic verification (2026-09-28, by grep): every scanned Rust file
/// has exactly ONE `mod tests` line and it opens the trailing test module,
/// so cutting at the first line containing `mod tests` keeps all production
/// code and no tests. A literal "cut at the first `#[cfg(test)]`" would be
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
/// violated pin (empty = clean).
fn check_body_against_pins(file: &str, lowered_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for token in TOKENS {
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
            report.push_str(&format!(
                "{file} | {token} | {} | {}\n",
                count_token(&body, token),
                pin_for(file, token)
            ));
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
