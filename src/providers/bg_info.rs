//! Shared background (wallpaper) information.
//!
//! Holds the theme-manifest file name, the capture of the live background
//! state, and the user notification that an animated background could not be
//! applied — the pieces BOTH backends (the Noctalia provider and the media
//! backend) may read, so neither depends on the other's internals (the
//! capability-routing rule: a backend never calls another backend).
//!
//! It is NOT provider-neutral: it resolves the live state location through the
//! Noctalia runtime (`noctalia_state_dir`) and its notification copy names a
//! third-party plugin identifier it does not own and must not reword. The
//! module holds no provider-specific POLICY: it decides nothing about
//! enabling, bouncing, delegating to, or skipping any plugin. Callers keep
//! their own policy; this module only records and reports information.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::providers::noctalia_runtime::noctalia_state_dir;

/// File HVE stores inside the theme provider dir.
pub const MANIFEST_FILE: &str = "mpvpaper-assignments.json";

// ── Manifest types (theme side) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpvpaperVideo {
    /// Base file name, used to detect existing copies in video_directory.
    pub filename: String,
    /// Absolute path from a local save (may be absent in distributed themes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    /// Remote source for distributed themes (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional sha256 of the remote file, verified after download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpvpaperManifest {
    pub version: u32,
    /// connector ("*" = all outputs, or a monitor name) -> video reference.
    pub assignments: HashMap<String, MpvpaperVideo>,
}

// ── Live plugin state (plugin side) ────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct LiveAssignments {
    pub(crate) assignments: HashMap<String, String>,
    #[serde(default, rename = "launchedAsSystemd")]
    pub(crate) launched_as_systemd: HashMap<String, bool>,
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Live `assignments.json` the running plugin supervises.
pub(crate) fn live_assignments_path() -> Option<PathBuf> {
    Some(noctalia_state_dir()?.join("mpvpaper").join("assignments.json"))
}

// ── Save ────────────────────────────────────────────────────────────────

/// Capture the plugin's current assignments into the theme manifest.
///
/// When the plugin has no live state, this is a no-op (theme ends up with no
/// mpvpaper manifest, i.e. a non-animated theme).
pub fn save_manifest(provider_dir: &Path) -> Result<(), String> {
    let Some(src) = live_assignments_path() else {
        return Err("Cannot resolve Noctalia state dir".into());
    };
    if !src.exists() {
        tracing::debug!("[mpvpaper] No live assignments file, skipping manifest");
        return Ok(());
    }

    let raw = fs::read_to_string(&src)
        .map_err(|e| format!("Cannot read live assignments: {}", e))?;
    let live: LiveAssignments = serde_json::from_str(&raw)
        .map_err(|e| format!("Cannot parse live assignments: {}", e))?;

    let mut assignments = HashMap::new();
    for (connector, path) in live.assignments {
        let filename = Path::new(&path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        assignments.insert(
            connector,
            MpvpaperVideo {
                filename,
                local_path: Some(path),
                url: None,
                sha256: None,
            },
        );
    }

    let manifest = MpvpaperManifest { version: 1, assignments };
    fs::create_dir_all(provider_dir)
        .map_err(|e| format!("Cannot create provider dir: {}", e))?;
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Manifest serialization error: {}", e))?;
    fs::write(provider_dir.join(MANIFEST_FILE), json)
        .map_err(|e| format!("Cannot write mpvpaper manifest: {}", e))?;

    tracing::info!("[mpvpaper] Saved manifest with {} assignment(s)", manifest.assignments.len());
    Ok(())
}

// ── Preview (read-preview-source) ───────────────────────────────────────

/// The assignment paths a saved theme manifest STATES, in the record's own
/// precedence order: the wildcard `"*"` assignment first, then the
/// remaining connectors in sorted-key order. Only non-empty `local_path`
/// values are candidates; existence is the CALLER's decision (the gallery
/// core probes it and keeps the first existing candidate). An absent,
/// unreadable or malformed manifest declares nothing — it is not an error,
/// it is a theme without an animated-background record. The record format
/// knowledge lives here, next to [`MANIFEST_FILE`] and the manifest types:
/// callers never parse this file themselves.
pub fn preview_candidates(provider_dir: &Path) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(provider_dir.join(MANIFEST_FILE)) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(assignments) = v.get("assignments").and_then(|a| a.as_object()) else {
        return Vec::new();
    };
    // Wildcard "*" wins; remaining entries keep sorted-key order.
    let mut keys: Vec<&String> = assignments.keys().collect();
    keys.sort();
    keys.sort_by_key(|k| k.as_str() != "*");
    keys.iter()
        .filter_map(|key| {
            assignments
                .get(key.as_str())
                .and_then(|e| e.get("local_path"))
                .and_then(|l| l.as_str())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .collect()
}

// ── Notification ────────────────────────────────────────────────────────

/// Notify the user that the mpvpaper plugin is required but not available,
/// so an animated theme cannot be applied.
pub fn notify_plugin_required() {
    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=HVE")
        .arg("⚠️ Fondo animado no aplicado")
        .arg("El tema incluye fondos animados, pero el plugin noctalia/mpvpaper no está instalado o está desactivado. Activá el plugin en los ajustes de Noctalia para poder aplicarlos.")
        .output();
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::providers::noctalia_runtime::noctalia_state_dir;
    use crate::test_utils::TempEnv;

    /// Seed the live plugin-state file that `save_manifest` captures from,
    /// inside the sandboxed `HOME` the test holds.
    fn seed_live_assignments(home: &str) {
        let state_dir = noctalia_state_dir().expect("state dir resolves under the sandboxed HOME");
        let live_dir = state_dir.join("mpvpaper");
        fs::create_dir_all(&live_dir).expect("seed the live state dir");
        let json = format!(
            r#"{{"assignments":{{"*":"{home}/Videos/movie.mp4"}},"launchedAsSystemd":{{}}}}"#
        );
        fs::write(live_dir.join("assignments.json"), json).expect("seed live assignments");
    }

    fn read_manifest(dir: &Path) -> Vec<u8> {
        fs::read(dir.join(crate::providers::mpvpaper::MANIFEST_FILE))
            .expect("the manifest must exist under the shared file name")
    }

    /// The whole point of this module: `noctalia.rs` reads the shared
    /// background information here instead of naming `mpvpaper`, while the
    /// historical `mpvpaper::save_manifest` path stays a thin re-export of
    /// the SAME implementation. Both paths must therefore write the same
    /// file name and byte-identical content.
    #[test]
    fn save_manifest_is_identical_through_both_paths() {
        let env = TempEnv::new();
        let home = std::env::var("HOME").expect("TempEnv sets HOME");
        seed_live_assignments(&home);

        let dir_neutral = tempfile::tempdir().expect("provider dir A");
        let dir_legacy = tempfile::tempdir().expect("provider dir B");

        crate::providers::bg_info::save_manifest(dir_neutral.path())
            .expect("bg_info::save_manifest must succeed on seeded live state");
        crate::providers::mpvpaper::save_manifest(dir_legacy.path())
            .expect("mpvpaper::save_manifest (re-export) must succeed on the same state");

        let bytes_neutral =
            fs::read(dir_neutral.path().join(crate::providers::bg_info::MANIFEST_FILE))
                .expect("bg_info must write the manifest under its shared name");
        let bytes_legacy = read_manifest(dir_legacy.path());

        assert_eq!(
            bytes_neutral, bytes_legacy,
            "both paths must produce byte-identical manifests"
        );

        let parsed: crate::providers::mpvpaper::MpvpaperManifest =
            serde_json::from_slice(&bytes_neutral).expect("manifest must parse");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.assignments.len(), 1);
        let video = &parsed.assignments["*"];
        assert_eq!(video.filename, "movie.mp4");
        assert_eq!(
            video.local_path.as_deref(),
            Some(format!("{home}/Videos/movie.mp4").as_str())
        );

        // API compatibility: the historical `mpvpaper` path must keep
        // resolving for every moved item. No call here — the binding only
        // proves the re-export exists with the same signature.
        let _notify: fn() = crate::providers::mpvpaper::notify_plugin_required;

        drop(env);
    }

    /// The chain this unit breaks: `noctalia.rs` must not reach into the
    /// mpvpaper backend for the shared background information — each of
    /// these items has exactly one implementation, in `bg_info`. The
    /// third-party plugin id, comments, and the policy calls reserved for
    /// the next unit are not this test's business.
    #[test]
    fn noctalia_reads_shared_background_info_from_bg_info() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        for item in ["save_manifest", "MANIFEST_FILE", "notify_plugin_required"] {
            let needle = format!("crate::providers::mpvpaper::{item}");
            assert!(
                !src.contains(&needle),
                "noctalia.rs must read `{item}` from bg_info, not from the mpvpaper \
                 backend (capability routing: a backend never calls another backend)"
            );
        }
    }

    /// The on-disk name is part of the saved-theme format: renaming it would
    /// strand every manifest already on disk. It must be one constant, seen
    /// through both paths.
    #[test]
    fn manifest_file_is_identical_through_both_paths() {
        assert_eq!(
            crate::providers::bg_info::MANIFEST_FILE,
            crate::providers::mpvpaper::MANIFEST_FILE,
            "mpvpaper must re-export the neutral constant instead of redefining it"
        );
        assert_eq!(
            crate::providers::mpvpaper::MANIFEST_FILE,
            "mpvpaper-assignments.json",
            "the manifest file name is part of the saved-theme format"
        );
    }
}
