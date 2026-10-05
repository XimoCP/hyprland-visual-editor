# Presets

A preset is one file that describes one look: an animation, a border or a
shader. HVE ships a set of them, scans their metadata, shows each as a card
with an icon, a colour and a description, and toggles between "this one" and
"none".

## What ships

| Kind | Directory | Count | Files |
|------|-----------|-------|-------|
| Animations | `assets/animations/` | 19 | `*.lua` |
| Borders | `assets/borders/` | 14 | `*.lua` |
| Shaders | `assets/shaders/` | 9 | `*.frag` |

Shaders have no second format because Hyprland reads the same shader file
regardless of how your configuration is written.

## The metadata block

Each preset carries its display metadata in a comment block:

```bash
# @Title: Lightning
# @Icon: bolt
# @Color: #f87171
# @Tag: FAST
# @Desc: Maximum visual response.
```

| Key | What it is | Default when missing |
|-----|------------|----------------------|
| `@Title` | Short name | the file name |
| `@Desc` | One-line description | empty |
| `@Icon` | Icon name for the card | `help` |
| `@Color` | Card colour, a hex value | `#888888` |
| `@Tag` | Badge shown on the card | `USER` |

`assets/scripts/scan.sh` reads a line when it starts with `#` or `--`, then
`@Key:` — so a Lua preset writes `-- @Title:` and the block above works too.

> The shipped `.frag` files write their block with a `//` prefix, which the
> scanner does not match. Their raw metadata is therefore ignored and the
> defaults apply — harmless, because the title and description shown in the
> interface come from the translations below anyway.

## Scanning

`scan.sh <folder>` looks in `assets/<folder>/`, one level deep, for `*.lua`
and `*.frag`, sorted by name. Files whose name contains `store` and any file
base named `geometry` are skipped (geometry is driven by the settings, not by
a card). There is no format detection and no `.conf` globbing.

The result is one JSON object per preset:

```json
{
    "file": "01_relampago.lua",
    "title": "animations.presets.01_relampago.title",
    "desc": "animations.presets.01_relampago.desc",
    "rawTitle": "Lightning",
    "rawDesc": "Maximum visual response.",
    "icon": "bolt",
    "color": "#f87171",
    "tag": "FAST"
}
```

`title` and `desc` are **translation keys**, built as
`<folder>.presets.<id>.title` and `.desc`; a dot inside a file name becomes
`_` in the key (the file itself keeps its original name).
`Engine::scan()` (`src/engine.rs`) parses the JSON into a `PresetInfo`.

### Translations win

`src/presets.rs` resolves each key through the translation map and falls back
to the raw metadata only when the key does not exist — so
`i18n/en.json`'s `"Lightning"` beats the file's `@Title`, and a preset you
add without a translation still shows something sensible. Icon, colour and
tag have no translation; the raw metadata is used for those.

## Applying one, and the toggle

Picking a card writes the preset into the active fragment
(`assets/fragments/animation.lua`, `border.lua` or `shader.lua`) and re-runs
`assemble.sh`. `none` or an empty name deletes the fragment instead. An
animation or border name that resolves to nothing falls back to a minimal
valid fragment rather than leaving a broken one behind; a shader that
resolves to nothing is rejected with an error notification and nothing is
written.

**Clicking the card that is already active deactivates it.** The stored
`active_anim_file` / `active_border_file` / `active_shader_file` becomes
empty, the active index becomes `-1`, and the script receives `none` — the
board goes back to "none" instead of staying stuck on the preset you just
turned off.

An active name that matches no scanned preset (a renamed or removed file) is
the same story: the index is set to an explicit `-1` and a warning is logged,
never a marker pointing at something that is not loaded.

## Your own presets

The tune panes can save a preset of your own. They are written as `.lua`
files under `~/.config/hve/presets/borders/` or
`~/.config/hve/presets/animations/` (`src/preset_store.rs`) and appear in the
interface tagged `CUSTOM`. When one is applied, the built-in directory is
searched first and the user directory second, so a user preset can never
shadow a built-in by accident.

## Reading a preset's values

`src/animation_preset.rs` and `src/border_preset.rs` read a preset's Lua
line by line — there is no Lua interpreter in the dependency tree — to fill
the tune panes with the preset's curve, speed, colours, angle and glow.
Unparseable input resolves to documented defaults instead of failing.

## See also

- [Themes and colours](Themes-and-Colours) — presets are one slice of a saved
  theme.
- [Configuration](Configuration) — `active_anim_file`, `active_border_file`
  and `active_shader_file`.
- [Project structure](Project-Structure) — where the preset files and your
  own presets live.
