//! Deterministic shell-contract assertions for the Lua-only script layer
//! (drop-conf-support R1, R9, R10).
//!
//! These tests execute the real scripts under `assets/scripts/` in hermetic
//! temporary homes. A stub `bin/` is prepended to `PATH` so `hyprctl`,
//! `pgrep`, `noctalia` and `inotifywait` can never touch the developer's real
//! compositor during a test run.
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::Output;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scripts_dir() -> PathBuf {
    repo_root().join("assets").join("scripts")
}

#[cfg(unix)]
fn make_exec(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).unwrap();
}

/// Inert stubs so scripts believe no Hyprland/Noctalia is present.
fn fake_bin_dir(base: &Path) -> PathBuf {
    let bin = base.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, body) in [
        ("pgrep", "#!/bin/bash\nexit 1\n"),
        ("hyprctl", "#!/bin/bash\nexit 0\n"),
        ("noctalia", "#!/bin/bash\nexit 1\n"),
        ("inotifywait", "#!/bin/bash\nexit 0\n"),
    ] {
        let p = bin.join(name);
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        make_exec(&p);
    }
    bin
}

fn run_bash(
    script: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    cwd: &Path,
) -> Output {
    let bin_tmp = tempfile::tempdir().unwrap();
    let bin = fake_bin_dir(bin_tmp.path());
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .args(args)
        .env("PATH", path)
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .output()
        .expect("bash must be available")
}

fn read_script(name: &str) -> String {
    std::fs::read_to_string(scripts_dir().join(name))
        .unwrap_or_else(|_| panic!("assets/scripts/{name} must exist"))
}

// ── Resolver: Lua only, never falls back to `.conf` ─────────────────────

#[test]
fn resolver_is_lua_only() {
    let utils = scripts_dir().join("utils.sh");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("preset.lua"), "hl.config({})\n").unwrap();
    std::fs::write(dir.path().join("preset.conf"), "general {}\n").unwrap();
    std::fs::write(dir.path().join("legacy.conf"), "general {}\n").unwrap();

    let script = format!(
        r#"source "{utils}"
target="$1"
printf 'resolved=%s\n' "$(hve_resolve_preset "$target" preset)"
if hve_resolve_preset "$target" legacy >/dev/null 2>&1; then
  printf 'legacy=resolved\n'
else
  printf 'legacy=refused\n'
fi
"#,
        utils = utils.display()
    );

    let out = run_bash(
        &script,
        &["hve-resolver-test", dir.path().to_str().unwrap()],
        &[("HVE_FORMAT", "conf")],
        dir.path(),
    );
    assert!(
        out.status.success(),
        "bash failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("preset.lua"),
        "resolver must return the .lua twin: {stdout}"
    );
    assert!(
        !stdout.contains("preset.conf"),
        "resolver must never return the .conf twin: {stdout}"
    );
    assert!(
        stdout.contains("legacy=refused"),
        "a conf-only preset must not resolve: {stdout}"
    );
}

// ── Static Lua-only contract across the script layer ────────────────────

#[test]
fn core_scripts_have_no_format_detection() {
    for name in [
        "utils.sh",
        "init.sh",
        "assemble.sh",
        "scan.sh",
        "border.sh",
        "apply_animation.sh",
        "shader.sh",
        "geometry.sh",
        "colors.sh",
        "color_watcher.sh",
    ] {
        let src = read_script(name);
        assert!(!src.contains("HVE_FORMAT"), "{name} must not read HVE_FORMAT");
        assert!(
            !src.contains("detect_format"),
            "{name} must not call detect_format"
        );
        assert!(
            !src.contains("hve_format"),
            "{name} must not touch the hve_format cache"
        );
    }
}

#[test]
fn prompts_target_lua_fragments_only() {
    let cases = [
        ("border.sh", "border.lua"),
        ("apply_animation.sh", "animation.lua"),
        ("shader.sh", "shader.lua"),
        ("geometry.sh", "geometry.lua"),
    ];
    for (name, fragment) in cases {
        let src = read_script(name);
        assert!(
            src.contains(fragment),
            "{name} must target {fragment}"
        );
        let opposite = fragment.replace(".lua", ".conf");
        assert!(
            !src.contains(&opposite),
            "{name} must not reference {opposite}"
        );
    }
}

#[test]
fn assembler_and_scanner_are_lua_only() {
    let assemble = read_script("assemble.sh");
    assert!(assemble.contains("overlay.lua"), "assembler targets overlay.lua");
    assert!(
        !assemble.contains("overlay.conf"),
        "assembler must not reference overlay.conf"
    );

    let scan = read_script("scan.sh");
    assert!(scan.contains("\"*.lua\""), "scanner must glob *.lua");
    assert!(
        !scan.contains("\"*.conf\""),
        "scanner must not glob *.conf"
    );
}

#[test]
fn color_helpers_drop_conf_paths() {
    let colors = read_script("colors.sh");
    assert!(
        !colors.contains("_hve_extract_conf_vars"),
        "colors.sh must drop the conf parser"
    );

    let watcher = read_script("color_watcher.sh");
    assert!(
        !watcher.contains("noctalia-colors.conf"),
        "watcher must not watch noctalia-colors.conf"
    );
    assert!(
        !watcher.contains("noctalia.conf"),
        "watcher must not watch noctalia.conf"
    );
}

// ── init.sh refuses a conf-only system (R8) ─────────────────────────────

#[test]
fn init_enable_refuses_conf_only_system() {
    let init = scripts_dir().join("init.sh");
    let home = tempfile::tempdir().unwrap();
    let hypr = home.path().join(".config").join("hypr");
    std::fs::create_dir_all(&hypr).unwrap();
    std::fs::write(hypr.join("hyprland.conf"), "general { gaps_in = 5 }\n").unwrap();
    let cache = home.path().join(".cache").join("hve");

    let out = run_bash(
        &format!("bash \"{}\" enable", init.display()),
        &[],
        &[
            ("HOME", home.path().to_str().unwrap()),
            ("HVE_CACHE_DIR", cache.to_str().unwrap()),
        ],
        home.path(),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "init enable must refuse a conf-only system; stdout={stdout}"
    );
    assert!(
        stdout.contains("hyprland.lua"),
        "refusal must name hyprland.lua: {stdout}"
    );
    assert!(
        stdout.contains("migrate") && stdout.contains("migre"),
        "refusal must carry EN and ES text: {stdout}"
    );
    assert!(
        !hypr.join("hyprland.lua").exists(),
        "refusal must not inject a Lua block"
    );
}

// ── install.sh runs a migration preflight before building (R8) ──────────

#[test]
fn install_sh_has_preflight_before_build() {
    let src = std::fs::read_to_string(repo_root().join("install.sh")).unwrap();
    let preflight = src
        .find("PREFLIGHT")
        .expect("install.sh must define a migration preflight section");
    let build = src
        .find("cargo build --release")
        .expect("install.sh must build HVE");
    assert!(
        preflight < build,
        "the migration preflight must run before the build"
    );
    assert!(
        src.contains("hyprland.lua"),
        "the preflight must inspect hyprland.lua"
    );
}
