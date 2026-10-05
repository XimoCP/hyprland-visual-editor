//! Theme preset packaging (theme-packages T4): a saved theme carries the
//! CUSTOM presets it uses, so its look travels with the folder. A preset HVE
//! ships (`assets/{animations,borders,shaders}`) is never copied — it is
//! already inside the app. On apply, a preset the theme carries under
//! `presets/` and the user's store lacks is installed into the user's store so
//! the existing apply path resolves it; an existing user preset of the same
//! name is KEPT, never clobbered.
//!
//! Packaging is BEST-EFFORT by contract: a missing source, an unreadable
//! directory or a copy error only warns and degrades. It never fails a save or
//! an apply, and a theme without `presets/` applies exactly as before.

use crate::preset_store::PresetStore;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Theme-relative directory that carries the custom presets.
pub const PRESETS_DIR: &str = "presets";

/// Preset categories a theme can carry, matching `assets/<category>/`.
pub const CATEGORIES: [&str; 3] = ["animations", "borders", "shaders"];

/// File extensions a bundled preset may carry (`*.lua` presets, `*.frag`
/// shaders). Anything else under `presets/` is ignored, so a stray file never
/// reaches the user's store.
const PRESET_EXTENSIONS: [&str; 2] = ["lua", "frag"];

/// The active preset file names a theme records, one per category. Mirrors the
/// `active_anim_file` / `active_border_file` / `active_shader_file` keys of
/// the `hve-presets` provider's `state.json` (and of `Config`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ActivePresets {
    pub animations: String,
    pub borders: String,
    pub shaders: String,
}

impl ActivePresets {
    /// The non-empty `(category, file_name)` pairs, in [`CATEGORIES`] order.
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        let pairs = [
            (CATEGORIES[0], self.animations.as_str()),
            (CATEGORIES[1], self.borders.as_str()),
            (CATEGORIES[2], self.shaders.as_str()),
        ];
        pairs
            .into_iter()
            .filter_map(|(category, file)| {
                let file = file.trim();
                (!file.is_empty()).then_some((category, file))
            })
            .collect()
    }
}

/// The file names HVE ships per category, keyed by category.
pub type ShippedCatalog = BTreeMap<String, BTreeSet<String>>;

/// Pure shipped/custom rule: `file_name` is shipped for `category` when it is
/// a member of that category's shipped set. No I/O, no environment, no global
/// state — the caller supplies the catalog, so the rule is testable in
/// isolation.
pub fn is_shipped_preset(file_name: &str, category: &str, catalog: &ShippedCatalog) -> bool {
    catalog
        .get(category)
        .is_some_and(|names| names.contains(file_name))
}

/// HVE's shipped preset set, read once from `assets/<category>/`.
///
/// A category whose directory cannot be read is marked UNKNOWN, and an unknown
/// category answers "shipped" for every name: refusing to copy is always safe,
/// because a shipped preset is already in the app, while copying a shipped
/// preset would violate the rule. [`ShippedPresets::load`] returns `None` when
/// the assets root itself is unreadable, so the caller degrades instead of
/// guessing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShippedPresets {
    catalog: ShippedCatalog,
    unknown: BTreeSet<String>,
}

impl ShippedPresets {
    /// Read every category's shipped file names. `None` when `assets_dir`
    /// itself cannot be read.
    pub fn load(assets_dir: &Path) -> Option<Self> {
        std::fs::read_dir(assets_dir).ok()?;
        let mut catalog = ShippedCatalog::new();
        let mut unknown = BTreeSet::new();
        for category in CATEGORIES {
            let dir = assets_dir.join(category);
            match std::fs::read_dir(&dir) {
                Ok(entries) => {
                    let names: BTreeSet<String> = entries
                        .flatten()
                        .filter_map(|e| e.file_name().to_str().map(str::to_string))
                        .collect();
                    catalog.insert(category.to_string(), names);
                }
                Err(e) => {
                    tracing::warn!(
                        "[theme-presets] cannot read shipped preset dir {}: {} — its presets are treated as shipped",
                        dir.display(),
                        e
                    );
                    unknown.insert(category.to_string());
                }
            }
        }
        Some(Self { catalog, unknown })
    }

    /// Pure decision for one preset: an unknown category answers "shipped"
    /// (the safe direction — a shipped preset must never be copied).
    pub fn is_shipped(&self, category: &str, file_name: &str) -> bool {
        if self.unknown.contains(category) {
            return true;
        }
        is_shipped_preset(file_name, category, &self.catalog)
    }

    /// Whether the shipped set for `category` was actually readable. Stale
    /// cleanup only runs for known categories.
    pub fn is_known(&self, category: &str) -> bool {
        !self.unknown.contains(category)
    }
}

/// One custom preset a theme must carry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BundledPreset {
    pub category: String,
    pub file_name: String,
}

impl BundledPreset {
    /// The theme-relative destination (`presets/<category>/<file>`).
    pub fn theme_relative(&self) -> PathBuf {
        Path::new(PRESETS_DIR)
            .join(&self.category)
            .join(&self.file_name)
    }
}

/// The bundle plan: the active presets that are NOT shipped, in category
/// order. Pure — computed from the recorded actives and the shipped catalog
/// alone, so it is testable without a user store or a theme dir.
pub fn plan_bundle(active: &ActivePresets, shipped: &ShippedPresets) -> Vec<BundledPreset> {
    active
        .entries()
        .into_iter()
        .filter(|(category, file)| !shipped.is_shipped(category, file))
        .map(|(category, file)| BundledPreset {
            category: category.to_string(),
            file_name: file.to_string(),
        })
        .collect()
}

/// Bundle the theme's custom presets under `{theme_dir}/presets/<category>/`.
///
/// A custom preset is by definition NOT shipped, so its source is the user's
/// store at `user_presets_root/<category>/<file_name>`. Each planned preset is
/// copied keeping its real file name; a preset no longer in the plan is removed
/// from the theme so it carries exactly the custom presets it uses (only for
/// categories whose shipped set is known).
///
/// Best-effort by contract: a missing source, an unreadable directory or a
/// copy error warns and is skipped — the save still succeeds. Returns the
/// presets actually copied.
///
/// SECURITY: the file name comes from the theme's active-preset config and is
/// untrusted. A name that is not a single, non-empty path component (an
/// absolute path or a `..` traversal) is refused before it can build a source
/// or destination: `Path::join` would otherwise escape the user store or the
/// theme folder. The refused name is only warned about and skipped.
pub fn bundle_theme_presets(
    theme_dir: &Path,
    active: &ActivePresets,
    shipped: &ShippedPresets,
    user_presets_root: &Path,
) -> Vec<BundledPreset> {
    let plan = plan_bundle(active, shipped);
    let mut copied = Vec::new();
    for preset in &plan {
        if let Err(reason) = PresetStore::validate_name(&preset.file_name) {
            tracing::warn!(
                "[theme-presets] refusing unsafe preset name {}: {} — it will not travel with the theme",
                preset.file_name,
                reason
            );
            continue;
        }
        let source = user_presets_root
            .join(&preset.category)
            .join(&preset.file_name);
        if !source.is_file() {
            tracing::warn!(
                "[theme-presets] custom preset {} not found in the user store ({}); \
                 it will not travel with the theme",
                preset.file_name,
                source.display()
            );
            continue;
        }
        let dest = theme_dir.join(preset.theme_relative());
        if copy_preset(&source, &dest) {
            tracing::info!(
                "[theme-presets] bundled custom preset {}/{} into the theme",
                preset.category,
                preset.file_name
            );
            copied.push(preset.clone());
        }
    }
    remove_stale_bundled_presets(theme_dir, &plan, shipped);
    copied
}

/// Copy `source` to `dest` through a temporary sibling, so a failed copy never
/// publishes a partial preset and never destroys a previously bundled one.
/// Returns `false` (with a warning) on any error.
fn copy_preset(source: &Path, dest: &Path) -> bool {
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("[theme-presets] cannot create {}: {}", parent.display(), e);
            return false;
        }
    }
    let Some(name) = dest.file_name().and_then(|n| n.to_str()) else {
        tracing::warn!("[theme-presets] invalid preset destination {}", dest.display());
        return false;
    };
    let tmp = dest.with_file_name(format!(".part-{name}"));
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = std::fs::copy(source, &tmp) {
        tracing::warn!(
            "[theme-presets] cannot copy preset {} -> {}: {}",
            source.display(),
            dest.display(),
            e
        );
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        tracing::warn!(
            "[theme-presets] cannot publish preset {}: {}",
            dest.display(),
            e
        );
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

/// Remove bundled preset files the current plan no longer lists, so a re-save
/// carries exactly the custom presets it uses. Only categories whose shipped
/// set is KNOWN are cleaned: an unreadable shipped set must never trigger a
/// delete. Best-effort: removal failures only warn.
fn remove_stale_bundled_presets(
    theme_dir: &Path,
    plan: &[BundledPreset],
    shipped: &ShippedPresets,
) {
    for category in CATEGORIES {
        if !shipped.is_known(category) {
            continue;
        }
        let dir = theme_dir.join(PRESETS_DIR).join(category);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let keep = plan
                .iter()
                .any(|p| p.category == category && p.file_name == name);
            if keep || !is_preset_file(&path) || !path.is_file() {
                continue;
            }
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(
                    "[theme-presets] cannot remove stale bundled preset {}: {}",
                    path.display(),
                    e
                );
            } else {
                tracing::info!(
                    "[theme-presets] removed stale bundled preset {}",
                    path.display()
                );
            }
        }
    }
}

/// Whether `path` carries a preset file extension (`*.lua`, `*.frag`).
fn is_preset_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PRESET_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// What an apply installed from the theme's `presets/` into the user store.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub installed: Vec<BundledPreset>,
    pub kept: Vec<BundledPreset>,
}

/// Install the presets a theme carries under `presets/` into the user's preset
/// store, so the existing apply path resolves them.
///
/// A file the user's store already has is KEPT untouched — the user's copy
/// wins — and reported under [`InstallReport::kept`]; a file the store lacks is
/// copied (creating the category dir) and reported under `installed`. Only
/// preset files (`*.lua`, `*.frag`) are considered. Best-effort by contract: a
/// missing theme dir, an unreadable directory or a copy error warns and is
/// skipped; a theme without `presets/` is a no-op and applies exactly as
/// before.
pub fn install_theme_presets(theme_dir: &Path, user_presets_root: &Path) -> InstallReport {
    let mut report = InstallReport::default();
    let presets_dir = theme_dir.join(PRESETS_DIR);
    if !presets_dir.is_dir() {
        return report;
    }
    for category in CATEGORIES {
        let dir = presets_dir.join(category);
        if !dir.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(
                    "[theme-presets] cannot read the theme's {} presets ({}): {}",
                    category,
                    dir.display(),
                    e
                );
                continue;
            }
        };
        for entry in entries.flatten() {
            let source = entry.path();
            if !is_preset_file(&source) || !source.is_file() {
                continue;
            }
            let Some(file_name) = source.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // SECURITY: never build a destination from an unsafe name. A
            // directory entry is normally a single component already; the guard
            // keeps the invariant explicit so nothing is ever installed outside
            // the user store.
            if let Err(reason) = PresetStore::validate_name(file_name) {
                tracing::warn!(
                    "[theme-presets] refusing unsafe preset name {}: {} — it will not be installed",
                    file_name,
                    reason
                );
                continue;
            }
            let preset = BundledPreset {
                category: category.to_string(),
                file_name: file_name.to_string(),
            };
            let dest = user_presets_root.join(category).join(file_name);
            if dest.exists() {
                tracing::info!(
                    "[theme-presets] keeping the user's preset {}/{} (the theme carries one with the same name)",
                    category,
                    file_name
                );
                report.kept.push(preset);
                continue;
            }
            if copy_preset(&source, &dest) {
                tracing::info!(
                    "[theme-presets] installed theme preset {}/{} into the user store",
                    category,
                    file_name
                );
                report.installed.push(preset);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn catalog(entries: &[(&str, &[&str])]) -> ShippedCatalog {
        entries
            .iter()
            .map(|(cat, names)| {
                (
                    cat.to_string(),
                    names.iter().map(|n| n.to_string()).collect(),
                )
            })
            .collect()
    }

    fn shipped(entries: &[(&str, &[&str])]) -> ShippedPresets {
        ShippedPresets {
            catalog: catalog(entries),
            unknown: BTreeSet::new(),
        }
    }

    fn active(anim: &str, border: &str, shader: &str) -> ActivePresets {
        ActivePresets {
            animations: anim.into(),
            borders: border.into(),
            shaders: shader.into(),
        }
    }

    #[test]
    fn is_shipped_preset_is_pure_and_category_scoped() {
        let cat = catalog(&[("animations", &["01_relampago.lua"])]);
        assert!(is_shipped_preset("01_relampago.lua", "animations", &cat));
        assert!(
            !is_shipped_preset("01_relampago.lua", "borders", &cat),
            "the same name is not shipped for another category"
        );
        assert!(!is_shipped_preset("custom.lua", "animations", &cat));
        assert!(!is_shipped_preset("custom.lua", "unknown-category", &cat));
    }

    #[test]
    fn shipped_presets_load_reads_every_category_from_assets() {
        let dir = tempfile::tempdir().unwrap();
        for category in CATEGORIES {
            let cat_dir = dir.path().join(category);
            fs::create_dir_all(&cat_dir).unwrap();
            fs::write(cat_dir.join("shipped.lua"), "x").unwrap();
        }
        let loaded = ShippedPresets::load(dir.path()).expect("a readable assets dir must load");
        for category in CATEGORIES {
            assert!(loaded.is_shipped(category, "shipped.lua"), "{category}");
            assert!(loaded.is_known(category), "{category}");
            assert!(!loaded.is_shipped(category, "custom.lua"), "{category}");
        }
    }

    #[test]
    fn shipped_presets_load_returns_none_for_an_unreadable_assets_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ShippedPresets::load(&dir.path().join("missing")).is_none());
    }

    #[test]
    fn shipped_presets_treat_a_missing_category_dir_as_shipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("animations")).unwrap();
        fs::write(dir.path().join("animations/shipped.lua"), "x").unwrap();
        let loaded = ShippedPresets::load(dir.path()).unwrap();
        // borders/shaders dirs are missing: unknown -> shipped (safe).
        assert!(loaded.is_shipped("borders", "anything.lua"));
        assert!(!loaded.is_known("borders"));
        assert!(loaded.is_known("animations"));
    }

    #[test]
    fn plan_bundle_keeps_custom_presets_and_drops_shipped_ones() {
        let shipped = shipped(&[
            ("animations", &["01_relampago.lua"]),
            ("borders", &["01_cascade.lua"]),
        ]);
        let plan = plan_bundle(&active("custom_anim.lua", "01_cascade.lua", ""), &shipped);
        assert_eq!(
            plan,
            vec![BundledPreset {
                category: "animations".into(),
                file_name: "custom_anim.lua".into()
            }],
            "only the custom animation travels; the shipped border and the empty shader do not"
        );
    }

    #[test]
    fn bundle_copies_a_custom_preset_into_the_theme() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        let src_dir = user_root.join("animations");
        fs::create_dir_all(&src_dir).unwrap();
        let bytes = b"-- custom animation\n";
        fs::write(src_dir.join("custom_anim.lua"), bytes).unwrap();

        let copied = bundle_theme_presets(
            &theme,
            &active("custom_anim.lua", "", ""),
            &shipped(&[("animations", &[])]),
            &user_root,
        );

        assert_eq!(copied.len(), 1);
        assert_eq!(
            fs::read(theme.join("presets/animations/custom_anim.lua")).unwrap(),
            bytes,
            "the custom preset must travel byte-for-byte under presets/<category>/"
        );
    }

    /// SECURITY: the active preset name comes from the theme's config and is
    /// untrusted. An ABSOLUTE name must never be bundled: `Path::join` with an
    /// absolute path replaces the base, so the bundler would build a source and
    /// destination outside the theme folder. The name is skipped, nothing is
    /// reported as copied, and no temporary copy is left behind.
    #[test]
    fn bundle_skips_an_absolute_preset_name_and_writes_nothing_outside_the_theme() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        // An absolute name resolves the source to itself; give it a real file
        // so the unguarded code would happily "copy" it.
        let absolute = dir.path().join("evil.lua");
        fs::write(&absolute, b"-- evil\n").unwrap();

        let copied = bundle_theme_presets(
            &theme,
            &active(absolute.to_str().unwrap(), "", ""),
            &shipped(&[("animations", &[])]),
            &user_root,
        );

        assert!(
            copied.is_empty(),
            "an absolute preset name must never be bundled, got {copied:?}"
        );
        assert!(
            !dir.path().join(".part-evil.lua").exists(),
            "no temporary copy may be published outside the theme"
        );
        assert!(
            !theme.join(PRESETS_DIR).exists(),
            "the theme must not gain a presets tree for an unsafe name"
        );
    }

    /// SECURITY: a `..` traversal name must never escape the theme folder. The
    /// source is made to exist outside the theme, so the unguarded code would
    /// copy it to `root/evil.lua` (one level above the theme).
    #[test]
    fn bundle_skips_a_traversal_preset_name_and_writes_nothing_outside_the_theme() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        let theme = root.join("theme");
        let user_root = root.join("user");
        fs::create_dir_all(user_root.join("animations")).unwrap();
        fs::write(user_root.join("animations/../../../evil.lua"), b"-- evil\n").unwrap();
        assert!(
            outer.path().join("evil.lua").is_file(),
            "fixture: the traversal source must exist"
        );

        let copied = bundle_theme_presets(
            &theme,
            &active("../../../evil.lua", "", ""),
            &shipped(&[("animations", &[])]),
            &user_root,
        );

        assert!(
            copied.is_empty(),
            "a traversal preset name must never be bundled, got {copied:?}"
        );
        assert!(
            !root.join("evil.lua").exists(),
            "a traversal name must never write outside the theme folder"
        );
    }

    /// The guard must not touch valid names: a plain single-component name
    /// still bundles byte-for-byte under `presets/<category>/`.
    #[test]
    fn bundle_still_copies_a_valid_custom_preset() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        let src = user_root.join("animations/valid.lua");
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        let bytes = b"-- valid custom animation\n";
        fs::write(&src, bytes).unwrap();

        let copied = bundle_theme_presets(
            &theme,
            &active("valid.lua", "", ""),
            &shipped(&[("animations", &[])]),
            &user_root,
        );

        assert_eq!(copied.len(), 1, "a valid name must still bundle");
        assert_eq!(
            fs::read(theme.join("presets/animations/valid.lua")).unwrap(),
            bytes
        );
    }

    #[test]
    fn bundle_never_copies_a_shipped_preset() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        // Even a same-named file in the user store must not be bundled: the
        // preset is shipped, so it is already in the app.
        let src_dir = user_root.join("borders");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("01_cascade.lua"), b"-- shipped border\n").unwrap();

        let copied = bundle_theme_presets(
            &theme,
            &active("", "01_cascade.lua", ""),
            &shipped(&[("borders", &["01_cascade.lua"])]),
            &user_root,
        );

        assert!(copied.is_empty());
        assert!(!theme.join("presets/borders/01_cascade.lua").exists());
        assert!(
            !theme.join(PRESETS_DIR).exists(),
            "nothing shipped is ever written"
        );
    }

    #[test]
    fn bundle_degrades_when_a_custom_source_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");

        let copied = bundle_theme_presets(
            &theme,
            &active("missing.lua", "", ""),
            &shipped(&[("animations", &[])]),
            &user_root,
        );

        assert!(copied.is_empty(), "a missing source must not fail the save");
        assert!(!theme.join("presets/animations/missing.lua").exists());
    }

    #[test]
    fn bundle_removes_a_stale_bundled_preset() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let stale_dir = theme.join("presets/animations");
        fs::create_dir_all(&stale_dir).unwrap();
        fs::write(stale_dir.join("old.lua"), b"-- no longer used\n").unwrap();

        bundle_theme_presets(
            &theme,
            &active("", "", ""),
            &shipped(&[("animations", &[])]),
            &dir.path().join("user"),
        );

        assert!(
            !stale_dir.join("old.lua").exists(),
            "a preset the theme no longer uses must not stay behind"
        );
    }

    #[test]
    fn install_installs_a_bundled_preset_absent_from_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        let preset_dir = theme.join("presets/animations");
        fs::create_dir_all(&preset_dir).unwrap();
        let bytes = b"-- travelling animation\n";
        fs::write(preset_dir.join("travel.lua"), bytes).unwrap();

        let report = install_theme_presets(&theme, &user_root);

        assert_eq!(
            report.installed,
            vec![BundledPreset {
                category: "animations".into(),
                file_name: "travel.lua".into()
            }]
        );
        assert_eq!(
            fs::read(user_root.join("animations/travel.lua")).unwrap(),
            bytes
        );
    }

    #[test]
    fn install_keeps_an_existing_user_preset() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        let theme_preset = theme.join("presets/borders");
        fs::create_dir_all(&theme_preset).unwrap();
        fs::write(theme_preset.join("keeper.lua"), b"-- theme copy\n").unwrap();
        let user_dir = user_root.join("borders");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("keeper.lua"), b"-- THE USER'S OWN COPY\n").unwrap();

        let report = install_theme_presets(&theme, &user_root);

        assert_eq!(
            report.kept,
            vec![BundledPreset {
                category: "borders".into(),
                file_name: "keeper.lua".into()
            }]
        );
        assert!(report.installed.is_empty());
        assert_eq!(
            fs::read(user_dir.join("keeper.lua")).unwrap(),
            b"-- THE USER'S OWN COPY\n",
            "the user's copy must never be clobbered"
        );
    }

    #[test]
    fn install_is_a_noop_for_a_theme_without_presets() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        fs::create_dir_all(&theme).unwrap();
        let user_root = dir.path().join("user");

        let report = install_theme_presets(&theme, &user_root);

        assert!(report.installed.is_empty());
        assert!(report.kept.is_empty());
        assert!(
            !user_root.exists(),
            "an old theme must not create the user store"
        );
    }

    #[test]
    fn install_ignores_non_preset_files_and_unreadable_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let user_root = dir.path().join("user");
        let preset_dir = theme.join("presets/animations");
        fs::create_dir_all(&preset_dir).unwrap();
        fs::write(preset_dir.join("notes.txt"), b"junk").unwrap();
        // A category path that is a FILE, not a dir: read_dir must not panic.
        fs::write(theme.join("presets/borders"), b"not a dir").unwrap();

        let report = install_theme_presets(&theme, &user_root);

        assert!(report.installed.is_empty());
        assert!(report.kept.is_empty());
        assert!(
            !user_root.join("animations/notes.txt").exists(),
            "non-preset files must not travel"
        );
    }
}
