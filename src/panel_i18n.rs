//! Panel i18n — fills the Slint text globals from the embedded translation map.
//!
//! Most panel strings travel through `window.set_*` properties, because a
//! handful of them are edited by one or two components. The Borders panel
//! carries ~50 strings and its tune pane is instantiated TWICE (two-column and
//! stacked layouts), so threading them through MainWindow → ShellRoot →
//! PanelRoot → section would cost several hundred lines of boilerplate for no
//! behaviour. They live in the exported `BordersText` global instead: exported
//! globals are already this codebase's channel for state shared between Slint
//! and Rust (`SkwdTokens` is read from Rust today, `ui/tokens.slint`), the pane
//! reads it directly wherever it is mounted, and this module fills it from the
//! same embedded map the shell strings above use.
//!
//! Every call passes the English string the global itself declares as the
//! fallback, so a key missing from the active language degrades to English
//! instead of blanking a label.

use crate::tr::Tr;
use crate::MainWindow;
use slint::Global as _;

/// Fill every `BordersText` string from `tr` for the selected language.
pub fn apply_borders(window: &MainWindow, tr: &Tr) {
    let t = crate::BordersText::get(window);

    t.set_header_title(tr.tr_shared("borders.header_title", "Borders"));
    t.set_header_subtitle(tr.tr_shared(
        "borders.header_subtitle",
        "Tune the border, then save it as a preset. Or pick an existing style.",
    ));
    t.set_live_dirty(tr.tr_shared("borders.live_dirty", "Live preview — unsaved changes"));
    t.set_live_clean(tr.tr_shared("borders.live_clean", "Live preview — no unsaved changes"));

    // Geometry block
    t.set_geometry_heading(tr.tr_shared("borders.geometry.heading", "Geometry"));
    t.set_thickness_label(tr.tr_shared("borders.geometry.title", "Border Thickness"));
    t.set_thickness_desc(tr.tr_shared(
        "borders.geometry.desc",
        "Border width in pixels (1 thin, 5 thick).",
    ));
    t.set_radius_label(tr.tr_shared("borders.radius.title", "Corner Radius"));
    t.set_radius_desc(tr.tr_shared(
        "borders.radius.desc",
        "Corner rounding in pixels (0 square, 100 fully round).",
    ));
    t.set_gap_in_label(tr.tr_shared("borders.gaps.in.title", "Inner Gap"));
    t.set_gap_in_desc(tr.tr_shared(
        "borders.gaps.in.desc",
        "Space between a window and its neighbors (inner gap).",
    ));
    t.set_gap_out_label(tr.tr_shared("borders.gaps.out.title", "Outer Gap"));
    t.set_gap_out_desc(tr.tr_shared(
        "borders.gaps.out.desc",
        "Space between windows and the screen edge (outer gap).",
    ));

    // Angle and inactive colour
    t.set_angle_label(tr.tr_shared("borders.angle.title", "Gradient Angle"));
    t.set_angle_desc(tr.tr_shared(
        "borders.angle.desc",
        "Direction of the active border gradient (30/45/90 degrees).",
    ));
    t.set_inactive_label(tr.tr_shared("borders.inactive.title", "Inactive"));
    t.set_inactive_desc(tr.tr_shared(
        "borders.inactive.desc",
        "Color applied to borders of unfocused (inactive) windows.",
    ));

    // Gradient colour slots
    t.set_active_colors_label(tr.tr_shared("borders.active_colors.title", "Active Border Colors"));
    t.set_active_colors_slots(tr.tr_shared("borders.active_colors.slots", " slots"));
    t.set_active_colors_desc(tr.tr_shared(
        "borders.active_colors.desc",
        "←→ select gradient colour, Enter to edit, Esc to close.",
    ));
    t.set_slots_count_suffix(tr.tr_shared("borders.slots.count_suffix", " of 8"));
    t.set_slots_desc(tr.tr_shared(
        "borders.slots.desc",
        "Each slot is one color in the active border gradient. Order matches the preset file. 2-8 slots supported by Hyprland.",
    ));

    // Glow / shadow group
    t.set_glow_title(tr.tr_shared("borders.glow.title", "Glow (shadow decoration)"));
    t.set_glow_add(tr.tr_shared("borders.glow.add", "Add"));
    t.set_glow_empty_desc(tr.tr_shared(
        "borders.glow.empty_desc",
        "This preset has no glow. Add one to give the border a coloured shadow around focused windows.",
    ));
    t.set_glow_range_label(tr.tr_shared("borders.glow.range.title", "Range"));
    t.set_glow_range_desc(tr.tr_shared(
        "borders.glow.range.desc",
        "How far the shadow extends from the window edge (5 near, 40 far).",
    ));
    t.set_glow_power_label(tr.tr_shared("borders.glow.power.title", "Power"));
    t.set_glow_power_desc(tr.tr_shared(
        "borders.glow.power.desc",
        "Shadow intensity (1 subtle, 8 strong).",
    ));
    t.set_glow_color_label(tr.tr_shared("borders.glow.color.title", "Glow Color"));
    t.set_glow_color_desc(tr.tr_shared(
        "borders.glow.color.desc",
        "Color of the glow on focused windows. Tap Custom… to change it.",
    ));
    t.set_glow_inactive_label(tr.tr_shared("borders.glow.inactive.title", "Inactive Glow"));
    t.set_glow_inactive_desc(tr.tr_shared(
        "borders.glow.inactive.desc",
        "Glow color for unfocused windows. Tap Custom… to change it.",
    ));

    // Save form and keyboard hint
    t.set_save_title(tr.tr_shared("borders.save.title", "Save as Preset"));
    t.set_save_placeholder(tr.tr_shared("borders.save.placeholder", "Border preset name…"));
    t.set_save_button(tr.tr_shared("borders.save.button", "Save"));
    t.set_kbd_hint(tr.tr_shared(
        "borders.kbd_hint",
        "↑↓ navigate • ←→ switch panel • Enter apply/engage • arrows ±1 • Esc back",
    ));

    // List pane
    t.set_list_builtin(tr.tr_shared("borders.list.builtin", "Built-in"));
    t.set_list_user(tr.tr_shared("borders.list.user", "My Border Presets"));
    t.set_list_empty(tr.tr_shared("borders.list.empty", "No border presets found."));
    t.set_card_apply(tr.tr_shared("borders.list.apply", "Apply"));
    t.set_card_rename(tr.tr_shared("borders.list.rename", "Rename"));
    t.set_card_delete(tr.tr_shared("borders.list.delete", "Delete"));

    // Compact colour rows and the shared picker
    t.set_custom_button(tr.tr_shared("borders.picker.custom", "Custom…"));
    t.set_editing_button(tr.tr_shared("borders.picker.editing", "Editing…"));
    t.set_picker_title_prefix(tr.tr_shared("borders.picker.title_prefix", "Custom colour — "));
    t.set_picker_done(tr.tr_shared("borders.picker.done", "Done"));
    t.set_picker_channel_gradient_prefix(
        tr.tr_shared("borders.picker.channel_gradient_prefix", "gradient colour "),
    );
    t.set_picker_channel_gradient_mid(tr.tr_shared("borders.picker.channel_gradient_mid", " of "));
    t.set_picker_channel_inactive(
        tr.tr_shared("borders.picker.channel_inactive", "the inactive border colour"),
    );
    t.set_picker_channel_glow(tr.tr_shared("borders.picker.channel_glow", "the glow colour"));
    t.set_picker_channel_glow_inactive(tr.tr_shared(
        "borders.picker.channel_glow_inactive",
        "the inactive glow colour",
    ));
    t.set_picker_channel_fallback(tr.tr_shared("borders.picker.channel_fallback", "this colour"));
    t.set_value_custom(tr.tr_shared("borders.picker.value_custom", "Custom"));
    t.set_value_none(tr.tr_shared("borders.picker.value_none", "(none)"));
}
