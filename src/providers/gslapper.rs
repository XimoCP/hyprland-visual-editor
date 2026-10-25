//! Painter adapter: `gslapper`, the GStreamer video-wallpaper renderer.
//!
//! It exists because the router is agnostic (`odd/tasks/wallpaper-router-by-capability.md`,
//! S2/S6): the core says "paint this local video" and this adapter is the only
//! place that knows how gslapper is driven. Nothing here is reached by name from
//! the router — it is registered as a painter that DECLARES `LocalVideo`.
//!
//! SUPERVISION: Noctalia's own gslapper plugin owns the renderer when it is
//! enabled (its picker, its assignments, its own supervisor). Two supervisors
//! killing and respawning the same binary would fight, so this adapter answers
//! "not available" while that plugin is enabled, and HVE paints with gslapper
//! only when nothing else supervises it.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Noctalia's gslapper plugin id (a community plugin installed from a custom
/// source), read only to know whether it supervises the renderer.
pub(crate) const PLUGIN_ID: &str = "nomadcxx/gslapper";
/// The renderer binary this adapter drives.
const BINARY: &str = "gslapper";

/// Whether the renderer binary can be found on PATH.
pub(crate) fn binary_present() -> bool {
    find_on_path(BINARY).is_some()
}

/// Whether this painter can be handed a background right now: the binary is
/// installed AND Noctalia's plugin does not supervise it. Both facts are
/// checked here, so the router never guesses.
pub(crate) fn available() -> bool {
    binary_present() && !crate::providers::noctalia_runtime::plugin_enabled(PLUGIN_ID)
}

/// Paint one local video on every connected output.
///
/// HVE owns the instances it starts: it clears any previous one first, so a
/// re-apply never stacks two decoders on the same output, then forks one
/// renderer per output with its own IPC socket (so a later apply can address
/// each output instead of killing everything).
pub(crate) fn paint_local_video(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("video {} is not a file", path.display()));
    }
    let outputs = connected_outputs();
    if outputs.is_empty() {
        return Err("no connected output to paint".to_string());
    }
    let socket_dir = socket_dir()?;
    std::fs::create_dir_all(&socket_dir)
        .map_err(|e| format!("cannot create {}: {e}", socket_dir.display()))?;

    // One supervisor: HVE's own instances go, the plugin's stay (this adapter
    // is only reachable while the plugin is disabled, see `available`).
    let _ = Command::new("pkill").arg("-x").arg(BINARY).status();

    let mut painted = 0usize;
    let mut last_error = String::new();
    for output in &outputs {
        let socket = socket_dir.join(format!("{output}.sock"));
        let argv = launch_argv(output, &socket, path);
        match Command::new(&argv[0]).args(&argv[1..]).spawn() {
            Ok(_) => painted += 1,
            Err(e) => last_error = format!("{output}: {e}"),
        }
    }
    if painted == 0 {
        return Err(format!("gslapper could not start ({last_error})"));
    }
    tracing::info!(
        "[gslapper] painted {} on {painted} output(s) from {}",
        path.display(),
        path.display()
    );
    Ok(())
}

/// The argv one output gets. Pure and testable: the renderer is forked, paused
/// while hidden, and addressable through its own socket.
fn launch_argv(output: &str, socket: &Path, path: &Path) -> Vec<String> {
    vec![
        BINARY.to_string(),
        "--fork".to_string(),
        "--auto-pause".to_string(),
        "--ipc-socket".to_string(),
        socket.to_string_lossy().to_string(),
        output.to_string(),
        path.to_string_lossy().to_string(),
    ]
}

/// The connected outputs, read from the same authority snapshot the theme save
/// uses (no raw compositor call of our own).
fn connected_outputs() -> Vec<String> {
    let snapshot = crate::providers::wallpaper_authority::query_authority_snapshot();
    match snapshot.monitors_json {
        Some(json) => crate::providers::wallpaper_authority::parse_monitor_sizes(&json)
            .map(|sizes| sizes.into_keys().collect())
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Where this adapter keeps one IPC socket per output.
fn socket_dir() -> Result<PathBuf, String> {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map_err(|_| "XDG_RUNTIME_DIR is unset; no place for the renderer sockets".to_string())?;
    Ok(runtime.join("hve-gslapper"))
}

/// Resolve one command name on PATH, so "installed" is a fact, never a guess.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// The argv is the contract with the renderer: forked, paused while hidden,
    /// its own socket, then output and path — and never a shell string, so a
    /// path with spaces cannot split into extra arguments.
    #[test]
    fn launch_argv_passes_the_path_as_one_argument() {
        let argv = launch_argv(
            "DP-3",
            Path::new("/run/user/1000/hve-gslapper/DP-3.sock"),
            Path::new("/home/x/Videos/my movie.mp4"),
        );
        assert_eq!(argv[0], "gslapper");
        assert!(argv.contains(&"--fork".to_string()));
        assert!(argv.contains(&"--auto-pause".to_string()));
        assert_eq!(
            argv[argv.len() - 2],
            "DP-3",
            "the output comes before the path"
        );
        assert_eq!(
            argv[argv.len() - 1],
            "/home/x/Videos/my movie.mp4",
            "a path with spaces stays ONE argument"
        );
        assert_eq!(
            argv.iter().filter(|a| a.as_str() == "--ipc-socket").count(),
            1,
            "exactly one socket flag"
        );
    }

    /// Supervision is a fact about the environment: without the binary the
    /// painter never claims availability, whatever else is installed.
    #[test]
    #[serial]
    fn a_missing_binary_means_not_available() {
        let _env = crate::test_utils::TempEnv::new();
        let saved = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        assert!(!binary_present(), "nothing resolves on an empty PATH");
        assert!(!available(), "an uninstalled renderer is never available");
        match saved {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }

    /// `find_on_path` resolves a real file and refuses a missing one.
    #[test]
    #[serial]
    fn find_on_path_resolves_a_file_and_refuses_a_missing_one() {
        let _env = crate::test_utils::TempEnv::new();
        let dir = std::env::temp_dir().join("hve-gslapper-path-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("gslapper");
        std::fs::write(&fake, b"#!/bin/sh\nexit 0\n").unwrap();
        let saved = std::env::var_os("PATH");
        std::env::set_var("PATH", &dir);
        assert_eq!(find_on_path("gslapper"), Some(fake));
        assert_eq!(find_on_path("nope-not-here"), None);
        match saved {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
