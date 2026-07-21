use crate::theme_manager::ThemeProvider;
use std::fs;
use std::path::{Path, PathBuf};

/// Noctalia config directory: ~/.config/noctalia/
fn noctalia_config_dir() -> Option<PathBuf> {
    dirs::config_dir()
        .or_else(|| {
            let home = std::env::var("HOME").ok()?;
            Some(PathBuf::from(home).join(".config"))
        })
        .map(|d| d.join("noctalia"))
}

/// Files we snapshot for Noctalia.
const NOCTALIA_FILES: &[&str] = &["settings.json", "colors.json", "plugins.json"];

pub struct NoctaliaProvider;

impl ThemeProvider for NoctaliaProvider {
    fn id(&self) -> &str {
        "noctalia"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.noctalia"
    }

    fn icon(&self) -> &str {
        "◈"
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let src = noctalia_config_dir().ok_or("Noctalia config dir not found")?;
        if !src.exists() {
            return Err("Noctalia config not found".into());
        }

        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        for file in NOCTALIA_FILES {
            let src_path = src.join(file);
            if src_path.exists() {
                let dst_path = provider_dir.join(file);
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
            }
        }

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let dst = noctalia_config_dir().ok_or("Noctalia config dir not found")?;
        let provider_dir = theme_dir.join("providers").join(self.id());

        if !provider_dir.exists() {
            return Err("Noctalia provider data not found in theme".into());
        }

        for file in NOCTALIA_FILES {
            let src_path = provider_dir.join(file);
            if src_path.exists() {
                let dst_path = dst.join(file);
                // Atomic write: copy to .tmp then rename
                let tmp = dst_path.with_extension("json.noctalia-tmp");
                fs::copy(&src_path, &tmp)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
                fs::rename(&tmp, &dst_path)
                    .map_err(|e| format!("Cannot rename {}: {}", file, e))?;
            }
        }

        Ok(())
    }
}
