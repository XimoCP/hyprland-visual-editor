use crate::config::Config;
use crate::engine::Engine;
use crate::theme_manager::ThemeProvider;
use std::fs;
use std::path::Path;

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

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let cfg = Config::load();
        let state = serde_json::json!({
            "active_anim_file": cfg.active_anim_file,
            "active_border_file": cfg.active_border_file,
            "active_shader_file": cfg.active_shader_file,
            "border_size": cfg.border_size,
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
                let _ = self.engine.apply_geometry(size);
                cfg.border_size = size;
                changed = true;
            }
        }

        if changed {
            let _ = cfg.save();
        }

        Ok(())
    }
}
