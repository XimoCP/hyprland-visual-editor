//! Noctalia's own path and launcher knowledge: the v5 native paths, plus the
//! `ShellProvider` trait those paths implement.
//!
//! This file used to be called `shell.rs` and read as if it were the generic
//! shell seam. It never was: every impl here is Noctalia. The generic seam now
//! lives in `shell_capabilities.rs`; naming this module after its owner is the
//! honest fix.

use std::path::{Path, PathBuf};

/// A shell knows how to resolve its configuration paths and perform shell-specific
/// operations like wallpaper IPC.
///
/// Includes source/rendered file lists, template processor paths, and
/// wallpaper IPC operations.
pub trait ShellProvider: Send + Sync {
    // `config_dir` is the ONE method production still reaches
    // (`NoctaliaV5Paths::is_active` reads the v5 `profiles` marker through
    // it); every other method here is exercised only by the test suite. The
    // allows are required: rustc's dead_code lint does not count
    // `#[cfg(test)]` usage during a plain `cargo check`.
    #[allow(dead_code)]
    fn id(&self) -> &str;
    #[allow(dead_code)]
    fn display_name(&self) -> &str;
    fn config_dir(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn rendered_dir(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn wallpapers_file(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn theming_config(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn source_files(&self) -> Vec<&'static str>;
    #[allow(dead_code)]
    fn rendered_color_files(&self) -> Vec<&'static str>;
    #[allow(dead_code)]
    fn template_processor(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn reload_command(&self) -> Vec<String>;
    #[allow(dead_code)]
    fn apply_wallpaper(&self, _path: &Path, _screen: &str) -> Result<(), String> {
        Ok(())
    }
    #[allow(dead_code)]
    fn get_wallpaper(&self, _screen: &str) -> Result<String, String> {
        Err("not supported".into())
    }
}

/// Noctalia v5 (Rust native shell) — paths for detection and palette access only.
///
/// The provider's save/apply is handled entirely over IPC (`noctalia msg`).
/// There is no template processor, rendered files, wallpapers.json cache, or reload.
/// v5 has automatic hot-reload.
pub struct NoctaliaV5Paths;

impl ShellProvider for NoctaliaV5Paths {
    fn id(&self) -> &str {
        "noctalia"
    }

    fn display_name(&self) -> &str {
        "Noctalia v5"
    }

    fn config_dir(&self) -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("HVE_NOCTALIA_CONFIG") {
            let p = PathBuf::from(dir);
            if p.exists() {
                return Some(p);
            }
        }
        dirs::config_dir().map(|d| d.join("noctalia"))
    }

    fn rendered_dir(&self) -> Option<PathBuf> {
        None
    }

    fn wallpapers_file(&self) -> Option<PathBuf> {
        None
    }

    fn theming_config(&self) -> Option<PathBuf> {
        None
    }

    fn source_files(&self) -> Vec<&'static str> {
        Vec::new()
    }

    fn rendered_color_files(&self) -> Vec<&'static str> {
        Vec::new()
    }

    fn template_processor(&self) -> Option<PathBuf> {
        None
    }

    fn reload_command(&self) -> Vec<String> {
        Vec::new()
    }

    fn apply_wallpaper(&self, path: &Path, screen: &str) -> Result<(), String> {
        let path_str = path.to_str().ok_or("Invalid wallpaper path")?;
        let output = std::process::Command::new("noctalia")
            .args(["msg", "wallpaper-set", screen, path_str])
            .output()
            .map_err(|e| format!("Failed to run noctalia msg: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Wallpaper IPC failed: {}", stderr.trim()))
        }
    }

    fn get_wallpaper(&self, screen: &str) -> Result<String, String> {
        let output = std::process::Command::new("noctalia")
            .args(["msg", "wallpaper-get", screen])
            .output()
            .map_err(|e| format!("Failed to run noctalia msg: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Wallpaper get failed: {}", stderr.trim()))
        }
    }
}

impl NoctaliaV5Paths {
    /// True when Noctalia v5 is running: both the `noctalia` process and the
    /// v5-only `~/.config/noctalia/profiles/` marker must exist.
    pub fn is_active(&self) -> bool {
        // `.output()`, never `.status()`: it captures the PID `pgrep` would
        // otherwise print straight into HVE's own terminal.
        let process_running = std::process::Command::new("pgrep")
            .arg("-x")
            .arg("noctalia")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let profiles_dir_exists = self
            .config_dir()
            .map(|c| c.join("profiles").exists())
            .unwrap_or(false);

        process_running && profiles_dir_exists
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── NoctaliaV5Paths tests ──

    #[test]
    fn test_noctalia_v5_paths_id() {
        let p = NoctaliaV5Paths;
        assert_eq!(<NoctaliaV5Paths as ShellProvider>::id(&p), "noctalia");
    }

    #[test]
    fn test_noctalia_v5_paths_display_name() {
        let p = NoctaliaV5Paths;
        assert_eq!(p.display_name(), "Noctalia v5");
    }

    #[test]
    fn test_noctalia_v5_paths_config_dir() {
        let _env = crate::test_utils::env_guard();
        let p = NoctaliaV5Paths;
        let cd = p.config_dir();
        assert!(cd.is_some());
        let cd = cd.unwrap();
        assert!(cd.ends_with("noctalia"));
    }

    #[test]
    fn test_noctalia_v5_paths_rendered_dir_none() {
        let _env = crate::test_utils::env_guard();
        let p = NoctaliaV5Paths;
        assert!(p.rendered_dir().is_none());
    }

    #[test]
    fn test_noctalia_v5_paths_theming_config_none() {
        let p = NoctaliaV5Paths;
        assert!(p.theming_config().is_none());
    }

    #[test]
    fn test_noctalia_v5_paths_template_processor_none() {
        let p = NoctaliaV5Paths;
        assert!(p.template_processor().is_none());
    }

    #[test]
    fn test_noctalia_v5_paths_wallpapers_file_none() {
        let p = NoctaliaV5Paths;
        assert!(p.wallpapers_file().is_none());
    }

    #[test]
    fn test_noctalia_v5_paths_source_files_empty() {
        let p = NoctaliaV5Paths;
        assert!(p.source_files().is_empty());
    }

    #[test]
    fn test_noctalia_v5_paths_rendered_color_files_empty() {
        let p = NoctaliaV5Paths;
        assert!(p.rendered_color_files().is_empty());
    }

    #[test]
    fn test_noctalia_v5_paths_reload_command_empty() {
        let p = NoctaliaV5Paths;
        assert!(p.reload_command().is_empty());
    }

    #[test]
    fn test_noctalia_v5_paths_apply_wallpaper_ipc_format() {
        let _env = crate::test_utils::env_guard();
        let p = NoctaliaV5Paths;
        // It can fail — we only check that it does not panic and the syntax is correct
        let result = p.apply_wallpaper(Path::new("/tmp/fake.png"), "DP-3");
        if let Err(e) = &result {
            assert!(
                e.contains("noctalia") || e.contains("Wallpaper IPC") || e.contains("No such file"),
                "unexpected error: {}",
                e
            );
        }
    }

    #[test]
    fn test_noctalia_v5_paths_get_wallpaper_ipc_format() {
        let _env = crate::test_utils::env_guard();
        let p = NoctaliaV5Paths;
        let result = p.get_wallpaper("DP-3");
        if let Err(e) = &result {
            assert!(
                e.contains("noctalia") || e.contains("Wallpaper get") || e.contains("No such file"),
                "unexpected error: {}",
                e
            );
        }
    }
}
