//! W1 of `odd/tasks/skwd-policy-toggle-around-apply.md`: primitives to
//! TEMPORARILY yield the skwd-wall engine's colour authority around one
//! theme apply.
//!
//! WHY this exists: the keeper runs the third-party skwd-wall engine as the
//! colour AUTHORITY (`theme.policy == "wallpaper"` in its config), so after
//! HVE hands it a wallpaper it regenerates a wallpaper-derived palette
//! asynchronously and re-imposes it, showing the wrong colours for ~4 s.
//! The keeper's design: set `theme.policy` to `off` for the duration of the
//! apply (the engine then leaves the scheme alone and the theme palette
//! sticks), and put the value back EXACTLY as it was afterwards.
//!
//! Scope of THIS module (W1): the pure planners plus the thin impure shell.
//! W2 (separate) wires `yield_color_authority` / `restore_color_authority` /
//! `recover_crashed_yield` around the apply path. Nothing here calls
//! `skwd-helm` or any engine command — the engine's config FILE is the only
//! interface, on purpose.
//!
//! ## The anchor rule (why the edit can only target ONE token)
//!
//! The keeper's config carries TWO kinds of `policy` keys:
//!
//! - the general switch `theme.policy` (the one HVE flips), and
//! - per-wallpaper pinned profiles under `theme.wallpaperProfiles`, whose
//!   entries carry the DOTTED key `"theme.policy"`.
//!
//! The dotted `"theme.policy"` key does NOT contain the bare 8-char token
//! `"policy"` (a `theme.` prefix sits right before `policy`), so the bare
//! token is a unique, byte-level anchor for the general switch. The edit
//! therefore REFUSES when the anchor is ambiguous: zero occurrences means
//! the general switch is absent, two or more means a hidden second
//! `"policy"` key could be mistaken for the switch. Text preservation is a
//! hard requirement — only the ONE value token changes; every other byte
//! of the document stays identical (the pinned profiles included).
//!
//! ## Marker protocol (crash safety)
//!
//! A crash between the yield flip and the restore would leave the engine
//! permanently off. A small marker file under the HVE cache dir
//! (`$XDG_CACHE_HOME/hve/skwd-policy-yield.json`, falling back to
//! `~/.cache/hve/...` via the codebase's `hve_cache_dir` helper) records
//! the previous value AND the config path that was flipped. The marker is
//! written BEFORE the flip (a marker without a flip is a harmless no-op for
//! recovery; a flip without a marker would be unrecoverable) and cleared
//! only after the value is back. `recover_crashed_yield` replays the repair
//! on the next start.
//!
//! ## Never-panic contract
//!
//! Every failure is an honest `Result` error the caller logs and carries on
//! from; a failed write never aborts an apply. A missing or unparsable
//! config means "no ownership": behave exactly as today.
//!
//! ## Sandboxing promise (tests)
//!
//! All tests redirect the `SKWD_WALL_V2_CONFIG` seam at a private temp file;
//! the keeper's live `/home/ximo/.config/skwd-wall-v2/config.json` is never
//! read, written or probed by any code in this module or its tests.

// W1 primitives are dormant until W2 wires them into the apply path: the
// binary build would flag everything as dead code. House pattern from
// `preset_store.rs` ("used by upcoming feature") — the allow is a promise
// that the wiring lands in W2, not a private admission of unused code.
#![allow(dead_code)]

use crate::config::hve_cache_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Pure plan of ONE yield: the exact edited document text plus the value
/// that must be put back by the matching restore.
#[derive(Debug, Clone)]
pub(crate) struct YieldPlan {
    pub(crate) new_text: String,
    pub(crate) previous_value: String,
}

/// Pure plan of ONE restore: the exact edited document text.
#[derive(Debug, Clone)]
pub(crate) struct RestorePlan {
    pub(crate) new_text: String,
}

/// Point-in-time record of a yield, so a crash between the flip and the
/// restore can be repaired on the next start.
#[derive(Debug, Serialize, Deserialize)]
struct YieldMarker {
    previous_value: String,
    config_path: String,
}

// ── Path resolver (the single seam) ──────────────────────────────────

/// Path to the skwd-wall engine config.
///
/// `SKWD_WALL_V2_CONFIG` overrides outright (tests point it at a temp file,
/// including a nonexistent path for the missing case); otherwise the
/// platform config dir (`$XDG_CONFIG_HOME/skwd-wall-v2/config.json`,
/// i.e. `~/.config/...`), which the `TempEnv` test sandbox already
/// redirects via HOME.
///
/// Single-resolver rule: `noctalia::skwd_wall_config_path` delegates here,
/// so a future change cannot fork the two copies.
pub(crate) fn config_path() -> PathBuf {
    if let Ok(custom) = std::env::var("SKWD_WALL_V2_CONFIG") {
        return PathBuf::from(custom);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"))
        .join("skwd-wall-v2")
        .join("config.json")
}

/// Crash marker location: HVE cache dir + explicit, documented name.
pub(crate) fn marker_path() -> PathBuf {
    hve_cache_dir().join("skwd-policy-yield.json")
}

// ── Pure planners ────────────────────────────────────────────────────

/// The anchor for the edit: the BARE `"policy"` key token, i.e. the exact
/// 8-char substring `"policy"`. The per-wallpaper pinned profiles write the
/// DOTTED key `"theme.policy"` — a `theme.` prefix sits right before
/// `policy` inside the JSON string, so the dotted key does NOT contain the
/// bare token and never counts as an anchor. The edit refuses when the
/// anchor is not exactly ONE occurrence: zero means the general switch is
/// absent, two or more means another `"policy"` key hides where the real
/// value lives, and guessing could edit the wrong document region.
fn locate_policy_value_token(text: &str) -> Option<(Range<usize>, &str)> {
    const ANCHOR: &str = "\"policy\"";
    let mut occurrences = text.match_indices(ANCHOR);
    let (key_start, _) = occurrences.next()?;
    if occurrences.next().is_some() {
        return None; // ambiguous anchor: exactly one bare token is required
    }
    let range = value_token_after(text, key_start + ANCHOR.len())?;
    Some((range.clone(), &text[range]))
}

/// Range of the JSON string token that is the VALUE of the key whose token
/// ends at `after_key`. Scans JSON whitespace, the `:` separator, more
/// whitespace, then the quoted token (honouring backslash escapes).
fn value_token_after(text: &str, after_key: usize) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let mut i = after_key;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'"' {
        return None;
    }
    let start = i;
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return Some(start..i + 1);
        }
        i += 1;
    }
    None
}

/// Rebuild the document with exactly one token replaced; every other byte
/// of the input is copied verbatim.
fn replace_token(text: &str, range: &Range<usize>, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() - range.len() + replacement.len());
    out.push_str(&text[..range.start]);
    out.push_str(replacement);
    out.push_str(&text[range.end..]);
    out
}

/// Decide whether a yield is due and produce the edited document.
///
/// Returns `None` (no yield) when the text is unparsable, the `policy` key
/// is absent, the value is anything other than `"wallpaper"`, or the bare
/// `"policy"` anchor is not exactly ONE occurrence (see the module docs for
/// the anchor rule).
pub(crate) fn plan_yield(text: &str) -> Option<YieldPlan> {
    // Parse FIRST: an unparsable document can never be an ownership claim.
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let policy = parsed.get("theme")?.get("policy")?.as_str()?;
    if policy != "wallpaper" {
        // fixed/off/missing → the engine does not own the scheme today.
        return None;
    }
    let (value_range, value_token) = locate_policy_value_token(text)?;
    // The value token must be the plain `"wallpaper"` string. Exotic-but-
    // valid encodings (e.g. `"\u0077allpaper"`) are REFUSED, never
    // mis-edited: unusual text is a signal to back off, not to guess.
    if value_token != "\"wallpaper\"" {
        return None;
    }
    Some(YieldPlan {
        new_text: replace_token(text, &value_range, "\"off\""),
        previous_value: "wallpaper".to_string(),
    })
}

/// Decide whether a restore is due and produce the edited document.
///
/// Only ever rewrites OUR OWN write: when the current value is not the
/// `"off"` token the yield wrote (the keeper or the engine changed it in
/// between, or the yield never happened), it returns `None` and the caller
/// keeps the file as-is — a concurrent write is never clobbered.
pub(crate) fn plan_restore(text: &str, previous_value: &str) -> Option<RestorePlan> {
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let (value_range, value_token) = locate_policy_value_token(text)?;
    // The current value must be exactly the token the yield wrote. This
    // guard also makes crash recovery idempotent: after a successful
    // restore the value is the previous one, not "off", so a stale marker
    // is cleared without re-editing the file.
    if value_token != "\"off\"" {
        return None;
    }
    // Semantic double-check: the parsed document must agree that the switch
    // is currently off (the token scan and the parse cannot disagree once
    // both say "off" with a single anchor — but each check is cheap).
    if parsed.get("theme").and_then(|t| t.get("policy")).and_then(|p| p.as_str()) != Some("off")
    {
        return None;
    }
    // Emit the remembered value JSON-escaped, so even a value with embedded
    // quotes or escapes comes back byte-faithfully.
    let replacement = serde_json::to_string(previous_value).ok()?;
    Some(RestorePlan {
        new_text: replace_token(text, &value_range, &replacement),
    })
}

// ── Impure shell ─────────────────────────────────────────────────────

/// Write `content` to `path` ATOMICALLY and preserve the file's mode.
///
/// The temp file lives in the SAME directory as the target, so the rename
/// is a same-filesystem atomic swap: the engine can never observe a
/// half-written config. The target's current permissions (the keeper's live
/// config is `0600`) are read BEFORE the swap and re-applied to the temp
/// file; on non-unix platforms the mode step is skipped. A crash may leave
/// a stray temp file behind — the next write truncates it.
fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("[skwd-policy] cannot atomically write {}: no parent dir", path.display()))?;
    let tmp = dir.join(".skwd-policy-yield.tmp");
    let mode = fs::metadata(path).ok().map(|m| m.permissions());
    {
        let mut f = fs::File::create(&tmp)
            .map_err(|e| format!("[skwd-policy] cannot create {}: {}", tmp.display(), e))?;
        f.write_all(content.as_bytes())
            .map_err(|e| format!("[skwd-policy] cannot write {}: {}", tmp.display(), e))?;
        f.sync_all()
            .map_err(|e| format!("[skwd-policy] cannot sync {}: {}", tmp.display(), e))?;
    }
    #[cfg(unix)]
    if let Some(perms) = &mode {
        fs::set_permissions(&tmp, perms.clone()).map_err(|e| {
            format!("[skwd-policy] cannot preserve mode on {}: {}", tmp.display(), e)
        })?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!(
            "[skwd-policy] atomic rename {} -> {} failed: {}",
            tmp.display(),
            path.display(),
            e
        )
    })?;
    Ok(())
}

fn write_marker(config_path: &Path, previous_value: &str) -> Result<(), String> {
    let marker = YieldMarker {
        previous_value: previous_value.to_string(),
        config_path: config_path.display().to_string(),
    };
    let raw = serde_json::to_string(&marker)
        .map_err(|e| format!("[skwd-policy] cannot serialize yield marker: {e}"))?;
    let path = marker_path();
    let dir = path
        .parent()
        .ok_or_else(|| format!("[skwd-policy] marker {} has no parent dir", path.display()))?;
    fs::create_dir_all(dir)
        .map_err(|e| format!("[skwd-policy] cannot create cache dir {}: {}", dir.display(), e))?;
    let tmp = dir.join(".skwd-policy-yield-marker.tmp");
    fs::write(&tmp, raw)
        .map_err(|e| format!("[skwd-policy] cannot write marker {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!(
            "[skwd-policy] cannot install marker {}: {}",
            path.display(),
            e
        )
    })?;
    Ok(())
}

fn clear_marker() -> Result<(), String> {
    let path = marker_path();
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "[skwd-policy] cannot clear yield marker {}: {}",
            path.display(),
            e
        )),
    }
}

fn read_marker() -> Result<Option<YieldMarker>, String> {
    let path = marker_path();
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "[skwd-policy] cannot read yield marker {}: {}",
                path.display(),
                e
            ))
        }
    };
    serde_json::from_str(&raw).map(Some).map_err(|e| {
        format!(
            "[skwd-policy] yield marker {} is unparsable: {}",
            path.display(),
            e
        )
    })
}

/// Flip `theme.policy` to `off` when the engine owns the scheme.
///
/// `Ok(None)`: no ownership (missing/unparsable config, non-`wallpaper`
/// value, ambiguous anchor) — nothing was written. `Ok(Some(previous))`:
/// the flip happened, the engine is held off, and the caller MUST arrange
/// a matching `restore_color_authority(&previous)`. `Err`: honest failure;
/// the caller logs and carries on.
pub(crate) fn yield_color_authority() -> Result<Option<String>, String> {
    let path = config_path();
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "[skwd-policy] cannot read engine config {}: {}",
                path.display(),
                e
            ))
        }
    };
    let plan = match plan_yield(&text) {
        Some(p) => p,
        None => return Ok(None),
    };
    // Marker FIRST, flip second: if the process dies between the two steps
    // the marker says what to restore; the reverse order would leave the
    // flip unrecoverable. A marker without a flip is a harmless no-op for
    // the next `recover_crashed_yield`.
    write_marker(&path, &plan.previous_value)?;
    write_atomic(&path, &plan.new_text)?;
    tracing::info!(
        "[skwd-policy] yielding engine colour authority for the apply: \
         theme.policy \"{}\" -> \"off\" ({})",
        plan.previous_value,
        path.display()
    );
    Ok(Some(plan.previous_value))
}

/// Put the previous value back and stop yielding.
///
/// Re-reads the file first (a concurrent engine write is never clobbered
/// with a stale copy), restores the remembered value, clears the marker.
/// Returns `Ok(true)` when the file was rewritten, `Ok(false)` when there
/// was nothing to do (config gone, or the current value is not ours to flip
/// back).
pub(crate) fn restore_color_authority(previous_value: &str) -> Result<bool, String> {
    let path = config_path();
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Engine config is gone: nothing to restore, and the pending
            // claim is moot. Clear the marker and report "nothing written".
            clear_marker()?;
            return Ok(false);
        }
        Err(e) => {
            return Err(format!(
                "[skwd-policy] cannot re-read engine config {}: {}",
                path.display(),
                e
            ))
        }
    };
    let plan = match plan_restore(&text, previous_value) {
        Some(p) => p,
        None => {
            // The current value is not the one we wrote (a concurrent
            // engine or keeper write won, or the yield never happened).
            // Never clobber a foreign value: back off, clear the marker,
            // keep the file as-is.
            clear_marker()?;
            return Ok(false);
        }
    };
    // `text` is the fresh file, so a concurrent engine write since the
    // yield is never clobbered with the stale flipped copy.
    write_atomic(&path, &plan.new_text)?;
    clear_marker()?;
    tracing::info!(
        "[skwd-policy] restored engine colour authority: theme.policy -> \
         \"{}\" ({})",
        previous_value,
        path.display()
    );
    Ok(true)
}

/// Repair a yield that a crash left un-restored: reads the marker, puts
/// the previous value back, clears the marker. Returns `Ok(true)` when a
/// marker was found and handled, `Ok(false)` when none existed.
pub(crate) fn recover_crashed_yield() -> Result<bool, String> {
    let marker = match read_marker()? {
        Some(m) => m,
        None => return Ok(false),
    };
    let path = PathBuf::from(&marker.config_path);
    tracing::warn!(
        "[skwd-policy] crash marker found ({}): a previous apply died \
         between yield and restore; repairing theme.policy",
        marker_path().display()
    );
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Config no longer exists; nothing to repair.
            clear_marker()?;
            return Ok(true);
        }
        Err(e) => {
            return Err(format!(
                "[skwd-policy] cannot read engine config {} for crash \
                 repair: {}",
                path.display(),
                e
            ))
        }
    };
    if let Some(plan) = plan_restore(&text, &marker.previous_value) {
        write_atomic(&path, &plan.new_text)?;
        tracing::warn!(
            "[skwd-policy] crash repair: theme.policy put back to \"{}\"",
            marker.previous_value
        );
    } else {
        tracing::warn!(
            "[skwd-policy] crash repair: nothing to do (value already \
             restored, or changed by the keeper in the meantime)"
        );
    }
    clear_marker()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    /// Realistic keeper-document shape: nested `theme` object carrying the
    /// general `policy` switch, PLUS a `wallpaperProfiles` array whose
    /// entries carry the dotted `"theme.policy"` key (pinned per-wallpaper
    /// profiles). Exactly ONE bare `"policy"` token; two dotted ones; three
    /// `"wallpaper"` value tokens — a shape that hides the ambiguity the
    /// anchor rule exists to refuse.
    const FIXTURE: &str = r#"{
  "theme": {
    "policy": "wallpaper",
    "wallpaperProfiles": [
      {
        "match": "static:Cars/car7.jpg",
        "theme.policy": "wallpaper",
        "blur": true
      },
      {
        "match": "static:wallp9.jpg",
        "theme.policy": "wallpaper"
      }
    ]
  },
  "noctalia": {
    "themeMode": "follow"
  },
  "outputs": {
    "eDP-1": {
      "scale": "1.25"
    }
  }
}"#;

    /// Two bare `"policy"` anchors: the general switch plus a hidden second
    /// key. The edit MUST refuse — which one is the switch to flip?
    const AMBIGUOUS: &str =
        r#"{"theme": {"policy": "wallpaper"}, "other": {"policy": "fixed"}}"#;

    /// Hermetic shell harness. `TempEnv` redirects HOME/XDG* under the
    /// shared env lock AND the `SKWD_WALL_V2_CONFIG` seam is redirected at
    /// a private temp file, so no test can ever touch the keeper's live
    /// `/home/ximo/.config/skwd-wall-v2/config.json`.
    struct PolicyEnv {
        _env: TempEnv,
        _tmp: tempfile::TempDir,
        config: PathBuf,
        saved_skwd: Option<String>,
    }

    impl PolicyEnv {
        fn new(config_text: Option<&str>, mode: Option<u32>) -> Self {
            let env = TempEnv::new();
            let saved_skwd = std::env::var("SKWD_WALL_V2_CONFIG").ok();
            let tmp = tempfile::tempdir().expect("create policy sandbox");
            let config = tmp.path().join("config.json");
            if let Some(text) = config_text {
                fs::write(&config, text).unwrap();
            }
            if let Some(m) = mode {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&config, fs::Permissions::from_mode(m)).unwrap();
                }
            }
            std::env::set_var("SKWD_WALL_V2_CONFIG", &config);
            Self {
                _env: env,
                _tmp: tmp,
                config,
                saved_skwd,
            }
        }

        fn marker(&self) -> PathBuf {
            hve_cache_dir().join("skwd-policy-yield.json")
        }

        fn read(&self) -> String {
            fs::read_to_string(&self.config).unwrap()
        }

        fn mode(&self) -> u32 {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::metadata(&self.config).unwrap().permissions().mode() & 0o777
            }
            #[cfg(not(unix))]
            {
                0
            }
        }
    }

    impl Drop for PolicyEnv {
        fn drop(&mut self) {
            match &self.saved_skwd {
                Some(v) => std::env::set_var("SKWD_WALL_V2_CONFIG", v),
                None => std::env::remove_var("SKWD_WALL_V2_CONFIG"),
            }
        }
    }

    // ── pure planner: yield ──

    #[test]
    fn yield_plan_flips_only_the_policy_value_token() {
        let plan = plan_yield(FIXTURE).expect("wallpaper policy must yield a plan");
        assert_eq!(plan.previous_value, "wallpaper");

        // Independent, implementation-agnostic expected text: the first
        // `"wallpaper"` value token after the single bare `"policy"` anchor
        // is the one and only token that may change.
        let anchor = "\"policy\"";
        let key_end = FIXTURE.find(anchor).unwrap() + anchor.len();
        let val_start = FIXTURE[key_end..].find("\"wallpaper\"").unwrap() + key_end;
        let expected = format!(
            "{}\"off\"{}",
            &FIXTURE[..val_start],
            &FIXTURE[val_start + "\"wallpaper\"".len()..]
        );
        assert_eq!(
            plan.new_text, expected,
            "only the one value token may change; every other byte must match"
        );
        // The change is exactly 11 -> 5 bytes (one quoted value token).
        assert_eq!(plan.new_text.len(), FIXTURE.len() - 11 + 5);

        // The document keeps its shape; the dotted per-wallpaper keys are
        // untouched and still say "wallpaper".
        assert_eq!(plan.new_text.matches("\"policy\"").count(), 1);
        assert_eq!(plan.new_text.matches("\"theme.policy\": \"wallpaper\"").count(), 2);
        assert!(plan.new_text.contains("\"policy\": \"off\""));
        assert!(!plan.new_text.contains("\"theme.policy\": \"off\""));
    }

    #[test]
    fn yield_plan_is_none_unless_value_is_exactly_wallpaper() {
        for policy in ["\"fixed\"", "\"off\""] {
            let text = format!(
                r#"{{"theme": {{"policy": {policy}}}, "noctalia": {{"themeMode": "manual"}}}}"#
            );
            assert!(
                plan_yield(&text).is_none(),
                "policy {policy} must not yield"
            );
        }
        // Key absent entirely.
        assert!(
            plan_yield(r#"{"theme": {"noctalia": {}}, "noctalia": {"themeMode": "follow"}}"#)
                .is_none()
        );
        // Key present but not a string.
        assert!(plan_yield(r#"{"theme": {"policy": 42}}"#).is_none());
    }

    #[test]
    fn yield_plan_is_none_for_unparsable_json() {
        assert!(plan_yield("{{{ not json").is_none());
        assert!(plan_yield("").is_none());
    }

    #[test]
    fn yield_plan_refuses_an_ambiguous_anchor() {
        // Two bare "policy" tokens: refuse, do not guess.
        assert!(plan_yield(AMBIGUOUS).is_none());
        // Duplicate keys in the SAME object also produce two tokens.
        assert!(
            plan_yield(r#"{"theme": {"policy": "wallpaper", "policy": "wallpaper"}}"#).is_none()
        );
        // Dotted per-wallpaper keys alone are NOT an anchor: no bare token,
        // and the general switch is absent anyway.
        let dotted_only = r#"{
  "theme": {
    "wallpaperProfiles": [
      { "match": "static:wallp9.jpg", "theme.policy": "wallpaper" }
    ]
  }
}"#;
        assert!(plan_yield(dotted_only).is_none());
    }

    // ── pure planner: restore ──

    #[test]
    fn restore_plan_puts_the_previous_value_back_byte_for_byte() {
        let plan = plan_yield(FIXTURE).expect("yield");
        let restore =
            plan_restore(&plan.new_text, &plan.previous_value).expect("restore must plan");
        assert_eq!(
            restore.new_text, FIXTURE,
            "restored text must return to the original bytes"
        );
    }

    #[test]
    fn restore_plan_refuses_foreign_or_ambiguous_values() {
        // A value we did NOT write ("fixed") must never be clobbered.
        assert!(plan_restore(r#"{"theme": {"policy": "fixed"}}"#, "wallpaper").is_none());
        // The engine untouched (still "wallpaper"): the yield never
        // happened, so there is nothing to restore.
        assert!(plan_restore(r#"{"theme": {"policy": "wallpaper"}}"#, "wallpaper").is_none());
        // Ambiguous anchor: refuse.
        assert!(
            plan_restore(r#"{"theme": {"policy": "off"}, "other": {"policy": "wallpaper"}}"#, "wallpaper")
                .is_none()
        );
        // Unparsable current text: refuse to touch it.
        assert!(plan_restore("{{{ broken", "wallpaper").is_none());
    }

    // ── impure shell ──

    #[test]
    fn shell_yield_then_restore_round_trips_bytes_and_mode() {
        let env = PolicyEnv::new(Some(FIXTURE), Some(0o600));

        let yielded = yield_color_authority().expect("yield returns a result");
        assert_eq!(yielded.as_deref(), Some("wallpaper"));

        let after_yield = env.read();
        assert!(after_yield.contains("\"policy\": \"off\""));
        assert_eq!(after_yield.matches("\"policy\"").count(), 1);
        assert_eq!(after_yield.matches("\"theme.policy\": \"wallpaper\"").count(), 2);
        assert_eq!(
            after_yield.len(),
            FIXTURE.len() - 11 + 5,
            "only the one value token may change"
        );
        // Permissions preserved by the yield write.
        assert_eq!(env.mode(), 0o600, "mode must survive the yield write");

        let restored = restore_color_authority("wallpaper").expect("restore returns a result");
        assert!(restored, "restore must have rewritten the file");

        assert_eq!(
            env.read(),
            FIXTURE,
            "restore must return the exact original bytes"
        );
        assert_eq!(env.mode(), 0o600, "mode must survive the restore write");
    }

    #[test]
    fn marker_is_written_on_yield_and_cleared_on_restore() {
        let env = PolicyEnv::new(Some(FIXTURE), None);
        let marker = env.marker();
        assert!(!marker.exists(), "no marker before any yield");

        assert_eq!(yield_color_authority().unwrap().as_deref(), Some("wallpaper"));
        assert!(marker.exists(), "marker must exist while the engine is held off");
        let marker_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&marker).unwrap()).unwrap();
        assert_eq!(marker_json["previous_value"], "wallpaper");
        assert_eq!(marker_json["config_path"], env.config.display().to_string());

        let _ = restore_color_authority("wallpaper").unwrap();
        assert!(!marker.exists(), "marker must be gone once restored");
    }

    #[test]
    fn shell_writes_nothing_when_policy_is_not_wallpaper() {
        for policy in ["fixed", "off"] {
            let text = format!(r#"{{"theme": {{"policy": "{policy}"}}}}"#);
            let env = PolicyEnv::new(Some(&text), None);
            let yielded = yield_color_authority().expect("no error, no panic");
            assert!(yielded.is_none(), "policy {policy} must not yield");
            assert_eq!(env.read(), text, "file bytes must stay untouched");
            assert!(!env.marker().exists(), "no marker for a non-yield");
        }
    }

    #[test]
    fn shell_yield_is_none_when_config_is_missing() {
        let env = PolicyEnv::new(None, None);
        assert!(!env.config.exists(), "sandbox starts with no config");
        let yielded = yield_color_authority().expect("missing file is not an error");
        assert!(yielded.is_none(), "missing config must not yield");
        assert!(!env.marker().exists());
    }

    #[test]
    fn shell_yield_is_none_when_config_is_unparsable() {
        let env = PolicyEnv::new(Some("{{{ not json"), None);
        let yielded = yield_color_authority().expect("unparsable file is not an error");
        assert!(yielded.is_none(), "unparsable config must not yield");
        assert_eq!(env.read(), "{{{ not json", "file bytes must stay untouched");
        assert!(!env.marker().exists());
    }

    #[test]
    fn shell_yield_is_none_when_anchor_is_ambiguous() {
        let env = PolicyEnv::new(Some(AMBIGUOUS), None);
        let yielded = yield_color_authority().expect("ambiguous anchor is not an error");
        assert!(yielded.is_none(), "ambiguous anchor must not yield");
        assert_eq!(env.read(), AMBIGUOUS, "file bytes must stay untouched");
        assert!(!env.marker().exists());
    }

    #[test]
    fn crash_recovery_restores_the_previous_value_and_clears_the_marker() {
        let env = PolicyEnv::new(Some(FIXTURE), Some(0o600));
        assert_eq!(yield_color_authority().unwrap().as_deref(), Some("wallpaper"));
        // "Crash": the restore never runs, the marker survives, the config
        // stays off. The next start replays the repair.
        let recovered = recover_crashed_yield().expect("recovery returns a result");
        assert!(recovered, "the crash marker must have been found");
        assert_eq!(env.read(), FIXTURE, "recovery must restore the exact bytes");
        assert_eq!(env.mode(), 0o600);
        assert!(!env.marker().exists(), "marker must be cleared after recovery");
        // A second recovery finds nothing.
        assert!(
            !recover_crashed_yield().unwrap(),
            "no marker means nothing to recover"
        );
    }

    #[test]
    fn restore_never_clobbers_a_value_the_engine_or_keeper_changed() {
        let env = PolicyEnv::new(Some(FIXTURE), None);
        assert_eq!(yield_color_authority().unwrap().as_deref(), Some("wallpaper"));
        // The keeper changes the switch mid-apply: our restore must back
        // off and keep the file as-is, not overwrite their choice.
        let foreign = r#"{"theme": {"policy": "fixed"}}"#;
        fs::write(&env.config, foreign).unwrap();
        let wrote = restore_color_authority("wallpaper").expect("restore returns a result");
        assert!(!wrote, "restore must not rewrite a foreign value");
        assert_eq!(env.read(), foreign, "the keeper's value must survive");
        assert!(!env.marker().exists(), "marker cleared: we are no longer yielding");
    }
}