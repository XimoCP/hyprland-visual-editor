use crate::config::Config;
use crate::engine::{Engine, PresetInfo};
use crate::preset_store::PresetStore;
use crate::tr::Tr;
use slint::{ModelRc, SharedString};

/// Translate a list of presets: resolve i18n_key → Tr, fallback to raw value.
fn translate_presets(items: &[PresetInfo], tr: &Tr) -> (Vec<SharedString>, Vec<SharedString>) {
    let titles: Vec<SharedString> = items
        .iter()
        .map(|i| SharedString::from(tr.tr_or(&i.i18n_title, &i.raw_title)))
        .collect();
    let descs: Vec<SharedString> = items
        .iter()
        .map(|i| SharedString::from(tr.tr_or(&i.i18n_desc, &i.raw_desc)))
        .collect();
    (titles, descs)
}

/// Resolve the active index for a stored file inside a preset list. `-1`
/// means "nothing is active": either the stored file is empty, or it matches
/// no entry (a renamed/removed preset — a real state, not an error). It is
/// never `None`: callers must write an explicit index.
///
/// The comparison is TOLERANT ([`PresetStore::preset_files_match`]): a theme
/// saved before the canonical form stores the bare stem (`Space`) while the
/// scan list holds `Space.lua`, and that stem has to resolve to its card
/// instead of being dropped by an exact-equality miss.
fn resolve_active_index(items: &[PresetInfo], active_file: &str) -> i32 {
    if active_file.is_empty() {
        return -1;
    }
    items
        .iter()
        .position(|i| PresetStore::preset_files_match(active_file, &i.file))
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// Collect tags/files from presets into Slint models, and set the active index.
fn populate_metadata(
    items: &[PresetInfo],
    active_file: &str,
    set_tags: impl FnOnce(ModelRc<SharedString>),
    set_files: impl FnOnce(ModelRc<SharedString>),
    set_active_index: impl FnOnce(i32),
) {
    set_tags(ModelRc::from(
        items
            .iter()
            .map(|i| SharedString::from(&i.tag))
            .collect::<Vec<_>>()
            .as_slice(),
    ));
    set_files(ModelRc::from(
        items
            .iter()
            .map(|i| SharedString::from(&i.file))
            .collect::<Vec<_>>()
            .as_slice(),
    ));

    // ALWAYS write an explicit index. Skipping the write when the stored file
    // matched nothing left a stale marker outliving its preset list: the board
    // kept pointing at a preset that was not the loaded one, silently. `-1`
    // plus the warning below makes the mismatch visible instead.
    let index = resolve_active_index(items, active_file);
    if index < 0 && !active_file.is_empty() {
        tracing::warn!(
            "[presets] active file '{}' matches no scanned preset — marker cleared",
            active_file
        );
    }
    set_active_index(index);
}

pub fn populate_presets(window: &crate::MainWindow, engine: &Engine, cfg: &Config, tr: &Tr) {
    let animations = engine.scan("animations").unwrap_or_default();
    let borders = engine.scan("borders").unwrap_or_default();
    let shaders = engine.scan("shaders").unwrap_or_default();

    // ── Translated titles + descriptions ──
    let (anim_titles, anim_descs) = translate_presets(&animations, tr);
    window.set_anim_titles(ModelRc::from(anim_titles.as_slice()));
    window.set_anim_descs(ModelRc::from(anim_descs.as_slice()));

    let (border_titles, border_descs) = translate_presets(&borders, tr);
    window.set_border_titles(ModelRc::from(border_titles.as_slice()));
    window.set_border_descs(ModelRc::from(border_descs.as_slice()));

    let (shader_titles, shader_descs) = translate_presets(&shaders, tr);
    window.set_shader_titles(ModelRc::from(shader_titles.as_slice()));
    window.set_shader_descs(ModelRc::from(shader_descs.as_slice()));

    // ── Tags, files, active index ──
    populate_metadata(
        &animations,
        &cfg.active_anim_file,
        |v| window.set_anim_tags(v),
        |v| window.set_anim_files(v),
        |i| window.set_active_anim_index(i),
    );
    populate_metadata(
        &borders,
        &cfg.active_border_file,
        |v| window.set_border_tags(v),
        |v| window.set_border_files(v),
        |i| window.set_active_border_index(i),
    );
    populate_metadata(
        &shaders,
        &cfg.active_shader_file,
        |v| window.set_shader_tags(v),
        |v| window.set_shader_files(v),
        |i| window.set_active_shader_index(i),
    );

    // ── User-created presets (saved to ~/.config/hve/presets/) ──
    let border_store = crate::preset_store::PresetStore::new("borders");
    let user_border_names: Vec<SharedString> = border_store
        .list()
        .into_iter()
        .map(SharedString::from)
        .collect();
    let user_border_tags: Vec<SharedString> = user_border_names
        .iter()
        .map(|_| SharedString::from("CUSTOM"))
        .collect();
    window.set_user_border_preset_names(ModelRc::from(user_border_names.as_slice()));
    window.set_user_border_preset_tags(ModelRc::from(user_border_tags.as_slice()));

    let anim_store = crate::preset_store::PresetStore::new("animations");
    let user_anim_names: Vec<SharedString> = anim_store
        .list()
        .into_iter()
        .map(SharedString::from)
        .collect();
    let user_anim_tags: Vec<SharedString> = user_anim_names
        .iter()
        .map(|_| SharedString::from("CUSTOM"))
        .collect();
    window.set_user_animation_preset_names(ModelRc::from(user_anim_names.as_slice()));
    window.set_user_animation_preset_tags(ModelRc::from(user_anim_tags.as_slice()));
}

/// Re-seed `active-border-index` from the persisted `active_border_file`
/// against the window's CURRENT border list. Called every time the Borders
/// section/panel opens — not only at startup and on apply — so the marker
/// always reflects what the app actually has loaded, including when the index
/// was never seeded or was seeded before the preset list existed. A stored
/// value that matches no preset becomes an explicit -1 and logs a warning: a
/// blank board is honest, a silently blank one is not. Returns the resulting
/// index (for tracing and tests).
///
/// The list it resolves against is the panel's COMBINED one: the scanned
/// built-in files first, then the user preset stems, whose cards continue the
/// same index space. A USER preset stored as a bare stem (the keeper's Space
/// theme records `"Space"` while the file is `borders/Space.lua`) must mark
/// its user card, not vanish — see [`combined_active_index`].
pub fn reseed_active_border_index(window: &crate::MainWindow, active_file: &str) -> i32 {
    let index = combined_active_index(
        &window.get_border_files(),
        &window.get_user_border_preset_names(),
        active_file,
    );
    if index < 0 && !active_file.is_empty() {
        tracing::warn!(
            "[borders] active_border_file '{}' matches no preset — marker cleared",
            active_file
        );
    }
    window.set_active_border_index(index);
    index
}

/// The Motion twin of [`reseed_active_border_index`]: re-seed
/// `active-anim-index` from the persisted `active_anim_file` against the Motion
/// panel's CURRENT list.
///
/// The Motion panel continues the same index space as Borders
/// (`PanelRoot.motion-list-len` = scanned `anim-files` + `user-animation-preset-
/// names`, and its card click reports `motion-focused-index`), so a USER
/// animation saved from the tune pane — stored as a bare stem — has to mark its
/// user card rather than leaving the marker where the last click put it. The
/// resolution is the same tolerant, combined one `combined_active_index`
/// performs for Borders; only the models differ.
pub fn reseed_active_anim_index(window: &crate::MainWindow, active_file: &str) -> i32 {
    let index = combined_active_index(
        &window.get_anim_files(),
        &window.get_user_animation_preset_names(),
        active_file,
    );
    if index < 0 && !active_file.is_empty() {
        tracing::warn!(
            "[motion] active_anim_file '{}' matches no preset — marker cleared",
            active_file
        );
    }
    window.set_active_anim_index(index);
    index
}

/// The index of `active_file` inside the Borders panel's COMBINED card list:
/// the scanned built-in files first, then the user preset stems, offset by the
/// built-in count (`PanelRoot.borders-list-len` builds the same list and the
/// card click reports the same index space). The comparison is tolerant on
/// both halves, so a legacy bare stem resolves whether the preset is built-in
/// or the user's own. `-1` when the reference is empty or matches nothing.
fn combined_active_index(
    builtin: &ModelRc<SharedString>,
    user_names: &ModelRc<SharedString>,
    active_file: &str,
) -> i32 {
    use slint::Model as _;

    fn find(files: &ModelRc<SharedString>, active_file: &str) -> Option<i32> {
        (0..files.row_count()).find_map(|i| {
            files
                .row_data(i)
                .as_ref()
                .map(|f| PresetStore::preset_files_match(active_file, f.as_str()))
                .unwrap_or(false)
                .then_some(i as i32)
        })
    }

    if active_file.is_empty() {
        return -1;
    }
    if let Some(index) = find(builtin, active_file) {
        return index;
    }
    find(user_names, active_file)
        .map(|index| builtin.row_count() as i32 + index)
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PresetInfo;
    use slint::Model;

    fn make_item(file: &str, title: &str, desc: &str, tag: &str) -> PresetInfo {
        PresetInfo {
            i18n_title: String::new(),
            i18n_desc: String::new(),
            raw_title: title.into(),
            raw_desc: desc.into(),
            file: file.into(),
            tag: tag.into(),
        }
    }

    fn collect_strings(m: &ModelRc<SharedString>) -> Vec<String> {
        (0..m.row_count())
            .map(|i| m.row_data(i).unwrap().to_string())
            .collect()
    }

    // ── translate_presets ─────────────────────────────────────────────

    #[test]
    fn test_translate_presets_falls_back_to_raw_when_no_i18n_key() {
        let items = vec![make_item("test.json", "Raw Title", "Raw desc", "TAG")];
        // Tr defaults to English; if i18n_title is empty it should use raw_title
        let tr = crate::tr::Tr::with_lang("en");
        let (titles, descs) = translate_presets(&items, &tr);
        assert_eq!(titles[0].as_str(), "Raw Title");
        assert_eq!(descs[0].as_str(), "Raw desc");
    }

    #[test]
    fn test_translate_presets_uses_i18n_key_when_available() {
        let items = vec![PresetInfo {
            i18n_title: "panel.tabs.home".into(),
            i18n_desc: String::new(),
            raw_title: "Raw Home".into(),
            raw_desc: String::new(),
            file: "home.json".into(),
            tag: "TAG".into(),
        }];
        let tr = crate::tr::Tr::with_lang("en");
        let (titles, _) = translate_presets(&items, &tr);
        // Should resolve the i18n key to "Home" instead of "Raw Home"
        assert_eq!(titles[0].as_str(), "Home");
    }

    #[test]
    fn test_translate_presets_spanish() {
        let items = vec![PresetInfo {
            i18n_title: "panel.tabs.home".into(),
            i18n_desc: String::new(),
            raw_title: "Home".into(),
            raw_desc: String::new(),
            file: "home.json".into(),
            tag: "TAG".into(),
        }];
        let tr = crate::tr::Tr::with_lang("es");
        let (titles, _) = translate_presets(&items, &tr);
        assert_eq!(titles[0].as_str(), "Inicio");
    }

    // ── populate_metadata — empty input ──────────────────────────────

    #[test]
    fn test_populate_metadata_empty_input() {
        let items: Vec<PresetInfo> = vec![];
        // 99 is a stale marker from a previous list. An empty stored file must
        // become an EXPLICIT -1, never survive as if something were active.
        let active_idx = std::cell::RefCell::new(99);

        populate_metadata(&items, "", |_| {}, |_| {}, |i| *active_idx.borrow_mut() = i);

        assert_eq!(
            *active_idx.borrow(),
            -1,
            "an empty stored file must clear the marker"
        );
    }

    // ── populate_metadata — values propagated ─────────────────────────

    #[test]
    fn test_populate_metadata_sets_tags_and_files() {
        let items = vec![
            make_item("a.json", "A", "", "USER"),
            make_item("b.json", "B", "", "SYSTEM"),
        ];

        let tags = std::cell::RefCell::new(Vec::new());
        let files = std::cell::RefCell::new(Vec::new());

        populate_metadata(
            &items,
            "",
            |v| *tags.borrow_mut() = collect_strings(&v),
            |v| *files.borrow_mut() = collect_strings(&v),
            |_| {},
        );

        let tg = tags.borrow();
        assert_eq!(tg[0], "USER");
        assert_eq!(tg[1], "SYSTEM");

        let f = files.borrow();
        assert_eq!(f[0], "a.json");
        assert_eq!(f[1], "b.json");
    }

    // ── populate_metadata — active index ──────────────────────────────

    #[test]
    fn test_populate_metadata_finds_active_index() {
        let items = vec![
            make_item("x.json", "X", "", "TAG"),
            make_item("y.json", "Y", "", "TAG"),
            make_item("z.json", "Z", "", "TAG"),
        ];

        let active_idx = std::cell::RefCell::new(-1);

        populate_metadata(
            &items,
            "y.json",
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        assert_eq!(
            *active_idx.borrow(),
            1,
            "active file 'y.json' should produce index 1"
        );
    }

    #[test]
    fn test_populate_metadata_active_not_found() {
        let items = vec![
            make_item("a.json", "A", "", "TAG"),
            make_item("b.json", "B", "", "TAG"),
        ];

        let active_idx = std::cell::RefCell::new(-1);

        populate_metadata(
            &items,
            "nonexistent.json",
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        assert_eq!(
            *active_idx.borrow(),
            -1,
            "non-matching active_file should not set index"
        );
    }

    #[test]
    fn test_populate_metadata_clears_stale_index_when_active_file_matches_nothing() {
        let items = vec![
            make_item("a.json", "A", "", "TAG"),
            make_item("b.json", "B", "", "TAG"),
        ];

        // 0 is the marker left over from a list where index 0 WAS the active
        // file. The stored value no longer matches any preset, so leaving 0 in
        // place points the board at a preset that is not the loaded one.
        let active_idx = std::cell::RefCell::new(0);

        populate_metadata(
            &items,
            "removed.json",
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        assert_eq!(
            *active_idx.borrow(),
            -1,
            "a stored file that matches no preset must clear the marker explicitly"
        );
    }

    // ── U2 (c): a legacy bare stem resolves against the scanned file ───
    // A theme saved before the canonical form records the stem alone
    // (`active_border_file: "Space"`) while the scanned list holds
    // `Space.lua`. Exact equality dropped the marker with a warning, so the
    // card showed unmarked even though the border was applied to the desktop.

    #[test]
    fn test_populate_metadata_resolves_a_bare_stem_against_the_scanned_file() {
        let items = vec![
            make_item("03_duo.lua", "Duo", "", "TAG"),
            make_item("Space.lua", "Space", "", "CUSTOM"),
        ];

        let active_idx = std::cell::RefCell::new(-1);

        populate_metadata(
            &items,
            "Space",
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        assert_eq!(
            *active_idx.borrow(),
            1,
            "the legacy bare stem `Space` must resolve to the scanned `Space.lua`"
        );
    }

    // ── U6: the same for an interior-dot STEM ──────────────────────────
    // `save("my.preset")` writes `my.preset.lua`, while a record written before
    // that rule held the name as typed. Resolving it clears the marker from the
    // real file for BOTH pane slots (the helper is shared by borders,
    // animations and shaders).

    #[test]
    fn test_populate_metadata_resolves_an_interior_dot_stem() {
        // Borders: the recorded `my.preset` must find `my.preset.lua`.
        let borders = vec![make_item("my.preset.lua", "Mine", "", "CUSTOM")];
        let border_idx = std::cell::RefCell::new(-1);
        populate_metadata(
            &borders,
            "my.preset",
            |_| {},
            |_| {},
            |i| *border_idx.borrow_mut() = i,
        );
        assert_eq!(
            *border_idx.borrow(),
            0,
            "an interior-dot border record must resolve to the file the save wrote"
        );

        // Animations: its own list and its own active field, same rule.
        let animations = vec![make_item("keeper.anim.lua", "Keeper", "", "CUSTOM")];
        let anim_idx = std::cell::RefCell::new(-1);
        populate_metadata(
            &animations,
            "keeper.anim",
            |_| {},
            |_| {},
            |i| *anim_idx.borrow_mut() = i,
        );
        assert_eq!(
            *anim_idx.borrow(),
            0,
            "an interior-dot animation record must resolve to the file the save wrote"
        );
    }

    // ── U2 (e)/(f): the shipped form is unchanged, and an unmatched name
    // stays an explicit -1 — never a silent blank ──────────────────────

    #[test]
    fn test_populate_metadata_keeps_the_full_name_and_clears_an_unmatched_one() {
        let items = vec![make_item("Space.lua", "Space", "", "CUSTOM")];

        let full = std::cell::RefCell::new(-1);
        populate_metadata(
            &items,
            "Space.lua",
            |_| {},
            |_| {},
            |i| *full.borrow_mut() = i,
        );
        assert_eq!(
            *full.borrow(),
            0,
            "a full `<name>.lua` must keep resolving exactly as before"
        );

        // `Ghost` canonicalises to `Ghost.lua`, which is not in the list: the
        // index stays -1 instead of silently pointing at something else.
        let unmatched = std::cell::RefCell::new(0);
        populate_metadata(
            &items,
            "Ghost",
            |_| {},
            |_| {},
            |i| *unmatched.borrow_mut() = i,
        );
        assert_eq!(
            *unmatched.borrow(),
            -1,
            "a name that matches no preset must stay an explicit -1"
        );
    }
}
