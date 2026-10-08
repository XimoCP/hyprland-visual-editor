//! Declarative colour-authority descriptor (`odd/tasks/hve-capability-routing.md`
/// unit 1a, serving the rule in `odd/tasks/theme-owns-the-palette.md`).
///
/// Meaning: when the file exists, the applied theme's backend owns the
/// colours — it names the backend, the theme, the theme's saved
/// `palette.json` snapshot and the palette name that snapshot restores as.
/// When the file is ABSENT, no theme claims authority and the colour
/// pipeline follows the live source, exactly as today.
///
/// Write-only for now: providers create, update and remove the descriptor;
/// NOTHING reads it yet (no behaviour change), until the central colour
/// scripts learn to consult it as a declaration they only read.

use crate::config::hve_cache_dir;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// File name of the descriptor inside HVE's cache dir.
pub const DESCRIPTOR_FILE: &str = "color-authority.json";

/// Which backend owns the colours and where its snapshot lives.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ColorAuthority {
    pub backend: String,
    pub theme: String,
    pub palette_file: String,
    pub palette_name: String,
}

/// Absolute path of the descriptor on this machine.
pub fn descriptor_path() -> PathBuf {
    hve_cache_dir().join(DESCRIPTOR_FILE)
}

/// Unique temp-file sibling of the descriptor: same directory (same
/// filesystem, so the rename stays an atomic swap), distinct per invocation
/// (pid + monotonic counter) so concurrent writers never collide.
fn temp_sibling() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = hve_cache_dir();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{}.{}.{}.tmp", "color-authority", std::process::id(), seq))
}

/// Write the descriptor atomically (temp file + rename in the same
/// directory), mode 0600 on unix. Creates the cache dir when missing.
/// A failed write leaves no temp file behind.
pub fn write_descriptor(authority: &ColorAuthority) -> std::io::Result<()> {
    let path = descriptor_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_string_pretty(authority)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = temp_sibling();
    let result = (|| {
        fs::write(&tmp, data.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    if result.is_ok() {
        crate::decision_log::record(
            "CLAIMED",
            &format!(
                "theme \"{}\" owns the palette (backend {})",
                authority.theme, authority.backend
            ),
        );
    }
    result
}

/// Read the descriptor. Returns `None` on any error — missing file,
/// unreadable file, malformed JSON or unknown fields. Never panics, never
/// propagates.
pub fn read_descriptor() -> Option<ColorAuthority> {
    let raw = fs::read_to_string(descriptor_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Remove the descriptor. Removing an absent file is Ok.
pub fn clear_descriptor() -> std::io::Result<()> {
    match fs::remove_file(descriptor_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    fn sample() -> ColorAuthority {
        ColorAuthority {
            backend: "noctalia-v5".to_string(),
            theme: "Animation".to_string(),
            palette_file: "/tmp/themes/Animation/providers/noctalia-v5/palette.json".to_string(),
            palette_name: "Animation-vib".to_string(),
        }
    }

    #[test]
    fn round_trip_write_read() {
        let _env = TempEnv::new();
        let authority = sample();
        write_descriptor(&authority).expect("write must succeed");
        assert_eq!(read_descriptor(), Some(authority));
    }

    /// W3 of `odd/tasks/palette-authority-mute-and-yield.md`: writing the
    /// descriptor is the moment a theme CLAIMS the palette, so the keeper
    /// must find it in the decision log. Read inside the `TempEnv` sandbox.
    #[test]
    fn writing_the_descriptor_records_the_claim_in_the_decision_log() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        let text = fs::read_to_string(crate::decision_log::log_path()).unwrap_or_default();
        assert!(
            text.contains("CLAIMED") && text.contains("Animation"),
            "the claim must name the theme, got: {text}"
        );
    }

    #[test]
    fn missing_file_reads_as_none() {
        let _env = TempEnv::new();
        assert!(!descriptor_path().exists());
        assert_eq!(read_descriptor(), None);
    }

    #[test]
    fn malformed_json_reads_as_none() {
        let _env = TempEnv::new();
        let path = descriptor_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(read_descriptor(), None);
    }

    #[test]
    fn unknown_field_is_refused() {
        let _env = TempEnv::new();
        let path = descriptor_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"backend":"noctalia-v5","theme":"Animation","palette_file":"p","palette_name":"n","extra":"nope"}"#,
        )
        .unwrap();
        assert_eq!(
            read_descriptor(),
            None,
            "deny_unknown_fields must refuse extra keys"
        );
    }

    #[test]
    fn clear_on_missing_file_is_ok() {
        let _env = TempEnv::new();
        assert!(!descriptor_path().exists());
        clear_descriptor().expect("clearing an absent descriptor must be Ok");
    }

    #[test]
    fn clear_removes_an_existing_descriptor() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        assert!(descriptor_path().exists());
        clear_descriptor().expect("clear must succeed");
        assert_eq!(read_descriptor(), None);
    }

    #[test]
    fn successful_write_leaves_no_temp_file_behind() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        let leftovers: Vec<_> = fs::read_dir(hve_cache_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp file may survive a write: {:?}",
            leftovers
        );
    }

    #[test]
    #[cfg(unix)]
    fn descriptor_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        let mode = fs::metadata(descriptor_path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "descriptor must be owner-only");
    }
}
