use std::fs;
use std::path::PathBuf;

/// User preset store for borders and animations.
/// Built-in presets live in `assets/{borders,animations}/`.
/// User presets live in `~/.config/hve/presets/{borders,animations}/`.
pub struct PresetStore {
    base: PathBuf,
}

#[derive(Debug, Clone)]
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
    pub fn exists(&self, name: &str) -> bool {
        self.base.join(format!("{}.lua", name)).exists()
    }

    /// Generate Lua content for a border preset with geometry values.
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

    /// Generate Lua content for an animation preset with bezier values.
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
}
