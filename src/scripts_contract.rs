//! Deterministic shell-contract assertions for the Lua-only script layer
//! (drop-conf-support R1, R9, R10).
//!
//! These tests execute the real scripts under `assets/scripts/` in hermetic
//! temporary homes. A stub `bin/` is prepended to `PATH` so `hyprctl`,
//! `pgrep`, `noctalia` and `inotifywait` can never touch the developer's real
//! compositor during a test run.
#![cfg(test)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Output;

use crate::border_preset::{AnimLeaf, BorderColor, BorderParams, GlowParams};
use crate::preset_store::PresetStore;

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

// ── Capability routing: the colour pipeline reloads through ONE seam ────
//
// `reload-config` (openspec/specs/capability-routing/spec.md): the script
// side's RELOAD reaches the base compositor in exactly one place — the
// coalescer. (Other base mutations in the scripts, such as `shader.sh`'s
// `hyprctl keyword`, are a separate, unpoliced surface: this pin guards the
// reload path only.)
// `assemble.sh` → `reload_coalescer.sh` → `hyprctl reload` is the core's OWN
// pipeline (central scripts writing HVE's own overlay) asking the base to
// re-read it; no declared backend sits in that sequence, so the siblings
// rule ("a backend never calls another backend") does not apply to it. The
// pin keeps that property true: a burst stays coalesced (one reload in, at
// most one out, never lost) because no script can fire a second, uncoupled
// reload beside the drainer.
#[test]
fn colour_pipeline_reloads_only_through_the_coalescer() {
    for name in [
        "color_watcher.sh",
        "assemble.sh",
        "border.sh",
        "apply_animation.sh",
        "shader.sh",
        "geometry.sh",
    ] {
        let src = read_script(name);
        assert!(
            !src.contains("hyprctl reload"),
            "{name} must not fire `hyprctl reload` itself — the colour pipeline's \
             reload belongs to reload_coalescer.sh (the single fire path)"
        );
    }
    assert!(
        read_script("reload_coalescer.sh").contains("hyprctl reload"),
        "reload_coalescer.sh must keep the single `hyprctl reload` fire path"
    );
    assert!(
        read_script("assemble.sh").contains("hve_reload_queue"),
        "assemble.sh must queue the reload through the coalescer after the overlay write"
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

/// Another integration's terminal template. Capability routing forbids one
/// integration filling another's gap, so this file must NEVER supply the
/// tertiary: it exists in tests only to prove it is not consulted.
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

/// Another integration's template must NOT fill the gap: with no tertiary
/// from the winning module, the documented distinct fallback applies even
/// when a terminal template of the same scheme is present.
#[test]
fn missing_tertiary_ignores_the_terminal_template() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_terminal_template(home.path(), "#9e70d6");

    let colors = run_get_colors(home.path());
    assert_eq!(
        colors["tertiary"].as_str().unwrap(),
        "#94e2d5",
        "another integration's template must not supply the tertiary; the fallback applies"
    );
}

/// The winning module supplies its own tertiary from its own backend
/// sources: a v4 rendering whose primary agrees with the active scheme is
/// a real tertiary source, preferred over the documented fallback even
/// when another integration's template is present.
#[test]
fn winning_module_supplies_coherent_v4_tertiary() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_terminal_template(home.path(), "#9e70d6");
    write_v4_palette(home.path(), "67abe4", "9d00ff");

    let colors = run_get_colors(home.path());
    assert_eq!(
        colors["tertiary"].as_str().unwrap(),
        "#9d00ff",
        "the winning module must supply a coherent v4 tertiary itself"
    );
}

/// The winning module declares its tertiary hook, and the loader routes
/// through it: with no other integration's file present, the tertiary
/// still resolves from the module's own backend sources.
#[test]
fn winning_module_declares_its_own_tertiary_hook() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_v4_palette(home.path(), "67abe4", "9d00ff");

    let script = format!(
        r#"source "{scripts}/colors.sh" >/dev/null 2>&1
source "{scripts}/color_sources.d/noctalia_lua.sh" >/dev/null 2>&1
declare -F hve_colour_source_tertiary >/dev/null 2>&1 || {{ echo NO-HOOK; exit 3; }}
printf 'tertiary=%s\n' "$HVE_TERTIARY""#,
        scripts = scripts_dir().display()
    );
    let out = run_bash(
        &script,
        &[],
        &[("HOME", home.path().to_str().unwrap())],
        home.path(),
    );
    assert!(
        out.status.success(),
        "the winning module must declare hve_colour_source_tertiary: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("tertiary=#9d00ff"),
        "the loader must route the tertiary through the winning module, got: {stdout}"
    );
}

/// A v4 palette left over from an OLDER scheme (its primary disagrees with
/// the active one) must not leak its tertiary into the current scheme.
/// With no other integration allowed to fill the gap, the documented
/// distinct fallback applies.
#[test]
fn stale_v4_tertiary_falls_back_to_documented_colour() {
    let home = tempfile::tempdir().unwrap();
    write_v5_without_tertiary(home.path(), "67abe4", "d6915c");
    write_terminal_template(home.path(), "#9e70d6");
    // Older scheme: primary is GREEN, not the active blue.
    write_v4_palette(home.path(), "2ec436", "9d00ff");

    let colors = run_get_colors(home.path());
    assert_eq!(
        colors["tertiary"].as_str().unwrap(),
        "#94e2d5",
        "a stale v4 palette from another scheme must not supply the tertiary; the fallback applies"
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
    let cache = home.join(".cache").join("hve");
    let out = std::process::Command::new("bash")
        .arg(&script)
        .env("PATH", path)
        .env("HOME", home)
        .env("HVE_SAFE_DIR", &cache)
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

/// Plant the colour-authority descriptor in the sandbox cache dir, exactly
/// where the provider would have written it on a real machine.
fn write_color_authority(home: &Path, theme: &str, palette_file: &str, palette_name: &str) {
    let dir = home.join(".cache").join("hve");
    std::fs::create_dir_all(&dir).unwrap();
    let body = serde_json::json!({
        "backend": "noctalia-v5",
        "theme": theme,
        "palette_file": palette_file,
        "palette_name": palette_name,
    });
    std::fs::write(
        dir.join("color-authority.json"),
        serde_json::to_string_pretty(&body).unwrap(),
    )
    .unwrap();
}

/// Snapshot planted at a layout-agnostic path under the theme dir: the
/// central script must only read the descriptor's declaration, never
/// reconstruct any backend's directory layout to find the file.
fn write_descriptor_snapshot(home: &Path, theme: &str, palette_json: &str) -> String {
    let dir = home.join(".config").join("hve").join("themes").join(theme);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("palette.json");
    std::fs::write(&path, palette_json).unwrap();
    path.to_str().unwrap().to_owned()
}

fn write_live_palette(home: &Path, name: &str, palette_json: &str) {
    let dir = home.join(".config").join("noctalia").join("palettes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), palette_json).unwrap();
}

/// The applied theme carries a descriptor naming its saved snapshot while
/// the live scheme was rewritten externally: the snapshot must win, not
/// the live palette.
#[test]
fn applied_theme_snapshot_wins_over_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
    let snapshot = write_descriptor_snapshot(home.path(), "Animation", THEME_SNAPSHOT_JSON);
    write_color_authority(home.path(), "Animation", &snapshot, "other-palette");
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

/// A `wallpaper`-source theme owns no snapshot, so no descriptor exists:
/// the live chain still applies.
#[test]
fn wallpaper_source_theme_without_snapshot_uses_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Walls"}"#);
    // No descriptor planted: a wallpaper-source theme declares nothing.
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "a wallpaper-source theme has no snapshot, so the live palette applies"
    );
}

/// A descriptor naming a different theme than the applied one is stale
/// (leftover from an earlier theme): it must not hijack the colours.
#[test]
fn stale_descriptor_theme_falls_through_to_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
    let snapshot = write_descriptor_snapshot(home.path(), "Other", THEME_SNAPSHOT_JSON);
    write_color_authority(home.path(), "Other", &snapshot, "other-palette");
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "a stale descriptor (theme != applied theme) must not own the colours"
    );
}

/// A descriptor whose palette file is gone (theme deleted, snapshot lost)
/// must fall through instead of breaking the colour pipeline.
#[test]
fn descriptor_with_missing_palette_file_uses_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
    // Point at a snapshot path that was never created.
    let missing = home
        .path()
        .join(".config")
        .join("hve")
        .join("themes")
        .join("Animation")
        .join("palette.json");
    write_color_authority(
        home.path(),
        "Animation",
        missing.to_str().unwrap(),
        "other-palette",
    );
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "a descriptor with a missing palette file must fall through to live"
    );
}

/// A descriptor whose palette path escapes through a symlink must fall
/// through to the live palette (unit 1e, F3): a symlinked theme-dir
/// component can point outside `$HOME/.config/hve/themes/`, so the
/// textual prefix match alone is not enough.
#[test]
fn descriptor_with_symlinked_theme_dir_falls_through_to_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
    // Attacker snapshot outside the theme dir, linked in through a
    // symlinked theme-dir component.
    let outside = home.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("evil.json"), THEME_SNAPSHOT_JSON).unwrap();
    let link = home
        .path()
        .join(".config")
        .join("hve")
        .join("themes")
        .join("Evil");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let palette = link.join("evil.json");
    write_color_authority(
        home.path(),
        "Animation",
        palette.to_str().unwrap(),
        "evil",
    );
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "a palette reached through a symlinked theme dir must not own the colours"
    );
}

/// A descriptor whose palette file itself is a symlink to the outside
/// must fall through to the live palette (unit 1e, F3).
#[test]
fn descriptor_with_symlinked_palette_file_falls_through_to_live_palette() {
    let home = tempfile::tempdir().unwrap();
    write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
    let outside = home.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("evil.json"), THEME_SNAPSHOT_JSON).unwrap();
    let theme_dir = home
        .path()
        .join(".config")
        .join("hve")
        .join("themes")
        .join("Animation");
    std::fs::create_dir_all(&theme_dir).unwrap();
    std::os::unix::fs::symlink(outside.join("evil.json"), theme_dir.join("link.json"))
        .unwrap();
    let palette = theme_dir.join("link.json");
    write_color_authority(
        home.path(),
        "Animation",
        palette.to_str().unwrap(),
        "evil",
    );
    write_live_palette(home.path(), "skwd-wall", LIVE_SKWALL_JSON);

    let colors = run_get_colors_with_scheme(home.path(), "custom skwd-wall");
    assert_eq!(
        colors["primary"].as_str().unwrap(),
        "#e4aa67",
        "a symlinked palette file must not own the colours"
    );
}

/// The palette-name whitelist (unit 1e, F1): `..`, `.`, slashes and
/// leading dot/dash names must never own colours; a plain name must.
#[test]
fn hostile_palette_names_never_own_colours() {
    let check = |palette_name: &str| -> String {
        let home = tempfile::tempdir().unwrap();
        write_hve_config(home.path(), r#"{"last_applied_theme": "Animation"}"#);
        let snapshot = write_descriptor_snapshot(home.path(), "Animation", THEME_SNAPSHOT_JSON);
        write_color_authority(home.path(), "Animation", &snapshot, palette_name);
        let cache = home.path().join(".cache").join("hve");
        let out = run_bash(
            &format!(
                "source \"{}\" >/dev/null 2>&1; \
                 if hve_theme_owns_colours >/dev/null 2>&1; then echo OWNS; else echo REFUSED; fi",
                scripts_dir().join("colors.sh").display()
            ),
            &[],
            &[
                ("HOME", home.path().to_str().unwrap()),
                ("HVE_SAFE_DIR", cache.to_str().unwrap()),
            ],
            home.path(),
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    for hostile in [
        "..",
        ".",
        "../evil",
        "a/b",
        "/etc/passwd",
        "-evil",
        ".hidden",
    ] {
        assert_eq!(
            check(hostile),
            "REFUSED",
            "palette name {hostile:?} must never own colours"
        );
    }
    assert_eq!(
        check("theme-blue"),
        "OWNS",
        "a plain palette name must keep owning colours"
    );
}

// ── Colour-source loader: a new file, not central edits (unit 1d) ───────
// Capability routing (odd/tasks/hve-capability-routing.md, unit 1d):
// colour sources are modules under assets/scripts/color_sources.d/; the
// central chain only reads each module's id, priority, try and watch
// declarations. `pywal` is the first migrated source.

/// Sandbox scripts dir mirroring the repo layout: colors.sh plus its
/// color_sources.d/ modules (when present), so the loader under test
/// resolves modules exactly like production.
fn sandbox_scripts_with_modules() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let scripts = tmp.path().join("scripts");
    std::fs::create_dir_all(scripts.join("color_sources.d")).unwrap();
    std::fs::copy(
        scripts_dir().join("colors.sh"),
        scripts.join("colors.sh"),
    )
    .unwrap();
    let repo_modules = scripts_dir().join("color_sources.d");
    if repo_modules.is_dir() {
        for entry in std::fs::read_dir(&repo_modules).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "sh") {
                std::fs::copy(&path, scripts.join("color_sources.d").join(entry.file_name()))
                    .unwrap();
            }
        }
    }
    (tmp, scripts)
}

/// A live pywal layout: ~/.cache/wal/colors.json with all six mapped roles.
fn write_pywal_colors(home: &Path) {
    let dir = home.join(".cache").join("wal");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("colors.json"),
        r##"{"wallpaper":"/dev/null","colors":{"color0":"#111111","color3":"#222222","color5":"#333333","color7":"#444444","color10":"#555555","color13":"#666666"}}"##,
    )
    .unwrap();
}

/// A module with a HIGHER priority (lower number) than the live source wins
/// the chain: proves the loader dispatches modules and honours priority.
#[test]
fn colour_module_with_higher_priority_wins_the_chain() {
    let home = tempfile::tempdir().unwrap();
    write_pywal_colors(home.path());
    let (_tmp, scripts) = sandbox_scripts_with_modules();
    std::fs::write(
        scripts.join("color_sources.d").join("00-sentinel.sh"),
        "hve_colour_source_id() { printf '%s\\n' \"sentinel\"; }\n\
         hve_colour_source_priority() { printf '%s\\n' \"5\"; }\n\
         hve_colour_source_try() { HVE_PRIMARY=\"#123456\"; export HVE_PRIMARY; return 0; }\n\
         hve_colour_source_watch() { return 0; }\n",
    )
    .unwrap();

    let script = format!(
        r#"source "{}/colors.sh" >/dev/null 2>&1; printf 'primary=%s\n' "$HVE_PRIMARY""#,
        scripts.display()
    );
    let out = run_bash(
        &script,
        &[],
        &[("HOME", home.path().to_str().unwrap())],
        home.path(),
    );
    assert!(
        out.status.success(),
        "bash failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("primary=#123456"),
        "the higher-priority module must win over the live pywal source, got: {stdout}"
    );
}

/// The migrated pywal source resolves exactly like the old inline step.
#[test]
fn pywal_colors_flow_through_the_module() {
    let home = tempfile::tempdir().unwrap();
    write_pywal_colors(home.path());

    let colors = run_get_colors(home.path());
    assert_eq!(colors["primary"].as_str().unwrap(), "#222222");
    assert_eq!(colors["secondary"].as_str().unwrap(), "#666666");
    assert_eq!(colors["tertiary"].as_str().unwrap(), "#333333");
    assert_eq!(colors["surface"].as_str().unwrap(), "#111111");
    assert_eq!(colors["surface_lowest"].as_str().unwrap(), "#444444");
    assert_eq!(colors["accent"].as_str().unwrap(), "#555555");
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

// ── The overlay palette: every referenced token must be defined ─────────
//
// Live defect (2026-09-30, `odd/tasks/overlay-palette-token-emission.md`):
// the assembler emitted five palette lines by hand while four presets
// reference `tertiary` and several reference `error`. An undefined name in
// Lua is `nil`, so the gradient list was invalid and the compositor rejected
// the WHOLE overlay — every border and decoration setting in the file, not
// just the offending line.
//
// This closes the CLASS, not the instance: the REAL assembler runs, driven by
// the REAL `border.sh` (the apply path the UI and the engine use), over EVERY
// preset in `assets/borders/`, and every bare palette identifier a preset or
// a default UI colour slot can reference must be defined by the emitted
// `[SYSTEM: COLORS]` block. The fragment directory is generated inside the
// sandbox: `assets/fragments/` is gitignored and must never be relied on.

/// Hermetic tree with the real scripts (plus their colour-source modules) and
/// the real border presets behind an inert stub `bin/`, so the real
/// `border.sh` → `assemble.sh` path runs end to end without touching the
/// developer's compositor, their presets or their `~/.cache/hve`
/// (house pattern: `reload_coalescer.rs`, `Sandbox::build`).
struct OverlaySandbox {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    cache: PathBuf,
    bin: PathBuf,
}

impl OverlaySandbox {
    fn build() -> OverlaySandbox {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        let cache = root.join("cache");
        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(scripts.join("color_sources.d")).unwrap();
        std::fs::create_dir_all(home.join(".config/hypr")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();

        for name in [
            "assemble.sh",
            "border.sh",
            "apply_animation.sh",
            "utils.sh",
            "colors.sh",
            "reload_coalescer.sh",
        ] {
            std::fs::copy(scripts_dir().join(name), scripts.join(name))
                .unwrap_or_else(|e| panic!("cannot stage {name}: {e}"));
        }
        for entry in std::fs::read_dir(scripts_dir().join("color_sources.d")).unwrap().flatten() {
            if entry.path().extension().is_some_and(|e| e == "sh") {
                std::fs::copy(entry.path(), scripts.join("color_sources.d").join(entry.file_name()))
                    .unwrap();
            }
        }

        // The real preset set the UI presents, so the test covers exactly what
        // a user can apply.
        let borders = root.join("assets/borders");
        std::fs::create_dir_all(&borders).unwrap();
        for entry in std::fs::read_dir(repo_root().join("assets/borders")).unwrap().flatten() {
            std::fs::copy(entry.path(), borders.join(entry.file_name())).unwrap();
        }

        // Same for the built-in animations: the animation apply path is
        // covered with the same realism as borders.
        let animations = root.join("assets/animations");
        std::fs::create_dir_all(&animations).unwrap();
        for entry in std::fs::read_dir(repo_root().join("assets/animations")).unwrap().flatten() {
            std::fs::copy(entry.path(), animations.join(entry.file_name())).unwrap();
        }

        // Inert `pgrep` (exit 1) is what keeps assemble.sh from queueing a
        // reload: no coalescer drainer is ever spawned here.
        let bin = fake_bin_dir(&root);

        OverlaySandbox {
            _tmp: tmp,
            root,
            home,
            cache,
            bin,
        }
    }

    fn env(&self) -> Vec<(String, String)> {
        vec![
            ("HOME".to_owned(), self.home.to_string_lossy().into_owned()),
            // The scripts must locate `hve/presets/…` through the same rule
            // `PresetStore::new` uses (`dirs::config_dir()`): XDG first, then
            // `$HOME/.config`. Deliberately NOT `$HOME/.config`: a resolver
            // that hardcoded the home path would pass by accident, so the
            // preset directory lives under a distinct XDG root.
            (
                "XDG_CONFIG_HOME".to_owned(),
                self.root.join("xdg").to_string_lossy().into_owned(),
            ),
            ("HVE_CACHE_DIR".to_owned(), self.cache.to_string_lossy().into_owned()),
            (
                "PATH".to_owned(),
                format!(
                    "{}:{}",
                    self.bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            ),
        ]
    }

    fn run(&self, script: &str, args: &[&str]) -> Output {
        std::process::Command::new("bash")
            .arg(self.root.join("assets/scripts").join(script))
            .args(args)
            .envs(self.env())
            .current_dir(&self.home)
            .output()
            .expect("bash must be available")
    }

    /// Apply one border preset through the real `border.sh`, which writes the
    /// fragment and runs the real `assemble.sh`, then return the overlay.
    fn apply_border_preset(&self, name: &str) -> String {
        let out = self.run("border.sh", &[name]);
        assert!(
            out.status.success(),
            "border.sh {name} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.overlay()
    }

    /// Apply one animation preset through the real `apply_animation.sh`,
    /// then return the overlay.
    fn apply_animation_preset(&self, name: &str) -> String {
        let out = self.run("apply_animation.sh", &[name]);
        assert!(
            out.status.success(),
            "apply_animation.sh {name} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.overlay()
    }

    /// Write a preset into the USER directory exactly where `PresetStore::save`
    /// puts it: `$XDG_CONFIG_HOME/hve/presets/<category>/<name>.lua`
    /// (`src/preset_store.rs:23-29`). The sandbox's `XDG_CONFIG_HOME` is
    /// `<root>/xdg`, deliberately distinct from `$HOME/.config`.
    fn write_user_preset(&self, category: &str, name: &str, content: &str) {
        let dir = self.root.join("xdg/hve/presets").join(category);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.lua")), content).unwrap();
    }

    fn assemble(&self) -> String {
        let out = self.run("assemble.sh", &[]);
        assert!(
            out.status.success(),
            "assemble.sh failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.overlay()
    }

    fn overlay(&self) -> String {
        std::fs::read_to_string(self.cache.join("overlay.lua"))
            .expect("assemble.sh must write overlay.lua into the sandbox cache")
    }

    /// Every preset name, sorted, as the border picker lists them.
    fn preset_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root.join("assets/borders"))
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                (path.extension().is_some_and(|x| x == "lua"))
                    .then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
            })
            .collect();
        names.sort();
        names
    }

    /// Write a fragment the way the UI's draft path does, inside the sandbox.
    fn write_fragment(&self, module: &str, body: &str) {
        let dir = self.root.join("assets/fragments");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{module}.lua")), body).unwrap();
    }

    /// Plant a scheme file at the path the colour source reads.
    fn write_scheme(&self, body: &str) {
        std::fs::write(self.home.join(".config/hypr/noctalia.lua"), body).unwrap();
    }

    /// Add a token to the roster in the sandbox copy of `colors.sh` — the way a
    /// future role would be added — WITHOUT adding its fallback. That is the
    /// wiring bug the loud refusal exists to catch: a roster entry the fallback
    /// function does not answer must never reach the overlay as an empty value.
    fn plant_unknown_roster_token(&self, token: &str) {
        let path = self.root.join("assets/scripts/colors.sh");
        let src = std::fs::read_to_string(&path).unwrap();
        let needle = "HVE_PALETTE_TOKENS=\"";
        let start = src
            .find(needle)
            .expect("colors.sh must declare the palette roster");
        let rest = &src[start + needle.len()..];
        let end = rest
            .find('"')
            .expect("the roster declaration must be closed");
        let new_src = format!(
            "{} {token}{}",
            &src[..start + needle.len()],
            &rest[end..]
        );
        std::fs::write(&path, new_src).unwrap();
    }
}

/// A bare Lua identifier: a palette token, never a literal colour or a path.
fn is_bare_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The palette the overlay's `[SYSTEM: COLORS]` block defines: name → value.
fn overlay_palette(overlay: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut inside = false;
    for line in overlay.lines() {
        let line = line.trim();
        if line.contains("[SYSTEM: COLORS]") {
            inside = true;
            continue;
        }
        if inside && line.starts_with("--") && line.contains('[') {
            break; // the next section header ends the block
        }
        if !inside {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            let name = name.trim();
            if is_bare_identifier(name) {
                out.insert(name.to_owned(), value.trim().trim_matches('"').to_owned());
            }
        }
    }
    out
}

/// Names a preset declares itself (`local joker_green = …`): those are its own
/// colours, not palette references.
fn preset_locals(overlay: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in overlay.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("local ") {
            let name = rest
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                out.insert(name.to_owned());
            }
        }
    }
    out
}

/// Every bare palette identifier the overlay's body references: the items of a
/// `colors = { … }` gradient list (any spacing around the `=`), plus a bare
/// `inactive_border` value. Those are the positions where a token, not a
/// literal colour, is written.
fn referenced_tokens(overlay: &str, locals: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (idx, _) in overlay.match_indices("colors") {
        let rest = &overlay[idx..];
        let Some(after) = rest.strip_prefix("colors") else {
            continue;
        };
        // Tolerate any whitespace between `colors` and `=`: `colors={…}` and
        // `colors  = {…}` are the same gradient list. `colors_lowest` is not.
        let after = after.trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let Some(open) = after.find('{') else { continue };
        let Some(close) = after[open..].find('}') else {
            continue;
        };
        for item in after[open + 1..open + close].split(',') {
            let item = item.trim();
            if is_bare_identifier(item) && !locals.contains(item) {
                out.insert(item.to_owned());
            }
        }
    }
    for (idx, _) in overlay.match_indices("inactive_border") {
        let rest = &overlay[idx..];
        let Some(eq) = rest.find('=') else { continue };
        let value = rest[eq + 1..]
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches(',')
            .trim();
        if is_bare_identifier(value) && !locals.contains(value) {
            out.insert(value.to_owned());
        }
    }
    out
}

/// The first hardcoded palette value in a script: `#` followed by six hex
/// digits. The palette belongs to the colour pipeline; no script may pin a
/// value of its own beside it.
fn first_palette_literal(script: &str) -> Option<&str> {
    for (idx, _) in script.match_indices('#') {
        let hex: String = script[idx + 1..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if hex.len() >= 6 {
            return Some(&script[idx..idx + 7]);
        }
    }
    None
}

/// Every preset the UI can apply must assemble into an overlay that DEFINES
/// every palette token it references.
#[test]
fn every_preset_token_is_defined_in_the_assembled_overlay() {
    let sb = OverlaySandbox::build();
    let presets = sb.preset_names();
    assert!(
        presets.len() >= 14,
        "the sandbox must hold the real preset set, found {presets:?}"
    );

    let mut missing: Vec<String> = Vec::new();
    let mut references = 0usize;
    for preset in &presets {
        let overlay = sb.apply_border_preset(preset);
        let defined = overlay_palette(&overlay);
        let locals = preset_locals(&overlay);
        for token in referenced_tokens(&overlay, &locals) {
            references += 1;
            if !defined.contains_key(&token) {
                missing.push(format!("{preset}: `{token}`"));
            }
        }
    }

    assert!(
        references > 0,
        "the scanner found no palette reference at all — it is blind, not green"
    );
    assert!(
        missing.is_empty(),
        "the assembled overlay never defines palette token(s) the presets \
         reference. Lua reads an undefined name as nil, which makes the gradient \
         list invalid and makes the compositor reject the WHOLE overlay — every \
         border and decoration setting in the file is dropped, not just this \
         one. Undefined token, by preset:\n  {}",
        missing.join("\n  ")
    );
}

/// The UI can put a token into a draft fragment without touching a preset:
/// `src/main.rs` `BORDER_SLOT_CYCLE` seeds new colour slots with
/// `p:tertiary` / `p:error`, and the tune pane offers all six roles. That path
/// must be covered by the same guarantee.
#[test]
fn every_ui_default_token_is_defined_in_the_assembled_overlay() {
    let sb = OverlaySandbox::build();
    sb.write_fragment(
        "border",
        "hl.config({ general = { col = { active_border = \
         { colors = { primary, secondary, tertiary, error, surface, surface_lowest, accent }, \
         angle = 45 } } } })\n",
    );
    let overlay = sb.assemble();

    let defined = overlay_palette(&overlay);
    let locals = preset_locals(&overlay);
    let missing: Vec<String> = referenced_tokens(&overlay, &locals)
        .into_iter()
        .filter(|t| !defined.contains_key(t))
        .collect();
    assert!(
        missing.is_empty(),
        "a colour slot the UI can create references undefined palette token(s) \
         {missing:?}; the emitted palette defines {defined:?}"
    );
}

/// The emitted values come from the colour pipeline and NOWHERE else: a scheme
/// declaring every role must reach the overlay verbatim, so the assembler can
/// never pin a palette value of its own (that would make it a second palette
/// authority beside the theme).
#[test]
fn overlay_palette_values_come_from_the_colour_pipeline() {
    let sb = OverlaySandbox::build();
    sb.write_scheme(
        "local primary = \"rgb(67abe4)\"\n\
         local secondary = \"rgb(d6915c)\"\n\
         local tertiary = \"rgb(9566cc)\"\n\
         local error = \"rgb(f38ba8)\"\n\
         local surface = \"rgb(11202c)\"\n",
    );

    let overlay = sb.assemble();
    let palette = overlay_palette(&overlay);
    for (token, value) in [
        ("primary", "#67abe4"),
        ("secondary", "#d6915c"),
        ("tertiary", "#9566cc"),
        ("error", "#f38ba8"),
        ("surface", "#11202c"),
        // Roles the scheme does not carry keep the pipeline's own documented
        // fallbacks: surface_lowest from the surface, accent from the primary.
        ("surface_lowest", "#11202c"),
        ("accent", "#67abe4"),
    ] {
        assert_eq!(
            palette.get(token).map(String::as_str),
            Some(value),
            "`{token}` must carry the colour pipeline's value, not a literal \
             written in the assembler: {palette:?}"
        );
    }

    // Structural half of the same rule: the assembler must not carry a palette
    // value in ANY form, not even one no scenario above would exercise.
    assert_eq!(
        first_palette_literal(&read_script("assemble.sh")),
        None,
        "assemble.sh must not hardcode a palette value — the theme owns the \
         palette, and colors.sh is where its values live"
    );
}

/// A roster token the fallback function does not answer is a wiring bug. The
/// pipeline must refuse loudly — non-zero exit, no overlay written — instead of
/// exporting an empty value that the compositor would reject.
#[test]
fn unknown_roster_token_is_refused_before_it_reaches_the_overlay() {
    let sb = OverlaySandbox::build();
    // A valid overlay first, so a refused run has something to leave intact.
    let good = sb.assemble();
    assert!(
        !good.contains("bogus"),
        "sanity: the good overlay must not mention the undeclared token"
    );

    sb.plant_unknown_roster_token("bogus");
    let out = sb.run("assemble.sh", &[]);
    assert!(
        !out.status.success(),
        "assemble.sh must refuse to emit when a roster token has no fallback: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let overlay = sb.overlay();
    assert!(
        !overlay.contains("bogus"),
        "an undeclared token must never reach the overlay, not even as an empty \
         value — the compositor rejects a gradient list containing an empty \
         string and drops the WHOLE overlay"
    );
}

/// A preset reformatted without spaces around `=` (`colors={…}`) must still be
/// scanned: the reference detector tolerates arbitrary whitespace, or a
/// formatting change silently blinds the whole palette check.
#[test]
fn preset_without_spaces_around_equals_is_still_scanned() {
    let sb = OverlaySandbox::build();
    sb.write_fragment(
        "border",
        "hl.config({ general = { col = { active_border = \
         { colors={primary,secondary,tertiary}, angle = 45 } } } })\n",
    );
    let overlay = sb.assemble();

    let locals = preset_locals(&overlay);
    let refs = referenced_tokens(&overlay, &locals);
    assert!(
        refs.contains("primary")
            && refs.contains("secondary")
            && refs.contains("tertiary"),
        "the scanner must find palette references written as `colors={{…}}`: {refs:?}"
    );

    let defined = overlay_palette(&overlay);
    let missing: Vec<String> = refs
        .iter()
        .filter(|t| !defined.contains_key(*t))
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "referenced token(s) {missing:?} are not defined in the overlay"
    );
}

// ── Saved presets must apply: the USER directory is a real location ────
//
// Live defect (2026-10-01,
// `odd/tasks/apply-saved-presets-borders-and-animations.md`): `border.sh` and
// `apply_animation.sh` resolved presets against the BUILT-IN directory only,
// so a preset the keeper SAVED under `$XDG_CONFIG_HOME/hve/presets/…` never
// reached the fragment — the desktop got the security fallback while the panel
// showed the saved values (the Rust read-back, `PresetStore::read_border_file`,
// checks both directories; the shell checked one).
//
// Decisions under test: the resolver takes the user directory as an optional
// second location, BUILT-IN FIRST (D2: a user preset never shadows a
// built-in), and an unknown name still yields the safe fallback (D3: that is
// a security property).

/// The params the save path builds (`tune_params_from_window` →
/// `generate_border_lua_full`): a keeper's own angle, gradient colours,
/// inactive colour and glow — none of which any built-in carries.
fn keeper_saved_border_params() -> BorderParams {
    BorderParams {
        active_colors: vec![
            BorderColor::Custom { r: 0x11, g: 0x22, b: 0x33, a: 0xff },
            BorderColor::Custom { r: 0x44, g: 0x55, b: 0x66, a: 0xff },
        ],
        angle: 77,
        inactive: BorderColor::Custom { r: 0x77, g: 0x88, b: 0x99, a: 0xff },
        border_size: Some(3),
        glow: Some(GlowParams {
            enabled: true,
            range: 21,
            render_power: 6,
            color: BorderColor::Custom { r: 0xaa, g: 0xbb, b: 0xcc, a: 0xff },
            color_inactive: BorderColor::Custom { r: 0x01, g: 0x02, b: 0x03, a: 0xff },
            offset: (3, 4),
        }),
        rule_enabled: false,
        animations: vec![AnimLeaf {
            leaf: "borderangle".to_string(),
            enabled: true,
            speed: Some(200),
            bezier: None,
            style: None,
        }],
        curves: Vec::new(),
    }
}

/// A border preset the keeper SAVED must reach the desktop fragment with its
/// own values — not the plain-`primary` security fallback. The content is
/// generated by the very function the save path calls, so this proves the
/// save format → resolver → fragment → overlay chain end to end.
#[test]
fn saved_user_border_preset_reaches_the_overlay() {
    let sb = OverlaySandbox::build();
    let content = PresetStore::generate_border_lua_full("keeper_saved", &keeper_saved_border_params());
    sb.write_user_preset("borders", "keeper_saved", &content);

    let overlay = sb.apply_border_preset("keeper_saved");

    for needle in [
        // Gradient colours and angle the keeper tuned.
        "colors = { \"rgba(112233ff)\", \"rgba(445566ff)\" }",
        "angle = 77",
        // Inactive colour.
        "inactive_border = \"rgba(778899ff)\"",
        // Glow (decoration.shadow).
        "range = 21",
        "color = \"rgba(aabbccff)\"",
        // The tune's animation leaf.
        "hl.animation({ leaf = \"borderangle\", enabled = true, speed = 200 })",
    ] {
        assert!(
            overlay.contains(needle),
            "the saved border preset's `{needle}` never reached the overlay — \
             the preset resolved to the security fallback instead:\n{overlay}"
        );
    }
    assert!(
        !overlay.contains("hl.config({ general = { [\"col.active_border\"] = primary } })"),
        "a SAVED preset must never fall back to the plain-primary border: {overlay}"
    );
}

/// Same contract for animations: a saved animation preset must deliver its
/// bezier/speed to the animation module of the overlay.
#[test]
fn saved_user_animation_preset_reaches_the_overlay() {
    let sb = OverlaySandbox::build();
    let content = PresetStore::generate_animation_lua("keeper_saved_anim", 0.25, 0.1, 0.25, 1.0);
    sb.write_user_preset("animations", "keeper_saved_anim", &content);

    let overlay = sb.apply_animation_preset("keeper_saved_anim");

    assert!(
        overlay.contains("bezier = ({ 0.25, 0.1, 0.25, 1 })"),
        "the saved animation preset's bezier never reached the overlay:\n{overlay}"
    );
    assert!(
        overlay.contains("-- @Source: user"),
        "the overlay must carry the keeper's saved animation file, not a built-in: {overlay}"
    );
    assert!(
        !overlay.contains("hl.config({ animations = { enabled = true } })"),
        "a SAVED animation preset must never take the safe-animation fallback: {overlay}"
    );
}

/// An unknown name must still produce the safe fallback for BOTH callers:
/// that behaviour is a security property (D3), not an accident of the bug.
#[test]
fn unknown_preset_name_still_takes_the_safe_fallback() {
    let sb = OverlaySandbox::build();

    let overlay = sb.apply_border_preset("no_such_preset_anywhere");
    assert!(
        overlay.contains("hl.config({ general = { [\"col.active_border\"] = primary } })"),
        "an unknown border preset must yield the safe fallback:\n{overlay}"
    );

    let overlay = sb.apply_animation_preset("no_such_preset_anywhere");
    assert!(
        overlay.contains("hl.config({ animations = { enabled = true } })"),
        "an unknown animation preset must yield the safe animation fallback:\n{overlay}"
    );
}

/// Search order is BUILT-IN FIRST (D2): a user preset must never shadow a
/// built-in of the same name, in either category.
#[test]
fn built_in_preset_wins_over_user_preset_with_the_same_name() {
    let sb = OverlaySandbox::build();
    sb.write_user_preset(
        "borders",
        "02_diagonal",
        "-- USER SHADOW MARKER (border)\n\
         hl.config({ general = { col = { active_border = { angle = 77 } } } })\n",
    );
    sb.write_user_preset(
        "animations",
        "07_lineal",
        "-- USER SHADOW MARKER (animation)\n\
         hl.config({ animations = { speed = 999 } })\n",
    );

    let overlay = sb.apply_border_preset("02_diagonal");
    assert!(
        overlay.contains("-- @Title: Diagonal"),
        "the BUILT-IN border preset must win the resolution: {overlay}"
    );
    assert!(
        !overlay.contains("USER SHADOW MARKER"),
        "a user preset must never shadow a built-in of the same name: {overlay}"
    );

    let overlay = sb.apply_animation_preset("07_lineal");
    assert!(
        overlay.contains("-- @Title: Linear"),
        "the BUILT-IN animation preset must win the resolution: {overlay}"
    );
    assert!(
        !overlay.contains("USER SHADOW MARKER"),
        "a user preset must never shadow a built-in of the same name: {overlay}"
    );
}
