use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ProviderCapabilities: u32 {
        const WALLPAPERS   = 0b0000_0001;
        const COLORS       = 0b0000_0010;
        const ANIMATIONS   = 0b0000_0100;
        const BORDERS      = 0b0000_1000;
        const SHADERS      = 0b0001_0000;
    }
}

/// One artifact a provider owns OUTSIDE its theme directory, offered to the
/// core for deletion when a theme that recorded it is removed (the backend
/// contract's `deletable-artefacts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletableArtifact {
    /// Live path of the artifact the core may remove.
    pub path: PathBuf,
    /// The reference key any OTHER theme's record of the SAME artifact
    /// carries. The core keeps the file alive while any surviving theme
    /// declares this same key through ANY provider — the registered list
    /// plus the read-only declarers of the survivor's own recorded ids.
    /// Keys are matched ACROSS providers on purpose: a survivor's record
    /// stored under a directory the owner never writes still references
    /// the file, so when two readings of the key disagree the decision
    /// must favour keeping. The admitted cost: two backends recording
    /// equal keys for different files protect each other's paths — a
    /// leak, never a deletion of something still referenced (over-keep
    /// beats over-delete).
    pub reference_key: String,
}

/// What one declared preview source contains (the backend contract's
/// `read-preview-source`): the provider's own statement of the media type
/// behind its record. The core's video-extension allowlist still gates how
/// a source is treated, so a declared video whose path carries no video
/// extension resolves exactly as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSourceKind {
    Image,
    Video,
}

/// The precedence slot a declared preview source fills in the gallery's
/// resolution chain. The CORE owns this vocabulary and the order below —
/// a live background assignment first, then the painter's video record,
/// then the static wallpaper manifest, then the static wallpaper text,
/// and finally the loose image scan — because deciding precedence is the
/// core's job. A provider only states which slot one of its own records
/// fills; it never sees another backend's records and never decides which
/// declaration wins. Variants are declared in precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRole {
    /// The theme's live background assignment (image or video).
    Assignment,
    /// The painter-identified video record.
    PainterVideo,
    /// The static wallpaper manifest record.
    WallpaperManifest,
    /// The static wallpaper text record.
    WallpaperText,
}

/// One preview source a provider declares for a theme: the precedence slot
/// its record fills, the media type it holds, and the candidate paths the
/// record states — in the record's OWN order, with no existence probing
/// (the core probes, the first existing candidate in role precedence
/// order wins).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSource {
    pub role: PreviewRole,
    pub kind: PreviewSourceKind,
    pub paths: Vec<PathBuf>,
}

/// A provider knows how to capture and restore one slice of desktop state.
///
/// Each provider is identified by a stable `id` (e.g. `"noctalia"`,
/// `"hve-presets"`) and stores its data in a subdirectory
/// `{theme_dir}/providers/{id}/`.
pub trait ThemeProvider: Send + Sync {
    fn id(&self) -> &str;
    // The four methods below are exercised only by the test suite (noctalia.rs
    // tests); production code uses `id`, `save`, `apply` and `post_apply`.
    // The allows are required: rustc's dead_code lint does not count
    // `#[cfg(test)]` usage during a plain `cargo check`.
    #[allow(dead_code)]
    fn display_name_key(&self) -> &str;
    #[allow(dead_code)]
    fn icon(&self) -> &str;

    #[allow(dead_code)]
    /// Return the desktop shell this provider targets (e.g. "hyprland", "noctalia").
    fn shell(&self) -> &str {
        "unknown"
    }

    /// Capabilities this provider manages.
    #[allow(dead_code)]
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::empty()
    }

    /// Capture current state into `{theme_dir}/providers/{id}/`.
    fn save(&self, theme_dir: &Path) -> Result<(), String>;

    /// Restore state from `{theme_dir}/providers/{id}/`.
    fn apply(&self, theme_dir: &Path) -> Result<(), String>;

    /// Optional hook called after all providers have applied successfully.
    ///
    /// Each provider defines post-apply actions specific to its shell:
    /// - Noctalia v4: runs the template-processor to regenerate GTK/QT/terminal themes
    /// - Noctalia v5: sends IPC to the daemon
    /// - Other shells: their own refresh mechanism
    ///
    /// Default implementation is no-op. Errors are logged but do NOT fail the apply.
    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        let _ = theme_name;
        Ok(())
    }

    /// Re-assert this provider's colour authority for an applied theme: the
    /// backend owns the colours of a theme it saved, so it — not the core, and not
    /// a shell script — puts them back if something external changed them.
    /// Idempotent: doing nothing when the state already matches is correct.
    /// Default: no-op, because most providers do not own colours.
    fn reassert_colours(&self, theme_dir: &std::path::Path) -> Result<(), String> {
        let _ = theme_dir;
        Ok(())
    }

    /// Rebuild this provider's colour-authority descriptor from the theme's
    /// OWN saved state (startup repair for themes applied before the
    /// descriptor existed). Read-only by contract: no palette is copied and
    /// no backend CLI runs — it only re-declares what a previous apply
    /// already recorded. `None` when the provider claims no colour
    /// authority for this theme, or when the state needed to declare it is
    /// missing. Default: `None`, because most providers do not own colours.
    fn derive_colour_authority(
        &self,
        theme_dir: &std::path::Path,
    ) -> Option<crate::color_authority::ColorAuthority> {
        let _ = theme_dir;
        None
    }

    /// The artifacts this provider owns OUTSIDE the theme dir that the core
    /// may delete when a theme recording them is deleted (the backend
    /// contract's `deletable-artefacts`,
    /// `openspec/specs/capability-routing/spec.md`). The provider supplies
    /// only its own KNOWLEDGE: which live file it owns and the reference
    /// key its own record format produces for it. The core keeps the
    /// DECISION: it removes the path only while no surviving theme
    /// declares the same key through any provider, fails OPEN (keeps the
    /// file) when a survivor's tree cannot be scanned, and never runs this
    /// for the currently applied theme. A theme's own recorded provider
    /// ids are asked even when their backend is not registered — the
    /// registration router supplies a read-only declarer for that id, so
    /// an inactive backend never makes its own record invisible. Default:
    /// nothing outside the theme dir, so nothing is declared and nothing
    /// is deleted.
    fn deletable_artifacts(&self, theme_dir: &std::path::Path) -> Vec<DeletableArtifact> {
        let _ = theme_dir;
        Vec::new()
    }

    /// The preview sources this provider can supply for a theme (the
    /// backend contract's `read-preview-source`,
    /// `openspec/specs/capability-routing/spec.md`). The provider reads its
    /// OWN record files inside `provider_dir` — the theme provider
    /// directory the core resolved for its id — and states, per record:
    /// which precedence slot the record fills ([`PreviewRole`]), whether it
    /// holds an image or a video ([`PreviewSourceKind`]), and the candidate
    /// paths in the record's OWN order, exactly as the record states them
    /// (existence is NOT probed here). The split mirrors
    /// `deletable_artifacts`: the provider supplies knowledge — its layout
    /// and its record format — while the core owns every decision: role
    /// precedence, provider-directory order, existence, the
    /// video-extension allowlist and every fall-through. The gallery must
    /// never learn a backend's layout or record format. Default: no
    /// preview record, so nothing is declared.
    fn preview_sources(&self, provider_dir: &Path) -> Vec<PreviewSource> {
        let _ = provider_dir;
        Vec::new()
    }
}

/// Provider ids that have been removed from the product itself.
///
/// A theme saved before a removal still lists the retired id in its
/// `meta.json`. `ThemeManager::list()` hides every theme whose providers are
/// not all registered — correct for a genuine capability mismatch (e.g.
/// `mpvpaper` unavailable on this machine), wrong for a product removal like
/// `hyprland-settings`. `migrate_removed_provider_ids` strips exactly these
/// ids so a product removal is never mistaken for an unavailable provider.
pub const REMOVED_PROVIDER_IDS: &[&str] = &["hyprland-settings"];

/// Metadata persisted inside each theme directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeMeta {
    pub saved_at: String,
    pub description: String,
    pub providers: Vec<String>,
}

/// Info about a saved theme, used by the Slint UI.
#[derive(Debug, Clone)]
pub struct ThemeInfo {
    pub name: String,
    pub saved_at: String,
    pub is_active: bool,
    pub providers: Vec<String>,
}

/// Manages themes: list, save, apply, delete, rename.
pub struct ThemeManager {
    themes_dir: PathBuf,
    /// Ordered list of registered providers.
    providers: Vec<Box<dyn ThemeProvider>>,
    /// Last applied theme name (empty = none).
    pub last_applied: String,
}

impl ThemeManager {
    pub fn new(config_dir: &Path) -> Self {
        let themes_dir = config_dir.join("hve").join("themes");
        // Migration lives in the constructor on purpose: every code path that
        // can observe themes goes through `ThemeManager` (`list()` is a method
        // on it), so running here guarantees the stale metas are fixed before
        // any observer reads them. `main.rs` builds a second manager for the
        // gallery; running the migration once per instance is harmless because
        // it rewrites a `meta.json` only when a removed id is present.
        Self::migrate_removed_provider_ids(&themes_dir);
        Self {
            themes_dir,
            providers: Vec::new(),
            last_applied: String::new(),
        }
    }

    pub fn register_provider(&mut self, provider: Box<dyn ThemeProvider>) {
        self.providers.push(provider);
    }

    /// Returns the IDs of all registered providers.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.id().to_string()).collect()
    }

    /// Resolve one registered provider by id. `None` when no provider with
    /// that id is registered — callers skip the missing one, never fail, so
    /// a theme that records a retired or unavailable provider still routes
    /// to the providers that are present.
    pub fn provider(&self, id: &str) -> Option<&dyn ThemeProvider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    // ─── Helpers ─────────────────────────────────────────────────────

    /// Strip retired provider ids from every `meta.json` under `themes_dir`.
    ///
    /// Idempotent: a `meta.json` is rewritten only when it actually lists a
    /// removed id, so callers can run it unconditionally.
    ///
    /// The rewrite operates on the raw `serde_json::Value` instead of the
    /// typed `ThemeMeta` so any field this code does not know about — written
    /// by a newer HVE or by hand — survives the round-trip untouched. Only the
    /// removed ids are dropped from `providers`; `saved_at`, `description` and
    /// the relative order of the remaining ids are preserved because the value
    /// is edited in place.
    ///
    /// The matching directory filter mirrors `list()`: names starting with `.`
    /// (hidden) or `_` (backup) are not themes and are left alone.
    ///
    /// Writes are atomic (temp file + `rename` in the same directory) because
    /// this runs automatically at startup over every user meta: a crash or a
    /// full disk must never truncate a `meta.json`.
    fn migrate_removed_provider_ids(themes_dir: &Path) {
        let Ok(entries) = fs::read_dir(themes_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Same filter as `list()`: hidden / backup dirs are not themes.
            if name.starts_with('.') || name.starts_with('_') {
                continue;
            }
            if !entry.path().is_dir() {
                continue;
            }
            let meta_path = entry.path().join("meta.json");
            let Ok(raw) = fs::read_to_string(&meta_path) else {
                continue;
            };
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(providers) = value.get_mut("providers").and_then(|p| p.as_array_mut())
            else {
                continue;
            };
            let before = providers.len();
            providers.retain(|id| {
                !id.as_str()
                    .map(|s| REMOVED_PROVIDER_IDS.contains(&s))
                    .unwrap_or(false)
            });
            if providers.len() == before {
                continue;
            }
            let Ok(json) = serde_json::to_string_pretty(&value) else {
                continue;
            };
            match Self::write_atomic(&meta_path, &json) {
                Ok(()) => tracing::info!(
                    "[themes] Migrated '{name}': removed retired provider id(s) from meta.json"
                ),
                Err(e) => {
                    tracing::warn!("[themes] Cannot migrate '{name}/meta.json': {e}")
                }
            }
        }
    }

    /// Atomically replace `path` with `contents`.
    ///
    /// Writes a sibling temp file and `rename`s it over the target, so a
    /// concurrent reader either sees the old bytes or the new bytes — never a
    /// truncated file. `rename` is atomic only within one filesystem, which is
    /// why the temp file lives in the target's own directory.
    fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "meta.json".to_string());
        let tmp_path = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));
        fs::write(&tmp_path, contents)?;
        match fs::rename(&tmp_path, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Best-effort cleanup; the original file is still intact.
                let _ = fs::remove_file(&tmp_path);
                Err(e)
            }
        }
    }

    fn timestamp() -> String {
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let millis = dur.subsec_millis();
        // Compute YMD HMS from epoch seconds (no chrono dep)
        let (y, mo, d, h, mi, s) = Self::epoch_to_utc(secs);
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
    }

    /// Convert epoch seconds to UTC date/time components.
    fn epoch_to_utc(epoch: u64) -> (u64, u64, u64, u64, u64, u64) {
        // Days since epoch
        let days = epoch / 86400;
        let time_secs = epoch % 86400;
        let h = time_secs / 3600;
        let mi = (time_secs % 3600) / 60;
        let s = time_secs % 60;

        // Civil date from days since 1970-01-01 (Rata Die algorithm)
        let z = days + 719468;
        let era = z / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        (y, mo, d, h, mi, s)
    }

    fn validate_name(name: &str) -> Result<String, String> {
        let t = name.trim();
        if t.is_empty() {
            return Err("name-empty".into());
        }
        if t.len() > 64 {
            return Err("name-too-long".into());
        }
        if t.contains('/') || t.contains('\\') || t.contains('\0') || t == ".." {
            return Err("name-invalid".into());
        }
        Ok(t.to_string())
    }

    // ─── Public API ──────────────────────────────────────────────────

    pub fn list(&self) -> Result<Vec<ThemeInfo>, String> {
        let dir = &self.themes_dir;
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read themes dir: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                !s.starts_with('.') && !s.starts_with('_')
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        let mut themes = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta_path = entry.path().join("meta.json");
            let meta = fs::read_to_string(&meta_path)
                .ok()
                .and_then(|s| serde_json::from_str::<ThemeMeta>(&s).ok());
            let providers = meta.as_ref().map(|m| m.providers.clone()).unwrap_or_default();
            themes.push(ThemeInfo {
                is_active: name == self.last_applied,
                saved_at: meta.as_ref().map(|m| m.saved_at.clone()).unwrap_or_default(),
                name,
                providers,
            });
        }

        // Filter: only show themes whose providers are all currently registered.
        // This ensures shell-specific themes (saved while a particular shell
        // version or future shell was active) are hidden when their shell is
        // not active. Shell-agnostic themes (e.g. presets-only) always show
        // because their providers are always registered.
        let registered = self.provider_ids();
        themes.retain(|t| {
            t.providers.iter().all(|pid| registered.contains(pid))
        });

        Ok(themes)
    }

    pub fn save(&mut self, name: &str, provider_ids: &[String]) -> Result<(), String> {
        let name = Self::validate_name(name)?;
        let theme_dir = self.themes_dir.join(&name);

        // Create directory structure
        fs::create_dir_all(&theme_dir)
            .map_err(|e| format!("Cannot create theme dir: {}", e))?;

        // Run each registered provider if selected
        let mut enabled = Vec::new();
        for p in &self.providers {
            if provider_ids.is_empty() || provider_ids.iter().any(|id| id == p.id()) {
                p.save(&theme_dir)?;
                enabled.push(p.id().to_string());
            }
        }

        // Write meta.json
        let meta = ThemeMeta {
            saved_at: Self::timestamp(),
            description: String::new(),
            providers: enabled,
        };
        let meta_json = serde_json::to_string_pretty(&meta)
            .map_err(|e| format!("Meta serialization error: {}", e))?;
        fs::write(theme_dir.join("meta.json"), &meta_json)
            .map_err(|e| format!("Cannot write meta.json: {}", e))?;

        Ok(())
    }

    pub fn apply(
        &mut self,
        name: &str,
        reload_after_post_apply: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let name = name.trim().to_string();
        let theme_dir = self.themes_dir.join(&name);

        if !theme_dir.exists() {
            return Err(format!("Theme '{}' not found", name));
        }

        // Read meta to know which providers were saved
        let meta_path = theme_dir.join("meta.json");
        let meta: ThemeMeta = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(ThemeMeta {
                saved_at: String::new(),
                description: String::new(),
                providers: self.providers.iter().map(|p| p.id().to_string()).collect(),
            });

        // ── Pass 1: All providers write files to disk ──
        for p in &self.providers {
            if meta.providers.iter().any(|id| id == p.id()) {
                p.apply(&theme_dir)?;
            }
        }

        // ── Pass 2: All providers run post-apply hooks (template processors, etc.) ──
        for p in &self.providers {
            if meta.providers.iter().any(|id| id == p.id()) {
                if let Err(e) = p.post_apply(&name) {
                    tracing::warn!(
                        "[themes] Provider '{}' post_apply warning: {}",
                        p.id(),
                        e
                    );
                }
            }
        }

        // ── Final reload AFTER all post_apply (including wallpaper IPC) ──
        if let Err(e) = reload_after_post_apply() {
            tracing::warn!("[themes] reload_after_post_apply warning: {e}");
        }

        self.last_applied = name;
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        let theme_dir = self.themes_dir.join(name);
        if !theme_dir.exists() {
            return Err(format!("Theme '{}' not found", name));
        }
        // Derived artifacts live outside the theme dir, so they are cleaned
        // BEFORE it is removed — resolving the preview source needs its
        // contents. Best-effort only: failures warn, never fail the delete.
        Self::cleanup_derived_artifacts(
            &self.themes_dir,
            &theme_dir,
            name,
            &self.last_applied,
            &self.providers,
        );
        fs::remove_dir_all(&theme_dir)
            .map_err(|e| format!("Failed to delete theme '{}': {}", name, e))?;
        if self.last_applied == name {
            self.last_applied.clear();
        }
        // A descriptor naming a theme that no longer exists would be a lie:
        // drop it, but only when it names the deleted theme.
        if crate::color_authority::read_descriptor()
            .map(|d| d.theme == name)
            .unwrap_or(false)
        {
            if let Err(e) = crate::color_authority::clear_descriptor() {
                tracing::warn!("[themes] Cannot clear colour-authority descriptor: {e}");
            }
        }
        Ok(())
    }

    /// Best-effort removal of everything a theme derived that lives OUTSIDE
    /// its own directory: the gallery's content-keyed bake artifacts plus,
    /// through each registered provider's `deletable_artifacts`
    /// declaration, any shared artifact no surviving theme still references.
    /// Never touches anything beyond the bake cache and the declared paths;
    /// every failure is logged and swallowed.
    fn cleanup_derived_artifacts(
        themes_dir: &Path,
        theme_dir: &Path,
        deleted_name: &str,
        last_applied: &str,
        providers: &[Box<dyn ThemeProvider>],
    ) {
        cleanup_bake_cache(theme_dir);
        // W2-4: the deleted theme's palette may be the LIVE one — the
        // desktop is currently rendering from it. Removing it would pull the
        // colors out from under the running session, so it stays (a later
        // delete or apply reaps it once it is no longer live).
        if !last_applied.is_empty() && deleted_name == last_applied {
            tracing::info!(
                theme = %theme_dir.display(),
                "theme delete: keeping shared palette — the deleted theme is currently applied and its palette is live"
            );
            return;
        }
        cleanup_shared_artifacts(themes_dir, theme_dir, providers);
    }

    pub fn rename(&mut self, old_name: &str, new_name: &str) -> Result<(), String> {
        let old_name = old_name.trim();
        let new_name = Self::validate_name(new_name)?;
        if old_name == new_name {
            return Ok(());
        }
        let old_path = self.themes_dir.join(old_name);
        let new_path = self.themes_dir.join(&new_name);
        if !old_path.exists() {
            return Err(format!("Theme '{}' not found", old_name));
        }
        if new_path.exists() {
            return Err("name-exists".into());
        }
        fs::rename(&old_path, &new_path)
            .map_err(|e| format!("Failed to rename theme: {}", e))?;
        if self.last_applied == old_name {
            self.last_applied = new_name;
        }
        // Same lie as on delete: the old name no longer exists.
        if crate::color_authority::read_descriptor()
            .map(|d| d.theme == old_name)
            .unwrap_or(false)
        {
            if let Err(e) = crate::color_authority::clear_descriptor() {
                tracing::warn!("[themes] Cannot clear colour-authority descriptor: {e}");
            }
        }
        Ok(())
    }

    /// Startup repair for the colour-authority upgrade window
    /// (`odd/tasks/hve-capability-routing.md`, R2): a theme applied before
    /// the descriptor existed declares no colour authority until it is
    /// applied again. When a theme IS applied and the descriptor is ABSENT,
    /// rebuild it once from the theme's own saved state through the
    /// provider that owns it — never from a fresh apply, never from a
    /// backend CLI.
    ///
    /// Quiet by contract: an empty last-applied name, an existing
    /// descriptor, a missing or unreadable theme, an unregistered owner and
    /// a failed write are all logged and swallowed. It never clears a
    /// descriptor and can never fail startup. Idempotent: with the
    /// descriptor present the write is skipped entirely.
    pub fn restore_missing_colour_authority(&self) {
        // validate_name also rejects hostile names: this string comes from
        // the on-disk config and becomes a directory lookup.
        let Ok(name) = Self::validate_name(&self.last_applied) else {
            return;
        };
        if crate::color_authority::read_descriptor().is_some() {
            tracing::debug!(
                "[themes] Colour-authority descriptor already present; startup repair skipped"
            );
            return;
        }
        let theme_dir = self.themes_dir.join(&name);
        if !theme_dir.is_dir() {
            tracing::debug!(
                "[themes] No applied theme dir for '{name}'; colour-authority repair skipped"
            );
            return;
        }
        let meta: ThemeMeta = match fs::read_to_string(theme_dir.join("meta.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(meta) => meta,
            None => {
                tracing::debug!(
                    "[themes] Unreadable metadata for '{name}'; colour-authority repair skipped"
                );
                return;
            }
        };
        for id in &meta.providers {
            let Some(provider) = self.provider(id) else {
                continue;
            };
            let Some(authority) = provider.derive_colour_authority(&theme_dir) else {
                continue;
            };
            match crate::color_authority::write_descriptor(&authority) {
                Ok(()) => {
                    tracing::info!("[themes] Restored colour-authority descriptor for '{name}'");
                    return;
                }
                Err(e) => {
                    tracing::warn!("[themes] Cannot write colour-authority descriptor: {e}");
                    return;
                }
            }
        }
        tracing::debug!(
            "[themes] No registered provider claims colour authority for '{name}'; descriptor left absent"
        );
    }
}

/// Remove the gallery bake artifacts derived from a theme's preview source.
///
/// Planning is probe-only (reads small records, checks existence — never
/// spawns ffmpeg), so this is safe wherever delete runs. An `Image` plan
/// removes the content-keyed artifacts for those bytes; a `Video` plan
/// removes the cached frame itself plus the content-keyed artifacts baked
/// from it (keyed by the frame bytes, read only when the frame exists —
/// an uncached video baked nothing, so there is nothing to remove), plus
/// the static fallback's artifacts the worker bakes when extraction fails.
/// Content addressing makes a source shared with another theme a non-event
/// (worst case: one re-bake). A cached frame is removed only when it is
/// REALLY inside the bake cache (canonicalized containment, W2-2) and
/// carries the `-frame.png` name — never a user file elsewhere. Missing
/// files are silent no-ops; other failures warn and are swallowed.
fn cleanup_bake_cache(theme_dir: &Path) {
    use crate::shell::gallery::thumbs;
    let cache = thumbs::cache_dir();
    let Some(spec) = thumbs::plan_preview_source(theme_dir) else {
        return;
    };
    match spec {
        thumbs::PreviewSpec::Image(source) => {
            remove_content_artifacts(&cache, &source, theme_dir);
            remove_cached_frame(&cache, &source);
        }
        thumbs::PreviewSpec::Video {
            cached_frame, theme_dir, ..
        } => {
            remove_cached_frame(&cache, &cached_frame);
            // Artifacts baked from the frame are content-keyed: only the
            // frame bytes reveal the key. A frame that was never extracted
            // baked nothing, so a missing/unreadable frame just skips.
            if let Ok(bytes) = fs::read(&cached_frame) {
                remove_keyed_artifacts(&cache, &thumbs::content_cache_key_from_bytes(&bytes));
            }
            // The worker bakes the static chain when extraction fails, so a
            // theme whose video never decoded may still own cached PNGs.
            if let Some(fallback) = thumbs::static_fallback_source(&theme_dir) {
                remove_content_artifacts_quiet(&cache, &fallback);
            }
        }
    }
}

/// Read `source`, key its bytes the SAME way the bake does, and remove the
/// five artifacts for that key. Warns (naming theme and source) when the
/// source cannot be read — the primary planned source should exist.
fn remove_content_artifacts(cache: &Path, source: &Path, theme_dir: &Path) {
    use crate::shell::gallery::thumbs;
    let bytes = match fs::read(source) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                theme = %theme_dir.display(),
                source = %source.display(),
                "theme delete: cannot read preview source for cache cleanup: {e}"
            );
            return;
        }
    };
    remove_keyed_artifacts(cache, &thumbs::content_cache_key_from_bytes(&bytes));
}

/// Quiet variant for the secondary static-fallback cleanup: a video-only
/// theme legitimately has no static source, so `None`/unreadable is normal.
fn remove_content_artifacts_quiet(cache: &Path, source: &Path) {
    use crate::shell::gallery::thumbs;
    let Ok(bytes) = fs::read(source) else {
        return;
    };
    remove_keyed_artifacts(cache, &thumbs::content_cache_key_from_bytes(&bytes));
}

/// Remove the five content-keyed artifacts for one bake key. Missing files
/// are silent no-ops; other failures warn and are swallowed.
fn remove_keyed_artifacts(cache: &Path, key: &str) {
    for suffix in ["-thumb.png", "-hero.png", "-slat.png", "-slat-exp.png", "-frame.png"] {
        let artifact = cache.join(format!("{key}{suffix}"));
        if let Err(e) = fs::remove_file(&artifact) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    artifact = %artifact.display(),
                    "theme delete: cannot remove cached bake artifact: {e}"
                );
            }
        }
    }
}

/// Remove `candidate` only when it is REALLY a cached frame inside the bake
/// cache (W2-2): the file name must end with `-frame.png`, the candidate
/// itself must NOT be a symlink, its parent directory must canonicalize to
/// the canonicalized cache dir (a direct cache entry — never a user file
/// elsewhere), AND the canonicalized path must sit under the canonicalized
/// cache dir. `Path::starts_with` alone is component-wise and ignores `..`,
/// so a recorded `<cache>/../user/secret-frame.png` would otherwise delete a
/// real user file; and canonicalization follows a trailing symlink, so an
/// outside link pointing INTO the cache would otherwise resolve as "inside"
/// and the removal would delete the outside link entry — while a link inside
/// the cache could redirect the removal outside. Canonicalization failure,
/// symlink, or any parent mismatch fails safe: the file stays. Missing files
/// are silent no-ops.
fn remove_cached_frame(cache: &Path, candidate: &Path) {
    let is_frame_name = candidate
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("-frame.png"));
    if !is_frame_name {
        return;
    }
    // WHY (symlink): `candidate.canonicalize()` below follows a trailing
    // symlink, so without this guard an outside link to a cache target reads
    // as "inside" and `remove_file` deletes the outside entry — and an
    // inside link would let the cache delete on someone else's behalf.
    // `symlink_metadata` (not `metadata`) is what sees the link itself.
    let is_regular = match candidate.symlink_metadata() {
        Ok(meta) => !meta.file_type().is_symlink(),
        Err(_) => false,
    };
    if !is_regular || !path_inside_dir(cache, candidate) {
        return;
    }
    // WHY (parent): containment alone still authorizes nested or redirected
    // paths that resolve under the cache; cache entries are direct children,
    // so the parent must canonicalize to exactly the cache dir.
    let Some(parent) = candidate.parent() else {
        return;
    };
    let is_direct_child = match (cache.canonicalize(), parent.canonicalize()) {
        (Ok(cache), Ok(parent)) => parent == cache,
        _ => false,
    };
    if !is_direct_child {
        return;
    }
    if let Err(e) = fs::remove_file(candidate) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                artifact = %candidate.display(),
                "theme delete: cannot remove cached video frame: {e}"
            );
        }
    }
}

/// True only when `candidate` canonicalizes to a path under the
/// canonicalized `dir`. Fails CLOSED (false) when either side cannot be
/// canonicalized — e.g. the candidate does not exist (nothing to remove
/// anyway) — so an unresolvable path is never deleted.
fn path_inside_dir(dir: &Path, candidate: &Path) -> bool {
    let (Ok(dir), Ok(canon)) = (dir.canonicalize(), candidate.canonicalize()) else {
        return false;
    };
    canon.starts_with(dir)
}

/// Remove a shared artifact a provider declared for the deleted theme IFF
/// no surviving theme still references it.
///
/// The provider supplies its own knowledge only — the live path of the
/// artifact it owns plus the reference key its own record format produces
/// for it. The core owns the decision: it asks EVERY provider that could
/// declare — the registered list plus a read-only declarer the
/// registration router supplies for each provider id the theme itself
/// recorded — whether any other theme dir still declares that key, and
/// removes the file only when none does. A scan that cannot run keeps the
/// file too — deleting a maybe-shared artifact is worse than leaking it.
fn cleanup_shared_artifacts(
    themes_dir: &Path,
    theme_dir: &Path,
    providers: &[Box<dyn ThemeProvider>],
) {
    let recorded = recorded_declarers(theme_dir, providers);
    for provider in providers
        .iter()
        .map(|p| p.as_ref())
        .chain(recorded.iter().map(|p| p.as_ref()))
    {
        for artifact in provider.deletable_artifacts(theme_dir) {
            if shared_artifact_still_referenced(
                themes_dir,
                theme_dir,
                providers,
                &artifact.reference_key,
            ) {
                continue;
            }
            if let Err(e) = fs::remove_file(&artifact.path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        palette = %artifact.path.display(),
                        "theme delete: cannot remove unreferenced palette: {e}"
                    );
                }
            }
        }
    }
}

/// Read-only declaration instances for the provider ids `theme_dir`
/// RECORDED when it was saved but that are NOT in `registered`: a theme's
/// own record must stay visible to cleanup even when its backend is
/// inactive (or disabled) today, or its shared artifacts leak forever —
/// nothing reaps them later. Ids already registered are skipped (the same
/// knowledge is already asked); ids the router does not ship declare
/// nothing. Constructing a declarer is side-effect free by contract:
/// `deletable_artifacts` only reads the theme's own record.
fn recorded_declarers(
    theme_dir: &Path,
    registered: &[Box<dyn ThemeProvider>],
) -> Vec<Box<dyn ThemeProvider>> {
    let Ok(entries) = fs::read_dir(theme_dir.join("providers")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|id| !registered.iter().any(|p| p.id() == id))
        .filter_map(|id| crate::providers::declaration_provider(&id))
        .collect()
}

/// True when any theme dir other than the one being deleted still declares
/// `reference_key`. EVERY provider that could declare is asked for each
/// survivor — the registered list plus the read-only declarers of the
/// survivor's own recorded ids — and the key is matched ACROSS providers:
/// a record stored under a directory the owner never writes still
/// references the file (see `DeletableArtifact::reference_key` for why
/// that match is deliberately not scoped per provider). Every directory
/// except the deleted one is scanned — HVE's own Save UI accepts names
/// starting with `_` or `.`, so skipping those hid real referrers and
/// deleted artifacts survivors still needed. Unreadable state fails OPEN
/// (keep the artifact): an unscannable tree proves nothing about who still
/// references the file — including a survivor whose `providers` dir cannot
/// even be listed.
fn shared_artifact_still_referenced(
    themes_dir: &Path,
    deleted_dir: &Path,
    providers: &[Box<dyn ThemeProvider>],
    reference_key: &str,
) -> bool {
    let entries = match fs::read_dir(themes_dir) {
        Ok(e) => e,
        Err(_) => return true,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == deleted_dir || !path.is_dir() {
            continue;
        }
        if fs::read_dir(path.join("providers")).is_err() {
            // Fail open: this survivor might reference the artifact and we
            // cannot prove otherwise — keep it (leak beats breakage).
            return true;
        }
        let recorded = recorded_declarers(&path, providers);
        for provider in providers
            .iter()
            .map(|p| p.as_ref())
            .chain(recorded.iter().map(|p| p.as_ref()))
        {
            if provider
                .deletable_artifacts(&path)
                .iter()
                .any(|declared| declared.reference_key == reference_key)
            {
                return true;
            }
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Minimal provider used to control which ids `list()` sees as registered.
    struct StubProvider {
        id: &'static str,
    }

    impl ThemeProvider for StubProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name_key(&self) -> &str {
            self.id
        }
        fn icon(&self) -> &str {
            self.id
        }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::empty()
        }
    }

    /// Test-only provider for the provider-declared cleanup seam: it owns
    /// ONE artifact OUTSIDE the theme dir — `{artifact_dir}/{record}.bin` —
    /// declared from its own `record.txt` record. The record text IS the
    /// reference key: any other theme carrying the same record keeps the
    /// file alive. Deliberately not any real backend layout: this proves
    /// the core consults `deletable_artifacts` declarations instead of
    /// knowing a provider's directory name or record format.
    struct DeclaringProvider {
        id: &'static str,
        artifact_dir: PathBuf,
    }

    impl ThemeProvider for DeclaringProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name_key(&self) -> &str {
            self.id
        }
        fn icon(&self) -> &str {
            "◆"
        }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn deletable_artifacts(&self, theme_dir: &Path) -> Vec<DeletableArtifact> {
            let record = theme_dir.join("providers").join(self.id).join("record.txt");
            let Ok(key) = fs::read_to_string(&record) else {
                return Vec::new();
            };
            let key = key.trim().to_string();
            if key.is_empty() {
                return Vec::new();
            }
            vec![DeletableArtifact {
                path: self.artifact_dir.join(format!("{key}.bin")),
                reference_key: key,
            }]
        }
    }

    /// Test-only provider that declares its OWN shared artifact from its
    /// `{id}/source.txt` record (`custom <key>`) — the record shape a
    /// backend writes, spelled out here so the CORE stays free of it. The
    /// D1 regression needs a declarer for the FOREIGN provider directory a
    /// survivor's record lives in: the reference scan may only learn what
    /// providers declare, so a record under a non-owner provider dir is
    /// visible exactly when a provider for that id can declare it.
    struct SourceRecordProvider {
        id: &'static str,
        artifact_dir: PathBuf,
    }

    impl ThemeProvider for SourceRecordProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name_key(&self) -> &str {
            self.id
        }
        fn icon(&self) -> &str {
            "◆"
        }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn deletable_artifacts(&self, theme_dir: &Path) -> Vec<DeletableArtifact> {
            let record = theme_dir.join("providers").join(self.id).join("source.txt");
            let Ok(text) = fs::read_to_string(&record) else {
                return Vec::new();
            };
            let mut parts = text.split_whitespace();
            if parts.next() != Some("custom") {
                return Vec::new();
            }
            let key = parts.collect::<Vec<_>>().join(" ");
            if key.is_empty() {
                return Vec::new();
            }
            vec![DeletableArtifact {
                path: self.artifact_dir.join(format!("{key}.json")),
                reference_key: key,
            }]
        }
    }

    /// Hardening: the theme name becomes a directory name, and provider
    /// save DELETES stale artifacts inside it — so `".."` escaping the
    /// themes dir would delete outside the sandbox. It must be rejected;
    /// legitimate names keep passing and blank names stay rejected.
    #[test]
    fn validate_name_rejects_parent_traversal() {
        assert!(ThemeManager::validate_name("..").is_err());
        assert!(ThemeManager::validate_name("  ").is_err());
        assert_eq!(
            ThemeManager::validate_name("Joker").unwrap(),
            "Joker".to_string()
        );
    }

    /// Unit 1c1 seam: a provider that owns no colours inherits the default —
    /// a no-op `Ok`. This pins the default so a future edit cannot silently
    /// turn every colour-less provider into a failure.
    #[test]
    fn reassert_colours_defaults_to_noop() {
        let provider = StubProvider { id: "plain" };
        assert!(
            provider
                .reassert_colours(Path::new("/nonexistent-theme-dir"))
                .is_ok(),
            "the default reassert_colours must be a no-op Ok"
        );
    }

    /// Seed `{themes_dir}/{name}/meta.json` and return its path.
    fn seed_meta(
        themes_dir: &Path,
        name: &str,
        saved_at: &str,
        description: &str,
        ids: &[&str],
    ) -> PathBuf {
        let theme_dir = themes_dir.join(name);
        fs::create_dir_all(&theme_dir).unwrap();
        let meta = ThemeMeta {
            saved_at: saved_at.to_string(),
            description: description.to_string(),
            providers: ids.iter().map(|s| s.to_string()).collect(),
        };
        let meta_path = theme_dir.join("meta.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        meta_path
    }

    /// Regression: a theme saved before the `hyprland-settings` provider was
    /// removed from the product must reappear in the gallery. The migration
    /// strips the retired id, so the `list()` capability filter no longer
    /// mistakes a product removal for an unavailable provider.
    #[test]
    fn migration_lists_a_theme_that_lists_a_removed_provider() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let meta_path = seed_meta(
            &themes_dir,
            "RedTest",
            "2026-09-06T22:24:00.000Z",
            "keeper theme",
            &["noctalia-v5", "hve-presets", "hyprland-settings"],
        );

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(StubProvider { id: "noctalia-v5" }));
        tm.register_provider(Box::new(StubProvider { id: "hve-presets" }));

        let themes = tm.list().unwrap();
        let theme = themes
            .iter()
            .find(|t| t.name == "RedTest")
            .expect("a theme listing a removed provider id must still be listed");
        assert_eq!(theme.saved_at, "2026-09-06T22:24:00.000Z");
        assert_eq!(theme.providers, vec!["noctalia-v5", "hve-presets"]);

        // The on-disk meta was migrated, preserving saved_at/description and
        // the relative order of the remaining ids.
        let meta: ThemeMeta =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(meta.saved_at, "2026-09-06T22:24:00.000Z");
        assert_eq!(meta.description, "keeper theme");
        assert_eq!(meta.providers, vec!["noctalia-v5", "hve-presets"]);
    }

    /// Invariant: the `list()` filter still protects against a genuinely
    /// unavailable provider. The migration must not touch such a meta.
    #[test]
    fn theme_with_unavailable_provider_outside_the_removed_list_stays_hidden() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let meta_path = seed_meta(
            &themes_dir,
            "WallpaperOnly",
            "2026-09-06T22:24:00.000Z",
            "",
            &["mpvpaper"],
        );
        let before = fs::read_to_string(&meta_path).unwrap();
        let mtime_before = fs::metadata(&meta_path).unwrap().modified().unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(StubProvider { id: "hve-presets" }));

        assert!(
            tm.list().unwrap().is_empty(),
            "a theme for an unavailable provider must stay hidden"
        );
        assert_eq!(
            fs::read_to_string(&meta_path).unwrap(),
            before,
            "the migration must not rewrite a meta without a removed id"
        );
        // Regression guard: skipping the rewrite must mean *no write at all*.
        // A rewrite that happens to produce identical bytes would still churn
        // the mtime and is exactly what this catches.
        assert_eq!(
            fs::metadata(&meta_path).unwrap().modified().unwrap(),
            mtime_before,
            "the migration must not touch a meta without a removed id (mtime churn)"
        );
    }

    /// Hardening: the migration runs at every startup over every user meta, so
    /// a field written by a newer HVE (or by hand) must not be lost. The old
    /// typed `ThemeMeta` round-trip silently dropped unknown keys.
    #[test]
    fn migration_preserves_unknown_meta_fields() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let theme_dir = themes_dir.join("Future");
        fs::create_dir_all(&theme_dir).unwrap();
        let meta_path = theme_dir.join("meta.json");
        fs::write(
            &meta_path,
            r#"{
                "saved_at": "2026-09-06T22:24:00.000Z",
                "description": "hand edited",
                "providers": ["hve-presets", "hyprland-settings", "noctalia-v5"],
                "future_field": { "nested": [1, 2, 3] }
            }"#,
        )
        .unwrap();

        let _tm = ThemeManager::new(config_dir.path());

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(
            value["future_field"]["nested"],
            serde_json::json!([1, 2, 3]),
            "an unknown field must survive the migration intact"
        );
        assert_eq!(value["saved_at"], "2026-09-06T22:24:00.000Z");
        assert_eq!(value["description"], "hand edited");
        assert_eq!(
            value["providers"],
            serde_json::json!(["hve-presets", "noctalia-v5"])
        );
    }

    /// Hardening: the migration must scan exactly what `list()` scans. Hidden
    /// (`.`-prefixed) and backup (`_`-prefixed) directories are not themes, so
    /// even one listing a removed id must be left byte-identical.
    #[test]
    fn migration_ignores_hidden_and_backup_directories() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for name in [".Hidden", "_Backup"] {
            let meta_path = seed_meta(
                &themes_dir,
                name,
                "2026-09-06T22:24:00.000Z",
                "",
                &["hyprland-settings"],
            );
            let before = fs::read_to_string(&meta_path).unwrap();

            let _tm = ThemeManager::new(config_dir.path());

            assert_eq!(
                fs::read_to_string(&meta_path).unwrap(),
                before,
                "'{name}' must not be migrated: it is not a listed theme"
            );
        }
    }

    /// U2 (i): the Noctalia palette file is shared across themes, so delete
    /// must be reference-counted. Two themes carrying `custom JokerTheme`:
    /// deleting the first leaves the palette in place, deleting the last
    /// removes it.
    #[test]
    fn delete_removes_palette_only_when_last_theme_using_it_is_deleted() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("JokerTheme.json");
        fs::write(&palette, r#"{"name":"JokerTheme"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for name in ["ThemeOne", "ThemeTwo"] {
            seed_meta(&themes_dir, name, "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
            let v5 = themes_dir.join(name).join("providers").join("noctalia-v5");
            fs::create_dir_all(&v5).unwrap();
            fs::write(v5.join("source.txt"), "custom JokerTheme\n").unwrap();
        }
        // Phase 3: the layout knowledge moved into the provider, so the
        // manager must have it registered for any declaration to exist.
        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));

        tm.delete("ThemeOne").expect("delete must succeed");
        assert!(palette.exists(), "palette still referenced by ThemeTwo must stay");

        tm.delete("ThemeTwo").expect("delete must succeed");
        assert!(!palette.exists(), "palette with no surviving referrer must be removed");

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// U2 (ii): deleting a theme removes the cached bake PNGs derived from
    /// ITS preview source and leaves another theme's cached PNGs untouched.
    #[test]
    fn delete_removes_cached_bake_artifacts_for_its_source_only() {
        use crate::shell::gallery::thumbs;
        let _env = crate::test_utils::TempEnv::new();

        let scratch = TempDir::new().unwrap();
        let img_a = scratch.path().join("a.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([10u8, 20, 30, 255]))
            .save(&img_a)
            .unwrap();
        let img_b = scratch.path().join("b.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([40u8, 50, 60, 255]))
            .save(&img_b)
            .unwrap();

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for (name, img) in [("ThemeA", &img_a), ("ThemeB", &img_b)] {
            seed_meta(&themes_dir, name, "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
            let v5 = themes_dir.join(name).join("providers").join("noctalia-v5");
            fs::create_dir_all(&v5).unwrap();
            fs::write(v5.join("wallpaper.txt"), format!("{}\n", img.display())).unwrap();
        }

        let cache = thumbs::cache_dir();
        fs::create_dir_all(&cache).unwrap();
        let key_a =
            thumbs::content_cache_key_from_bytes(&fs::read(&img_a).unwrap());
        let key_b =
            thumbs::content_cache_key_from_bytes(&fs::read(&img_b).unwrap());
        assert_ne!(key_a, key_b, "distinct sources must key distinctly");
        let mut files_a = Vec::new();
        for suffix in ["-thumb.png", "-hero.png", "-slat.png", "-slat-exp.png", "-frame.png"] {
            let p = cache.join(format!("{key_a}{suffix}"));
            fs::write(&p, b"cached").unwrap();
            files_a.push(p);
        }
        let mut files_b = Vec::new();
        for suffix in ["-thumb.png", "-hero.png", "-slat.png", "-slat-exp.png"] {
            let p = cache.join(format!("{key_b}{suffix}"));
            fs::write(&p, b"cached").unwrap();
            files_b.push(p);
        }

        let mut tm = ThemeManager::new(config_dir.path());
        tm.delete("ThemeA").expect("delete must succeed");

        for p in &files_a {
            assert!(!p.exists(), "deleted theme's cache artifact must be gone: {}", p.display());
        }
        for p in &files_b {
            assert!(p.exists(), "surviving theme's cache artifact must stay: {}", p.display());
        }
        assert!(!themes_dir.join("ThemeA").exists(), "theme dir itself must be gone");
        assert!(themes_dir.join("ThemeB").exists(), "other theme dir must stay");
    }

    /// U2 (iii): every cleanup step is best-effort — a palette directory that
    /// cannot serve the removal (here a regular file, so the join is not a
    /// directory) must never turn the delete itself into an error.
    #[test]
    fn delete_succeeds_when_derived_cleanup_fails() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        // `palettes` as a FILE: any palette removal beneath it fails.
        fs::write(noct_home.path().join("palettes"), b"not a dir").unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Broken", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let v5 = themes_dir.join("Broken").join("providers").join("noctalia-v5");
        fs::create_dir_all(&v5).unwrap();
        fs::write(v5.join("source.txt"), "custom JokerTheme\n").unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.delete("Broken").expect("cleanup failure must not fail the delete");
        assert!(!themes_dir.join("Broken").exists(), "theme dir itself must be gone");

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// W2-2: a recorded source that is LEXICALLY under the bake cache but
    /// really lives elsewhere (`<cache>/../elsewhere/x-frame.png`) must
    /// survive the delete. `Path::starts_with` is component-wise and ignores
    /// `..`, so only canonicalized containment may authorize the removal.
    #[test]
    fn delete_keeps_user_file_outside_cache_with_dotdot_frame_name() {
        use crate::shell::gallery::thumbs;
        let _env = crate::test_utils::TempEnv::new();
        let cache = thumbs::cache_dir();
        fs::create_dir_all(&cache).unwrap();
        // Synthetic escape: lexically `<cache>/../elsewhere/x-frame.png`
        // (passes a naive starts_with AND the -frame.png name check) but
        // canonically a sibling of the cache — a real user file.
        let elsewhere = cache.join("../elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let user_file = elsewhere.join("x-frame.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1u8, 2, 3, 255]))
            .save(&user_file)
            .unwrap();

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Escaper", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let v5 = themes_dir.join("Escaper").join("providers").join("noctalia-v5");
        fs::create_dir_all(&v5).unwrap();
        fs::write(v5.join("wallpaper.txt"), format!("{}\n", user_file.display())).unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.delete("Escaper").expect("delete must succeed");
        assert!(
            user_file.exists(),
            "a user file outside the bake cache must survive the delete, even named *-frame.png"
        );
        assert!(!themes_dir.join("Escaper").exists(), "theme dir itself must be gone");
    }

    /// W2-2 unit: containment itself. A `..` escape is not inside, a genuine
    /// child is, and an unresolvable candidate fails CLOSED (false).
    #[test]
    fn path_inside_dir_rejects_dotdot_escape_and_fails_closed() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let user_file = elsewhere.join("x-frame.png");
        fs::write(&user_file, b"user").unwrap();
        let escaped = cache.join("../elsewhere/x-frame.png");
        assert!(
            !path_inside_dir(&cache, &escaped),
            "a canonicalized-sibling path must not count as inside"
        );
        let child = cache.join("k-frame.png");
        fs::write(&child, b"cached").unwrap();
        assert!(path_inside_dir(&cache, &child), "a genuine child must count as inside");
        assert!(
            !path_inside_dir(&cache, &cache.join("missing-frame.png")),
            "an unresolvable candidate must fail closed"
        );
    }

    /// W2-3a: a survivor whose `source.txt` separates `custom` from the name
    /// with a TAB still references the palette — deleting the other theme
    /// must keep the file.
    #[test]
    fn delete_keeps_palette_when_survivor_uses_tab_separator() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("JokerTheme.json");
        fs::write(&palette, r#"{"name":"JokerTheme"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Gone", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let gone_v5 = themes_dir.join("Gone").join("providers").join("noctalia-v5");
        fs::create_dir_all(&gone_v5).unwrap();
        fs::write(gone_v5.join("source.txt"), "custom JokerTheme\n").unwrap();
        seed_meta(&themes_dir, "Stays", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let stays_v5 = themes_dir.join("Stays").join("providers").join("noctalia-v5");
        fs::create_dir_all(&stays_v5).unwrap();
        fs::write(stays_v5.join("source.txt"), "custom\tJokerTheme\n").unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.delete("Gone").expect("delete must succeed");
        assert!(
            palette.exists(),
            "a survivor separating with TAB still references the palette — it must stay"
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// W2-3b: a survivor living in a `_`-prefixed dir (which HVE's own Save
    /// UI can create) still references the palette — it must be scanned.
    #[test]
    fn delete_keeps_palette_when_survivor_dir_starts_with_underscore() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("JokerTheme.json");
        fs::write(&palette, r#"{"name":"JokerTheme"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Gone", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let gone_v5 = themes_dir.join("Gone").join("providers").join("noctalia-v5");
        fs::create_dir_all(&gone_v5).unwrap();
        fs::write(gone_v5.join("source.txt"), "custom JokerTheme\n").unwrap();
        seed_meta(&themes_dir, "_Backup", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let stays_v5 = themes_dir.join("_Backup").join("providers").join("noctalia-v5");
        fs::create_dir_all(&stays_v5).unwrap();
        fs::write(stays_v5.join("source.txt"), "custom JokerTheme\n").unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.delete("Gone").expect("delete must succeed");
        assert!(
            palette.exists(),
            "a survivor in a `_`-prefixed dir still references the palette — it must stay"
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// W2-3c: a survivor whose `providers` dir cannot be listed might still
    /// reference the palette — the scan fails OPEN and keeps the file.
    #[test]
    fn delete_keeps_palette_when_survivor_providers_dir_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("JokerTheme.json");
        fs::write(&palette, r#"{"name":"JokerTheme"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Gone", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let gone_v5 = themes_dir.join("Gone").join("providers").join("noctalia-v5");
        fs::create_dir_all(&gone_v5).unwrap();
        fs::write(gone_v5.join("source.txt"), "custom JokerTheme\n").unwrap();
        seed_meta(&themes_dir, "Locked", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let locked_prov = themes_dir.join("Locked").join("providers");
        fs::create_dir_all(locked_prov.join("noctalia-v5")).unwrap();
        fs::write(locked_prov.join("noctalia-v5").join("source.txt"), "custom JokerTheme\n").unwrap();
        fs::set_permissions(&locked_prov, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked_prov).is_ok() {
            // Permissive environment (e.g. running as root): the fixture
            // cannot be made unreadable, so there is nothing to prove here.
            fs::set_permissions(&locked_prov, fs::Permissions::from_mode(0o755)).unwrap();
            match old_noctalia {
                Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
                None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
            }
            println!("unreadable-dir fixture impossible here — skipping");
            return;
        }

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.delete("Gone").expect("delete must succeed");
        fs::set_permissions(&locked_prov, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            palette.exists(),
            "an unscannable survivor fails OPEN — the palette must stay"
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// W2-4: deleting the CURRENTLY APPLIED theme must not remove the LIVE
    /// palette the desktop is rendering from — it stays with a log line.
    #[test]
    fn delete_applied_theme_keeps_live_palette() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("JokerTheme.json");
        fs::write(&palette, r#"{"name":"JokerTheme"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Live", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let v5 = themes_dir.join("Live").join("providers").join("noctalia-v5");
        fs::create_dir_all(&v5).unwrap();
        fs::write(v5.join("source.txt"), "custom JokerTheme\n").unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.last_applied = "Live".to_string();
        tm.delete("Live").expect("delete must succeed");
        assert!(
            palette.exists(),
            "the live palette of the applied theme must survive its delete"
        );
        assert!(!themes_dir.join("Live").exists(), "theme dir itself must be gone");
        assert!(tm.last_applied.is_empty(), "last_applied still clears");

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// Capability-routing Phase 3: the core cleans up ONLY what a provider
    /// declares through `deletable_artifacts` — no provider layout lives in
    /// the core. The fake provider declares `<sandbox>/<key>.bin` from its
    /// own record: an artifact another theme still declares survives the
    /// delete, and the one no surviving theme declares anymore is removed
    /// as soon as its last declarer is gone.
    #[test]
    fn delete_cleans_provider_declared_artifacts_by_declaration() {
        let _env = crate::test_utils::TempEnv::new();
        let artifacts = TempDir::new().unwrap();
        let shared = artifacts.path().join("shared-key.bin");
        let orphan = artifacts.path().join("orphan-key.bin");
        fs::write(&shared, b"shared").unwrap();
        fs::write(&orphan, b"orphan").unwrap();

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for (name, key) in [
            ("Gone", "shared-key"),
            ("Stays", "shared-key"),
            ("Solo", "orphan-key"),
        ] {
            seed_meta(
                &themes_dir,
                name,
                "2026-09-21T00:00:00.000Z",
                "",
                &["declares-stuff"],
            );
            let prov = themes_dir.join(name).join("providers").join("declares-stuff");
            fs::create_dir_all(&prov).unwrap();
            fs::write(prov.join("record.txt"), key).unwrap();
        }

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(DeclaringProvider {
            id: "declares-stuff",
            artifact_dir: artifacts.path().to_path_buf(),
        }));

        tm.delete("Gone").expect("delete must succeed");
        assert!(
            shared.exists(),
            "an artifact a surviving theme still declares must stay"
        );
        assert!(
            orphan.exists(),
            "an artifact another theme still declares must stay too"
        );

        tm.delete("Solo").expect("delete must succeed");
        assert!(
            !orphan.exists(),
            "an unreferenced declared artifact must be removed"
        );
        assert!(shared.exists(), "the survivor's artifact must stay untouched");

        tm.delete("Stays").expect("delete must succeed");
        assert!(
            !shared.exists(),
            "once the last declarer is gone, the declared artifact must go"
        );
    }

    /// Fail-open through the generic declaration scan: a survivor whose
    /// `providers` tree cannot be listed might still declare the artifact —
    /// the core keeps it (leak beats breakage), the same policy the record
    /// scan had before the cleanup moved behind the seam.
    #[test]
    fn delete_keeps_declared_artifact_when_survivor_providers_dir_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        let _env = crate::test_utils::TempEnv::new();
        let artifacts = TempDir::new().unwrap();
        let shared = artifacts.path().join("shared-key.bin");
        fs::write(&shared, b"shared").unwrap();

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for name in ["Gone", "Locked"] {
            seed_meta(
                &themes_dir,
                name,
                "2026-09-21T00:00:00.000Z",
                "",
                &["declares-stuff"],
            );
            let prov = themes_dir.join(name).join("providers").join("declares-stuff");
            fs::create_dir_all(&prov).unwrap();
            fs::write(prov.join("record.txt"), "shared-key").unwrap();
        }
        let locked_prov = themes_dir.join("Locked").join("providers");
        fs::set_permissions(&locked_prov, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked_prov).is_ok() {
            // Permissive environment (e.g. running as root): the fixture
            // cannot be made unreadable, so there is nothing to prove here.
            fs::set_permissions(&locked_prov, fs::Permissions::from_mode(0o755)).unwrap();
            println!("unreadable-dir fixture impossible here — skipping");
            return;
        }

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(DeclaringProvider {
            id: "declares-stuff",
            artifact_dir: artifacts.path().to_path_buf(),
        }));

        tm.delete("Gone").expect("delete must succeed");
        fs::set_permissions(&locked_prov, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            shared.exists(),
            "an unscannable survivor fails OPEN — the declared artifact must stay"
        );
    }

    /// D1 regression (adversarial verification of `3e0fcc1`): the
    /// reference scan must see a survivor's record stored under a provider
    /// directory that does NOT own the artifact. The pre-seam code scanned
    /// every survivor record; the seam's owner-only scan deleted a palette
    /// a survivor still referenced — the dangerous direction. A matching
    /// key declared through ANY provider keeps the file: a false keep only
    /// leaks a file, a false miss deletes one a live theme needs.
    #[test]
    fn delete_keeps_palette_referenced_through_a_non_owner_provider_dir() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("K2.json");
        fs::write(&palette, r#"{"name":"K2"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Gone", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let gone_v5 = themes_dir.join("Gone").join("providers").join("noctalia-v5");
        fs::create_dir_all(&gone_v5).unwrap();
        fs::write(gone_v5.join("source.txt"), "custom K2\n").unwrap();
        // The survivor's record lives under a NON-owner provider dir.
        seed_meta(&themes_dir, "Stays", "2026-09-21T00:00:00.000Z", "", &["noctalia"]);
        let stays_other = themes_dir.join("Stays").join("providers").join("noctalia");
        fs::create_dir_all(&stays_other).unwrap();
        fs::write(stays_other.join("source.txt"), "custom K2\n").unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.register_provider(Box::new(SourceRecordProvider {
            id: "noctalia",
            artifact_dir: palettes_dir.clone(),
        }));

        tm.delete("Gone").expect("delete must succeed");
        assert!(
            palette.exists(),
            "a survivor's record under a NON-owner provider dir still references the palette — it must stay"
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// D2 regression (adversarial verification of `3e0fcc1`): the deleted
    /// theme's OWN declaration must not depend on its backend being
    /// registered. The pre-seam code read the record unconditionally; the
    /// seam's registered-only scan leaked the palette whenever the backend
    /// was inactive — the safe direction, but a real behaviour divergence
    /// and a PERMANENT leak, since nothing ever reaps it later. Chosen
    /// behaviour: REMOVE it. The theme's own `providers/` tree records
    /// which backend's knowledge applies, and the registration router
    /// supplies a read-only declarer for that id without registering the
    /// backend for save/apply. The invariant is untouched: the survivor
    /// scan asks the same declarers, so an artifact a survivor still
    /// references is never deleted.
    #[test]
    fn delete_removes_palette_when_the_owning_provider_is_not_registered() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("K3.json");
        fs::write(&palette, r#"{"name":"K3"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_meta(&themes_dir, "Gone", "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
        let gone_v5 = themes_dir.join("Gone").join("providers").join("noctalia-v5");
        fs::create_dir_all(&gone_v5).unwrap();
        fs::write(gone_v5.join("source.txt"), "custom K3\n").unwrap();

        // NO provider is registered: the theme's recorded backend is inactive.
        let mut tm = ThemeManager::new(config_dir.path());
        tm.delete("Gone").expect("delete must succeed");
        assert!(
            !palette.exists(),
            "an unreferenced palette whose owning backend is not registered must still be removed — the theme's own record decides, not the registration state"
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// Invariant guard for the D2 fix's NEW deletion authority: when the
    /// owning backend is not registered the core may now declare the
    /// deleted theme's artifact through the router's read-only declarer,
    /// so the survivor scan MUST ask the same declarers — otherwise a
    /// survivor recording the same key through the same inactive backend
    /// would lose the file. An artifact a survivor still references is
    /// NEVER deleted, registered backend or not.
    #[test]
    fn delete_keeps_palette_when_survivor_references_through_an_unregistered_provider() {
        let _env = crate::test_utils::TempEnv::new();
        let noct_home = TempDir::new().unwrap();
        let palettes_dir = noct_home.path().join("palettes");
        fs::create_dir_all(&palettes_dir).unwrap();
        let palette = palettes_dir.join("K4.json");
        fs::write(&palette, r#"{"name":"K4"}"#).unwrap();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for name in ["Gone", "Stays"] {
            seed_meta(&themes_dir, name, "2026-09-21T00:00:00.000Z", "", &["noctalia-v5"]);
            let v5 = themes_dir.join(name).join("providers").join("noctalia-v5");
            fs::create_dir_all(&v5).unwrap();
            fs::write(v5.join("source.txt"), "custom K4\n").unwrap();
        }

        // NO provider is registered: both themes recorded an inactive backend.
        let mut tm = ThemeManager::new(config_dir.path());
        tm.delete("Gone").expect("delete must succeed");
        assert!(
            palette.exists(),
            "a survivor recording the same key through the SAME inactive backend still references the palette — it must stay"
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// Symlink hardening: an OUTSIDE link whose target lives INSIDE the cache
    /// canonicalizes as "inside" (canonicalization follows the trailing
    /// symlink), so containment alone authorizes deleting the outside link
    /// entry. Only a real non-symlink direct child of the cache may be
    /// removed; a genuine `<key>-frame.png` cache entry still is.
    #[test]
    fn remove_cached_frame_keeps_outside_symlink_but_removes_genuine_entry() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();

        // Outside link -> real file INSIDE the cache. The link name passes
        // the `-frame.png` gate and canonicalizes into the cache.
        let target = cache.join("real-frame.png");
        fs::write(&target, b"frame").unwrap();
        let outlink = outside.join("outlink-frame.png");
        std::os::unix::fs::symlink(&target, &outlink).unwrap();

        // Inside link -> real file OUTSIDE the cache. Removing through it
        // would delete the link entry on the cache's behalf.
        let user_file = outside.join("user.png");
        fs::write(&user_file, b"user").unwrap();
        let inlink = cache.join("inlink-frame.png");
        std::os::unix::fs::symlink(&user_file, &inlink).unwrap();

        remove_cached_frame(&cache, &outlink);
        remove_cached_frame(&cache, &inlink);

        assert!(
            fs::symlink_metadata(&outlink).is_ok(),
            "an outside symlink to a cache target must survive the delete"
        );
        assert!(
            target.exists(),
            "the cache target behind an outside link must survive the delete"
        );
        assert!(
            fs::symlink_metadata(&inlink).is_ok(),
            "a symlink inside the cache must survive the delete"
        );
        assert!(
            user_file.exists(),
            "the outside target of an inside link must survive the delete"
        );

        // Inverse guard: a genuine cache entry IS removed.
        let genuine = cache.join("abc123-frame.png");
        fs::write(&genuine, b"frame").unwrap();
        remove_cached_frame(&cache, &genuine);
        assert!(
            !genuine.exists(),
            "a genuine non-symlink cache entry named *-frame.png must be removed"
        );
    }

    // ── Colour-authority startup repair (capability-routing R2) ──────

    /// Seed a theme that carries the colour-owning provider's saved state
    /// (source line + snapshot) — every theme applied before the descriptor
    /// existed looks exactly like this. Returns the provider dir.
    fn seed_colours_theme(
        themes_dir: &Path,
        name: &str,
        source: &str,
        with_snapshot: bool,
    ) -> PathBuf {
        seed_meta(
            themes_dir,
            name,
            "2026-09-20T00:00:00.000Z",
            "",
            &["noctalia-v5"],
        );
        let provider_dir = themes_dir.join(name).join("providers").join("noctalia-v5");
        fs::create_dir_all(&provider_dir).unwrap();
        fs::write(provider_dir.join("source.txt"), source).unwrap();
        if with_snapshot {
            fs::write(provider_dir.join("palette.json"), r#"{"palette":"demo"}"#).unwrap();
        }
        provider_dir
    }

    /// Build the manager a fresh startup would build, run the repair, and
    /// assert the descriptor stayed absent — the quiet no-op contract.
    fn repair_and_expect_noop(config_dir: &Path, last_applied: &str, register_provider: bool) {
        let mut tm = ThemeManager::new(config_dir);
        if register_provider {
            tm.register_provider(Box::new(
                crate::providers::noctalia::NoctaliaV5Provider::new(),
            ));
        }
        tm.last_applied = last_applied.to_string();

        tm.restore_missing_colour_authority();

        assert!(
            crate::color_authority::read_descriptor().is_none(),
            "repair must be a quiet no-op (last_applied={last_applied:?}, \
             provider registered={register_provider})"
        );
    }

    /// The upgrade window: a theme applied before the descriptor existed
    /// declares no colour authority until it is applied again. Startup must
    /// close that window from the state ALREADY on disk — the theme's own
    /// saved snapshot and its recorded palette name — never from a fresh
    /// apply.
    #[test]
    fn restore_missing_colour_authority_rebuilds_it_from_saved_state() {
        let _env = crate::test_utils::TempEnv::new();
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let provider_dir = seed_colours_theme(&themes_dir, "Legacy", "custom LegacyPal\n", true);

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.last_applied = "Legacy".to_string();

        assert!(
            crate::color_authority::read_descriptor().is_none(),
            "precondition: the upgrade window is open"
        );

        tm.restore_missing_colour_authority();

        assert_eq!(
            crate::color_authority::read_descriptor(),
            Some(crate::color_authority::ColorAuthority {
                backend: "noctalia-v5".to_string(),
                theme: "Legacy".to_string(),
                palette_file: provider_dir.join("palette.json").display().to_string(),
                palette_name: "LegacyPal".to_string(),
            }),
            "the descriptor must be rebuilt from the theme's own saved snapshot"
        );
    }

    /// Repair runs only while the descriptor is ABSENT: a second run writes
    /// nothing (not even the same bytes), and an existing descriptor — even
    /// one that disagrees with the saved state — is never rewritten or
    /// cleared by startup.
    #[test]
    fn restore_missing_colour_authority_skips_an_existing_descriptor_and_is_idempotent() {
        let _env = crate::test_utils::TempEnv::new();
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        seed_colours_theme(&themes_dir, "Legacy", "custom LegacyPal\n", true);

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(
            crate::providers::noctalia::NoctaliaV5Provider::new(),
        ));
        tm.last_applied = "Legacy".to_string();

        tm.restore_missing_colour_authority();
        let path = crate::color_authority::descriptor_path();
        let bytes = fs::read(&path).expect("the repair must have written a descriptor");
        let stamped = fs::metadata(&path).unwrap().modified().unwrap();

        tm.restore_missing_colour_authority();

        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "a second run must leave byte-identical content"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            stamped,
            "a second run must SKIP the write, not rewrite the same bytes"
        );

        // Existing-but-different wins: startup repairs the ABSENT case only.
        let existing = crate::color_authority::ColorAuthority {
            backend: "something-else".to_string(),
            theme: "Other".to_string(),
            palette_file: "/tmp/other.json".to_string(),
            palette_name: "OtherPal".to_string(),
        };
        crate::color_authority::write_descriptor(&existing).unwrap();
        tm.restore_missing_colour_authority();
        assert_eq!(
            crate::color_authority::read_descriptor(),
            Some(existing),
            "startup must never rewrite or clear a descriptor that exists"
        );
    }

    /// Missing state is a quiet no-op in every shape it takes: nothing
    /// applied, the theme gone, unreadable metadata, the owner not
    /// registered, no usable snapshot, or a hostile last-applied name. No
    /// descriptor written, nothing cleared, nothing panicked.
    #[test]
    fn restore_missing_colour_authority_is_a_quiet_noop_when_state_is_missing() {
        let _env = crate::test_utils::TempEnv::new();
        assert!(crate::color_authority::read_descriptor().is_none());

        // (a) nothing applied at all
        let cfg = TempDir::new().unwrap();
        repair_and_expect_noop(cfg.path(), "", true);

        // (b) the applied theme is gone from disk
        let cfg = TempDir::new().unwrap();
        repair_and_expect_noop(cfg.path(), "Ghost", true);

        // (c) a theme dir without a readable meta.json
        let cfg = TempDir::new().unwrap();
        fs::create_dir_all(cfg.path().join("hve").join("themes").join("NoMeta")).unwrap();
        repair_and_expect_noop(cfg.path(), "NoMeta", true);

        // (d) the colour-owning provider is not registered
        let cfg = TempDir::new().unwrap();
        seed_colours_theme(
            &cfg.path().join("hve").join("themes"),
            "Unregistered",
            "custom Demo\n",
            true,
        );
        repair_and_expect_noop(cfg.path(), "Unregistered", false);

        // (e) a builtin scheme carries no snapshot of ours
        let cfg = TempDir::new().unwrap();
        seed_colours_theme(
            &cfg.path().join("hve").join("themes"),
            "Builtin",
            "builtin Ocean\n",
            true,
        );
        repair_and_expect_noop(cfg.path(), "Builtin", true);

        // (f) a custom source with no snapshot on disk
        let cfg = TempDir::new().unwrap();
        seed_colours_theme(
            &cfg.path().join("hve").join("themes"),
            "NoSnapshot",
            "custom Demo\n",
            false,
        );
        repair_and_expect_noop(cfg.path(), "NoSnapshot", true);

        // (g) a hostile last-applied name never escapes the themes dir
        let cfg = TempDir::new().unwrap();
        repair_and_expect_noop(cfg.path(), "..", true);
    }

    /// The new hook defaults to "this provider owns no colours", the same
    /// shape as `reassert_colours`' default: a colour-less provider stays
    /// silent without writing an override.
    #[test]
    fn derive_colour_authority_defaults_to_none() {
        let provider = StubProvider { id: "plain" };
        assert!(
            provider
                .derive_colour_authority(Path::new("/nonexistent-theme-dir"))
                .is_none(),
            "the default derive_colour_authority must be None"
        );
    }

    /// The declaration hook defaults to "this provider owns nothing outside
    /// its theme dir", the same shape as `reassert_colours` and
    /// `derive_colour_authority`: a provider that declares nothing keeps the
    /// delete path away from its (nonexistent) artifacts without writing an
    /// override.
    #[test]
    fn deletable_artifacts_defaults_to_empty() {
        let provider = StubProvider { id: "plain" };
        assert!(
            provider
                .deletable_artifacts(Path::new("/nonexistent-theme-dir"))
                .is_empty(),
            "the default deletable_artifacts must declare nothing"
        );
    }

    /// The `read-preview-source` hook defaults to "this provider supplies
    /// no preview record", the same shape as `deletable_artifacts`: a
    /// provider whose theme carries no preview declares nothing and the
    /// gallery falls through to its own loose scan, without an override.
    #[test]
    fn preview_sources_defaults_to_empty() {
        let provider = StubProvider { id: "plain" };
        assert!(
            provider
                .preview_sources(Path::new("/nonexistent-provider-dir"))
                .is_empty(),
            "the default preview_sources must declare nothing"
        );
    }
}
