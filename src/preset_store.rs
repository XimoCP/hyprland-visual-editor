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

    /// Read a border preset Lua file from the built-in or user preset directory.
    /// `project_dir` is the resolved project root (see `project_dir()` in main.rs).
    /// Returns the file content if found, or an error message.
    pub fn read_border_file(name: &str, project_dir: &std::path::Path) -> Result<String, String> {
        // Built-in directory (assets/borders/)
        let builtin = project_dir
            .join("assets")
            .join("borders")
            .join(format!("{}.lua", name));
        if builtin.exists() {
            return fs::read_to_string(&builtin)
                .map_err(|e| format!("Cannot read {}: {}", builtin.display(), e));
        }
        // User preset directory
        let user = Self::new("borders").base.join(format!("{}.lua", name));
        if user.exists() {
            return fs::read_to_string(&user)
                .map_err(|e| format!("Cannot read {}: {}", user.display(), e));
        }
        Err(format!("Preset '{}' not found in built-in or user dirs", name))
    }

    /// Write a draft border Lua file to the fragments directory.
    /// This is the D4 live-draft path: it bypasses `apply_border`
    /// (which resolves names inside scanned dirs) and writes directly
    /// to the fragment that `border.sh` would have produced.
    /// Returns the path to the written fragment.
    pub fn write_draft(content: &str, proj: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let fragment_dir = proj.join("assets").join("fragments");
        fs::create_dir_all(&fragment_dir)
            .map_err(|e| format!("Cannot create fragments dir: {}", e))?;
        let fragment = fragment_dir.join("border.lua");
        fs::write(&fragment, content)
            .map_err(|e| format!("Cannot write draft fragment: {}", e))?;
        Ok(fragment)
    }

    /// Remove the draft border fragment.
    pub fn remove_draft(proj: &std::path::Path) {
        let fragment = proj.join("assets").join("fragments").join("border.lua");
        let _ = fs::remove_file(fragment);
    }

    /// Check whether a draft border fragment exists on disk.
    #[cfg(test)]
    pub fn has_draft(proj: &std::path::Path) -> bool {
        proj.join("assets").join("fragments").join("border.lua").exists()
    }

    /// Encode BorderParams color slots into a Slint-compat string for the
    /// Encode BorderParams color slots → Slint `tune-active-colors` string.
    /// Format: comma-separated `type:value` where type is `p` (palette) or `c` (custom RGBA hex).
    pub fn encode_colors(colors: &[crate::border_preset::BorderColor]) -> String {
        use crate::border_preset::BorderColor;
        let parts: Vec<String> = colors.iter().map(|c| match c {
            BorderColor::Token(t) => format!("p:{}", t.as_str()),
            BorderColor::Custom { r, g, b, a } => format!("c:{:02x}{:02x}{:02x}{:02x}", r, g, b, a),
        }).collect();
        parts.join(",")
    }

    /// Decode a Slint `tune-active-colors` string → Vec<BorderColor>.
    pub fn decode_colors(s: &str) -> Vec<crate::border_preset::BorderColor> {
        use crate::border_preset::{BorderColor, PaletteToken};
        if s.is_empty() { return Vec::new(); }
        s.split(',').filter(|p| !p.is_empty()).map(|p| {
            if let Some(rest) = p.strip_prefix("p:") {
                let tok = match rest.trim() {
                    "primary" => PaletteToken::Primary,
                    "secondary" => PaletteToken::Secondary,
                    "tertiary" => PaletteToken::Tertiary,
                    "error" => PaletteToken::Error,
                    "surface" => PaletteToken::Surface,
                    "surface_lowest" => PaletteToken::SurfaceLowest,
                    _ => PaletteToken::Primary,
                };
                BorderColor::Token(tok)
            } else if let Some(raw) = p.strip_prefix("c:") {
                // Two encoders feed this field: `encode_colors` writes a bare
                // "rrggbb[aa]", and the Slint colour picker writes "#rrggbbaa"
                // (its `hex8` output). Accept both.
                //
                // Slicing `hex[i..i+2]` over the 9-byte "#rrggbbaa" form runs
                // one byte past the end and panicked the whole process the
                // first time a custom colour was committed from the running
                // GUI. `chunks(2)` cannot overrun, and anything that is not a
                // clean 6/8-digit payload degrades to the palette default
                // rather than taking the app down with it.
                let hex = raw.trim().trim_start_matches('#');
                let bytes: Vec<u8> = if hex.len() == 6 || hex.len() == 8 {
                    hex.as_bytes()
                        .chunks(2)
                        .filter_map(|c| std::str::from_utf8(c).ok())
                        .filter_map(|c| u8::from_str_radix(c, 16).ok())
                        .collect()
                } else {
                    Vec::new()
                };
                match bytes.as_slice() {
                    [r, g, b, a] => BorderColor::Custom { r: *r, g: *g, b: *b, a: *a },
                    [r, g, b] => BorderColor::Custom { r: *r, g: *g, b: *b, a: 0xff },
                    _ => BorderColor::Token(PaletteToken::Primary),
                }
            } else {
                BorderColor::Token(PaletteToken::Primary)
            }
        }).collect()
    }

    /// Encode inactive border color → Slint string.
    pub fn encode_inactive(c: &crate::border_preset::BorderColor) -> String {
        Self::encode_colors(&[c.clone()])
    }

    /// Encode glow state → pipe-separated `key=value` pairs for Slint.
    pub fn encode_glow(g: Option<&crate::border_preset::GlowParams>) -> String {
        let Some(glow) = g else { return String::new(); };
        format!("enabled:{}|range:{}|power:{}|color:{}|inactive:{}|ox:{}|oy:{}",
            glow.enabled, glow.range, glow.render_power,
            Self::encode_colors(&[glow.color.clone()]),
            Self::encode_colors(&[glow.color_inactive.clone()]),
            glow.offset.0, glow.offset.1)
    }

    /// Decode glow string → Option<GlowParams>.
    pub fn decode_glow(s: &str) -> Option<crate::border_preset::GlowParams> {
        if s.is_empty() { return None; }
        let (mut enabled, mut range, mut power) = (false, 20, 4);
        let (mut ox, mut oy) = (0i32, 0i32);
        let (mut color, mut inactive) = (
            crate::border_preset::BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0x44 },
            crate::border_preset::BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0x00 },
        );
        for part in s.split('|') {
            if let Some(v) = part.strip_prefix("enabled:") { enabled = v == "true"; }
            else if let Some(v) = part.strip_prefix("range:") { range = v.parse().unwrap_or(20); }
            else if let Some(v) = part.strip_prefix("power:") { power = v.parse().unwrap_or(4); }
            else if let Some(v) = part.strip_prefix("color:") { if let Some(c) = Self::decode_colors(v).into_iter().next() { color = c; } }
            else if let Some(v) = part.strip_prefix("inactive:") { if let Some(c) = Self::decode_colors(v).into_iter().next() { inactive = c; } }
            else if let Some(v) = part.strip_prefix("ox:") { ox = v.parse().unwrap_or(0); }
            else if let Some(v) = part.strip_prefix("oy:") { oy = v.parse().unwrap_or(0); }
        }
        Some(crate::border_preset::GlowParams { enabled, range, render_power: power, color, color_inactive: inactive, offset: (ox, oy) })
    }

    /// Encode animations → pipe-separated leaf entries for Slint.
    pub fn encode_animations(a: &[crate::border_preset::AnimLeaf]) -> String {
        a.iter().map(|l| {
            format!("{}|{}|{}|{}|{}", l.leaf, l.enabled,
                l.speed.map(|s| s.to_string()).unwrap_or_default(),
                l.bezier.as_deref().unwrap_or(""), l.style.as_deref().unwrap_or(""))
        }).collect::<Vec<_>>().join(",")
    }

    /// Decode animations string → Vec<AnimLeaf>.
    pub fn decode_animations(s: &str) -> Vec<crate::border_preset::AnimLeaf> {
        if s.is_empty() { return Vec::new(); }
        s.split(',').filter(|p| !p.is_empty()).map(|p| {
            let parts: Vec<&str> = p.split('|').collect();
            crate::border_preset::AnimLeaf {
                leaf: parts.first().unwrap_or(&"").to_string(),
                enabled: parts.get(1).map(|v| *v == "true").unwrap_or(false),
                speed: parts.get(2).and_then(|v| v.parse().ok()),
                bezier: parts.get(3).filter(|v| !v.is_empty()).map(|v| v.to_string()),
                style: parts.get(4).filter(|v| !v.is_empty()).map(|v| v.to_string()),
            }
        }).collect()
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
        assert!(content.contains("-- @Source: user"));
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
    fn read_border_file_finds_builtin_preset() {
        let proj = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let content = PresetStore::read_border_file("01_cascade", &proj).unwrap();
        assert!(content.contains("colors"), "should contain colors block");
        assert!(content.contains("angle"), "should contain angle");
    }

    #[test]
    fn read_border_file_missing_returns_err() {
        let proj = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let err = PresetStore::read_border_file("nonexistent_preset_xyz", &proj);
        assert!(err.is_err(), "missing preset should return error");
        assert!(err.unwrap_err().contains("not found"));
    }

    #[test]
    fn read_border_file_explicit_dir_no_env_dependency() {
        // Verify that read_border_file works with an explicitly passed dir,
        // with NO env!("CARGO_MANIFEST_DIR") dependency at call time.
        let proj = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // Point to a different dir (tmp) to prove the param is used, not env!
        let fake = temp_dir();
        let err = PresetStore::read_border_file("01_cascade", &fake);
        assert!(err.is_err(), "wrong dir must fail");
        // Correct dir succeeds
        let ok = PresetStore::read_border_file("01_cascade", &proj);
        assert!(ok.is_ok(), "correct dir must succeed");
        fs::remove_dir_all(&fake).unwrap();
    }

    #[test]
    fn encode_decode_colors_round_trip() {
        use crate::border_preset::{BorderColor, PaletteToken};
        let colors = vec![
            BorderColor::Token(PaletteToken::Primary),
            BorderColor::Custom { r: 0xff, g: 0x00, b: 0x88, a: 0xcc },
            BorderColor::Token(PaletteToken::SurfaceLowest),
        ];
        let encoded = PresetStore::encode_colors(&colors);
        assert_eq!(encoded, "p:primary,c:ff0088cc,p:surface_lowest");
        let decoded = PresetStore::decode_colors(&encoded);
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0], BorderColor::Token(PaletteToken::Primary));
        assert_eq!(decoded[1], BorderColor::Custom { r: 0xff, g: 0x00, b: 0x88, a: 0xcc });
        assert_eq!(decoded[2], BorderColor::Token(PaletteToken::SurfaceLowest));
        // Empty round-trip
        assert_eq!(PresetStore::encode_colors(&[]), "");
        assert!(PresetStore::decode_colors("").is_empty());
    }

    /// The Slint colour picker emits `#rrggbbaa` (its `hex8` output) while
    /// `encode_colors` emits a bare `rrggbbaa`; BOTH must decode. And a
    /// mis-sized payload must never panic: slicing `hex[i..i+2]` over a 9-byte
    /// "#rrggbbaa" ran one byte past the end and killed the whole app the first
    /// time a custom colour was committed from the running GUI. The render test
    /// never caught it because its fixture fed the bare form.
    #[test]
    fn decode_colors_accepts_picker_hash_form_and_never_panics() {
        use crate::border_preset::{BorderColor, PaletteToken};
        assert_eq!(
            PresetStore::decode_colors("c:#371e1bff"),
            vec![BorderColor::Custom { r: 0x37, g: 0x1e, b: 0x1b, a: 0xff }]
        );
        assert_eq!(
            PresetStore::decode_colors("c:ff0088cc"),
            vec![BorderColor::Custom { r: 0xff, g: 0x00, b: 0x88, a: 0xcc }]
        );
        // 6-digit payloads are opaque.
        assert_eq!(
            PresetStore::decode_colors("c:#371e1b"),
            vec![BorderColor::Custom { r: 0x37, g: 0x1e, b: 0x1b, a: 0xff }]
        );
        // Short / empty / odd payloads degrade to the palette default instead
        // of taking the process down with them.
        for bad in ["c:#37", "c:", "c:#", "c:#371e1bf"] {
            let out = PresetStore::decode_colors(bad);
            assert_eq!(out.len(), 1, "`{bad}` must still yield one colour");
            assert!(
                matches!(out[0], BorderColor::Token(PaletteToken::Primary)),
                "`{bad}` must fall back to the palette default, got {:?}",
                out[0]
            );
        }
    }

    #[test]
    fn encode_decode_glow_round_trip() {
        use crate::border_preset::{BorderColor, GlowParams};
        let glow = GlowParams {
            enabled: true, range: 20, render_power: 4,
            color: BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0x88 },
            color_inactive: BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0x00 },
            offset: (0, 0),
        };
        let encoded = PresetStore::encode_glow(Some(&glow));
        assert!(encoded.contains("enabled:true"));
        assert!(encoded.contains("range:20"));
        let decoded = PresetStore::decode_glow(&encoded).unwrap();
        assert_eq!(decoded.enabled, true);
        assert_eq!(decoded.range, 20);
        assert_eq!(decoded.render_power, 4);
        assert_eq!(decoded.color, BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0x88 });
        // None round-trip
        assert_eq!(PresetStore::encode_glow(None), "");
        assert!(PresetStore::decode_glow("").is_none());
    }

    #[test]
    fn encode_decode_animations_round_trip() {
        use crate::border_preset::AnimLeaf;
        let anims = vec![
            AnimLeaf { leaf: "borderangle".into(), enabled: true, speed: Some(30), bezier: Some("my_curve".into()), style: Some("loop".into()) },
            AnimLeaf { leaf: "border".into(), enabled: false, speed: None, bezier: None, style: None },
        ];
        let encoded = PresetStore::encode_animations(&anims);
        assert_eq!(encoded, "borderangle|true|30|my_curve|loop,border|false|||");
        let decoded = PresetStore::decode_animations(&encoded);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].leaf, "borderangle");
        assert_eq!(decoded[0].enabled, true);
        assert_eq!(decoded[0].speed, Some(30));
        assert_eq!(decoded[0].bezier, Some("my_curve".into()));
        assert_eq!(decoded[0].style, Some("loop".into()));
        assert_eq!(decoded[1].leaf, "border");
        assert_eq!(decoded[1].enabled, false);
        assert!(decoded[1].speed.is_none());
    }

    #[test]
    fn build_border_params_loads_all_tune_properties() {
        let proj = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let content = PresetStore::read_border_file("07_infinity", &proj).unwrap();
        let params = crate::border_preset::parse(&content);
        // Task 2.1: 8 slots + angle 45 + rule on
        assert_eq!(params.active_colors.len(), 8, "infinity has 8 color slots");
        assert_eq!(params.angle, 45, "infinity angle is 45");
        assert!(params.rule_enabled, "infinity rule is on");
    }

    #[test]
    fn build_border_params_joker_missing_size_keeps_current() {
        let proj = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let content = PresetStore::read_border_file("13_the_joker", &proj).unwrap();
        let params = crate::border_preset::parse(&content);
        // D3: missing border_size keeps current value — the parse returns None
        assert_eq!(params.border_size, None, "joker has no border_size key");
    }

    #[test]
    fn save_round_trip_preserves_tune_state() {
        use crate::border_preset::{AnimLeaf, BorderColor, BorderParams, GlowParams};
        let params = BorderParams {
            active_colors: vec![
                BorderColor::Custom { r: 0xff, g: 0x00, b: 0x00, a: 0xff },
                BorderColor::Token(crate::border_preset::PaletteToken::Primary),
                BorderColor::Custom { r: 0x00, g: 0xff, b: 0x00, a: 0x88 },
            ],
            angle: 45,
            inactive: BorderColor::Token(crate::border_preset::PaletteToken::SurfaceLowest),
            border_size: Some(2),
            glow: Some(GlowParams {
                enabled: true,
                range: 20,
                render_power: 4,
                color: BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0x44 },
                color_inactive: BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0x00 },
                offset: (0, 0),
            }),
            rule_enabled: true,
            animations: vec![
                AnimLeaf { leaf: "borderangle".into(), enabled: true, speed: Some(30), bezier: Some("my_curve".into()), style: Some("loop".into()) },
                AnimLeaf { leaf: "border".into(), enabled: false, speed: None, bezier: None, style: None },
                AnimLeaf { leaf: "fadeShadow".into(), enabled: false, speed: None, bezier: None, style: None },
            ],
            curves: vec!["my_curve".to_string()],
        };
        let dir = temp_dir();
        let store = PresetStore::new("borders");
        // Override the base for testing (the field is private but we access via the constructor)
        // Instead, use a trick: save via the generated content and read back
        let content = PresetStore::generate_border_lua_full("roundtrip", &params);
        // Write to a temp file directly
        let preset_dir = dir.join("borders");
        fs::create_dir_all(&preset_dir).unwrap();
        fs::write(preset_dir.join("roundtrip.lua"), &content).unwrap();
        let loaded = fs::read_to_string(preset_dir.join("roundtrip.lua")).unwrap();
        let reparsed = crate::border_preset::parse(&loaded);
        assert_eq!(reparsed, params, "round-trip: parse(generate(params)) == params");
        fs::remove_dir_all(&dir).unwrap();
        // Suppress unused variable warning
        let _ = store;
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

    #[test]
    fn draft_lifecycle_write_list_unchanged_cleanup() {
        use crate::border_preset::{BorderParams, BorderColor, PaletteToken};
        let dir = temp_dir();
        // Simulate a project dir with fragments/ and assets/borders/
        let proj = dir.join("project");
        let frag_dir = proj.join("assets").join("fragments");
        fs::create_dir_all(&frag_dir).unwrap();
        // Write a draft
        let params = BorderParams {
            active_colors: vec![BorderColor::Token(PaletteToken::Primary)],
            angle: 45,
            inactive: BorderColor::Token(PaletteToken::SurfaceLowest),
            border_size: Some(1),
            glow: None,
            rule_enabled: false,
            animations: vec![],
            curves: vec![],
        };
        let content = PresetStore::generate_border_lua_full("draft", &params);
        let fragment = PresetStore::write_draft(&content, &proj).unwrap();
        assert!(fragment.exists(), "draft fragment must exist after write");
        assert!(PresetStore::has_draft(&proj));
        // User preset list must NOT contain the draft
        let store = PresetStore { base: dir.join("user_presets") };
        fs::create_dir_all(&store.base).unwrap();
        store.save("real_preset", "-- test").unwrap();
        let names = store.list();
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"real_preset".to_string()));
        // Cleanup removes the draft
        PresetStore::remove_draft(&proj);
        assert!(!fragment.exists(), "fragment must be gone after cleanup");
        assert!(!PresetStore::has_draft(&proj));
        // Cleanup is idempotent
        PresetStore::remove_draft(&proj);
        assert!(!PresetStore::has_draft(&proj));
        fs::remove_dir_all(&dir).unwrap();
    }
}
