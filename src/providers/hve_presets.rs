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
    let Ok(raw) = fs::read_to_string(provider_dir.join("state.json")) else {
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

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let state_path = theme_dir.join("providers").join(self.id()).join("state.json");
        let raw = fs::read_to_string(&state_path)
            .map_err(|e| format!("Cannot read state.json: {}", e))?;
        let state: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| format!("Parse error: {}", e))?;

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
}
