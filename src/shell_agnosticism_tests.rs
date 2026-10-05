//! The agnostic proof: the core, driven end-to-end by a FAKE shell adapter
//! and a FAKE theme provider it has never heard of. If adding a new shell
//! ever forces an edit to these tests to keep them passing, the agnosticism
//! claim is false.

use crate::providers::shell_capabilities::ShellCapabilities;
use crate::theme_manager::{
    PreviewRole, PreviewSource, PreviewSourceKind, ThemeFact, ThemeManager, ThemeProvider,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A shell the core has never heard of: every capability is answered from
/// this struct's own fields.
#[derive(Default)]
struct FakeShell {
    live: Option<String>,
    saved: HashMap<String, String>,
    arms: bool,
    dnd: Option<String>,
    calls: Mutex<Vec<String>>,
}

impl ShellCapabilities for FakeShell {
    fn live_wallpaper(&self) -> Option<String> {
        self.live.clone()
    }

    fn saved_theme_wallpaper(&self, _root: &Path, name: &str) -> Option<String> {
        self.saved.get(name).cloned()
    }

    fn arms_palette_reassert(&self, _root: &Path, _name: &str) -> bool {
        self.arms
    }

    fn dnd_status(&self) -> Option<String> {
        self.dnd.clone()
    }

    fn dnd_set(&self, state: &str) -> bool {
        self.calls.lock().unwrap().push(state.to_string());
        true
    }
}

/// A provider the core has never heard of: it saves/applies, declares a
/// preview source and a fact.
struct FakeProvider;

impl ThemeProvider for FakeProvider {
    fn id(&self) -> &str {
        "fake"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.fake"
    }

    fn icon(&self) -> &str {
        "fake"
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let dir = theme_dir.join("providers").join("fake");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("state.txt"), "saved").map_err(|e| e.to_string())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let marker = theme_dir.join("providers").join("fake").join("applied.txt");
        std::fs::write(marker, "applied").map_err(|e| e.to_string())
    }

    fn preview_sources(&self, provider_dir: &Path) -> Vec<PreviewSource> {
        vec![PreviewSource {
            role: PreviewRole::WallpaperText,
            kind: PreviewSourceKind::Image,
            paths: vec![provider_dir.join("state.txt")],
        }]
    }

    fn theme_facts(&self, _provider_dir: &Path) -> Vec<ThemeFact> {
        vec![ThemeFact {
            label: "Fake".into(),
            value: "yes".into(),
            swatches: vec![],
        }]
    }
}

fn sandbox() -> PathBuf {
    dirs::config_dir().expect("sandbox config dir")
}

#[test]
fn the_core_backfills_the_active_theme_through_a_fake_shell() {
    let _env = crate::test_utils::TempEnv::new();
    let config_dir = sandbox();
    let mut tm = ThemeManager::new(&config_dir);
    tm.register_provider(Box::new(FakeProvider));
    for name in ["Alpha", "Beta"] {
        tm.save(name, &["fake".to_string()]).expect("save fake theme");
    }

    let mut shell = FakeShell::default();
    shell.live = Some("/wall/beta.png".into());
    shell.saved.insert("Alpha".into(), "/wall/alpha.png".into());
    shell.saved.insert("Beta".into(), "/wall/beta.png".into());

    let picked = crate::backfill_active_from_live(&tm, &config_dir, &shell);
    assert_eq!(
        picked.as_deref(),
        Some("Beta"),
        "the core must resolve the active theme from whatever the shell adapter reports"
    );
}

#[test]
fn the_core_asks_the_adapter_whether_an_apply_arms_the_reassert() {
    let root = Path::new("/nonexistent");
    let armed = FakeShell {
        arms: true,
        ..Default::default()
    };
    let quiet = FakeShell::default();
    assert!(crate::apply_arms_palette_reassert_with(root, "T", &armed));
    assert!(!crate::apply_arms_palette_reassert_with(root, "T", &quiet));
}

#[test]
fn the_core_engages_and_restores_dnd_through_a_fake_shell() {
    // The static slot may hold a value from another test in this binary.
    crate::THEME_DND_ORIG.lock().unwrap().take();

    let shell = FakeShell {
        dnd: Some("off".into()),
        ..Default::default()
    };
    crate::theme_dnd_engage_with(&shell);
    crate::theme_dnd_release_with(&shell);
    assert_eq!(
        shell.calls.lock().unwrap().as_slice(),
        &["on".to_string(), "off".to_string()]
    );

    // A shell with no DND at all: nothing recorded, nothing breaks.
    let silent = FakeShell::default();
    crate::theme_dnd_engage_with(&silent);
    crate::theme_dnd_release_with(&silent);
    assert!(silent.calls.lock().unwrap().is_empty());
}

#[test]
fn a_fake_provider_drives_list_save_apply_preview_and_facts() {
    let _env = crate::test_utils::TempEnv::new();
    let config_dir = sandbox();
    let mut tm = ThemeManager::new(&config_dir);
    tm.register_provider(Box::new(FakeProvider));

    tm.save("FakeTheme", &["fake".to_string()]).expect("save");
    let names: Vec<String> = tm.list().unwrap().into_iter().map(|t| t.name).collect();
    assert!(
        names.contains(&"FakeTheme".to_string()),
        "list must see the fake provider's theme"
    );

    tm.apply("FakeTheme", || Ok(())).expect("apply");
    let provider_dir = config_dir
        .join("hve")
        .join("themes")
        .join("FakeTheme")
        .join("providers")
        .join("fake");
    assert!(
        provider_dir.join("applied.txt").is_file(),
        "apply must reach the fake provider"
    );

    let provider = tm.provider("fake").expect("registered fake");
    assert_eq!(provider.preview_sources(&provider_dir).len(), 1);
    assert_eq!(
        tm.theme_info("FakeTheme").unwrap().rows,
        vec![("Fake".to_string(), "yes".to_string())]
    );
}
