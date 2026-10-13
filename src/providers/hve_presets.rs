use crate::config::Config;
use crate::engine::Engine;
use crate::theme_manager::{ProviderCapabilities, ThemeFact, ThemeProvider};
use std::fs;
use std::path::Path;

/// `06_impacto_clasico.lua` → `06 impacto clasico`. Display only: the saved
/// record keeps the real file name, the panel shows a human-readable stem.
fn pretty_preset_file(name: &str) -> String {
    let leaf = name.rsplit('/').next().unwrap_or(name);
    let stem = leaf.rsplit_once('.').map(|(s, _)| s).unwrap_or(leaf);
    stem.replace('_', " ").trim().to_string()
}

/// Push one label/value line, skipping empty values (a theme that never set a
/// shader must not render an empty row).
fn push_text(facts: &mut Vec<ThemeFact>, label: &str, value: String) {
    if !value.is_empty() {
        facts.push(ThemeFact {
            label: label.to_string(),
            value,
            swatches: Vec::new(),
        });
    }
}

/// Read the theme's own `state.json` into panel facts. Missing or unreadable
/// state is not an error: the panel simply shows nothing from this provider.
fn preset_facts(provider_dir: &Path) -> Vec<ThemeFact> {
    // W10b: the theme's `state.json` is an UNTRUSTED file — bounded,
    // regular-file-guarded read (W10 owns the ONE set of limits and the ONE
    // error type). A refusal shows nothing, exactly like a missing state.
    let Ok(raw) = crate::theme_media::read_theme_record(&provider_dir.join("state.json")) else {
        return Vec::new();
    };
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };

    let mut facts: Vec<ThemeFact> = Vec::new();
    if let Some(v) = state.get("active_anim_file").and_then(|v| v.as_str()) {
        push_text(&mut facts, "Animation", pretty_preset_file(v));
    }
    if let Some(v) = state.get("active_border_file").and_then(|v| v.as_str()) {
        push_text(&mut facts, "Border", pretty_preset_file(v));
    }
    if let Some(v) = state.get("active_shader_file").and_then(|v| v.as_str()) {
        push_text(&mut facts, "Shader", pretty_preset_file(v));
    }
    if let Some(v) = state.get("border_size").and_then(|v| v.as_i64()) {
        push_text(&mut facts, "Border size", format!("{}px", v));
    }
    if let Some(v) = state.get("border_radius").and_then(|v| v.as_i64()) {
        push_text(&mut facts, "Radius", format!("{}px", v));
    }
    if let (Some(a), Some(b)) = (
        state.get("gaps_in").and_then(|v| v.as_i64()),
        state.get("gaps_out").and_then(|v| v.as_i64()),
    ) {
        push_text(&mut facts, "Gaps in / out", format!("{} / {}", a, b));
    }

    facts
}

pub struct HvePresetsProvider {
    engine: Engine,
}

impl HvePresetsProvider {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }

    /// Bundle the custom presets the active selection uses (theme-packages T4).
    /// Best-effort by contract: an unreadable shipped set only warns and the
    /// save continues; a missing source is skipped inside the bundler.
    fn bundle_active_presets(&self, theme_dir: &Path, active: &crate::theme_presets::ActivePresets) {
        let Some(shipped) =
            crate::theme_presets::ShippedPresets::load(&self.engine.assets_dir())
        else {
            tracing::warn!(
                "[hve-presets] cannot read the shipped preset set; skipping custom preset bundling"
            );
            return;
        };
        crate::theme_presets::bundle_theme_presets(
            theme_dir,
            active,
            &shipped,
            &crate::preset_store::PresetStore::root(),
        );
    }
}

impl ThemeProvider for HvePresetsProvider {
    fn id(&self) -> &str {
        "hve-presets"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.hve-presets"
    }

    fn icon(&self) -> &str {
        "◈"
    }

    fn shell(&self) -> &str {
        "hyprland"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::ANIMATIONS
            | ProviderCapabilities::BORDERS
            | ProviderCapabilities::SHADERS
    }

    /// The theme's own saved selection: which animation / border / shader was
    /// active and the geometry values, straight from `state.json`.
    fn theme_facts(&self, provider_dir: &Path) -> Vec<ThemeFact> {
        preset_facts(provider_dir)
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let cfg = Config::load();
        // T4: the active preset file names the theme records, captured before
        // the state JSON so the same selection drives the bundle below.
        let active = crate::theme_presets::ActivePresets {
            animations: cfg.active_anim_file.clone(),
            borders: cfg.active_border_file.clone(),
            shaders: cfg.active_shader_file.clone(),
        };
        let state = serde_json::json!({
            "active_anim_file": cfg.active_anim_file,
            "active_border_file": cfg.active_border_file,
            "active_shader_file": cfg.active_shader_file,
            "border_size": cfg.border_size,
            "border_radius": cfg.border_radius,
            "gaps_in": cfg.gaps_in,
            "gaps_out": cfg.gaps_out,
        });

        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| format!("Serialize error: {}", e))?;
        fs::write(provider_dir.join("state.json"), &json)
            .map_err(|e| format!("Cannot write state.json: {}", e))?;

        // T4: bundle the CUSTOM presets the theme uses under `presets/`, so the
        // look travels. Best-effort: an unreadable shipped set or a missing
        // source only warns and never fails the save.
        self.bundle_active_presets(theme_dir, &active);

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let state_path = theme_dir.join("providers").join(self.id()).join("state.json");
        // W10b: the theme's `state.json` is an UNTRUSTED file — bounded,
        // regular-file-guarded read (W10 owns the ONE set of limits and the
        // ONE error type). A FIFO or over-bound state refuses the apply with
        // the ordinary error below instead of blocking or slurping.
        let raw = crate::theme_media::read_theme_record(&state_path)
            .map_err(|e| format!("Cannot read state.json: {}", e))?;
        let state: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| format!("Parse error: {}", e))?;

        // T4: install the CUSTOM presets the theme carries under `presets/`
        // into the user's store BEFORE the apply path resolves them, so a
        // preset that travelled with the theme is found. An existing user
        // preset is kept (the user's copy wins). Best-effort by contract: a
        // theme without `presets/` is a no-op and applies exactly as before.
        let report = crate::theme_presets::install_theme_presets(
            theme_dir,
            &crate::preset_store::PresetStore::root(),
        );
        if !report.installed.is_empty() || !report.kept.is_empty() {
            tracing::info!(
                "[hve-presets] theme presets: {} installed, {} kept (user copy wins)",
                report.installed.len(),
                report.kept.len()
            );
        }

        let mut cfg = Config::load();
        let mut changed = false;

        if let Some(v) = state.get("active_anim_file").and_then(|v| v.as_str()) {
            if !v.is_empty() && cfg.active_anim_file != v {
                let _ = self.engine.apply_animation(v);
                cfg.active_anim_file = v.to_string();
                changed = true;
            } else if v.is_empty() && !cfg.active_anim_file.is_empty() {
                let _ = self.engine.apply_animation("none");
                cfg.active_anim_file = String::new();
                changed = true;
            }
        }
        if let Some(v) = state.get("active_border_file").and_then(|v| v.as_str()) {
            if !v.is_empty() && cfg.active_border_file != v {
                let _ = self.engine.apply_border(v);
                cfg.active_border_file = v.to_string();
                changed = true;
            } else if v.is_empty() && !cfg.active_border_file.is_empty() {
                let _ = self.engine.apply_border("none");
                cfg.active_border_file = String::new();
                changed = true;
            }
        }
        if let Some(v) = state.get("active_shader_file").and_then(|v| v.as_str()) {
            if !v.is_empty() && cfg.active_shader_file != v {
                let _ = self.engine.apply_shader(v);
                cfg.active_shader_file = v.to_string();
                changed = true;
            } else if v.is_empty() && !cfg.active_shader_file.is_empty() {
                let _ = self.engine.apply_shader("none");
                cfg.active_shader_file = String::new();
                changed = true;
            }
        }
        if let Some(v) = state.get("border_size").and_then(|v| v.as_i64()) {
            let size = v as i32;
            if size != cfg.border_size {
                cfg.border_size = size;
                changed = true;
            }
        }
        // Corner radius (decoration.rounding). Applied together with geometry
        // so the fragment always carries both values.
        if let Some(v) = state.get("border_radius").and_then(|v| v.as_i64()) {
            let radius = v as i32;
            if radius != cfg.border_radius {
                cfg.border_radius = radius;
                changed = true;
            }
        }
        // Gaps. Read both independently (they are saved separately) but the
        // geometry fragment carries all four values together.
        if let Some(v) = state.get("gaps_in").and_then(|v| v.as_i64()) {
            let gap = v as i32;
            if gap != cfg.gaps_in {
                cfg.gaps_in = gap;
                changed = true;
            }
        }
        if let Some(v) = state.get("gaps_out").and_then(|v| v.as_i64()) {
            let gap = v as i32;
            if gap != cfg.gaps_out {
                cfg.gaps_out = gap;
                changed = true;
            }
        }

        if changed {
            let _ = self
                .engine
                .apply_geometry(cfg.border_size, cfg.border_radius, cfg.gaps_in, cfg.gaps_out);
            let _ = cfg.save();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_state(dir: &Path, json: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("state.json"), json).unwrap();
    }

    #[test]
    fn preset_facts_read_the_saved_selection() {
        let tmp = TempDir::new().unwrap();
        write_state(
            tmp.path(),
            r#"{
                "active_anim_file": "06_impacto_clasico.lua",
                "active_border_file": "14_looper.lua",
                "active_shader_file": "",
                "border_size": 3,
                "border_radius": 22,
                "gaps_in": 10,
                "gaps_out": 12
            }"#,
        );

        let facts = preset_facts(tmp.path());
        let rows: Vec<(String, String)> = facts
            .iter()
            .map(|f| (f.label.clone(), f.value.clone()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("Animation".to_string(), "06 impacto clasico".to_string()),
                ("Border".to_string(), "14 looper".to_string()),
                ("Border size".to_string(), "3px".to_string()),
                ("Radius".to_string(), "22px".to_string()),
                ("Gaps in / out".to_string(), "10 / 12".to_string()),
            ],
            "a shader that was never set must not produce an empty row"
        );
        assert!(
            facts.iter().all(|f| f.swatches.is_empty()),
            "presets own no palette colors"
        );
    }

    #[test]
    fn preset_facts_are_empty_when_state_is_missing_or_broken() {
        let tmp = TempDir::new().unwrap();
        assert!(
            preset_facts(tmp.path()).is_empty(),
            "no state.json means nothing to show"
        );
        write_state(tmp.path(), "{not json");
        assert!(
            preset_facts(tmp.path()).is_empty(),
            "unreadable state must not panic"
        );
    }

    #[test]
    fn pretty_preset_file_strips_the_directory_and_extension() {
        assert_eq!(pretty_preset_file("06_impacto_clasico.lua"), "06 impacto clasico");
        assert_eq!(pretty_preset_file("/abs/path/12_rebote.lua"), "12 rebote");
        assert_eq!(pretty_preset_file("rgb.json"), "rgb");
        assert_eq!(pretty_preset_file("none"), "none");
    }

    /// theme-packages T4 wiring: the save path bundles a CUSTOM preset into the
    /// theme and skips a shipped one. `TempEnv` redirects HOME so the user
    /// store and the theme live in a throwaway tree; the fixture `assets/`
    /// decides shipped vs custom exactly like the real repo.
    #[test]
    fn save_bundles_custom_presets_and_skips_shipped_ones() {
        let _env = crate::test_utils::TempEnv::new();
        let proj = TempDir::new().unwrap();
        for category in crate::theme_presets::CATEGORIES {
            fs::create_dir_all(proj.path().join("assets").join(category)).unwrap();
        }
        fs::write(proj.path().join("assets/borders/shipped.lua"), "-- shipped").unwrap();

        let user_anim = dirs::config_dir().unwrap().join("hve/presets/animations");
        fs::create_dir_all(&user_anim).unwrap();
        let bytes = b"-- keeper's custom animation\n";
        fs::write(user_anim.join("custom.lua"), bytes).unwrap();

        let theme = TempDir::new().unwrap();
        let provider = HvePresetsProvider::new(Engine::new(proj.path()));
        provider.bundle_active_presets(
            theme.path(),
            &crate::theme_presets::ActivePresets {
                animations: "custom.lua".into(),
                borders: "shipped.lua".into(),
                shaders: String::new(),
            },
        );

        assert_eq!(
            fs::read(theme.path().join("presets/animations/custom.lua")).unwrap(),
            bytes,
            "the custom animation must be bundled into the theme"
        );
        assert!(
            !theme.path().join("presets/borders/shipped.lua").exists(),
            "a shipped border must never be copied into the theme"
        );
    }

    /// theme-packages T4 wiring: `apply` installs the theme's bundled custom
    /// preset into the user store BEFORE the existing apply path resolves it,
    /// and then runs that path with the theme's active file name. Stub scripts
    /// record their argument so the test proves the order without a compositor.
    #[test]
    fn apply_installs_bundled_presets_before_applying() {
        let _env = crate::test_utils::TempEnv::new();
        let proj = TempDir::new().unwrap();
        let scripts = proj.path().join("assets/scripts");
        fs::create_dir_all(&scripts).unwrap();
        let log = proj.path().join("applied.log");
        for name in ["apply_animation.sh", "border.sh", "shader.sh", "geometry.sh"] {
            fs::write(
                scripts.join(name),
                format!("#!/bin/bash\necho \"$1\" >> {}\n", log.display()),
            )
            .unwrap();
        }

        let theme = TempDir::new().unwrap();
        let provider_dir = theme.path().join("providers/hve-presets");
        fs::create_dir_all(&provider_dir).unwrap();
        fs::write(
            provider_dir.join("state.json"),
            r#"{"active_anim_file":"theme_custom.lua","active_border_file":"","active_shader_file":""}"#,
        )
        .unwrap();
        let preset_dir = theme.path().join("presets/animations");
        fs::create_dir_all(&preset_dir).unwrap();
        fs::write(preset_dir.join("theme_custom.lua"), "-- custom").unwrap();

        let provider = HvePresetsProvider::new(Engine::new(proj.path()));
        provider.apply(theme.path()).unwrap();

        let installed = dirs::config_dir()
            .unwrap()
            .join("hve/presets/animations/theme_custom.lua");
        assert!(
            installed.is_file(),
            "apply must install the bundled preset into the user store before applying"
        );
        let applied = fs::read_to_string(&log).unwrap();
        assert!(
            applied.contains("theme_custom.lua"),
            "the existing apply path must run unchanged with the theme's active file: {applied}"
        );
    }

    /// U2 end to end through the REAL save path: the keeper's Space theme
    /// records the bare stem `"Space"` while the user store holds
    /// `borders/Space.lua`. Re-saving that theme must bundle
    /// `presets/borders/Space.lua` into the package, so the theme carries — and
    /// installs on another machine — the border it actually uses.
    #[test]
    fn save_bundles_a_legacy_bare_stem_preset_into_the_package() {
        let _env = crate::test_utils::TempEnv::new();
        let proj = TempDir::new().unwrap();
        for category in crate::theme_presets::CATEGORIES {
            fs::create_dir_all(proj.path().join("assets").join(category)).unwrap();
        }

        // The record an older save left behind: the stem alone, no extension.
        let mut cfg = Config::default();
        cfg.active_border_file = "Space".into();
        cfg.save().unwrap();

        let user_border = dirs::config_dir().unwrap().join("hve/presets/borders");
        fs::create_dir_all(&user_border).unwrap();
        let bytes = b"-- keeper's space border\n";
        fs::write(user_border.join("Space.lua"), bytes).unwrap();

        let theme = TempDir::new().unwrap();
        let provider = HvePresetsProvider::new(Engine::new(proj.path()));
        provider
            .save(theme.path())
            .expect("a legacy bare stem must never fail a save");

        let bundled = theme.path().join("presets/borders/Space.lua");
        assert!(
            bundled.is_file(),
            "the theme must carry presets/borders/Space.lua for a bare stem record"
        );
        assert_eq!(fs::read(&bundled).unwrap(), bytes);
    }

    /// SECURITY: an unsafe `active_anim_file` (a `..` traversal) must never
    /// fail a save, and must never let the bundler write outside the theme
    /// folder. The source is made to exist so the unguarded code would copy it
    /// to `<theme>/evil.lua`.
    #[test]
    fn save_with_an_unsafe_active_preset_name_is_ok_and_writes_nothing_outside() {
        let _env = crate::test_utils::TempEnv::new();
        let proj = TempDir::new().unwrap();
        for category in crate::theme_presets::CATEGORIES {
            fs::create_dir_all(proj.path().join("assets").join(category)).unwrap();
        }
        let mut cfg = Config::default();
        cfg.active_anim_file = "../../evil.lua".into();
        cfg.save().unwrap();
        let user_root = dirs::config_dir().unwrap().join("hve/presets");
        fs::create_dir_all(user_root.join("animations")).unwrap();
        fs::write(user_root.join("animations/../../evil.lua"), b"-- evil\n").unwrap();

        let theme = TempDir::new().unwrap();
        let provider = HvePresetsProvider::new(Engine::new(proj.path()));
        let result = provider.save(theme.path());

        assert!(
            result.is_ok(),
            "an unsafe preset name must never fail a save: {result:?}"
        );
        assert!(
            !theme.path().join("evil.lua").exists(),
            "an unsafe preset name must not be bundled outside the theme"
        );
    }

    // ── W10b: the theme's state.json is an UNTRUSTED file ──────────────

    /// A FIFO named like the theme's `state.json` must be REFUSED by `apply`,
    /// not block it forever: the open of a FIFO blocks, so the refusal has to
    /// happen before it. The call runs on a worker thread, and a timeout
    /// proves it returned with the usual apply error instead of hanging.
    #[test]
    fn apply_refuses_a_fifo_state_json_instead_of_blocking() {
        use std::os::unix::ffi::OsStrExt;
        let proj = TempDir::new().unwrap();
        let theme = TempDir::new().unwrap();
        let provider_dir = theme.path().join("providers").join("hve-presets");
        fs::create_dir_all(&provider_dir).unwrap();
        let fifo = provider_dir.join("state.json");
        let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
        assert_eq!(rc, 0, "mkfifo must succeed for the sandbox fixture");

        let provider = HvePresetsProvider::new(Engine::new(proj.path()));
        let theme_dir = theme.path().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(provider.apply(&theme_dir));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(result) => assert!(
                result.is_err(),
                "a FIFO state.json must be refused, got {result:?}"
            ),
            Err(_) => panic!("apply blocked on a FIFO state.json instead of refusing it"),
        }
    }

    /// An oversized (but valid) `state.json` is refused by W10's per-file
    /// bound instead of being slurped whole for the panel: the facts are
    /// empty, exactly like an unreadable state.
    #[test]
    fn preset_facts_refuse_an_oversized_state_json() {
        let tmp = TempDir::new().unwrap();
        let mut state =
            br#"{"active_anim_file":"custom.lua","active_border_file":"","active_shader_file":""}"#
                .to_vec();
        state.resize(crate::theme_package::MAX_THEME_FILE_BYTES as usize + 1, b' ');
        fs::write(tmp.path().join("state.json"), state).unwrap();

        assert!(
            preset_facts(tmp.path()).is_empty(),
            "an over-bound state.json must declare no facts"
        );
    }
}
