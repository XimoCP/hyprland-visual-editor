use std::fs;
use std::path::PathBuf;

use crate::border_preset::BorderParams;

/// User preset store for borders and animations.
/// Built-in presets live in `assets/{borders,animations}/`.
/// User presets live in `~/.config/hve/presets/{borders,animations}/`.
pub struct PresetStore {
    base: PathBuf,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // used by Apply feature (upcoming)
pub struct PresetMeta {
    pub name: String,
    pub tag: String,
    pub is_user: bool,
}

impl PresetStore {
    /// Create a store for a specific category ("borders" or "animations").
    pub fn new(category: &str) -> Self {
        let base = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hve")
            .join("presets")
            .join(category);
        Self { base }
    }

    /// Ensure the user presets directory exists.
    pub fn init(&self) -> Result<(), String> {
        fs::create_dir_all(&self.base)
            .map_err(|e| format!("Cannot create preset dir {}: {}", self.base.display(), e))
    }

    /// List user preset names (file stems, sorted).
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("lua") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        names
    }

    /// Load a user preset file content by name.
    #[allow(dead_code)] // used by Apply feature (upcoming)
    pub fn load(&self, name: &str) -> Result<String, String> {
        let path = self.base.join(format!("{}.lua", name));
        fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read preset {}: {}", name, e))
    }

    /// Save a user preset. Overwrites if it already exists.
    /// `content` is the full Lua file content (header + config).
    pub fn save(&self, name: &str, content: &str) -> Result<(), String> {
        self.init()?;
        let path = self.base.join(format!("{}.lua", name));
        fs::write(&path, content)
            .map_err(|e| format!("Cannot write preset {}: {}", name, e))
    }

    /// Rename a user preset. Returns error if target already exists.
    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<(), String> {
        let old_path = self.base.join(format!("{}.lua", old_name));
        let new_path = self.base.join(format!("{}.lua", new_name));

        if new_path.exists() {
            return Err(format!("Preset '{}' already exists", new_name));
        }

        if !old_path.exists() {
            return Err(format!("Preset '{}' not found", old_name));
        }

        fs::rename(&old_path, &new_path)
            .map_err(|e| format!("Cannot rename {} to {}: {}", old_name, new_name, e))
    }

    /// Delete a user preset.
    pub fn delete(&self, name: &str) -> Result<(), String> {
        let path = self.base.join(format!("{}.lua", name));
        if !path.exists() {
            return Err(format!("Preset '{}' not found", name));
        }
        fs::remove_file(&path)
            .map_err(|e| format!("Cannot delete preset {}: {}", name, e))
    }

    /// Check if a user preset exists.
    #[allow(dead_code)] // used by Apply feature (upcoming)
    pub fn exists(&self, name: &str) -> bool {
        self.base.join(format!("{}.lua", name)).exists()
    }

    /// Generate Lua content for a border preset with geometry values.
    /// NOTE: kept for the Unit 2 call-site switch (zero-warning rule);
    /// superseded by [`PresetStore::generate_border_lua_full`].
    pub fn generate_border_lua(
        name: &str,
        size: i32,
        radius: i32,
        gap_in: i32,
        gap_out: i32,
    ) -> String {
        format!(
            r#"-- @Title: {name}
-- @Source: user
-- @Tag: CUSTOM
-- @Desc: User-created border preset.
---@diagnostic disable: undefined-global

hl.config({{
    general = {{
        border_size = {size},
        rounding = {radius},
        gaps_in = {gap_in},
        gaps_out = {gap_out},
    }}
}})
"#,
            name = name,
            size = size,
            radius = radius,
            gap_in = gap_in,
            gap_out = gap_out,
        )
    }

    /// Generate Lua content for a border preset with the COMPLETE tune
    /// schema (D5): active colors + angle, inactive color, border size
    /// (omitted when `None` = keep-current), the `decoration.shadow`
    /// block iff glow is present, the fixed `windowrulev2` pair iff the
    /// rule toggle is on, file-local `hl.curve` definitions for every
    /// non-`default` bezier referenced, and the three `hl.animation`
    /// leaves. Tokens emit bare; custom colors emit `"rgba(rrggbbaa)"`.
    ///
    /// Round-trip contract: `parse(generate(params)) == params` for
    /// every params value whose animations hold the canonical triple
    /// (single-leaf file shapes normalize to the triple on save).
    pub fn generate_border_lua_full(name: &str, params: &BorderParams) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "-- @Title: {name}\n\
             -- @Source: user\n\
             -- @Tag: CUSTOM\n\
             -- @Desc: User-created border preset.\n\
             ---@diagnostic disable: undefined-global\n\
             \n\
             hl.config({{\n\
             \x20   general = {{\n\
             \x20       col = {{\n"
        ));
        let colors: Vec<String> = params.active_colors.iter().map(|c| c.emit()).collect();
        out.push_str(&format!(
            "\x20           active_border = {{\n\
             \x20               colors = {{ {} }},\n\
             \x20               angle = {}\n\
             \x20           }},\n\
             \x20           inactive_border = {}\n\
             \x20       }},\n",
            colors.join(", "),
            params.angle,
            params.inactive.emit(),
        ));
        if let Some(size) = params.border_size {
            out.push_str(&format!("\x20       border_size = {size},\n"));
        }
        out.push_str("\x20   },\n");
        if let Some(glow) = &params.glow {
            let enabled = if glow.enabled { "true" } else { "false" };
            out.push_str(&format!(
                "\x20   decoration = {{\n\
                 \x20       shadow = {{\n\
                 \x20           enabled = {enabled},\n\
                 \x20           range = {},\n\
                 \x20           render_power = {},\n\
                 \x20           color = {},\n\
                 \x20           color_inactive = {},\n\
                 \x20           offset = {{ {}, {} }}\n\
                 \x20       }}\n\
                 \x20   }},\n",
                glow.range,
                glow.render_power,
                glow.color.emit(),
                glow.color_inactive.emit(),
                glow.offset.0,
                glow.offset.1,
            ));
        }
        if params.rule_enabled {
            out.push_str(
                "\x20   windowrulev2 = {\n\
                 \x20       \"noshadow, focus:0\",\n\
                 \x20       \"dim_around, floating:1\"\n\
                 \x20   },\n",
            );
        }
        out.push_str("})\n");
        // File-local curves: ordered union of stored names and every
        // non-`default` bezier referenced by the leaves. Curve points are
        // not part of the tune state, so a neutral bezier shape is
        // emitted; the reader only recovers names (documented in D5).
        let mut curves: Vec<&str> = Vec::new();
        for name in &params.curves {
            if !curves.iter().any(|c| *c == name) {
                curves.push(name.as_str());
            }
        }
        for leaf in &params.animations {
            if let Some(bezier) = &leaf.bezier {
                if bezier != "default" && !curves.iter().any(|c| *c == bezier) {
                    curves.push(bezier.as_str());
                }
            }
        }
        for name in curves {
            out.push_str(&format!(
                "\nhl.curve(\"{name}\", {{ type = \"bezier\", points = {{ {{ 0.4, 0 }}, {{ 0.2, 1 }} }} }})\n"
            ));
        }
        // The three leaves in canonical order; absent leaves default to
        // disabled (D3).
        for leaf_name in ["borderangle", "border", "fadeShadow"] {
            out.push_str(&Self::emit_leaf(params, leaf_name));
        }
        out
    }

    /// Emit one `hl.animation({...})` leaf line (trailing newline
    /// included). Fields that are `None` are omitted so they parse back
    /// to `None`.
    fn emit_leaf(params: &BorderParams, leaf_name: &str) -> String {
        let (enabled, speed, bezier, style) = match params.find_leaf(leaf_name) {
            Some(leaf) => (leaf.enabled, leaf.speed, leaf.bezier.as_deref(), leaf.style.as_deref()),
            None => (false, None, None, None),
        };
        let mut line = format!(
            "\nhl.animation({{ leaf = \"{leaf_name}\", enabled = {}",
            if enabled { "true" } else { "false" }
        );
        if let Some(speed) = speed {
            line.push_str(&format!(", speed = {speed}"));
        }
        if let Some(bezier) = bezier {
            line.push_str(&format!(", bezier = \"{bezier}\""));
        }
        if let Some(style) = style {
            line.push_str(&format!(", style = \"{style}\""));
        }
        line.push_str(" })\n");
        line
    }
    pub fn generate_animation_lua(
        name: &str,
        bezier_a: f64,
        bezier_b: f64,
        bezier_c: f64,
        bezier_d: f64,
    ) -> String {
        format!(
            r#"-- @Title: {name}
-- @Source: user
-- @Tag: CUSTOM
-- @Desc: User-created animation preset.
---@diagnostic disable: undefined-global

hl.config({{
    animations = {{
        bezier = ({{ {a}, {b}, {c}, {d} }}),
        speed = 0,
    }}
}})
"#,
            name = name,
            a = bezier_a,
            b = bezier_b,
            c = bezier_c,
            d = bezier_d,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = PathBuf::from(format!(
            "/tmp/preset_store_test_{}_{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_and_list_presets() {
        let dir = temp_dir();
        let store = PresetStore {
            base: dir.join("borders"),
        };

        store.save("my_border", "-- test").unwrap();
        let names = store.list();
        assert!(names.contains(&"my_border".to_string()));
        assert_eq!(names.len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_preset() {
        let dir = temp_dir();
        let store = PresetStore {
            base: dir.join("borders"),
        };

        store.save("old_name", "-- test").unwrap();
        store.rename("old_name", "new_name").unwrap();
        assert!(!store.exists("old_name"));
        assert!(store.exists("new_name"));
        assert_eq!(store.load("new_name").unwrap(), "-- test");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn delete_preset() {
        let dir = temp_dir();
        let store = PresetStore {
            base: dir.join("borders"),
        };

        store.save("to_delete", "-- test").unwrap();
        assert!(store.exists("to_delete"));
        store.delete("to_delete").unwrap();
        assert!(!store.exists("to_delete"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn generate_border_lua_content() {
        let content = PresetStore::generate_border_lua("Test", 2, 32, 5, 5);
        assert!(content.contains("-- @Title: Test"));
        assert!(content.contains("-- @Source: user"));
        assert!(content.contains("border_size = 2"));
        assert!(content.contains("rounding = 32"));
    }

    #[test]
    fn generate_animation_lua_content() {
        let content = PresetStore::generate_animation_lua("Bounce", 0.25, 0.1, 0.25, 1.0);
        assert!(content.contains("-- @Title: Bounce"));
        assert!(content.contains("-- @Source: user"));
        assert!(content.contains("0.25, 0.1, 0.25, 1"));
    }

    #[test]
    fn generate_full_token_preset_emits_schema_and_round_trips() {
        use crate::border_preset::{AnimLeaf, BorderColor, BorderParams, PaletteToken};
        let params = BorderParams {
            active_colors: vec![
                BorderColor::Token(PaletteToken::Primary),
                BorderColor::Token(PaletteToken::Surface),
            ],
            angle: 90,
            inactive: BorderColor::Token(PaletteToken::SurfaceLowest),
            border_size: Some(1),
            glow: None,
            rule_enabled: false,
            animations: vec![
                AnimLeaf {
                    leaf: "borderangle".to_string(),
                    enabled: false,
                    speed: None,
                    bezier: None,
                    style: None,
                },
                AnimLeaf {
                    leaf: "border".to_string(),
                    enabled: false,
                    speed: None,
                    bezier: None,
                    style: None,
                },
                AnimLeaf {
                    leaf: "fadeShadow".to_string(),
                    enabled: false,
                    speed: None,
                    bezier: None,
                    style: None,
                },
            ],
            curves: vec![],
        };
        let content = PresetStore::generate_border_lua_full("Duo", &params);
        assert!(content.contains("-- @Title: Duo"));
        assert!(content.contains("colors = { primary, surface }"));
        assert!(content.contains("angle = 90"));
        assert!(content.contains("inactive_border = surface_lowest"));
        assert!(content.contains("border_size = 1"));
        // Toggles off omit their blocks.
        assert!(!content.contains("shadow"));
        assert!(!content.contains("windowrulev2"));
        assert_eq!(crate::border_preset::parse(&content), params);
    }

    #[test]
    fn generate_full_glow_preset_emits_blocks_and_round_trips() {
        use crate::border_preset::{AnimLeaf, BorderColor, BorderParams};
        let custom = |r, g, b, a| BorderColor::Custom { r, g, b, a };
        let params = BorderParams {
            active_colors: vec![custom(0x00, 0xff, 0xf7, 0xff), custom(0x1a, 0x00, 0x26, 0xff)],
            angle: 45,
            inactive: custom(0x0f, 0x0f, 0x26, 0x55),
            border_size: Some(2),
            glow: Some(crate::border_preset::GlowParams {
                enabled: true,
                range: 20,
                render_power: 4,
                color: custom(0xff, 0xff, 0xff, 0x44),
                color_inactive: custom(0xff, 0xff, 0xff, 0x00),
                offset: (0, 0),
            }),
            rule_enabled: true,
            animations: vec![
                AnimLeaf {
                    leaf: "borderangle".to_string(),
                    enabled: true,
                    speed: Some(30),
                    bezier: Some("nv_neon_flow".to_string()),
                    style: Some("loop".to_string()),
                },
                AnimLeaf {
                    leaf: "border".to_string(),
                    enabled: true,
                    speed: Some(30),
                    bezier: Some("default".to_string()),
                    style: None,
                },
                AnimLeaf {
                    leaf: "fadeShadow".to_string(),
                    enabled: true,
                    speed: Some(15),
                    bezier: Some("default".to_string()),
                    style: None,
                },
            ],
            curves: vec!["nv_neon_flow".to_string()],
        };
        let content = PresetStore::generate_border_lua_full("Glow", &params);
        assert!(content.contains("decoration"));
        assert!(content.contains("range = 20"));
        assert!(content.contains("render_power = 4"));
        assert!(content.contains("\"rgba(ffffff44)\""));
        assert!(content.contains("windowrulev2"));
        assert!(content.contains("noshadow, focus:0"));
        assert!(content.contains("hl.curve(\"nv_neon_flow\""));
        assert!(content.contains("leaf = \"borderangle\""));
        assert!(content.contains("leaf = \"border\""));
        assert!(content.contains("leaf = \"fadeShadow\""));
        assert_eq!(crate::border_preset::parse(&content), params);
    }
}
