//! Shared access to the Noctalia runtime: IPC (`noctalia msg`) and on-disk
//! path resolution (config dir, state dir).
//!
//! This is the neutral shared piece used by the `noctalia` and `mpvpaper`
//! providers, so neither depends on the other provider's internals.

use std::path::PathBuf;

/// Run `noctalia msg <args...>` and return stdout on success.
pub(crate) fn noctalia_msg(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("noctalia")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run noctalia: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("noctalia {} failed: {}", args.join(" "), stderr.trim()))
    }
}

/// Resolve Noctalia config dir (~/.config/noctalia).
pub(crate) fn noctalia_config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("HVE_NOCTALIA_CONFIG") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return Some(p);
        }
    }
    dirs::config_dir().map(|d| d.join("noctalia"))
}

/// Resolve Noctalia state dir (~/.local/state/noctalia).
///
/// v5 stores its runtime settings in `settings.toml` inside this directory,
/// NOT in `~/.config/noctalia/settings.json` (which is a v4 artifact).
pub(crate) fn noctalia_state_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local").join("state").join("noctalia"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noctalia_state_dir() {
        let path = noctalia_state_dir();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with(".local/state/noctalia"));
        assert!(path.is_absolute());
    }

    #[test]
    fn test_noctalia_state_dir_exists() {
        let path = noctalia_state_dir().unwrap();
        // This should exist on a real system when noctalia v5 is installed
        // We just verify it doesn't crash — existence depends on the system
        let _ = path.exists();
    }
}