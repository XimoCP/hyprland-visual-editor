// HVE — Hexagon grid (theme-gallery PR3).
//
// Honeycomb layout r140, point-in-hex hit test, pull-out with dashed
// border, subtle parallax, video on selected hex.
// Spec R2.2, S4. Translated from skwd-wall honeycomb (MIT, © liixini).
//
// Pure Rust — no Slint types. Slint side is HexDelegate.slint.

use super::{GalleryStyle, GalleryView, GalleryKey};

/// Hexagon radius (center to vertex) ~140px (spec R2.2).
pub const HEX_RADIUS: f32 = 140.0;
/// Horizontal spacing for pointy-top honeycomb: sqrt(3) * r
pub const HEX_H_SPACING: f32 = 242.487_12; // 140 * 1.7320508
/// Vertical spacing: 1.5 * r
pub const HEX_V_SPACING: f32 = 210.0; // 140 * 1.5
/// Pull-out scale on selection (dashed border animate).
pub const HEX_PULL_OUT_SCALE: f32 = 1.08;
/// Pull-out dashed border width (px).
pub const HEX_PULL_OUT_DASH: f32 = 2.0;
/// Parallax factor: thumb offset = (mouse - center) * factor (subtle).
pub const HEX_PARALLAX_FACTOR: f32 = 0.08;
/// Max parallax offset (clamped) ~12px for subtle effect.
pub const HEX_PARALLAX_MAX: f32 = 12.0;
/// Video preview delay (reuse slice bounds 100-300ms).
pub const HEX_VIDEO_DELAY_MIN_MS: u64 = 100;
pub const HEX_VIDEO_DELAY_MAX_MS: u64 = 300;
pub const HEX_VIDEO_DELAY_DEFAULT_MS: u64 = 200;
/// Image sourceSize cap for hex thumb: 1.3 * radius ≈ 182 per axis, capped
/// at 400 for Slint source-clip; we keep constant for consistency.
pub const HEX_SOURCE_SIZE: u32 = 400;

/// Video state for hex selected (spec R2.2 video on selected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexVideoState {
    Idle,
    Pending,
    Playing,
    Released,
}

/// Honeycomb position for a given index with `cols` columns (gallery-hex-cols=3).
/// Pointy-top odd-row offset: x = col * H + (row%2)*(H/2), y = row * V.
/// Returns (center_x, center_y) in logical px.
pub fn hex_center(index: usize, cols: usize) -> (f32, f32) {
    if cols == 0 {
        return (0.0, 0.0);
    }
    let row = index / cols;
    let col = index % cols;
    let x = col as f32 * HEX_H_SPACING + (row % 2) as f32 * (HEX_H_SPACING * 0.5);
    let y = row as f32 * HEX_V_SPACING;
    (x, y)
}

/// Returns the 6 vertices of a pointy-top hexagon centered at (cx,cy) with radius r.
/// Vertex angles start at 30° (pointy top) — i.e. -30,30,90,150,210,270deg offset
/// but pointy-top is angles 0,60,... shifted by 30. We use pointy-top with first
/// vertex at angle 0° (to the right) for deterministic hit-test; both give same
/// circumradius.
pub fn hex_vertices(cx: f32, cy: f32, radius: f32) -> [(f32, f32); 6] {
    let mut verts = [(0.0, 0.0); 6];
    // pointy-top: offset 30deg so flat sides top/bottom
    let offset = std::f32::consts::PI / 6.0; // 30deg
    for i in 0..6 {
        let angle = offset + i as f32 * std::f32::consts::PI / 3.0;
        verts[i] = (cx + radius * angle.cos(), cy + radius * angle.sin());
    }
    verts
}

/// Point-in-hexagon hit test: checks if (x,y) lies inside hexagon centered at
/// (cx,cy) with radius r. Uses ray-casting / winding via polygon vertices
/// (robust to orientation). Also fast-rejects via bounding circle.
pub fn point_in_hex(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> bool {
    if radius <= 0.0 {
        return false;
    }
    // quick circle reject — hexagon circumscribes radius
    let dx = x - cx;
    let dy = y - cy;
    if dx * dx + dy * dy > radius * radius {
        // still may be near corner but circle bounds hex, so actually hex is inside
        // circle — if outside circle, definitely outside hex.
        return false;
    }
    // alternative: use axial distance for pointy-top hex (more exact).
    // Convert to hex cube coords: check max(|q|,|r|,|s|) <= radius-like.
    // Simpler: point-in-polygon winding via vertices.
    let verts = hex_vertices(cx, cy, radius);
    // winding number / crossing test
    let mut inside = false;
    for i in 0..6 {
        let (x1, y1) = verts[i];
        let (x2, y2) = verts[(i + 1) % 6];
        // ray to the right
        if ((y1 > y) != (y2 > y)) && (x < (x2 - x1) * (y - y1) / (y2 - y1) + x1) {
            inside = !inside;
        }
    }
    inside
}

/// Hexagon grid view — pure Rust state.
#[derive(Debug, Clone)]
pub struct HexagonView {
    count: usize,
    cols: usize,
    focused_index: usize,
    /// Whether the focused hex is pulled out (selected) with dashed border.
    pulled_out: bool,
    /// Parallax offset for thumbnail (x,y) driven by mouse, scaled by factor.
    parallax_x: f32,
    parallax_y: f32,
    /// Video state (on selected hex only).
    video_state: HexVideoState,
    video_delay_ms: u64,
    has_video_for_selected: bool,
    /// Dashed border visible when pulled_out.
    dash_visible: bool,
}

impl HexagonView {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            cols: 3, // gallery-hex-cols
            focused_index: 0,
            pulled_out: false,
            parallax_x: 0.0,
            parallax_y: 0.0,
            video_state: HexVideoState::Idle,
            video_delay_ms: HEX_VIDEO_DELAY_DEFAULT_MS,
            has_video_for_selected: false,
            dash_visible: false,
        }
    }

    pub fn with_cols(count: usize, cols: usize) -> Self {
        let mut v = Self::new(count);
        v.cols = cols.max(1);
        v
    }

    pub fn count(&self) -> usize { self.count }
    pub fn focused_index(&self) -> usize { self.focused_index }
    pub fn is_pulled_out(&self) -> bool { self.pulled_out }
    pub fn parallax(&self) -> (f32, f32) { (self.parallax_x, self.parallax_y) }
    pub fn video_state(&self) -> HexVideoState { self.video_state }
    pub fn is_video_playing(&self) -> bool { self.video_state == HexVideoState::Playing }
    pub fn dash_visible(&self) -> bool { self.dash_visible }
    pub fn video_delay_ms(&self) -> u64 { self.video_delay_ms }
    pub fn cols(&self) -> usize { self.cols }

    /// Focus setter clamped 0..count-1, resets pull-out/parallax on blur
    pub fn set_focused_index(&mut self, idx: usize) -> usize {
        if self.count == 0 { self.focused_index = 0; return 0; }
        let clamped = idx.min(self.count - 1);
        if clamped != self.focused_index {
            self.release_video();
            self.pulled_out = false;
            self.dash_visible = false;
            self.parallax_x = 0.0;
            self.parallax_y = 0.0;
        }
        self.focused_index = clamped;
        self.focused_index
    }

    pub fn move_focus(&mut self, delta: isize) -> usize {
        if self.count == 0 { return 0; }
        let max = self.count as isize - 1;
        let next = self.focused_index as isize + delta;
        let clamped = next.clamp(0, max) as usize;
        self.set_focused_index(clamped)
    }

    pub fn handle_key(&mut self, key: GalleryKey) -> bool {
        match key {
            GalleryKey::Left => { self.move_focus(-1); true }
            GalleryKey::Right => { self.move_focus(1); true }
            GalleryKey::Up => {
                // move up one row = -cols
                self.move_focus(-(self.cols as isize)); true
            }
            GalleryKey::Down => {
                self.move_focus(self.cols as isize); true
            }
            _ => false,
        }
    }

    /// Select / pull-out the focused hex (click). Toggles dashed border.
    pub fn select_focused(&mut self) {
        if self.count == 0 { return; }
        self.pulled_out = true;
        self.dash_visible = true;
        if self.has_video_for_selected {
            self.video_state = HexVideoState::Pending;
        }
    }

    pub fn deselect(&mut self) {
        self.pulled_out = false;
        self.dash_visible = false;
        self.release_video();
        self.parallax_x = 0.0;
        self.parallax_y = 0.0;
    }

    /// Parallax update from mouse position relative to hex center.
    /// Subtle: offset = (mouse - center) * factor, clamped to ±MAX.
    pub fn update_parallax(&mut self, mouse_x: f32, mouse_y: f32) -> (f32, f32) {
        let (cx, cy) = hex_center(self.focused_index, self.cols);
        let mut dx = (mouse_x - cx) * HEX_PARALLAX_FACTOR;
        let mut dy = (mouse_y - cy) * HEX_PARALLAX_FACTOR;
        dx = dx.clamp(-HEX_PARALLAX_MAX, HEX_PARALLAX_MAX);
        dy = dy.clamp(-HEX_PARALLAX_MAX, HEX_PARALLAX_MAX);
        self.parallax_x = dx;
        self.parallax_y = dy;
        (dx, dy)
    }

    /// Direct parallax setter (for Slint binding).
    pub fn set_parallax(&mut self, dx: f32, dy: f32) {
        self.parallax_x = dx.clamp(-HEX_PARALLAX_MAX, HEX_PARALLAX_MAX);
        self.parallax_y = dy.clamp(-HEX_PARALLAX_MAX, HEX_PARALLAX_MAX);
    }

    /// Hit-test against hexagon centered at its honeycomb position.
    pub fn contains_point(&self, x: f32, y: f32, index: usize) -> bool {
        let (cx, cy) = hex_center(index, self.cols);
        point_in_hex(x, y, cx, cy, HEX_RADIUS)
    }

    pub fn set_has_video(&mut self, has_video: bool) {
        self.has_video_for_selected = has_video;
        if !has_video && self.video_state == HexVideoState::Playing {
            self.video_state = HexVideoState::Released;
        }
    }

    pub fn set_video_delay(&mut self, delay_ms: u64) -> u64 {
        self.video_delay_ms = delay_ms.clamp(HEX_VIDEO_DELAY_MIN_MS, HEX_VIDEO_DELAY_MAX_MS);
        self.video_delay_ms
    }

    pub fn request_video(&mut self) {
        if self.has_video_for_selected && self.pulled_out {
            self.video_state = HexVideoState::Pending;
        } else {
            self.video_state = HexVideoState::Idle;
        }
    }

    pub fn activate_video(&mut self) {
        if self.video_state == HexVideoState::Pending && self.has_video_for_selected {
            self.video_state = HexVideoState::Playing;
        }
    }

    pub fn release_video(&mut self) {
        if self.video_state == HexVideoState::Playing || self.video_state == HexVideoState::Pending {
            self.video_state = HexVideoState::Released;
        }
    }

    pub fn on_blur(&mut self) { self.release_video(); }
    pub fn reset_video(&mut self) { self.video_state = HexVideoState::Idle; }

    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        if self.count == 0 { self.focused_index = 0; self.pulled_out = false; }
        else if self.focused_index >= self.count { self.focused_index = self.count - 1; }
    }

    /// Position for index (public for tests).
    pub fn pos_for(&self, index: usize) -> (f32, f32) {
        hex_center(index, self.cols)
    }

    /// Scale for index: pull-out scale if focused+pulled, else 1.0.
    pub fn scale_for(&self, index: usize) -> f32 {
        if index == self.focused_index && self.pulled_out { HEX_PULL_OUT_SCALE } else { 1.0 }
    }
}

impl GalleryView for HexagonView {
    fn style(&self) -> GalleryStyle { GalleryStyle::Hexagon }
    fn handle_key(&mut self, key: GalleryKey) -> bool { HexagonView::handle_key(self, key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 3.1 honeycomb r140 ─────────────────────────────────────
    #[test]
    fn hex_pos_honeycomb_r140_layout() {
        assert_eq!(HEX_RADIUS, 140.0, "radius must be 140");
        assert!((HEX_H_SPACING - 242.48712).abs() < 0.01, "H spacing = sqrt3*r");
        assert_eq!(HEX_V_SPACING, 210.0, "V spacing = 1.5*r");
        // cols=3 layout
        let (x0,y0) = hex_center(0,3);
        assert_eq!(x0, 0.0); assert_eq!(y0, 0.0);
        let (x1,y1) = hex_center(1,3);
        assert!((x1 - HEX_H_SPACING).abs() < 0.01); assert_eq!(y1, 0.0);
        let (x3,_y3) = hex_center(3,3); // next row, col0 with odd offset
        assert!((x3 - HEX_H_SPACING*0.5).abs() < 0.01, "odd row offset H/2");
        let (_x3b,y3) = hex_center(3,3);
        assert_eq!(y3, HEX_V_SPACING);
        let (x4,y4) = hex_center(4,3);
        assert!((x4 - (HEX_H_SPACING*1.5)).abs() < 0.01);
        assert_eq!(y4, HEX_V_SPACING);
    }

    #[test]
    fn hex_pos_scale_and_focus_preserved() {
        let mut view = HexagonView::new(6);
        assert_eq!(view.count(), 6);
        assert_eq!(view.cols(), 3);
        // initial pos for index 2 (row0 col2)
        let (x2, y2) = view.pos_for(2);
        assert!((x2 - HEX_H_SPACING*2.0).abs() < 0.01);
        assert_eq!(y2, 0.0);
        // move focus
        view.set_focused_index(4);
        assert_eq!(view.focused_index(), 4);
        assert_eq!(view.scale_for(4), 1.0, "not pulled out yet");
        view.select_focused();
        assert_eq!(view.scale_for(4), HEX_PULL_OUT_SCALE);
        assert!(view.is_pulled_out());
        assert!(view.dash_visible());
    }

    #[test]
    fn hex_pos_handle_key_up_down_rows() {
        let mut view = HexagonView::new(9); // 3 rows
        assert_eq!(view.focused_index(), 0);
        assert!(view.handle_key(GalleryKey::Down)); // +3
        assert_eq!(view.focused_index(), 3);
        assert!(view.handle_key(GalleryKey::Right));
        assert_eq!(view.focused_index(), 4);
        assert!(view.handle_key(GalleryKey::Up)); // -3
        assert_eq!(view.focused_index(), 1);
        assert!(!view.handle_key(GalleryKey::Enter));
    }

    // ── 3.2 point-in-hex pull-out ──────────────────────────────
    #[test]
    fn hex_point_in_hex_hit_test_and_pull_out() {
        // hexagon at origin r140
        assert!(point_in_hex(0.0, 0.0, 0.0, 0.0, 140.0), "center inside");
        assert!(point_in_hex(50.0, 30.0, 0.0, 0.0, 140.0), "near center inside");
        assert!(!point_in_hex(140.0, 140.0, 0.0, 0.0, 140.0), "corner outside");
        assert!(!point_in_hex(200.0, 0.0, 0.0, 0.0, 140.0), "far outside");
        // on the edge slightly inside
        assert!(point_in_hex(120.0, 0.0, 0.0, 0.0, 140.0));
        // contains_point via view
        let view = HexagonView::new(3);
        let (cx,cy) = hex_center(0,3);
        assert!(view.contains_point(cx, cy, 0));
        assert!(!view.contains_point(cx+300.0, cy, 0));
        // pull-out dashed
        let mut v = HexagonView::new(2);
        assert!(!v.is_pulled_out());
        v.set_focused_index(0);
        v.select_focused();
        assert!(v.is_pulled_out());
        assert_eq!(v.scale_for(0), HEX_PULL_OUT_SCALE);
        assert_eq!(HEX_PULL_OUT_DASH, 2.0);
        v.deselect();
        assert!(!v.is_pulled_out());
        assert_eq!(v.scale_for(0), 1.0);
    }

    #[test]
    fn hex_point_vertices_have_radius_distance() {
        let verts = hex_vertices(10.0, 20.0, 140.0);
        for (x,y) in verts.iter() {
            let dx = x - 10.0; let dy = y - 20.0;
            let dist = (dx*dx+dy*dy).sqrt();
            assert!((dist - 140.0).abs() < 0.01, "vertex on radius");
        }
    }

    // ── 3.3 parallax + video on selected ───────────────────────
    #[test]
    fn hex_parallax_video_on_selected() {
        let mut view = HexagonView::new(4);
        view.set_has_video(true);
        // video idle before select
        assert_eq!(view.video_state(), HexVideoState::Idle);
        view.request_video();
        // request before pull-out stays Idle (requires selected)
        assert_eq!(view.video_state(), HexVideoState::Idle, "video only on selected hex");
        view.select_focused();
        assert_eq!(view.video_state(), HexVideoState::Pending);
        view.activate_video();
        assert_eq!(view.video_state(), HexVideoState::Playing);
        assert!(view.is_video_playing());
        // parallax subtle
        let (cx,cy) = hex_center(view.focused_index(), view.cols());
        // mouse 100px to the right → parallax 100*0.08=8
        let (dx,dy) = view.update_parallax(cx+100.0, cy+50.0);
        assert!((dx - 8.0).abs() < 0.01);
        assert!((dy - 4.0).abs() < 0.01);
        assert_eq!(HEX_PARALLAX_FACTOR, 0.08);
        assert_eq!(HEX_PARALLAX_MAX, 12.0);
        // clamp
        let (dx2,_) = view.update_parallax(cx+1000.0, cy);
        assert_eq!(dx2, HEX_PARALLAX_MAX, "clamped to max");
        // release on blur/focus change
        view.set_focused_index(1);
        assert_eq!(view.video_state(), HexVideoState::Released);
        assert_eq!(view.parallax(), (0.0,0.0), "parallax reset on blur");
    }

    #[test]
    fn hex_parallax_factor_and_dash() {
        assert_eq!(HEX_PULL_OUT_SCALE, 1.08);
        assert_eq!(HEX_VIDEO_DELAY_DEFAULT_MS, 200);
        let mut v = HexagonView::new(1);
        assert_eq!(v.set_video_delay(50), 100);
        assert_eq!(v.set_video_delay(500), 300);
        // has_video false never pending even on select
        v.set_has_video(false);
        v.select_focused();
        v.request_video();
        assert_eq!(v.video_state(), HexVideoState::Idle);
    }
}
