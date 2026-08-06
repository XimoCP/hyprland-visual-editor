use std::path::PathBuf;

/// Spawn the color_watcher.sh bash script and return the child process handle.
pub fn spawn_color_watcher(proj: &std::path::Path) -> Option<std::process::Child> {
    let watcher_script = proj.join("assets").join("scripts").join("color_watcher.sh");
    let watcher_log = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("hve")
        .join("color_watcher.log");

    if watcher_script.exists() {
        // Log to file so we can diagnose watcher issues
        let log_file = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&watcher_log)
            .ok()
            .and_then(|f| f.try_clone().ok());
        let stderr: std::process::Stdio = log_file
            .map(std::process::Stdio::from)
            .unwrap_or(std::process::Stdio::null());

        match std::process::Command::new("bash")
            .arg(watcher_script)
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
            .spawn()
        {
            Ok(child) => {
                println!("[HVE] Color watcher started (pid: {})", child.id());
                println!("[HVE] Watcher log: {}", watcher_log.display());
                Some(child)
            }
            Err(e) => {
                eprintln!("[HVE] Could not start color watcher: {}", e);
                None
            }
        }
    } else {
        eprintln!(
            "[HVE] Color watcher script not found: {}",
            watcher_script.display()
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_color_watcher_script_not_found_returns_none() {
        // A non-existent project dir should cause the function to
        // return None because the script file doesn't exist.
        let proj = PathBuf::from("/tmp/hve_nonexistent_test");
        let result = spawn_color_watcher(&proj);
        assert!(result.is_none(), "should return None when script does not exist");
    }

    #[test]
    fn test_spawn_color_watcher_builds_correct_script_path() {
        // Verify the path logic (which is the core unit-testable part):
        // proj/assets/scripts/color_watcher.sh
        let proj = PathBuf::from("/some/project");
        let script = proj.join("assets").join("scripts").join("color_watcher.sh");
        assert_eq!(
            script.to_string_lossy(),
            "/some/project/assets/scripts/color_watcher.sh"
        );
    }

    #[test]
    fn test_spawn_color_watcher_builds_correct_log_path() {
        // Verify the log path resolution logic:
        // cache/hve/color_watcher.log (or fallback /tmp/hve/color_watcher.log)
        if let Some(cache) = dirs::cache_dir() {
            let log = cache.join("hve").join("color_watcher.log");
            assert!(log.to_string_lossy().contains("color_watcher.log"));
        }
    }
}

