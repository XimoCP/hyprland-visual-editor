use crate::config::Config;
use crate::engine::{Engine, PresetInfo};
use slint::{ModelRc, SharedString};

fn populate_category(
    items: &[PresetInfo],
    active_file: &str,
    set_titles: impl FnOnce(ModelRc<SharedString>),
    set_descs: impl FnOnce(ModelRc<SharedString>),
    set_tags: impl FnOnce(ModelRc<SharedString>),
    set_files: impl FnOnce(ModelRc<SharedString>),
    set_active_index: impl FnOnce(i32),
) {
    let titles: Vec<SharedString> = items.iter().map(|i| SharedString::from(&i.title)).collect();
    let descs: Vec<SharedString> = items.iter().map(|i| SharedString::from(&i.desc)).collect();
    let tags: Vec<SharedString> = items.iter().map(|i| SharedString::from(&i.tag)).collect();
    let files: Vec<SharedString> = items.iter().map(|i| SharedString::from(&i.file)).collect();

    set_titles(titles.as_slice().into());
    set_descs(descs.as_slice().into());
    set_tags(tags.as_slice().into());
    set_files(files.as_slice().into());

    if !active_file.is_empty() {
        if let Some(idx) = items.iter().position(|i| i.file == active_file) {
            set_active_index(idx as i32);
        }
    }
}

pub fn populate_presets(window: &crate::MainWindow, engine: &Engine, cfg: &Config) {
    let animations = engine.scan("animations").unwrap_or_default();
    let borders = engine.scan("borders").unwrap_or_default();
    let shaders = engine.scan("shaders").unwrap_or_default();

    populate_category(
        &animations,
        &cfg.active_anim_file,
        |v| window.set_anim_titles(v),
        |v| window.set_anim_descs(v),
        |v| window.set_anim_tags(v),
        |v| window.set_anim_files(v),
        |i| window.set_active_anim_index(i),
    );

    populate_category(
        &borders,
        &cfg.active_border_file,
        |v| window.set_border_titles(v),
        |v| window.set_border_descs(v),
        |v| window.set_border_tags(v),
        |v| window.set_border_files(v),
        |i| window.set_active_border_index(i),
    );

    populate_category(
        &shaders,
        &cfg.active_shader_file,
        |v| window.set_shader_titles(v),
        |v| window.set_shader_descs(v),
        |v| window.set_shader_tags(v),
        |v| window.set_shader_files(v),
        |i| window.set_active_shader_index(i),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PresetInfo;
    use slint::Model;

    fn make_item(file: &str, title: &str, desc: &str, tag: &str) -> PresetInfo {
        PresetInfo {
            title: title.into(),
            desc: desc.into(),
            file: file.into(),
            tag: tag.into(),
        }
    }

    fn collect_strings(m: &ModelRc<SharedString>) -> Vec<String> {
        (0..m.row_count())
            .map(|i| m.row_data(i).unwrap().to_string())
            .collect()
    }

    // ── empty input ──────────────────────────────────────────────────

    #[test]
    fn test_populate_category_empty_input() {
        let items: Vec<PresetInfo> = vec![];

        let titles = std::cell::RefCell::new(Vec::<String>::new());
        let active_idx = std::cell::RefCell::new(99);

        populate_category(
            &items,
            "",
            |v| *titles.borrow_mut() = collect_strings(&v),
            |_| {},
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        assert_eq!(titles.borrow().len(), 0, "empty items should produce empty lists");
        // active_file is empty string, so set_active_index is NOT called
        assert_eq!(*active_idx.borrow(), 99, "index should not change when active_file is empty");
    }

    // ── values are propagated to closures ────────────────────────────

    #[test]
    fn test_populate_category_sets_all_lists() {
        let items = vec![
            make_item("a.json", "Alpha", "First item", "USER"),
            make_item("b.json", "Beta", "Second item", "SYSTEM"),
        ];

        let titles = std::cell::RefCell::new(Vec::new());
        let descs = std::cell::RefCell::new(Vec::new());
        let tags = std::cell::RefCell::new(Vec::new());
        let files = std::cell::RefCell::new(Vec::new());

        populate_category(
            &items,
            "",
            |v| *titles.borrow_mut() = collect_strings(&v),
            |v| *descs.borrow_mut() = collect_strings(&v),
            |v| *tags.borrow_mut() = collect_strings(&v),
            |v| *files.borrow_mut() = collect_strings(&v),
            |_| {},
        );

        let t = titles.borrow();
        assert_eq!(t[0], "Alpha");
        assert_eq!(t[1], "Beta");

        let d = descs.borrow();
        assert_eq!(d[0], "First item");
        assert_eq!(d[1], "Second item");

        let tg = tags.borrow();
        assert_eq!(tg[0], "USER");
        assert_eq!(tg[1], "SYSTEM");

        let f = files.borrow();
        assert_eq!(f[0], "a.json");
        assert_eq!(f[1], "b.json");
    }

    // ── active index ─────────────────────────────────────────────────

    #[test]
    fn test_populate_category_finds_active_index() {
        let items = vec![
            make_item("x.json", "X", "", "TAG"),
            make_item("y.json", "Y", "", "TAG"),
            make_item("z.json", "Z", "", "TAG"),
        ];

        let active_idx = std::cell::RefCell::new(-1);

        populate_category(
            &items,
            "y.json",
            |_| {},
            |_| {},
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        assert_eq!(*active_idx.borrow(), 1, "active file 'y.json' should produce index 1");
    }

    #[test]
    fn test_populate_category_active_not_found_does_not_set_index() {
        let items = vec![
            make_item("a.json", "A", "", "TAG"),
            make_item("b.json", "B", "", "TAG"),
        ];

        let active_idx = std::cell::RefCell::new(-1);

        populate_category(
            &items,
            "nonexistent.json",
            |_| {},
            |_| {},
            |_| {},
            |_| {},
            |i| *active_idx.borrow_mut() = i,
        );

        // The file is not in the list, so set_active_index should not be called
        assert_eq!(*active_idx.borrow(), -1, "non-matching active_file should not set index");
    }
}
