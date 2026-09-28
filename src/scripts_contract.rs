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
        "reload_coalescer.sh",
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

// ── Tertiary completion: never alias the secondary ──────────────────────

/// Run the colour extractor in a hermetic HOME laid out by the caller and
/// parse the JSON it prints on stdout.
fn run_get_colors(home: &Path) -> serde_json::Value {
    let script = scripts_dir().join("get_colors.sh");
    let out = run_bash(
        &format!("bash \"{}\"", script.display()),
        &[],
        &[("HOME", home.to_str().unwrap())],
        home,
    );
    assert!(
        out.status.success(),
        "get_colors.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("get_colors.sh must print JSON: {stdout}"));
    serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("bad JSON from get_colors.sh: {e}: {json}"))
}

/// Noctalia v5 template output: primary/secondary but NO tertiary (the
/// wallpaper path renders exactly this, verified on the maintainer machine).
fn write_v5_without_tertiary(home: &Path, primary: &str, secondary: &str) {
    let hypr = home.join(".config").join("hypr");
    std::fs::create_dir_all(&hypr).unwrap();
    std::fs::write(
        hypr.join("noctalia.lua"),
        format!(
            "-- Generated by Noctalia\nlocal primary = \"rgb({primary})\"\n\
             local secondary = \"rgb({secondary})\"\n"
        ),
    )
    .unwrap();
}

/// Noctalia-generated terminal template (kitty palette). `color4` is the
/// palette's 4th accent — the third theme role after color2/primary and
/// color3/secondary.
fn write_terminal_template(home: &Path, color4: &str) {
    let dir = home.join(".config").join("kitty").join("themes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("noctalia.conf"),
        format!("color4 {color4}\ncolor12 {color4}\n"),
    )
    .unwrap();
}

/// A v4 palette file. `primary` is what the coherence gate compares against.
fn write_v4_palette(home: &Path, primary: &str, tertiary: &str) {
    let dir = home.join(".config").join("hypr").join("noctalia");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("noctalia-colors.lua"),
        format!("primary = \"rgb({primary})\"\ntertiary = \"rgb({tertiary})\"\n"),
    )
    .unwrap();
}

/// A scheme with no tertiary anywhere must not paint the same colour twice:
/// the secondary is a real source for `secondary` only.
#[test]
fn missing_tertiary_never_aliases_the_secondary() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");

    let colors = run_get_colors(home.path());
    let secondary = colors["secondary"].as_str().unwrap();
    let tertiary = colors["tertiary"].as_str().unwrap();

    assert_eq!(colors["primary"].as_str().unwrap(), "#67abe4");
    assert_eq!(secondary, "#d6915c");
    assert_ne!(
        tertiary, secondary,
        "tertiary must never silently become the secondary"
    );
    assert_eq!(
        tertiary, "#94e2d5",
        "with no real source at all, the documented distinct fallback is used"
    );
}

/// The shell-generated terminal template of the SAME scheme supplies the
/// fourth accent when the v5 output carries no tertiary.
#[test]
fn missing_tertiary_resolves_from_the_terminal_template() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_terminal_template(home.path(), "#9e70d6");

    let colors = run_get_colors(home.path());
    assert_eq!(
        colors["tertiary"].as_str().unwrap(),
        "#9e70d6",
        "the terminal template's 4th accent must supply the tertiary"
    );
}

/// A v4 palette whose primary agrees with the active scheme is a real
/// tertiary source and is preferred over the terminal template.
#[test]
fn coherent_v4_tertiary_wins_over_the_terminal_template() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_terminal_template(home.path(), "#9e70d6");
    write_v4_palette(home.path(), "67abe4", "9d00ff");

    let colors = run_get_colors(home.path());
    assert_eq!(
        colors["tertiary"].as_str().unwrap(),
        "#9d00ff",
        "a coherent v4 palette is the first tertiary source after v5"
    );
}

/// A v4 palette left over from an OLDER scheme (its primary disagrees with
/// the active one) must not leak its tertiary into the current scheme.
#[test]
fn stale_v4_tertiary_is_skipped_for_the_same_scheme_template() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_terminal_template(home.path(), "#9e70d6");
    // Older scheme: primary is GREEN, not the active blue.
    write_v4_palette(home.path(), "2ec436", "9d00ff");

    let colors = run_get_colors(home.path());
    assert_eq!(
        colors["tertiary"].as_str().unwrap(),
        "#9e70d6",
        "a stale v4 palette from another scheme must not supply the tertiary"
    );
}

// ── Theme owns the palette: the applied snapshot beats the live one ──────

const THEME_SNAPSHOT_JSON: &str = r##"{"dark":{"mPrimary":"#67abe4","mSecondary":"#d6915c","mTertiary":"#9566cc","mSurface":"#11202c","mSurfaceVariant":"#1d3549"},"light":{"mPrimary":"#2279c3"}}"##;
const LIVE_SKWALL_JSON: &str = r##"{"dark":{"mPrimary":"#e4aa67","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#291f14","mSurfaceVariant":"#372a1b"}}"##;

/// Run get_colors.sh with a stub noctalia reporting the given scheme.
/// Mirrors run_get_colors, except the noctalia stub answers
/// `color-scheme-get` with `scheme` so the live-palette path is exercised.
fn run_get_colors_with_scheme(home: &Path, scheme: &str) -> serde_json::Value {
    let bin_tmp = tempfile::tempdir().unwrap();
    let bin = fake_bin_dir(bin_tmp.path());
    std::fs::write(
        bin.join("noctalia"),
        format!("#!/bin/bash\nprintf '%s\\n' \"{scheme}\"\n"),
    )
    .unwrap();
    #[cfg(unix)]
    make_exec(&bin.join("noctalia"));
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let script = scripts_dir().join("get_colors.sh");
    let out = std::process::Command::new("bash")
        .arg(&script)
        .env("PATH", path)
        .env("HOME", home)
        .current_dir(home)
        .output()
        .expect("bash must be available");
    assert!(
        out.status.success(),
        "get_colors.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("get_colors.sh must print JSON: {stdout}"));
    serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("bad JSON from get_colors.sh: {e}: {json}"))
}

fn write_hve_config(home: &Path, body: &str) {
    let dir = home.join(".config").join("hve");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.json"), body).unwrap();
}

fn write_theme_snapshot(home: &Path, theme: &str, palette_json: &str, source_exact: &str) {
    let dir = home
        .join(".config")
        .join("hve")
        .join("themes")
        .join(theme)
        .join("providers")
        .join("noctalia-v5");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("palette.json"), palette_json).unwrap();
    // Exact bytes: the caller decides about the trailing newline.
    std::fs::write(dir.join("source.txt"), source_exact).unwrap();
}

fn write_theme_source_only(home: &Path, theme: &str, source: &str) {
    let dir = home
        .join(".config")
        .join("hve")
        .join("themes")
        .join(theme)
        .join("providers")
        .join("noctalia-v5");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("source.txt"), source).unwrap();
}

fn write_live_palette(home: &Path, name: &str, palette_json: &str) {
    let dir = home.join(".config").join("noctalia").join("palettes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), palette_json).unwrap();
}

/// The applied theme carries a `custom` snapshot while the live scheme was
/// rewritten externally: the snapshot must win, not the live palette.
#[test]
fn applied_theme_snapshot_wins_over_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
    // No trailing newline on purpose: the resolver must accept the raw bytes.
    write_theme_snapshot(
        home.path(),
        "Animation",
        THEME_SNAPSHOT_JSON,
        "custom other-palette",
    );
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#67abe4",
        "the theme snapshot must own the colours, not the live palette"
    );
    assert_eq!(colors["secondary"].as_str().unwrap(), "#d6915c");
    assert_eq!(colors["tertiary"].as_str().unwrap(), "#9566cc");
    assert_eq!(colors["surface"].as_str().unwrap(), "#11202c");
    assert_ne!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "the externally rewritten palette must not repaint HVE"
    );
}

/// With no theme applied, behaviour is exactly today's: the live wins.
#[test]
fn no_theme_config_falls_through_to_live_palette() {
    // Case 1: no config.json at all.
    let home = tempfile::tempdir().unwrap();
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);
    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "without a theme the live palette must keep flowing through"
    );

    // Case 2: config.json exists but names no theme.
    let home2 = tempfile::tempdir().unwrap();
    write_hve_config(home2.path(), r#"{"last_applied_theme": ""}"#);
    write_live_palette(home2.path(), "skwd-wall", LIVE_SKWALL_JSON);
    let colors2 = run_get_colors_with_scheme(home2.path(), "custom skwd-wall");
    assert_eq!(
        colors2["primary"].as_str().unwrap(),
        "#e4aa67",
        "with an empty theme name the live palette must keep flowing through"
    );
}

/// A `wallpaper`-source theme owns no snapshot: the live chain still applies.
#[test]
fn wallpaper_source_theme_without_snapshot_uses_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Walls"}"#);
    write_theme_source_only(home.path(), "Walls", "wallpaper\n");
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "a wallpaper-source theme has no snapshot, so the live palette applies"
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
