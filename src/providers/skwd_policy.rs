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

// W2 wires `yield_color_authority` / `restore_color_authority` /
// `recover_crashed_yield` and the RAII `YieldGuard` into the apply path;
// everything here is reachable from `main` via the apply wiring and the
// startup repair.

use crate::config::hve_cache_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

// ── W2: RAII restore guard + atomic-write hardening ──────────────────

/// Temp-file stem for the engine-config writes.
const TEMP_STEM: &str = "skwd-policy-yield";
/// Temp-file stem for the marker writes.
const MARKER_TEMP_STEM: &str = "skwd-policy-yield-marker";

/// RAII arm for a colour-authority yield: restores the previous value when
/// dropped while armed, so NO exit path between the flip and the worker
/// hand-off can lose the restore. Best-effort and honest: failures are
/// logged, never panicked (a Drop must never panic).
pub(crate) struct YieldGuard {
    previous_value: Option<String>,
    armed: bool,
}

impl YieldGuard {
    pub(crate) fn new(previous_value: String) -> Self {
        Self {
            previous_value: Some(previous_value),
            armed: true,
        }
    }

    /// Detach the restore from this scope: the async re-assert worker takes
    /// ownership and restores once the palette is verified (or at the cap).
    /// Returns the previous value the worker must put back.
    pub(crate) fn disarm(&mut self) -> Option<String> {
        self.armed = false;
        self.previous_value.take()
    }
}

impl Drop for YieldGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(previous) = self.previous_value.take() {
            match restore_color_authority(&previous) {
                Ok(wrote) => tracing::warn!(
                    "[skwd-policy] yield guard restoring engine colour authority on scope \
                     exit (theme.policy -> \"{}\", wrote={})",
                    previous,
                    wrote
                ),
                Err(e) => tracing::warn!(
                    "[skwd-policy] yield guard could NOT restore engine colour authority: {}",
                    e
                ),
            }
        }
    }
}

/// Removes its temp file on drop unless disarmed — the ONE cleanup path
/// for every failure between temp creation and the atomic rename, so a
/// write or sync failure cannot leave a temp behind (W1 finding 1).
struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Unique temp-file sibling of `path`: same dir (same filesystem → the
/// rename swap stays atomic), distinct per invocation (pid + monotonic
/// counter) so two concurrent writers can never collide (W1 finding 2).
fn temp_sibling(path: &Path, stem: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or_else(|| Path::new("/"));
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{stem}.{}.{}.tmp", std::process::id(), seq))
}

/// Best-effort sweep of temp siblings left by a CRASHED process (a hard
/// kill between create and rename is the only way a temp survives — a
/// failure path never leaves one). Unique names mean the next write can no
/// longer "truncate" the old fixed name, so the sweep keeps the old
/// promise: the next write leaves the directory clean.
fn sweep_orphaned_temps(dir: &Path, stem: &str) {
    let our_pid = std::process::id();
    let prefix = format!(".{stem}.");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        if !rest.ends_with(".tmp") {
            continue;
        }
        let Some(pid) = rest.split('.').next().and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid != our_pid {
            if let Err(e) = fs::remove_file(entry.path()) {
                tracing::debug!(
                    "[skwd-policy] cannot sweep stale temp {}: {}",
                    entry.path().display(),
                    e
                );
            }
        }
    }
}

/// One-shot startup repair: replay a yield a previous process died between
/// flip and restore, and log the outcome (repaired / nothing / failed).
/// Never panics and never aborts startup.
pub(crate) fn startup_recover_crashed_yield() {
    match recover_crashed_yield() {
        Ok(true) => tracing::warn!(
            "[skwd-policy] startup repair: found a crashed yield; engine colour authority \
             restored (theme.policy put back)"
        ),
        Ok(false) => {
            tracing::info!("[skwd-policy] startup repair: nothing to repair");
        }
        Err(e) => {
            tracing::warn!("[skwd-policy] startup repair FAILED: {}", e);
        }
    }
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
/// file; on non-unix platforms the mode step is skipped. The temp name is
/// unique per invocation (W1 finding 2), the cleanup guard covers EVERY
/// failure path so a failed write leaves nothing behind (W1 finding 1),
/// and crash orphans from another pid are swept before writing.
fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("[skwd-policy] cannot atomically write {}: no parent dir", path.display()))?;
    sweep_orphaned_temps(dir, TEMP_STEM);
    let tmp = temp_sibling(path, TEMP_STEM);
    let mode = fs::metadata(path).ok().map(|m| m.permissions());
    // Arm the cleanup BEFORE the first fallible op: create, write, sync and
    // mode failures all share this one removal; only the successful atomic
    // rename disarms it.
    let mut cleanup = TempFileGuard::new(&tmp);
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
        format!(
            "[skwd-policy] atomic rename {} -> {} failed: {}",
            tmp.display(),
            path.display(),
            e
        )
    })?;
    cleanup.disarm();
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
    sweep_orphaned_temps(dir, MARKER_TEMP_STEM);
    let tmp = temp_sibling(&path, MARKER_TEMP_STEM);
    let mut cleanup = TempFileGuard::new(&tmp);
    {
        let mut f = fs::File::create(&tmp)
            .map_err(|e| format!("[skwd-policy] cannot create marker {}: {}", tmp.display(), e))?;
        f.write_all(raw.as_bytes())
            .map_err(|e| format!("[skwd-policy] cannot write marker {}: {}", tmp.display(), e))?;
        // As durable as the config write it protects: a power cut must not
        // leave a corrupt marker that recovery refuses to clear (W1
        // finding 3) — the marker exists precisely to survive that.
        f.sync_all()
            .map_err(|e| format!("[skwd-policy] cannot sync marker {}: {}", tmp.display(), e))?;
    }
    fs::rename(&tmp, &path).map_err(|e| {
        format!(
            "[skwd-policy] cannot install marker {}: {}",
            path.display(),
            e
        )
    })?;
    cleanup.disarm();
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
    // While the engine is held off no theme owns the colours: drop any
    // stale descriptor. Best-effort — a failed clear never aborts the apply.
    if let Err(e) = crate::color_authority::clear_descriptor() {
        tracing::warn!(
            "[skwd-policy] cannot clear colour-authority descriptor: {}",
            e
        );
    }
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

    // ── W2: findings closure ──────────────────────────────────────────

    /// In-memory tracing sink so startup-repair logs can be asserted
    /// without a global subscriber (thread-local default only).
    #[derive(Clone, Default)]
    struct LogCapture {
        buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl std::io::Write for LogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = LogCapture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn temp_siblings_are_unique_per_invocation() {
        // W1 finding 2: the temp name was fixed, so two concurrent writers
        // would collide in the same directory. Each invocation must get its
        // own name, still a hidden dotfile next to the target (the rename
        // swap must stay same-directory and atomic).
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("config.json");
        let a = temp_sibling(&target, "skwd-policy-yield");
        let b = temp_sibling(&target, "skwd-policy-yield");
        assert_ne!(a, b, "two invocations must never collide on the temp name");
        assert_eq!(a.parent(), Some(tmp.path()), "temp must live beside the target");
        let name = a.file_name().and_then(|n| n.to_str()).expect("utf8 temp name");
        assert!(name.starts_with(".skwd-policy-yield."), "hidden dotfile, stem prefixed: {}", name);
        assert!(name.ends_with(".tmp"), "dotfile suffix: {}", name);
    }

    #[test]
    fn write_atomic_failure_leaves_no_temp_behind() {
        // W1 finding 1, behavioral proof on the hard-to-fake arm: when the
        // rename itself fails (target is a directory), the temp that was
        // fully written and synced must be removed. The write/sync arms use
        // the same cleanup guard (pinned by construction below), so one
        // live failure covers the shared mechanism.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("victim");
        fs::create_dir_all(&target).unwrap();
        let err = write_atomic(&target, "{}")
            .expect_err("renaming a file onto a directory must fail");
        assert!(err.contains("[skwd-policy]"), "honest error: {}", err);
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with(".skwd-policy-yield.")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "a failed write must leave nothing behind: {:?}",
            leftovers
        );
    }

    #[test]
    fn write_atomic_all_failure_paths_share_one_cleanup_guard() {
        // W1 finding 1, by construction: the cleanup guard must be armed
        // BEFORE the first fallible op (File::create) and disarmed only
        // after the atomic rename succeeds — so write and sync failures are
        // covered by exactly the same removal as the rename failure.
        let src = std::fs::read_to_string("src/providers/skwd_policy.rs")
            .expect("src/providers/skwd_policy.rs must exist");
        let start = src.find("fn write_atomic").expect("write_atomic must exist");
        let end = src.find("fn write_marker").expect("write_marker must exist");
        let body = &src[start..end];
        let guard_at = body.find("TempFileGuard::new").expect("guard must arm");
        let create_at = body.find("File::create").expect("temp creation must exist");
        assert!(
            guard_at < create_at,
            "the cleanup guard must arm before the first fallible op"
        );
        let rename_at = body.find("fs::rename").expect("the swap must exist");
        let disarm_at = body[rename_at..].find("disarm").expect("guard must disarm") + rename_at;
        assert!(
            disarm_at > rename_at,
            "the guard must disarm only after the atomic rename"
        );
    }

    #[test]
    fn stale_temp_from_a_crashed_process_is_swept_on_the_next_write() {
        // Unique temp names mean the next write can no longer "truncate" a
        // crash orphan (the old fixed-name promise), so the next write must
        // sweep leftovers whose pid differs from ours.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("config.json");
        let stale = tmp
            .path()
            .join(".skwd-policy-yield.999999.0.tmp");
        fs::write(&stale, b"orphan").unwrap();
        write_atomic(&target, r#"{"theme": {"policy": "off"}}"#).unwrap();
        assert!(
            !stale.exists(),
            "a crash orphan from another pid must be swept by the next write"
        );
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".skwd-policy-yield."))
            .collect();
        assert!(leftovers.is_empty(), "no temp may survive a write: {:?}", leftovers);
    }

    #[test]
    fn marker_write_is_as_durable_as_the_config_write() {
        // W1 finding 3, by construction: the marker exists precisely to
        // survive a power cut, so it must fsync before the rename exactly
        // like the config write — a corrupt marker that recovery refuses to
        // clear would leave the switch off until a later startup.
        let src = std::fs::read_to_string("src/providers/skwd_policy.rs")
            .expect("src/providers/skwd_policy.rs must exist");
        let start = src.find("fn write_marker").expect("write_marker must exist");
        let end = src.find("fn clear_marker").expect("clear_marker must exist");
        let body = &src[start..end];
        assert!(
            body.contains("sync_all"),
            "the marker must fsync before the rename, like the config write"
        );
    }

    // ── W2: the yield guard (RAII restore) ────────────────────────────

    #[test]
    fn yield_guard_drop_restores_when_armed() {
        let env = PolicyEnv::new(Some(FIXTURE), Some(0o600));
        let previous = yield_color_authority()
            .unwrap()
            .expect("wallpaper policy must yield");
        let after_yield = env.read();
        assert!(after_yield.contains("\"policy\": \"off\""));
        {
            let guard = YieldGuard::new(previous);
            drop(guard); // armed: the Drop must restore on scope exit
        }
        assert_eq!(
            env.read(),
            FIXTURE,
            "an armed guard must restore the yield on scope exit"
        );
        assert_eq!(env.mode(), 0o600, "restore keeps the file's permissions");
        assert!(!env.marker().exists(), "restore clears the marker");
    }

    #[test]
    fn yield_guard_drop_does_nothing_when_disarmed() {
        let env = PolicyEnv::new(Some(FIXTURE), None);
        let previous = yield_color_authority()
            .unwrap()
            .expect("wallpaper policy must yield");
        let mut guard = YieldGuard::new(previous);
        let handed = guard.disarm();
        assert_eq!(handed.as_deref(), Some("wallpaper"));
        drop(guard); // disarmed: Drop must NOT restore
        assert!(
            env.read().contains("\"policy\": \"off\""),
            "a disarmed guard must not restore (the worker owns it now)"
        );
        assert!(
            env.marker().exists(),
            "marker stays: the worker's restore clears it, not the guard"
        );
    }

    // ── W2: startup crash repair ──────────────────────────────────────

    #[test]
    fn startup_repair_finds_a_crashed_yield_repairs_and_logs() {
        let env = PolicyEnv::new(Some(FIXTURE), Some(0o600));
        assert_eq!(yield_color_authority().unwrap().as_deref(), Some("wallpaper"));
        // "Crash": the restore never ran; the config stays off, the marker
        // survives. The next start replays the repair and logs the outcome.
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        startup_recover_crashed_yield();
        assert_eq!(env.read(), FIXTURE, "startup repair must restore the bytes");
        assert_eq!(env.mode(), 0o600);
        assert!(!env.marker().exists(), "startup repair clears the marker");
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("startup repair") && text.contains("restored"),
            "must log the repaired outcome, got: {}",
            text
        );
    }

    #[test]
    fn startup_repair_with_nothing_pending_logs_nothing_to_repair() {
        let env = PolicyEnv::new(Some(FIXTURE), None);
        assert!(!env.marker().exists());
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        startup_recover_crashed_yield();
        assert_eq!(env.read(), FIXTURE, "nothing to repair: config untouched");
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("nothing to repair"),
            "must log the nothing-to-repair outcome, got: {}",
            text
        );
    }

    #[test]
    fn startup_repair_logs_a_failed_recovery() {
        let env = PolicyEnv::new(Some(FIXTURE), None);
        // Marker whose recorded config path is a DIRECTORY: recovery cannot
        // read it, so the repair fails honestly instead of guessing.
        let target = env._tmp.path().join("adir");
        fs::create_dir_all(&target).unwrap();
        write_marker(&target, "wallpaper").unwrap();
        assert!(env.marker().exists(), "precondition: pending marker");
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        startup_recover_crashed_yield();
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("FAILED"),
            "a failed recovery must be logged honestly, got: {}",
            text
        );
        // The marker stays (recovery refused to clear what it could not
        // repair) so the next startup retries — the temp sandbox cleans up.
        assert!(env.marker().exists(), "unrepaired marker must survive for retry");
    }
}