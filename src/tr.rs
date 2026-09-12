// ── Translation module ───────────────────────────────────────────────
// Embeds i18n/*.json at compile time, detects system locale at runtime,
// resolves dot-notation keys with fallback chain: selected → en → raw key.

use serde_json::Value;

static EN_JSON: &str = include_str!("../i18n/en.json");
static ES_JSON: &str = include_str!("../i18n/es.json");

/// Holds the loaded translation map and the active language code.
pub struct Tr {
    data: Value,
    pub lang: String,
}

impl Tr {
    #[allow(dead_code)]
    /// Detect system language and load the matching translation.
    /// Falls back to English if the detected language isn't available.
    pub fn new() -> Self {
        Self::with_lang(&detect_language())
    }

    /// Load translations for a specific language code (used by tests).
    pub fn with_lang(lang: &str) -> Self {
        let data = load_embedded(lang).unwrap_or_else(|| {
            // Last resort: English is always embedded
            load_embedded("en").expect("en.json embedded")
        });
        Self {
            data,
            lang: lang.to_string(),
        }
    }

    /// Resolve a dot‑notation key against the translation map.
    ///
    /// # Examples
    /// ```ignore
    /// let t = Tr::new();
    /// assert_eq!(t.tr("panel.tabs.home"), Some("Home"));
    /// assert_eq!(t.tr("nonexistent.key"), None);
    /// ```
    pub fn tr(&self, key: &str) -> Option<&str> {
        let parts: Vec<&str> = key.split('.').collect();
        let mut current = &self.data;
        for part in &parts {
            current = current.get(*part)?;
        }
        current.as_str()
    }

    /// Resolve a key, falling back to the given string if not found.
    pub fn tr_or<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.tr(key).unwrap_or(fallback)
    }

    /// Convenience: resolve a key and return as `SharedString` for Slint bindings.
    pub fn tr_shared(&self, key: &str, fallback: &str) -> slint::SharedString {
        slint::SharedString::from(self.tr_or(key, fallback))
    }

    /// Resolve a key that holds a JSON array of strings and return it as `Vec<SharedString>`.
    /// Returns an empty vector when the key is missing or is not an array of strings.
    pub fn tr_array(&self, key: &str) -> Vec<slint::SharedString> {
        let parts: Vec<&str> = key.split('.').collect();
        let mut current = &self.data;
        for part in &parts {
            match current.get(*part) {
                Some(v) => current = v,
                None => return Vec::new(),
            }
        }
        current
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(slint::SharedString::from)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Detect the user's language from the `LANG` environment variable.
///
/// Parses formats like `es_AR.UTF-8`, `en_US`, `es` and normalises to
/// a two‑letter ISO 639‑1 code. Currently supports `es` and `en`.
pub fn detect_language() -> String {
    let lang = std::env::var("LANG").unwrap_or_default();
    let code = lang
        .split('.')
        .next()
        .unwrap_or("en")
        .split('_')
        .next()
        .unwrap_or("en")
        .to_lowercase();

    match code.as_str() {
        "es" => "es".to_string(),
        _ => "en".to_string(),
    }
}

/// Load an embedded translation JSON by language code.
fn load_embedded(lang: &str) -> Option<Value> {
    let raw = match lang {
        "es" => ES_JSON,
        _ => EN_JSON,
    };
    serde_json::from_str(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// RAII guard: sets `LANG` for the duration of the test and restores the
    /// previous value on drop so no mutation leaks into the process.
    struct LangGuard {
        old: Option<String>,
    }

    impl LangGuard {
        fn set(lang: Option<&str>) -> Self {
            let old = std::env::var("LANG").ok();
            match lang {
                Some(l) => std::env::set_var("LANG", l),
                None => std::env::remove_var("LANG"),
            }
            Self { old }
        }
    }

    impl Drop for LangGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(l) => std::env::set_var("LANG", l),
                None => std::env::remove_var("LANG"),
            }
        }
    }

    #[test]
    fn test_tr_resolves_top_level() {
        let t = Tr::with_lang("en");
        assert_eq!(t.tr("meta.lang"), Some("en"));
    }

    #[test]
    fn test_tr_resolves_nested() {
        let t = Tr::with_lang("en");
        assert_eq!(t.tr("panel.tabs.home"), Some("Home"));
        assert_eq!(t.tr("panel.tabs.animations"), Some("Animations"));
        assert_eq!(t.tr("panel.tabs.borders"), Some("Borders"));
        assert_eq!(t.tr("panel.tabs.effects"), Some("Effects"));
    }

    #[test]
    fn test_tr_missing_key_returns_none() {
        let t = Tr::new();
        assert_eq!(t.tr("this.does.not.exist"), None);
    }

    #[test]
    fn test_tr_or_uses_fallback() {
        let t = Tr::new();
        assert_eq!(t.tr_or("missing.key", "fallback"), "fallback");
    }

    #[test]
    fn test_tr_or_returns_translation() {
        let t = Tr::with_lang("en");
        assert_eq!(t.tr_or("panel.tabs.home", "fallback"), "Home");
    }

    #[test]
    #[serial]
    fn test_detect_language_parses_full_locale() {
        let _guard = LangGuard::set(Some("es_AR.UTF-8"));
        assert_eq!(detect_language(), "es");
    }

    #[test]
    #[serial]
    fn test_detect_language_parses_short() {
        let _guard = LangGuard::set(Some("en_US"));
        assert_eq!(detect_language(), "en");
    }

    #[test]
    #[serial]
    fn test_detect_language_defaults_to_en() {
        let _guard = LangGuard::set(None);
        assert_eq!(detect_language(), "en");
    }

    #[test]
    fn test_load_embedded_spanish() {
        let v = load_embedded("es").unwrap();
        assert_eq!(v["meta"]["lang"].as_str(), Some("es"));
        assert_eq!(v["panel"]["tabs"]["home"].as_str(), Some("Inicio"));
    }

    #[test]
    fn test_load_embedded_english() {
        let v = load_embedded("en").unwrap();
        assert_eq!(v["meta"]["lang"].as_str(), Some("en"));
        assert_eq!(v["panel"]["tabs"]["home"].as_str(), Some("Home"));
    }

    #[test]
    #[serial]
    fn test_tr_es_loads_spanish() {
        let _guard = LangGuard::set(Some("es_ES.UTF-8"));
        let t = Tr::new();
        assert_eq!(t.lang, "es");
        assert_eq!(t.tr("panel.tabs.home"), Some("Inicio"));
    }

    // ── Shell i18n keys (delivery 4, task 4.4 — nav-shell spec R5) ────

    #[test]
    fn test_shell_keys_resolve_in_english() {
        let t = Tr::with_lang("en");
        assert_eq!(t.tr("shell.brand"), Some("HVE"));
        assert_eq!(t.tr("shell.subtitle"), Some("Hyprland Visual Editor"));
        assert_eq!(t.tr("shell.status_ready"), Some("Ready"));
        assert_eq!(t.tr("shell.back_hint"), Some("Esc hides"));
        assert_eq!(t.tr("shell.slots.home"), Some("Home"));
        assert_eq!(t.tr("shell.slots.gallery"), Some("Gallery"));
        assert_eq!(t.tr("shell.slots.workshop"), Some("Workshop"));
        assert_eq!(t.tr("shell.home.hint"), Some("Theme cards land in module 2"));
    }

    #[test]
    fn test_shell_keys_resolve_in_spanish() {
        let t = Tr::with_lang("es");
        assert_eq!(t.tr("shell.brand"), Some("HVE"));
        assert_eq!(t.tr("shell.subtitle"), Some("Editor Visual de Hyprland"));
        assert_eq!(t.tr("shell.status_ready"), Some("Listo"));
        assert_eq!(t.tr("shell.back_hint"), Some("Esc oculta"));
        assert_eq!(t.tr("shell.slots.home"), Some("Inicio"));
        assert_eq!(t.tr("shell.slots.gallery"), Some("Galería"));
        assert_eq!(t.tr("shell.slots.workshop"), Some("Taller"));
        assert_eq!(t.tr("shell.home.hint"), Some("Las tarjetas de temas llegan en el módulo 2"));
    }

    #[test]
    fn test_shell_unknown_locale_falls_back_to_english() {
        // tr.rs only knows "es" and "en"; any other code falls back to the
        // embedded English map (nav-shell spec R5 "Unknown locale fallback").
        let t = Tr::with_lang("fr");
        assert_eq!(t.tr("shell.brand"), Some("HVE"));
        assert_eq!(t.tr("shell.back_hint"), Some("Esc hides"));
    }

    // ── Migration guard status strings (drop-conf-support R8) ─────────

    #[test]
    fn test_shell_guard_keys_resolve_in_english() {
        let t = Tr::with_lang("en");
        assert_eq!(
            t.tr("shell.lua_migration"),
            Some("HVE only manages hyprland.lua — migrate from hyprland.conf")
        );
        assert_eq!(
            t.tr("shell.lua_enable"),
            Some("Run init.sh enable to let HVE manage hyprland.lua")
        );
    }

    #[test]
    fn test_shell_guard_keys_resolve_in_spanish() {
        let t = Tr::with_lang("es");
        assert_eq!(
            t.tr("shell.lua_migration"),
            Some("HVE solo gestiona hyprland.lua — migre desde hyprland.conf")
        );
        assert_eq!(
            t.tr("shell.lua_enable"),
            Some("Ejecute init.sh enable para que HVE gestione hyprland.lua")
        );
    }
}
