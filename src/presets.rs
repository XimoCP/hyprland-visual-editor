use crate::config::Config;
use crate::engine::{Engine, PresetInfo};
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

/// Collect tags/files from presets into Slint models, and set the active index.
fn populate_metadata(
    items: &[PresetInfo],
    active_file: &str,
    set_tags: impl FnOnce(ModelRc<SharedString>),
    set_files: impl FnOnce(ModelRc<SharedString>),
    set_active_index: impl FnOnce(i32),
) {
    set_tags(ModelRc::from(
        items.iter().map(|i| SharedString::from(&i.tag)).collect::<Vec<_>>().as_slice(),
    ));
    set_files(ModelRc::from(
        items.iter().map(|i| SharedString::from(&i.file)).collect::<Vec<_>>().as_slice(),
    ));

    if !active_file.is_empty() {
        if let Some(idx) = items.iter().position(|i| i.file == active_file) {
            set_active_index(idx as i32);
        }
    }
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
    populate_metadata(&animations, &cfg.active_anim_file, |v| window.set_anim_tags(v), |v| window.set_anim_files(v), |i| window.set_active_anim_index(i));
    populate_metadata(&borders, &cfg.active_border_file, |v| window.set_border_tags(v), |v| window.set_border_files(v), |i| window.set_active_border_index(i));
    populate_metadata(&shaders, &cfg.active_shader_file, |v| window.set_shader_tags(v), |v| window.set_shader_files(v), |i| window.set_active_shader_index(i));
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
        let active_idx = std::cell::RefCell::new(99);

        populate_metadata(
            &items,
            "",
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        // active_file is empty string, so set_active_index is NOT called
        assert_eq!(*active_idx.borrow(), 99, "index should not change when active_file is empty");
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

        assert_eq!(*active_idx.borrow(), 1, "active file 'y.json' should produce index 1");
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

        assert_eq!(*active_idx.borrow(), -1, "non-matching active_file should not set index");
    }
}
