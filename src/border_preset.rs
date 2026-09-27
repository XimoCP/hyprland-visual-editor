//! Pure Lua-preset reader for the border tune pane (Unit 1).
//!
//! Line-pattern parsing only (design D1): there is no Lua interpreter in
//! the dependency tree, so this module matches line-oriented patterns
//! (`local` aliases, `colors = {...}`, `angle = N`, ...). Unparseable
//! input resolves to documented defaults (D2/D3) and never panics.

use std::collections::HashMap;

/// Palette tokens accepted as bare identifiers in preset files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteToken {
    Primary,
    Secondary,
    Tertiary,
    Error,
    Surface,
    SurfaceLowest,
}

impl PaletteToken {
    /// Canonical token name as written bare in preset files.
    pub fn as_str(&self) -> &'static str {
        match self {
            PaletteToken::Primary => "primary",
            PaletteToken::Secondary => "secondary",
            PaletteToken::Tertiary => "tertiary",
            PaletteToken::Error => "error",
            PaletteToken::Surface => "surface",
            PaletteToken::SurfaceLowest => "surface_lowest",
        }
    }
}

/// A single border color: either a theme token or a custom RGBA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderColor {
    Token(PaletteToken),
    Custom { r: u8, g: u8, b: u8, a: u8 },
}

impl BorderColor {
    /// Canonical emit form: bare token or `"rgba(rrggbbaa)"` (D5).
    pub fn emit(&self) -> String {
        match self {
            BorderColor::Token(t) => t.as_str().to_string(),
            BorderColor::Custom { r, g, b, a } => {
                format!("\"rgba({:02x}{:02x}{:02x}{:02x})\"", r, g, b, a)
            }
        }
    }
}

/// Glow state from the `decoration.shadow` block (`None` = absent, D3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlowParams {
    pub enabled: bool,
    pub range: i32,
    pub render_power: i32,
    pub color: BorderColor,
    pub color_inactive: BorderColor,
    pub offset: (i32, i32),
}

/// One `hl.animation({...})` leaf. `speed`/`bezier`/`style` are `None`
/// when the file omits them (the `01`-`05` single-leaf shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimLeaf {
    pub leaf: String,
    pub enabled: bool,
    pub speed: Option<i32>,
    pub bezier: Option<String>,
    pub style: Option<String>,
}

/// Structured border preset parameters (file order preserved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderParams {
    pub active_colors: Vec<BorderColor>,
    pub angle: i32,
    pub inactive: BorderColor,
    /// `None` = file has no `border_size` key: keep the current tune
    /// value instead of resetting the slider (D3).
    pub border_size: Option<i32>,
    pub glow: Option<GlowParams>,
    pub rule_enabled: bool,
    /// Leaves in file order (single leaf for the `01`-`05` shape,
    /// triple for `06`-`14`).
    pub animations: Vec<AnimLeaf>,
    /// File-local curve names in file order.
    pub curves: Vec<String>,
}

impl Default for BorderParams {
    /// Documented defaults for unparseable input (D2/D3): never panics.
    fn default() -> Self {
        Self {
            active_colors: vec![BorderColor::Token(PaletteToken::Primary)],
            angle: 45,
            inactive: BorderColor::Token(PaletteToken::SurfaceLowest),
            border_size: None,
            glow: None,
            rule_enabled: false,
            animations: Vec::new(),
            curves: Vec::new(),
        }
    }
}

impl BorderParams {
    /// Find a parsed animation leaf by name.
    pub fn find_leaf(&self, leaf: &str) -> Option<&AnimLeaf> {
        self.animations.iter().find(|l| l.leaf == leaf)
    }
}

/// Parse preset file text into structured params. Never panics.
pub fn parse(text: &str) -> BorderParams {
    let aliases = collect_locals(text);
    let mut params = BorderParams::default();
    params.active_colors = extract_colors(text, &aliases);
    if let Some(angle) = extract_int_value(text, "angle") {
        params.angle = angle;
    }
    if let Some(raw) = extract_assignment_value(text, "inactive_border") {
        params.inactive = classify_color(&raw, &aliases, inactive_fallback());
    }
    // Absent key stays None: keep the current tune value (D3).
    params.border_size = extract_int_value(text, "border_size");
    params.animations = extract_animations(text);
    params.glow = extract_glow(text, &aliases);
    // Absent `windowrulev2` = rule toggle off (D3).
    params.rule_enabled = find_rule_block(text);
    params.curves = extract_curves(text);
    params
}

/// Fallback for active slots and glow `color` (D2).
fn active_fallback() -> BorderColor {
    BorderColor::Token(PaletteToken::Primary)
}

/// Fallback for inactive slots and glow `color_inactive` (D2).
fn inactive_fallback() -> BorderColor {
    BorderColor::Token(PaletteToken::SurfaceLowest)
}

/// Collect `local <name> = "<literal>"` bindings (D1).
fn collect_locals(text: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let rest = match line.strip_prefix("local ") {
            Some(r) => r.trim(),
            None => continue,
        };
        let eq = match rest.find('=') {
            Some(i) => i,
            None => continue,
        };
        let name = rest[..eq].trim();
        if name.is_empty() || !is_ident(name) {
            continue;
        }
        let literal = strip_quotes(rest[eq + 1..].trim());
        if !literal.is_empty() {
            aliases.insert(name.to_string(), literal.to_string());
        }
    }
    aliases
}

/// True when `s` is a plain Lua identifier.
fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !s.bytes().next().is_some_and(|b| b.is_ascii_digit())
}

/// Strip one pair of surrounding single or double quotes.
fn strip_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"')
            || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parse exactly two hex digits into a byte. `None` on any malformed input.
fn hex_byte(s: &str) -> Option<u8> {
    if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u8::from_str_radix(s, 16).ok()
}

/// Parse `rrggbbaa` (8 hex digits, RRGGBBAA order) into a custom color.
fn parse_rrggbbaa(hex: &str) -> Option<BorderColor> {
    if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(BorderColor::Custom {
        r: hex_byte(&hex[0..2])?,
        g: hex_byte(&hex[2..4])?,
        b: hex_byte(&hex[4..6])?,
        a: hex_byte(&hex[6..8])?,
    })
}

/// Parse `rrggbb` (6 hex digits, alpha forced to `ff`) into a custom color.
fn parse_rrggbb(hex: &str) -> Option<BorderColor> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(BorderColor::Custom {
        r: hex_byte(&hex[0..2])?,
        g: hex_byte(&hex[2..4])?,
        b: hex_byte(&hex[4..6])?,
        a: 0xff,
    })
}

/// Classify one color item: resolve locals, then match bare tokens
/// (including `error` per A2 — display resolution lives elsewhere, the
/// reader preserves the token), `rgba()`, `0xff…`, and `#…` literals.
/// Anything else resolves to `fallback` (D2) — never panics.
fn classify_color(item: &str, aliases: &HashMap<String, String>, fallback: BorderColor) -> BorderColor {
    let mut candidate = strip_quotes(item.trim()).trim();
    // Resolve local aliases (one level; alias values are literals).
    if let Some(resolved) = aliases.get(candidate) {
        candidate = resolved.as_str();
    }
    match candidate {
        "primary" => BorderColor::Token(PaletteToken::Primary),
        "secondary" => BorderColor::Token(PaletteToken::Secondary),
        "tertiary" => BorderColor::Token(PaletteToken::Tertiary),
        "error" => BorderColor::Token(PaletteToken::Error),
        "surface" => BorderColor::Token(PaletteToken::Surface),
        "surface_lowest" => BorderColor::Token(PaletteToken::SurfaceLowest),
        _ => {
            if let Some(rest) = candidate
                .strip_prefix("rgba(")
                .and_then(|s| s.strip_suffix(')'))
            {
                if let Some(c) = parse_rrggbbaa(rest.trim()) {
                    return c;
                }
            }
            // `0xffRRGGBB`: alpha forced to ff (10_golden.lua usage).
            if let Some(hex) = candidate
                .strip_prefix("0xff")
                .or_else(|| candidate.strip_prefix("0xFF"))
            {
                if let Some(c) = parse_rrggbb(hex.trim()) {
                    return c;
                }
            }
            // `#rrggbbaa` / `#rrggbb` (alpha defaults to ff).
            if let Some(hex) = candidate.strip_prefix('#') {
                let hex = hex.trim();
                if hex.len() == 8 {
                    if let Some(c) = parse_rrggbbaa(hex) {
                        return c;
                    }
                } else if hex.len() == 6 {
                    if let Some(c) = parse_rrggbb(hex) {
                        return c;
                    }
                }
            }
            fallback
        }
    }
}

/// Locate the `colors = {...}` block. Every standalone `colors`
/// occurrence is tried in order: header comments mention "colors" too
/// (e.g. `@Desc: ... Noctalia colors. ...`), so only an occurrence
/// followed by `=` starts the real block.
fn find_colors_block(text: &str) -> Option<&str> {
    let mut search = text;
    loop {
        let rel = find_key(search, "colors")?;
        let abs = text.len() - search.len() + rel;
        let after_key = text[abs + "colors".len()..].trim_start();
        if after_key.starts_with('=') {
            let after_eq = after_key[1..].trim_start();
            if after_eq.starts_with('{') {
                return match_brace(after_eq);
            }
        }
        // False positive (comment prose): continue after this occurrence.
        search = &search[rel + 1..];
    }
}

/// Extract the `colors = {...}` list. Falls back to `[primary]` when the
/// block is absent or empty (D2).
fn extract_colors(text: &str, aliases: &HashMap<String, String>) -> Vec<BorderColor> {
    let inner = match find_colors_block(text) {
        Some(s) => s,
        None => return vec![active_fallback()],
    };
    let colors: Vec<BorderColor> = inner
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| classify_color(item, aliases, active_fallback()))
        .collect();
    if colors.is_empty() {
        vec![active_fallback()]
    } else {
        colors
    }
}

/// Find `key` as a standalone identifier (not inside a longer name like
/// `border_size` when looking for `border`, or `color_inactive` when
/// looking for `color`). Returns the byte index of the key start.
fn find_key(text: &str, key: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(rel) = text[start..].find(key) {
        let i = start + rel;
        let before_ok = i == 0
            || !text.as_bytes()[i - 1].is_ascii_alphanumeric() && text.as_bytes()[i - 1] != b'_';
        let after = i + key.len();
        let after_ok = after >= text.len()
            || !text.as_bytes()[after].is_ascii_alphanumeric() && text.as_bytes()[after] != b'_';
        if before_ok && after_ok {
            return Some(i);
        }
        start = i + 1;
    }
    None
}

/// Extract the inner text of a `{...}` block starting at `s[0] == '{'`,
/// honouring nesting. Returns `None` when unbalanced.
fn match_brace(s: &str) -> Option<&str> {
    if !s.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(&s[1..i]);
            }
        }
    }
    None
}

/// Extract a raw `key = value` assignment: the text after `=` on the
/// first matching line, with a trailing comma stripped.
fn extract_assignment_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let ki = match find_key(line, key) {
            Some(i) => i,
            None => continue,
        };
        let after = line[ki + key.len()..].trim_start();
        let value = match after.strip_prefix('=') {
            Some(v) => v.trim(),
            None => continue,
        };
        // Cut trailing Lua comments, then cut at the first top-level
        // comma so single-line multi-key tables (`hl.animation({...})`)
        // do not bleed into following keys. Commas nested inside
        // `{...}` (e.g. `offset = { 0, 0 }`) are preserved.
        let value = match value.find("--") {
            Some(i) => value[..i].trim(),
            None => value,
        };
        let value = cut_top_level_comma(value).trim();
        if value.is_empty() {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

/// Cut `s` at the first comma that is not nested inside `{...}` or
/// `(...)`. Returns the whole string when there is no such comma.
fn cut_top_level_comma(s: &str) -> &str {
    let mut depth_brace = 0usize;
    let mut depth_paren = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            ',' if depth_brace == 0 && depth_paren == 0 => return &s[..i],
            _ => {}
        }
    }
    s
}

/// Extract an integer `key = N` assignment. `None` when absent or
/// unparseable (never panics).
fn extract_int_value(text: &str, key: &str) -> Option<i32> {
    extract_assignment_value(text, key)?.trim().parse::<i32>().ok()
}

/// Locate the `decoration.shadow` block. Comment prose may mention the word
/// before the real assignment (`13_the_joker`'s header comment does), so every
/// boundary-valid `shadow` occurrence is tried in order and only one followed
/// by `= {` starts the block — the same rule `find_colors_block` applies to
/// `colors` and `find_rule_block` to `windowrulev2`.
fn find_shadow_block(text: &str) -> Option<&str> {
    let mut search = text;
    loop {
        let rel = find_key(search, "shadow")?;
        let abs = text.len() - search.len() + rel;
        let after = text[abs + "shadow".len()..].trim_start();
        if after.starts_with('=') {
            let after_eq = after[1..].trim_start();
            if after_eq.starts_with('{') {
                return match_brace(after_eq);
            }
        }
        // False positive (comment prose): continue after this occurrence.
        search = &search[rel + 1..];
    }
}

/// Extract the `decoration.shadow` block. Absent block = `None`, and the
/// pane shows the addable empty state (D3). Locals resolve before
/// classification; unresolvable colors fall back per D2.
fn extract_glow(text: &str, aliases: &HashMap<String, String>) -> Option<GlowParams> {
    let block = find_shadow_block(text)?;
    Some(GlowParams {
        enabled: extract_bool_value(block, "enabled").unwrap_or(false),
        range: extract_int_value(block, "range").unwrap_or(0),
        render_power: extract_int_value(block, "render_power").unwrap_or(0),
        color: match extract_assignment_value(block, "color") {
            // NOTE: `color` must not match `color_inactive`: find_key
            // enforces identifier boundaries, so this is exact.
            Some(raw) => classify_color(&raw, aliases, active_fallback()),
            None => active_fallback(),
        },
        color_inactive: match extract_assignment_value(block, "color_inactive") {
            Some(raw) => classify_color(&raw, aliases, inactive_fallback()),
            None => inactive_fallback(),
        },
        offset: extract_offset(block).unwrap_or((0, 0)),
    })
}

/// Extract an `offset = { x, y }` pair. `None` when absent/unparseable.
fn extract_offset(block: &str) -> Option<(i32, i32)> {
    let raw = extract_assignment_value(block, "offset")?;
    let raw = raw.trim();
    if !raw.starts_with('{') {
        return None;
    }
    let inner = match_brace(raw)?;
    let mut parts = inner.split(',');
    let x: i32 = parts.next()?.trim().parse().ok()?;
    let y: i32 = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}
#[allow(dead_code)]
fn extract_bool_value(text: &str, key: &str) -> Option<bool> {
    match extract_assignment_value(text, key)?.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Extract a quoted `key = "value"` assignment (quotes stripped).
#[allow(dead_code)]
fn extract_quoted_value(text: &str, key: &str) -> Option<String> {
    let raw = extract_assignment_value(text, key)?;
    let raw = raw.trim();
    let quote = raw.as_bytes().first()?;
    if *quote != b'"' && *quote != b'\'' {
        return None;
    }
    // Cut at the closing quote so trailing `, ...` keys are ignored.
    let rest = &raw[1..];
    let end = rest.find(*quote as char)?;
    Some(rest[..end].to_string())
}

/// True when the file carries a `windowrulev2 = {...}` block. The rule is
/// a single toggle emitting the fixed pair; free-text editing is
/// forbidden, so only presence matters (D3).
fn find_rule_block(text: &str) -> bool {
    let mut search = text;
    loop {
        let rel = match find_key(search, "windowrulev2") {
            Some(i) => i,
            None => return false,
        };
        let abs = text.len() - search.len() + rel;
        if text[abs + "windowrulev2".len()..].trim_start().starts_with('=') {
            return true;
        }
        search = &search[rel + 1..];
    }
}

/// Extract file-local `hl.curve("name", ...)` names in file order.
fn extract_curves(text: &str) -> Vec<String> {
    let mut curves = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("hl.curve") {
        let after_key = &rest[pos + "hl.curve".len()..];
        let open = match after_key.find('(') {
            Some(i) => i,
            None => break,
        };
        let args = match match_paren(&after_key[open..]) {
            Some(s) => s,
            None => break,
        };
        let args = args.trim_start();
        let quote = match args.as_bytes().first() {
            Some(b'"') => b'"',
            Some(b'\'') => b'\'',
            _ => {
                rest = &after_key[open + 1..];
                continue;
            }
        };
        let body = &args[1..];
        if let Some(end) = body.find(quote as char) {
            let name = &body[..end];
            if !name.is_empty() && !curves.iter().any(|c| c == name) {
                curves.push(name.to_string());
            }
        }
        rest = &after_key[open + 1..];
    }
    curves
}
fn extract_animations(text: &str) -> Vec<AnimLeaf> {
    let mut leaves = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("hl.animation") {
        let after_key = &rest[pos + "hl.animation".len()..];
        let open = match after_key.find('(') {
            Some(i) => i,
            None => break,
        };
        let args = match match_paren(&after_key[open..]) {
            Some(s) => s,
            None => break,
        };
        if let Some(leaf) = extract_quoted_value(args, "leaf") {
            leaves.push(AnimLeaf {
                leaf,
                enabled: extract_bool_value(args, "enabled").unwrap_or(false),
                speed: extract_int_value(args, "speed"),
                bezier: extract_quoted_value(args, "bezier"),
                style: extract_quoted_value(args, "style"),
            });
        }
        rest = &after_key[open + 1..];
    }
    leaves
}

/// Extract the inner text of a `(...)` group starting at `s[0] == '('`.
/// Returns `None` when unbalanced.
fn match_paren(s: &str) -> Option<&str> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(&s[1..i]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASCADE: &str = include_str!("../assets/borders/01_cascade.lua");
    const JOKER: &str = include_str!("../assets/borders/13_the_joker.lua");
    const GOLDEN: &str = include_str!("../assets/borders/10_golden.lua");
    const LOOPER: &str = include_str!("../assets/borders/14_looper.lua");
    const NEON: &str = include_str!("../assets/borders/08_neon.lua");
    const INFINITY: &str = include_str!("../assets/borders/07_infinity.lua");
    const CYBER: &str = include_str!("../assets/borders/12_neon_cyberpunk.lua");
    const SPECTRUM: &str = include_str!("../assets/borders/05_spectrum.lua");

    #[test]
    fn cascade_parses_core_fields() {
        let p = parse(CASCADE);
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Token(PaletteToken::Primary),
                BorderColor::Token(PaletteToken::Surface),
            ]
        );
        assert_eq!(p.angle, 90);
        assert_eq!(p.inactive, BorderColor::Token(PaletteToken::SurfaceLowest));
        assert_eq!(p.border_size, Some(1));
        assert_eq!(p.glow, None);
        assert!(!p.rule_enabled);
        assert_eq!(p.animations.len(), 1);
        let leaf = &p.animations[0];
        assert_eq!(leaf.leaf, "borderangle");
        assert!(!leaf.enabled);
        assert!(p.curves.is_empty());
    }

    #[test]
    fn joker_resolves_locals_with_pinned_byte_order() {
        let p = parse(JOKER);
        // Locals resolve to RRGGBBAA-ordered customs: 39ff14 = acid green,
        // 1a0026 = deep purple, 9d00ff = electric violet, all full alpha.
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Custom { r: 0x39, g: 0xff, b: 0x14, a: 0xff },
                BorderColor::Custom { r: 0x1a, g: 0x00, b: 0x26, a: 0xff },
                BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0xff },
            ]
        );
        assert_eq!(p.angle, 45);
        assert_eq!(
            p.inactive,
            BorderColor::Custom { r: 0x1a, g: 0x00, b: 0x26, a: 0x55 }
        );
    }

    #[test]
    fn joker_glow_present_and_size_is_keep_current() {
        let p = parse(JOKER);
        // NO `border_size` key in the file: keep-current sentinel (D3).
        assert_eq!(p.border_size, None);
        let glow = p.glow.as_ref().expect("joker has a shadow block");
        assert!(glow.enabled);
        assert_eq!(glow.range, 20);
        assert_eq!(glow.render_power, 4);
        assert_eq!(
            glow.color,
            BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0x88 }
        );
        assert_eq!(
            glow.color_inactive,
            BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0x00 }
        );
        assert_eq!(glow.offset, (0, 0));
    }

    #[test]
    fn golden_parses_0xff_slots_with_forced_alpha() {
        let p = parse(GOLDEN);
        // `0xffRRGGBB`: alpha forced to ff, RRGGBB byte order pinned.
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Custom { r: 0xc5, g: 0xa0, b: 0x00, a: 0xff },
                BorderColor::Custom { r: 0xff, g: 0xd7, b: 0x00, a: 0xff },
                BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0xff },
                BorderColor::Custom { r: 0xff, g: 0xd7, b: 0x00, a: 0xff },
                BorderColor::Custom { r: 0xc5, g: 0xa0, b: 0x00, a: 0xff },
            ]
        );
        assert_eq!(p.angle, 45);
        assert_eq!(p.inactive, BorderColor::Token(PaletteToken::SurfaceLowest));
        assert_eq!(p.border_size, Some(1));
    }

    #[test]
    fn looper_parses_hash_inactive_with_alpha() {
        let p = parse(LOOPER);
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Token(PaletteToken::Primary),
                BorderColor::Token(PaletteToken::Secondary),
                BorderColor::Token(PaletteToken::Tertiary),
            ]
        );
        // `#1a002655`: RRGGBBAA with alpha 0x55.
        assert_eq!(
            p.inactive,
            BorderColor::Custom { r: 0x1a, g: 0x00, b: 0x26, a: 0x55 }
        );
        // Like the joker, looper carries no `border_size` key.
        assert_eq!(p.border_size, None);
    }

    #[test]
    fn neon_parses_six_token_slots() {
        use PaletteToken::*;
        let p = parse(NEON);
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Token(Primary),
                BorderColor::Token(Surface),
                BorderColor::Token(Primary),
                BorderColor::Token(Surface),
                BorderColor::Token(Primary),
                BorderColor::Token(Surface),
            ]
        );
        assert_eq!(p.angle, 45);
        assert_eq!(p.border_size, Some(1));
    }

    #[test]
    fn infinity_preserves_error_token() {
        use PaletteToken::*;
        let p = parse(INFINITY);
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Token(Primary),
                BorderColor::Token(Secondary),
                BorderColor::Token(Tertiary),
                BorderColor::Token(Error),
                BorderColor::Token(Primary),
                BorderColor::Token(Secondary),
                BorderColor::Token(Tertiary),
                BorderColor::Token(Error),
            ]
        );
        assert_eq!(p.angle, 45);
        assert_eq!(p.border_size, Some(1));
    }

    #[test]
    fn cyberpunk_parses_glow_rule_leaves_and_curves() {
        let p = parse(CYBER);
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Custom { r: 0x00, g: 0xff, b: 0xf7, a: 0xff },
                BorderColor::Custom { r: 0x1a, g: 0x00, b: 0x26, a: 0xff },
                BorderColor::Custom { r: 0xff, g: 0x00, b: 0xff, a: 0xff },
            ]
        );
        assert_eq!(p.angle, 45);
        assert_eq!(
            p.inactive,
            BorderColor::Custom { r: 0x0f, g: 0x0f, b: 0x26, a: 0x55 }
        );
        assert_eq!(p.border_size, Some(2));
        let glow = p.glow.as_ref().expect("cyberpunk has a shadow block");
        assert!(glow.enabled);
        assert_eq!(glow.range, 20);
        assert_eq!(glow.render_power, 4);
        assert_eq!(
            glow.color,
            BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0x44 }
        );
        assert_eq!(
            glow.color_inactive,
            BorderColor::Custom { r: 0xff, g: 0xff, b: 0xff, a: 0x00 }
        );
        assert_eq!(glow.offset, (0, 0));
        assert!(p.rule_enabled);
        assert_eq!(
            p.animations,
            vec![
                AnimLeaf {
                    leaf: "borderangle".to_string(),
                    enabled: true,
                    speed: Some(30),
                    bezier: Some("nv_neon_flow".to_string()),
                    style: Some("loop".to_string()),
                },
                AnimLeaf {
                    leaf: "border".to_string(),
                    enabled: true,
                    speed: Some(30),
                    bezier: Some("default".to_string()),
                    style: None,
                },
                AnimLeaf {
                    leaf: "fadeShadow".to_string(),
                    enabled: true,
                    speed: Some(15),
                    bezier: Some("default".to_string()),
                    style: None,
                },
            ]
        );
        assert_eq!(p.curves, vec!["nv_neon_flow".to_string()]);
    }

    #[test]
    fn spectrum_has_no_glow_rule_or_curves() {
        use PaletteToken::*;
        let p = parse(SPECTRUM);
        assert_eq!(
            p.active_colors,
            vec![
                BorderColor::Token(Primary),
                BorderColor::Token(Secondary),
                BorderColor::Token(Tertiary),
                BorderColor::Token(Error),
            ]
        );
        assert_eq!(p.angle, 45);
        assert_eq!(p.border_size, Some(1));
        assert_eq!(p.glow, None);
        assert!(!p.rule_enabled);
        assert_eq!(
            p.animations,
            vec![AnimLeaf {
                leaf: "borderangle".to_string(),
                enabled: false,
                speed: None,
                bezier: None,
                style: None,
            }]
        );
        assert!(p.curves.is_empty());
    }

    /// Slot counts, angles, and sizes pinned for every shipped fixture.
    /// Evidence: the `colors = {...}` item count and `angle = N` /
    /// `border_size = N` lines in each file under `assets/borders/`.
    /// `None` size = the file carries no `border_size` key (13, 14).
    #[test]
    fn all_fixtures_pin_slots_angle_and_size() {
        let cases: &[(&str, usize, i32, Option<i32>)] = &[
            (include_str!("../assets/borders/01_cascade.lua"), 2, 90, Some(1)),
            (include_str!("../assets/borders/02_diagonal.lua"), 2, 45, Some(1)),
            (include_str!("../assets/borders/03_duo.lua"), 2, 90, Some(1)),
            (include_str!("../assets/borders/04_tri.lua"), 3, 90, Some(1)),
            (include_str!("../assets/borders/05_spectrum.lua"), 4, 45, Some(1)),
            (include_str!("../assets/borders/06_pulse.lua"), 4, 45, Some(1)),
            (include_str!("../assets/borders/07_infinity.lua"), 8, 45, Some(1)),
            (include_str!("../assets/borders/08_neon.lua"), 6, 45, Some(1)),
            (include_str!("../assets/borders/09_glitch.lua"), 4, 90, Some(1)),
            (include_str!("../assets/borders/10_golden.lua"), 5, 45, Some(1)),
            (include_str!("../assets/borders/11_toxic.lua"), 3, 30, Some(2)),
            (include_str!("../assets/borders/12_neon_cyberpunk.lua"), 3, 45, Some(2)),
            (include_str!("../assets/borders/13_the_joker.lua"), 3, 45, None),
            (include_str!("../assets/borders/14_looper.lua"), 3, 45, None),
        ];
        assert_eq!(cases.len(), 14);
        for (text, slots, angle, size) in cases {
            let p = parse(text);
            assert_eq!(p.active_colors.len(), *slots, "slot count");
            assert_eq!(p.angle, *angle, "angle");
            assert_eq!(p.border_size, *size, "border_size");
        }
    }

    /// Glow present only in 12/13/14; rule present in 06-14 only.
    /// Evidence: `shadow = {` appears solely in 12/13/14 and
    /// `windowrulev2` solely in 06-14.
    #[test]
    fn all_fixtures_pin_glow_and_rule_presence() {
        let glow_files = [
            include_str!("../assets/borders/12_neon_cyberpunk.lua"),
            include_str!("../assets/borders/13_the_joker.lua"),
            include_str!("../assets/borders/14_looper.lua"),
        ];
        for text in glow_files {
            assert!(parse(text).glow.is_some(), "glow present");
            assert!(parse(text).rule_enabled, "rule on");
        }
        let plain_files = [
            include_str!("../assets/borders/01_cascade.lua"),
            include_str!("../assets/borders/02_diagonal.lua"),
            include_str!("../assets/borders/03_duo.lua"),
            include_str!("../assets/borders/04_tri.lua"),
            include_str!("../assets/borders/05_spectrum.lua"),
        ];
        for text in plain_files {
            assert_eq!(parse(text).glow, None, "glow absent");
            assert!(!parse(text).rule_enabled, "rule off");
            assert_eq!(parse(text).animations.len(), 1, "single leaf");
        }
    }

    /// A comment that mentions the word `shadow` before the block must not hide
    /// the glow. `13_the_joker`'s English header comment does exactly that, and
    /// the first-occurrence lookup reported the whole preset as glow-less —
    /// caught by `all_fixtures_pin_glow_and_rule_presence`. `colors` and
    /// `windowrulev2` already skip prose occurrences; `shadow` did not.
    #[test]
    fn glow_is_found_when_a_comment_mentions_shadow_first() {
        let text = r#"
-- Keep the original purple shadow.
local shadow_glow = "rgba(9d00ff88)"
hl.config({
    decoration = {
        shadow = {
            enabled = true,
            range = 20,
            render_power = 4,
            color = shadow_glow,
            color_inactive = "rgba(9d00ff00)",
            offset = { 0, 0 }
        }
    }
})
"#;
        let glow = parse(text)
            .glow
            .expect("the shadow block must be found past the comment prose");
        assert!(glow.enabled);
        assert_eq!(glow.range, 20);
        assert_eq!(glow.render_power, 4);
        assert_eq!(
            glow.color,
            BorderColor::Custom { r: 0x9d, g: 0x00, b: 0xff, a: 0x88 }
        );
    }

    /// Garbage input resolves to documented defaults and never panics.
    #[test]
    fn garbage_input_resolves_to_defaults() {
        let p = parse("this is not lua {{{ !!!");
        assert_eq!(p, BorderParams::default());
        let empty = parse("");
        assert_eq!(empty, BorderParams::default());
    }

    /// Every shipped border preset opens with the same three metadata fields, and
    /// its hand-written comments stay in the codebase's language.
    ///
    /// `13_the_joker` was the file that broke this: no `@Color`, and five Spanish
    /// comments (12 and 14 carried one Spanish line each, the same one). Walking
    /// the directory — not a hard-coded list — means a new preset is covered too.
    ///
    /// The character check is a tripwire, not a language detector: it allows the
    /// typographic marks an English comment legitimately carries here (a degree
    /// sign on an angle) and rejects accents, `¿` and `¡`, which is what four of
    /// the five leaks had. Accent-free Spanish still needs a reviewer's eye.
    #[test]
    fn every_border_preset_declares_title_desc_and_color() {
        const ALLOWED_NON_ASCII: &[char] = &['°', '±', '→', '←', '•', '…', '—'];

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/borders");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("assets/borders must be readable")
            .map(|e| e.expect("directory entry").path())
            .filter(|p| p.extension().map(|e| e == "lua").unwrap_or(false))
            .collect();
        files.sort();
        assert_eq!(files.len(), 14, "the shipped border preset set: {files:?}");

        for path in files {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).expect("preset file");

            for field in ["@Title:", "@Desc:", "@Color: #"] {
                assert!(
                    text.lines()
                        .any(|l| l.trim_start().starts_with("--") && l.contains(field)),
                    "{name} must declare {field} in its header"
                );
            }

            let leaks: Vec<String> = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    let trimmed = line.trim_start();
                    // Metadata and the rule divider are exempt: `@Desc` may
                    // carry a degree sign on purpose.
                    if !trimmed.starts_with("--") || trimmed.starts_with("-- @") {
                        return None;
                    }
                    let bad: Vec<char> = line
                        .chars()
                        .filter(|c| !c.is_ascii() && !ALLOWED_NON_ASCII.contains(c))
                        .collect();
                    (!bad.is_empty()).then(|| format!("{name}:{} {line:?} -> {bad:?}", i + 1))
                })
                .collect();
            assert!(
                leaks.is_empty(),
                "border preset comments must be English: {leaks:#?}"
            );
        }
    }
}
