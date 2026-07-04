use std::path::PathBuf;

use slint::Color;

use crate::engine::ColorScheme;

// ─── Color parsing ──────────────────────────────────────────────

/// Parse a hex color string (`"#ff8800"` or `"ff8800"`) into a Slint `Color`.
/// Returns a neutral fallback on parse failure.
pub fn parse_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(230);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(237);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(243);
        Color::from_rgb_u8(r, g, b)
    } else {
        Color::from_rgb_u8(230, 237, 243)
    }
}

// ─── Color manipulation ─────────────────────────────────────────

/// Relative luminance of a color (sRGB). 0 = black, 1 = white.
pub fn luminance(c: &Color) -> f32 {
    fn linearize(ch: u8) -> f32 {
        let c = ch as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let r = linearize(c.red());
    let g = linearize(c.green());
    let b = linearize(c.blue());
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Lighten a colour towards white by `amount` (0.0 – 1.0).
pub fn lighten(c: &Color, amount: f32) -> Color {
    Color::from_rgb_u8(
        (c.red() as f32 + (255.0 - c.red() as f32) * amount) as u8,
        (c.green() as f32 + (255.0 - c.green() as f32) * amount) as u8,
        (c.blue() as f32 + (255.0 - c.blue() as f32) * amount) as u8,
    )
}

/// Darken a colour towards black by `amount` (0.0 – 1.0).
pub fn darken(c: &Color, amount: f32) -> Color {
    Color::from_rgb_u8(
        (c.red() as f32 * (1.0 - amount)) as u8,
        (c.green() as f32 * (1.0 - amount)) as u8,
        (c.blue() as f32 * (1.0 - amount)) as u8,
    )
}

/// Blend two colours by fraction `t` (0 = all c1, 1 = all c2).
pub fn blend(c1: &Color, c2: &Color, t: f32) -> Color {
    Color::from_rgb_u8(
        (c1.red() as f32 * (1.0 - t) + c2.red() as f32 * t) as u8,
        (c1.green() as f32 * (1.0 - t) + c2.green() as f32 * t) as u8,
        (c1.blue() as f32 * (1.0 - t) + c2.blue() as f32 * t) as u8,
    )
}

// ─── Theme preference ──────────────────────────────────────────

/// Detect whether the system prefers dark mode.
/// Tries `gsettings` first (GNOME/GTK), falls back to dark.
pub fn detect_system_dark() -> bool {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output();
    match output {
        Ok(out) => {
            let s = String::from_utf8_lossy(&out.stdout);
            // "prefer-dark" → dark, anything else ("default") → light
            s.contains("prefer-dark")
        }
        Err(_) => {
            // No gsettings → try darkman indicator file
            let darkman = PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".cache")
                .join("darkman")
                .join("mode");
            if let Ok(mode) = std::fs::read_to_string(&darkman) {
                mode.trim() == "dark"
            } else {
                true // default to dark
            }
        }
    }
}

/// Built-in dark palette (Tailwind-inspired).
/// Used when user explicitly selects "dark" mode.
fn dark_palette() -> ColorScheme {
    ColorScheme {
        primary: "#38bdf8".to_string(),    // sky-400
        secondary: "#fbbf24".to_string(),  // amber-400
        tertiary: "#c084fc".to_string(),   // violet-400
        accent: "#34d399".to_string(),     // emerald-400
        surface: "#1e293b".to_string(),    // slate-800
        surface_lowest: "#0f172a".to_string(), // slate-900
    }
}

/// Built-in light palette (Tailwind-inspired).
/// Used when user explicitly selects "light" mode.
fn light_palette() -> ColorScheme {
    ColorScheme {
        primary: "#0284c7".to_string(),    // sky-600
        secondary: "#d97706".to_string(),  // amber-600
        tertiary: "#7c3aed".to_string(),   // violet-600
        accent: "#059669".to_string(),     // emerald-600
        surface: "#f5f5f4".to_string(),    // stone-100
        surface_lowest: "#ffffff".to_string(), // white
    }
}

/// Derive a light version of a dark color scheme.
/// Keeps accent colors but flips surfaces to light.
fn light_from_base(base: &ColorScheme) -> ColorScheme {
    ColorScheme {
        primary: base.primary.clone(),
        secondary: base.secondary.clone(),
        tertiary: base.tertiary.clone(),
        accent: base.accent.clone(),
        surface: "#f5f5f4".to_string(),
        surface_lowest: "#ffffff".to_string(),
    }
}

/// Resolve a `ColorScheme` according to the user's theme preference.
///
/// - `"dark"` → built-in dark palette (no Noctalia)
/// - `"light"` → built-in light palette (no Noctalia)
/// - `"system"` → use `base` (Noctalia) for dark, derive light from it
pub fn resolve_scheme(base: &ColorScheme, preference: &str) -> ColorScheme {
    match preference {
        "dark" => dark_palette(),
        "light" => light_palette(),
        _ => {
            // "system" → follow system preference, use Noctalia as source
            if detect_system_dark() {
                base.clone()
            } else {
                light_from_base(base)
            }
        }
    }
}

// ─── Theme application ──────────────────────────────────────────

/// Apply a `ColorScheme` to the MainWindow's theme properties.
///
/// Derives card, hover, border, and text colors from the scheme's
/// surface luminance so the UI always has proper contrast regardless
/// of whether the color source is light or dark.
pub fn apply_theme(window: &crate::MainWindow, colors: &ColorScheme) {
    let p = parse_hex(&colors.primary);
    let s = parse_hex(&colors.secondary);
    let t = parse_hex(&colors.tertiary);
    let surf = parse_hex(&colors.surface);
    let surf_low = parse_hex(&colors.surface_lowest);
    let acc = parse_hex(&colors.accent);

    // Detect light vs dark surface to pick contrasting text
    let is_dark = luminance(&surf) < 0.5;

    // Text colours — adapt to background luminance
    let text = if is_dark {
        Color::from_rgb_u8(230, 237, 243) // near-white on dark bg
    } else {
        Color::from_rgb_u8(30, 33, 36) // near-black on light bg
    };

    let text_muted = if is_dark {
        Color::from_rgb_u8(148, 158, 168) // softer light
    } else {
        Color::from_rgb_u8(108, 115, 122) // softer dark
    };

    // Visual depth — derive card & hover from surface
    let bg_card = if is_dark {
        lighten(&surf, 0.08)
    } else {
        darken(&surf, 0.08)
    };
    let bg_hover = if is_dark {
        lighten(&surf, 0.16)
    } else {
        darken(&surf, 0.16)
    };

    // Accent-ink: blend primary toward pure red for a distinct alert colour
    let accent_red = blend(&p, &Color::from_rgb_u8(220, 50, 50), 0.6);

    // Border: neutral structural colour derived from surface, NOT from accent
    // palette. Keeps borders subtle so active-state accents actually pop.
    let border = if is_dark {
        lighten(&surf, 0.22)
    } else {
        darken(&surf, 0.22)
    };

    // ── Apply everything ──
    window.set_bg_dark(surf_low);
    window.set_bg_surface(surf);
    window.set_bg_card(bg_card);
    window.set_bg_hover(bg_hover);
    window.set_text(text);
    window.set_text_muted(text_muted);
    window.set_accent_cyan(p);
    window.set_accent_amber(s);
    window.set_accent_green(acc);
    window.set_accent_purple(t);
    window.set_accent_red(accent_red);
    window.set_border(border);
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_luminance_black() {
        let black = Color::from_rgb_u8(0, 0, 0);
        assert!((luminance(&black) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_luminance_white() {
        let white = Color::from_rgb_u8(255, 255, 255);
        assert!((luminance(&white) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_luminance_mid_gray() {
        let gray = Color::from_rgb_u8(128, 128, 128);
        let lum = luminance(&gray);
        // 128/255 ≈ 0.502 → linearized ≈ 0.214 → luminance ≈ 0.214
        assert!((lum - 0.214).abs() < 0.01);
    }

    #[test]
    fn test_lighten_increases_values() {
        let c = Color::from_rgb_u8(100, 150, 200);
        let lighter = lighten(&c, 0.5);
        assert!(lighter.red() > c.red());
        assert!(lighter.green() > c.green());
        assert!(lighter.blue() > c.blue());
    }

    #[test]
    fn test_darken_decreases_values() {
        let c = Color::from_rgb_u8(100, 150, 200);
        let darker = darken(&c, 0.5);
        assert!(darker.red() < c.red());
        assert!(darker.green() < c.green());
        assert!(darker.blue() < c.blue());
    }

    #[test]
    fn test_lighten_white_stays_white() {
        let white = Color::from_rgb_u8(255, 255, 255);
        let result = lighten(&white, 0.5);
        assert_eq!(result.red(), 255);
        assert_eq!(result.green(), 255);
        assert_eq!(result.blue(), 255);
    }

    #[test]
    fn test_darken_black_stays_black() {
        let black = Color::from_rgb_u8(0, 0, 0);
        let result = darken(&black, 0.5);
        assert_eq!(result.red(), 0);
        assert_eq!(result.green(), 0);
        assert_eq!(result.blue(), 0);
    }

    #[test]
    fn test_blend_endpoints() {
        let a = Color::from_rgb_u8(0, 0, 0);
        let b = Color::from_rgb_u8(255, 255, 255);
        let all_a = blend(&a, &b, 0.0);
        assert_eq!(all_a.red(), 0);
        let all_b = blend(&a, &b, 1.0);
        assert_eq!(all_b.red(), 255);
    }

    #[test]
    fn test_parse_hex_full() {
        let white = parse_hex("#ffffff");
        assert_eq!(white.red(), 255);
        assert_eq!(white.green(), 255);
        assert_eq!(white.blue(), 255);
    }

    #[test]
    fn test_parse_hex_no_hash() {
        let black = parse_hex("000000");
        assert_eq!(black.red(), 0);
        assert_eq!(black.green(), 0);
        assert_eq!(black.blue(), 0);
    }

    #[test]
    fn test_parse_hex_invalid_fallback() {
        let fallback = parse_hex("xyz");
        assert_eq!(fallback.red(), 230);
        assert_eq!(fallback.green(), 237);
        assert_eq!(fallback.blue(), 243);
    }

    #[test]
    fn test_parse_hex_short_fallback() {
        let fallback = parse_hex("#ff");
        assert_eq!(fallback.red(), 230);
    }
}
