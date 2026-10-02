//! Pure Lua-preset model and reader for animation (Motion) presets.
//!
//! Mirrors the line-oriented approach of `border_preset.rs`: there is no Lua
//! interpreter in the dependency tree, so this module matches line patterns
//! (`hl.curve(...)`, `hl.animation({...})`). Unparseable input resolves to
//! `None` and never panics.

// Phase 1 ships the model and parser ahead of its consumers; Phase 2 wires
// them into the Motion section, so the staged API is intentionally unused.
#![allow(dead_code)]

/// The user-facing animation style family (Phase 1 global model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationStyle {
    Slide,
    Fade,
    Popin,
    SlideFade,
}

impl AnimationStyle {
    /// Canonical Lua family name.
    pub fn as_lua(&self) -> &'static str {
        match self {
            AnimationStyle::Slide => "slide",
            AnimationStyle::Fade => "fade",
            AnimationStyle::Popin => "popin",
            AnimationStyle::SlideFade => "slidefade 20%",
        }
    }

    /// Match the leading family token, ignoring parameters such as `70%`
    /// or `right`, then normalize the real built-in spellings onto the four
    /// families. `slidefade*` is checked before `slide` so the blend is not
    /// swallowed by the plain slide prefix (`slidefade`, `slidefadevert`).
    /// Unknown families resolve to `None`.
    pub fn from_lua(raw: &str) -> Option<AnimationStyle> {
        let family: String = raw
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        if family == "fade" {
            return Some(AnimationStyle::Fade);
        }
        if family == "popin" {
            return Some(AnimationStyle::Popin);
        }
        if family.starts_with("slidefade") {
            return Some(AnimationStyle::SlideFade);
        }
        if family.starts_with("slide") {
            return Some(AnimationStyle::Slide);
        }
        None
    }
}

/// Global animation parameters: one curve, one speed, one style family.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationParams {
    /// The two bezier control points, flattened as `[x1, y1, x2, y2]`.
    pub bezier: [f32; 4],
    pub speed: f32,
    pub style: AnimationStyle,
}

/// Canonical leaves a generated user preset drives.
const CANONICAL_LEAVES: [&str; 4] = ["windowsIn", "windowsOut", "windowsMove", "workspaces"];

/// Emit the user preset in the built-in schema the reader understands.
///
/// Floats use Rust's shortest round-tripping `Display`, so
/// `parse(generate(p)) == Some(p)` holds exactly.
pub fn generate(name: &str, p: &AnimationParams) -> String {
    let [x1, y1, x2, y2] = p.bezier;
    let style = p.style.as_lua();
    let mut out = String::new();
    out.push_str(&format!("-- @Title: {name}\n"));
    out.push_str("-- @Source: user\n");
    out.push_str("-- @Tag: CUSTOM\n");
    out.push_str("-- @Desc: User-created animation preset.\n");
    out.push_str("---@diagnostic disable: undefined-global\n\n");
    out.push_str(&format!(
        "hl.curve(\"hve_user\", {{ type = \"bezier\", points = {{ {{ {x1}, {y1} }}, {{ {x2}, {y2} }} }} }})\n\n"
    ));
    out.push_str(
        "hl.animation({ leaf = \"global\", enabled = true, speed = 1, bezier = \"default\" })\n\n",
    );
    for leaf in CANONICAL_LEAVES {
        out.push_str(&format!(
            "hl.animation({{ leaf = \"{leaf}\", enabled = true, speed = {}, bezier = \"hve_user\", style = \"{style}\" }})\n",
            p.speed
        ));
    }
    out
}

/// Parse a preset into global params. `None` when no usable curve or styled
/// animation exists. Never panics.
pub fn parse(content: &str) -> Option<AnimationParams> {
    let curves = extract_curves(content);
    let anims = extract_animations(content);

    let chosen = anims.iter().find(|a| a.leaf == "windowsIn");

    // Style: prefer windowsIn, else the first animation that declares one.
    let style = chosen
        .and_then(|a| a.style.as_deref())
        .and_then(AnimationStyle::from_lua)
        .or_else(|| {
            anims
                .iter()
                .filter_map(|a| a.style.as_deref())
                .find_map(AnimationStyle::from_lua)
        })?;

    // Speed: prefer windowsIn, else the first animation that declares one.
    let speed = chosen
        .and_then(|a| a.speed)
        .or_else(|| anims.iter().find_map(|a| a.speed))
        .unwrap_or(0.0);

    // Curve: prefer the one windowsIn references, else the first curve.
    let bezier = chosen
        .and_then(|a| a.bezier.as_deref())
        .and_then(|name| curves.iter().find(|(n, _)| n == name).map(|(_, pts)| *pts))
        .or_else(|| curves.first().map(|(_, pts)| *pts))?;

    Some(AnimationParams {
        bezier,
        speed,
        style,
    })
}

/// One parsed `hl.animation({...})` leaf, reduced to what the global model needs.
struct RawAnim {
    leaf: String,
    speed: Option<f32>,
    style: Option<String>,
    bezier: Option<String>,
}

/// Extract every `hl.curve("name", { type = "bezier", points = {...} })` pair.
fn extract_curves(text: &str) -> Vec<(String, [f32; 4])> {
    let mut curves = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("hl.curve") {
        let after = &rest[pos + "hl.curve".len()..];
        let open = match after.find('(') {
            Some(i) => i,
            None => break,
        };
        let args = match match_paren(&after[open..]) {
            Some(s) => s,
            None => break,
        };
        if let Some(curve) = parse_curve_args(args) {
            curves.push(curve);
        }
        rest = &after[open + 1..];
    }
    curves
}

/// Parse the argument list of one `hl.curve(...)` call.
fn parse_curve_args(args: &str) -> Option<(String, [f32; 4])> {
    let args = args.trim_start();
    let quote = *args.as_bytes().first()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let body = &args[1..];
    let end = body.find(quote as char)?;
    let name = body[..end].to_string();
    if name.is_empty() {
        return None;
    }
    let after_name = &body[end + 1..];
    let points_key = after_name.find("points")?;
    let after_points = after_name[points_key + "points".len()..].trim_start();
    let after_eq = after_points.strip_prefix('=')?.trim_start();
    if !after_eq.starts_with('{') {
        return None;
    }
    let inner = match_brace(after_eq)?;
    let bezier = parse_points(inner)?;
    Some((name, bezier))
}

/// Read two `{ x, y }` pairs from a `points` table body into `[x1, y1, x2, y2]`.
fn parse_points(inner: &str) -> Option<[f32; 4]> {
    let mut pairs: Vec<[f32; 2]> = Vec::new();
    let mut rest = inner;
    while let Some(open) = rest.find('{') {
        let seg = &rest[open..];
        let body = match_brace(seg)?;
        let mut parts = body.split(',');
        let x: f32 = parts.next()?.trim().parse().ok()?;
        let y: f32 = parts.next()?.trim().parse().ok()?;
        pairs.push([x, y]);
        let next = 1 + body.len() + 1;
        rest = seg.get(next..).unwrap_or("");
    }
    if pairs.len() != 2 {
        return None;
    }
    Some([pairs[0][0], pairs[0][1], pairs[1][0], pairs[1][1]])
}

/// Extract every `hl.animation({...})` leaf in file order.
fn extract_animations(text: &str) -> Vec<RawAnim> {
    let mut anims = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("hl.animation") {
        let after = &rest[pos + "hl.animation".len()..];
        let open = match after.find('(') {
            Some(i) => i,
            None => break,
        };
        let args = match match_paren(&after[open..]) {
            Some(s) => s,
            None => break,
        };
        if let Some(leaf) = extract_quoted_value(args, "leaf") {
            anims.push(RawAnim {
                leaf,
                speed: extract_f32_value(args, "speed"),
                style: extract_quoted_value(args, "style"),
                bezier: extract_quoted_value(args, "bezier"),
            });
        }
        rest = &after[open + 1..];
    }
    anims
}

/// Find `key` as a standalone identifier (not inside a longer name).
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

/// Extract a raw `key = value` assignment, cutting at the first top-level
/// comma and any trailing Lua comment.
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

/// Cut `s` at the first comma not nested inside `{...}` or `(...)`.
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

/// Extract a floating-point `key = N` assignment.
fn extract_f32_value(text: &str, key: &str) -> Option<f32> {
    extract_assignment_value(text, key)?.trim().parse::<f32>().ok()
}

/// Extract a quoted `key = "value"` assignment (quotes stripped).
fn extract_quoted_value(text: &str, key: &str) -> Option<String> {
    let raw = extract_assignment_value(text, key)?;
    let raw = raw.trim();
    let quote = raw.as_bytes().first()?;
    if *quote != b'"' && *quote != b'\'' {
        return None;
    }
    let rest = &raw[1..];
    let end = rest.find(*quote as char)?;
    Some(rest[..end].to_string())
}

/// Extract the inner text of a `(...)` group starting at `s[0] == '('`.
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

/// Extract the inner text of a `{...}` block starting at `s[0] == '{'`.
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

#[cfg(test)]
mod tests {
    use super::*;

    const STYLIZED: &str = include_str!("../assets/animations/19_stylized2.5D.lua");

    #[test]
    fn round_trip_preserves_params() {
        let cases = [
            AnimationParams {
                bezier: [0.4, -0.3, 0.2, 1.15],
                speed: 4.5,
                style: AnimationStyle::Popin,
            },
            AnimationParams {
                bezier: [0.0, 0.0, 1.0, 1.0],
                speed: 2.0,
                style: AnimationStyle::Slide,
            },
            AnimationParams {
                bezier: [0.25, 0.1, 0.25, 1.0],
                speed: 3.0,
                style: AnimationStyle::Fade,
            },
            AnimationParams {
                bezier: [0.4, -0.3, 0.2, 1.15],
                speed: 4.5,
                style: AnimationStyle::SlideFade,
            },
        ];
        for p in cases {
            let lua = generate("Test", &p);
            assert_eq!(parse(&lua), Some(p.clone()), "round trip failed for {p:?}\n{lua}");
        }
    }

    #[test]
    fn parses_builtin_stylized_curve_speed_and_style() {
        let p = parse(STYLIZED).expect("stylized 2.5D must parse");
        assert_eq!(p.bezier, [0.4, -0.3, 0.2, 1.15]);
        assert_eq!(p.speed, 4.5);
        assert_eq!(p.style, AnimationStyle::Popin);
    }

    #[test]
    fn style_from_lua_matches_family_prefix() {
        assert_eq!(AnimationStyle::from_lua("popin 70%"), Some(AnimationStyle::Popin));
        assert_eq!(AnimationStyle::from_lua("slide right"), Some(AnimationStyle::Slide));
        assert_eq!(AnimationStyle::from_lua("slidevert"), Some(AnimationStyle::Slide));
        assert_eq!(AnimationStyle::from_lua("fade"), Some(AnimationStyle::Fade));
        // The blend must not be swallowed by the plain `slide` prefix.
        assert_eq!(AnimationStyle::from_lua("slidefade 15%"), Some(AnimationStyle::SlideFade));
        assert_eq!(AnimationStyle::from_lua("slidefadevert 20%"), Some(AnimationStyle::SlideFade));
        assert_eq!(AnimationStyle::from_lua("loop"), None);
    }

    #[test]
    fn garbage_and_empty_parse_to_none() {
        assert_eq!(parse("not lua {{{"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn generate_emits_canonical_schema() {
        let p = AnimationParams {
            bezier: [0.4, -0.3, 0.2, 1.15],
            speed: 4.5,
            style: AnimationStyle::Popin,
        };
        let lua = generate("My Curve", &p);
        assert!(lua.contains("-- @Title: My Curve"));
        assert!(lua.contains("-- @Source: user"));
        assert!(lua.contains("-- @Tag: CUSTOM"));
        assert!(lua.contains("-- @Desc:"));
        assert!(lua.contains("---@diagnostic disable: undefined-global"));
        assert!(lua.contains(
            r#"hl.curve("hve_user", { type = "bezier", points = { { 0.4, -0.3 }, { 0.2, 1.15 } } })"#
        ));
        assert!(lua.contains(
            r#"hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })"#
        ));
        for leaf in ["windowsIn", "windowsOut", "windowsMove", "workspaces"] {
            assert!(
                lua.contains(&format!(
                    r#"leaf = "{leaf}", enabled = true, speed = 4.5, bezier = "hve_user", style = "popin""#
                )),
                "missing leaf {leaf}\n{lua}"
            );
        }
    }
}
