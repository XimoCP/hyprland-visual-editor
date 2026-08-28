// HVE 2 — Slice carousel (gallery-immersive-redesign PR2).
//
// Parallelogram carousel with skwd-wall exact geometry: collapsed 135 ↔
// expanded 924, height 520, spacing +4 (small gap), skew 35px X-shear,
// radius 0, 350ms OutCubic. Manual x placement (design D2): cum_offset
// sums per-card widths plus small positive gaps; snap_x pixel-centers the
// focused card (design D5); wheel steps ±1 per gesture.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

use super::{GalleryStyle, GalleryView};

/// Slice carousel geometry (skwd-wall exact values, design D2/D3).
pub const SLICE_COLLAPSED_WIDTH: f32 = 135.0;
pub const SLICE_EXPANDED_WIDTH: f32 = 924.0;
pub const SLICE_HEIGHT: f32 = 520.0;
/// Small gap between cards — thin hairline separation (design D2).
pub const SLICE_SPACING_PX: f32 = 4.0;
pub const SLICE_ANIM_DURATION_MS: u64 = 350;
/// OutCubic cubic-bezier(0.215, 0.61, 0.355, 1.0) — skwd default Behavior.
pub const SLICE_ANIM_EASING: (f32, f32, f32, f32) = (0.215, 0.61, 0.355, 1.0);
/// Parallelogram skew offset (px) — horizontal X-shear (design D3).
pub const SLICE_SKEW_PX: f32 = 35.0;
/// Flip 180° Y-axis timing (spec S3).
pub const SLICE_FLIP_DURATION_MS: u64 = 400;
/// InOutQuad approx cubic-bezier(0.455, 0.03, 0.515, 0.955).
pub const SLICE_FLIP_EASING: (f32, f32, f32, f32) = (0.455, 0.03, 0.515, 0.955);
/// Video preview delay bounds (spec S16/17): 100-300ms.
pub const SLICE_VIDEO_DELAY_MIN_MS: u64 = 100;
pub const SLICE_VIDEO_DELAY_MAX_MS: u64 = 300;
pub const SLICE_VIDEO_DELAY_DEFAULT_MS: u64 = 200;
/// Image preheat (spec R8): 120ms, sourceSize 400x720, cache async.
pub const SLICE_PREHEAT_MS: u64 = 120;
pub const SLICE_SOURCE_W: u32 = 400;
pub const SLICE_SOURCE_H: u32 = 720;

/// ── Depth cues (design D4, skwd-wall exact) ────────────────────────────
/// Edge-fade end: card opacity reaches 0 at this normalized distance.
pub const EDGE_FADE_END: f32 = 1.2;
/// Full-opacity zone cap (wide viewports clamp here).
pub const EDGE_FADE_FULL_ZONE_CAP: f32 = 0.6;

/// Center x of card `i` inside the moving row (row-local px) when
/// `focused` is the expanded card. Feeds `edge_norm_dist`.
pub fn card_center_x(i: usize, focused: usize) -> f32 {
    cum_offset(i, focused) + card_width_at(i, focused) / 2.0
}

/// Normalized distance of a card center from the viewport center:
/// `|card_center − view_center| / halfView` (skwd `_normDist`, design D4).
pub fn edge_norm_dist(card_center_x_px: f32, view_center_x_px: f32, half_view: f32) -> f32 {
    if half_view <= 0.0 {
        return 0.0;
    }
    (card_center_x_px - view_center_x_px).abs() / half_view
}

/// Width of the fully-opaque zone in normDist units (skwd `_fullZone`):
/// `min(0.6, (expanded/2 + 2·(collapsed + spacing)) / halfView)`.
pub fn edge_fade_full_zone(half_view: f32) -> f32 {
    if half_view <= 0.0 {
        return EDGE_FADE_FULL_ZONE_CAP;
    }
    let numerator = SLICE_EXPANDED_WIDTH / 2.0 + 2.0 * (SLICE_COLLAPSED_WIDTH + SLICE_SPACING_PX);
    (numerator / half_view).min(EDGE_FADE_FULL_ZONE_CAP)
}

/// Edge-fade opacity curve (skwd exact): flat 1.0 inside `full_zone`,
/// then LINEAR down to 0 at normDist `EDGE_FADE_END` (1.2).
pub fn fade_opacity(norm_dist: f32, full_zone: f32) -> f32 {
    if full_zone >= EDGE_FADE_END || norm_dist <= full_zone {
        return 1.0;
    }
    (1.0 - (norm_dist - full_zone) / (EDGE_FADE_END - full_zone)).max(0.0)
}

/// Visual state of a slice card — drives dim/shadow/paint-layer precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceCardState {
    Idle,
    Hovered,
    Current,
}

/// Card state from its identity flags (current wins over hover).
pub fn card_state(is_current: bool, is_hovered: bool) -> SliceCardState {
    if is_current {
        SliceCardState::Current
    } else if is_hovered {
        SliceCardState::Hovered
    } else {
        SliceCardState::Idle
    }
}

/// Dim overlay black alpha (skwd exact): 0 current / 0.15 hover / 0.4 idle.
pub fn dim_level(state: SliceCardState) -> f32 {
    match state {
        SliceCardState::Current => 0.0,
        SliceCardState::Hovered => 0.15,
        SliceCardState::Idle => 0.4,
    }
}

/// Paint-layer rank for card `i` (design D4): runtime z-index does NOT
/// exist in Slint (`z` must be a compile-time constant), so the carousel
/// paints distance-tiered idle passes (farthest behind, nearest on top,
/// matching skwd `z: 50 - |index - currentIndex|`) plus hovered/current
/// on top. `z_layer` encodes only the three-state tier — 0 idle (bottom),
/// 1 hovered, 2 current (top) — and the Slint distance-tiered paint order
/// in `ui/gallery/SliceCarousel.slint` is the source of truth for the
/// right-side reversal (farther first, nearer later). Mirrors skwd's
/// z-rank 100/90/50−dist.
pub fn z_layer(i: usize, focused: usize, hovered: Option<usize>) -> u8 {
    if i == focused {
        2
    } else if Some(i) == hovered {
        1
    } else {
        0
    }
}

/// Keyboard key for gallery navigation (pure Rust, no Slint dep).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryKey {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Other,
}

/// Video preview state (spec S16/17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoState {
    Idle,
    Pending,
    Playing,
    Released,
}

/// Slice carousel view — horizontal ListView, current item expanded.
///
/// Pure Rust state, no Slint types; mirrors focus via ThemeGalleryModel
/// indices and drives width/flip/video via constants above.
#[derive(Debug, Clone)]
pub struct SliceView {
    /// Total number of cards in the carousel.
    count: usize,
    /// Currently focused/expanded index (clamped 0..count-1).
    focused_index: usize,
    /// Flip state: true = back face visible (180°).
    flipped: bool,
    /// Current flip angle (0 or 180, animated 400ms InOutQuad on Slint side).
    flip_angle: f32,
    /// Video preview delay (clamped 100-300ms).
    video_delay_ms: u64,
    /// Video playback state.
    video_state: VideoState,
    /// Whether video source exists for current card (spec S16: has video).
    has_video_for_current: bool,
}

/// Container x that pixel-centers the focused card (design D5):
/// `viewport_center − expanded/2 − cum_offset(focused, focused)`.
pub fn snap_x(viewport_center_x: f32, focused: usize) -> f32 {
    viewport_center_x - SLICE_EXPANDED_WIDTH / 2.0 - cum_offset(focused, focused)
}

/// Wheel gesture target index: advances `current` by **±1 max per gesture**
/// regardless of the event's notch count (`dir` sign wins), clamped to
/// 0..count−1, never wrapping (design D5).
pub fn wheel_target(count: usize, current: usize, dir: isize) -> usize {
    if count == 0 {
        return 0;
    }
    let next = current as isize + dir.signum();
    next.clamp(0, count as isize - 1) as usize
}

/// Width of card `j` (px) when `focused` is the expanded card (design D2).
pub fn card_width_at(j: usize, focused: usize) -> f32 {
    if j == focused {
        SLICE_EXPANDED_WIDTH
    } else {
        SLICE_COLLAPSED_WIDTH
    }
}

/// Cumulative x offset of card `i` (px) when `focused` is the expanded card.
///
/// Design D2: `cum_offset(i, focused) = Σ_{j<i} width(j) + i·spacing` with
/// `width(j) = 924 if j == focused else 135` and `spacing = +4` (small gap).
pub fn cum_offset(i: usize, focused: usize) -> f32 {
    (0..i).map(|j| card_width_at(j, focused)).sum::<f32>() + i as f32 * SLICE_SPACING_PX
}

/// ── Ring model S1 (infinite carousel, pure math) ─────────────────────
/// Focused card centered at stage_width/2 with expanded width 924;
/// collapsed slats 135 contiguous (zero gap); clones wrap modulo count.
/// Deterministic: computed from focused index + geometry, no running offsets.

/// Visible slot in the infinite ring — feeds Slint positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSlot {
    /// Virtual index (unwrapped, may be negative or beyond count).
    pub virtual_index: isize,
    /// Real theme index in [0, real_count), rem_euclid mapping.
    pub real_index: usize,
    /// Absolute x of slot left edge in stage coordinates [0, stage_width].
    pub x: f32,
    /// Width of this slot (expanded if focused, else collapsed).
    pub width: f32,
    /// Whether this slot is the expanded focused slot.
    pub is_expanded: bool,
}

/// Map a virtual (unwrapped) index to its real theme index.
pub fn ring_real_index(virtual_index: isize, real_count: usize) -> usize {
    // STUB for RED — intentionally wrong for negative values (uses truncating rem)
    if real_count == 0 {
        return 0;
    }
    (virtual_index % real_count as isize).abs() as usize
}

/// X of the focused (expanded) slot's left edge when centered.
pub fn ring_focused_x(stage_width: f32) -> f32 {
    // STUB for RED — misses expanded/2 offset
    stage_width / 2.0
}

/// Absolute x of slot at virtual_index when focused is centered.
pub fn ring_slot_x(virtual_index: isize, focused: usize, stage_width: f32) -> f32 {
    // STUB for RED — constant, breaks centering/contiguity
    let _ = (virtual_index, focused);
    stage_width / 2.0
}

/// Width of slot at virtual_index (expanded iff virtual == focused).
pub fn ring_slot_width(virtual_index: isize, focused: usize) -> f32 {
    if virtual_index == focused as isize {
        SLICE_EXPANDED_WIDTH
    } else {
        SLICE_COLLAPSED_WIDTH
    }
}

/// Wrap step with modulo (both directions). Degenerate count 0 → 0.
pub fn ring_step(focused: usize, delta: isize, real_count: usize) -> usize {
    // STUB for RED — clamped, no wrap
    if real_count == 0 {
        return 0;
    }
    let next = focused as isize + delta;
    next.clamp(0, real_count as isize - 1) as usize
}

/// All visible slots intersecting [0, stage_width], extending cyclically
/// outward from focused in both directions. Edge-to-edge fill.
pub fn ring_visible_slots(real_count: usize, focused: usize, stage_width: f32) -> Vec<RingSlot> {
    // STUB for RED — only the focused slot, never fills edge-to-edge nor clones beyond count
    if real_count == 0 || !(stage_width > 0.0) {
        return Vec::new();
    }
    let fx = ring_focused_x(stage_width);
    let w = if focused < real_count { SLICE_EXPANDED_WIDTH } else { SLICE_COLLAPSED_WIDTH };
    vec![RingSlot {
        virtual_index: focused as isize,
        real_index: focused.min(real_count - 1),
        x: fx,
        width: w,
        is_expanded: true,
    }]
}

impl SliceView {
    /// Create a new carousel for `count` cards, focus 0.
    pub fn new(count: usize) -> Self {
        Self {
            count,
            focused_index: 0,
            flipped: false,
            flip_angle: 0.0,
            video_delay_ms: SLICE_VIDEO_DELAY_DEFAULT_MS,
            video_state: VideoState::Idle,
            has_video_for_current: false,
        }
    }

    /// Number of cards.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Currently focused index.
    pub fn focused_index(&self) -> usize {
        self.focused_index
    }

    /// Is the current card flipped to back face?
    pub fn is_flipped(&self) -> bool {
        self.flipped
    }

    /// Current flip angle (0 or 180 degrees).
    pub fn flip_angle(&self) -> f32 {
        self.flip_angle
    }

    /// Video preview delay (100-300ms, default 200).
    pub fn video_delay_ms(&self) -> u64 {
        self.video_delay_ms
    }

    /// Current video state.
    pub fn video_state(&self) -> VideoState {
        self.video_state
    }

    /// Whether video is currently playing (on current card after delay).
    pub fn is_video_playing(&self) -> bool {
        self.video_state == VideoState::Playing
    }

    /// Set whether the current focused card has a video wallpaper.
    pub fn set_has_video(&mut self, has_video: bool) {
        self.has_video_for_current = has_video;
        if !has_video && self.video_state == VideoState::Playing {
            self.video_state = VideoState::Released;
        }
    }

    /// Width for a given card index: 768 if focused, 108 otherwise (spec R2.1).
    pub fn card_width(&self, index: usize) -> f32 {
        if self.count == 0 {
            return SLICE_COLLAPSED_WIDTH;
        }
        if index == self.focused_index {
            SLICE_EXPANDED_WIDTH
        } else {
            SLICE_COLLAPSED_WIDTH
        }
    }

    /// Convenience: width of the focused card.
    pub fn focused_width(&self) -> f32 {
        self.card_width(self.focused_index)
    }

    /// Set focused index, clamped, resets flip + releases video on blur (S16/17).
    pub fn set_focused_index(&mut self, idx: usize) -> usize {
        if self.count == 0 {
            self.focused_index = 0;
            return 0;
        }
        let clamped = idx.min(self.count - 1);
        if clamped != self.focused_index {
            // Blur previous card: release video, reset flip (S17, flip reset on nav).
            self.release_video();
            self.flipped = false;
            self.flip_angle = 0.0;
        }
        self.focused_index = clamped;
        self.focused_index
    }

    /// Move focus by delta, clamped, no wrap (S13).
    pub fn move_focus(&mut self, delta: isize) -> usize {
        if self.count == 0 {
            return 0;
        }
        let max = self.count as isize - 1;
        let next = self.focused_index as isize + delta;
        let clamped = next.clamp(0, max) as usize;
        self.set_focused_index(clamped)
    }

    /// Advance one step per wheel gesture toward `dir` (+1 next / −1 prev),
    /// clamped, no wrap (design D5). A multi-notch burst still moves ±1.
    pub fn wheel_step(&mut self, dir: isize) -> usize {
        self.focused_index = wheel_target(self.count, self.focused_index, dir);
        self.focused_index
    }

    /// Handle a key press: Left/Right consumed and moves focus (S2),
    /// others not consumed. Returns true if consumed (spec R2 GalleryView).
    pub fn handle_key(&mut self, key: GalleryKey) -> bool {
        match key {
            GalleryKey::Left => {
                self.move_focus(-1);
                true
            }
            GalleryKey::Right => {
                self.move_focus(1);
                true
            }
            _ => false,
        }
    }

    /// Toggle flip state 180° (right-click on Slice, spec S3, 400ms InOutQuad).
    pub fn toggle_flip(&mut self) {
        self.flipped = !self.flipped;
        self.flip_angle = if self.flipped { 180.0 } else { 0.0 };
    }

    /// Directly set flip state (for Slint binding).
    pub fn set_flipped(&mut self, flipped: bool) {
        self.flipped = flipped;
        self.flip_angle = if flipped { 180.0 } else { 0.0 };
    }

    /// Hit-test against the parallelogram mask (spec R2.1 containment mask).
    ///
    /// Shape: rect w×h, top edge shifted right by skew, bottom left aligned.
    /// Point (x,y) in local coords: left bound lerps skew..0 over y.
    pub fn contains_point(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        Self::point_in_parallelogram(x, y, w, h, SLICE_SKEW_PX)
    }

    /// Static hit-test for a parallelogram with horizontal skew offset.
    pub fn point_in_parallelogram(x: f32, y: f32, w: f32, h: f32, skew: f32) -> bool {
        if w <= 0.0 || h <= 0.0 {
            return false;
        }
        if x < 0.0 || x > w || y < 0.0 || y > h {
            // still need to check skewed bounds; outer rect rejection not sufficient
            // because top edge extends to w, bottom starts at 0 — but outer rect
            // already covers max extents, so we keep this quick reject.
        }
        if y < 0.0 || y > h {
            return false;
        }
        // Left edge: lerp from skew at y=0 to 0 at y=h.
        let t = y / h;
        let left = skew * (1.0 - t);
        let right = left + (w - skew);
        x >= left && x <= right
    }

    /// Video delay setter, clamped to 100-300ms (S16 config).
    pub fn set_video_delay(&mut self, delay_ms: u64) -> u64 {
        self.video_delay_ms = delay_ms.clamp(SLICE_VIDEO_DELAY_MIN_MS, SLICE_VIDEO_DELAY_MAX_MS);
        self.video_delay_ms
    }

    /// Request video preview for current card — transitions to Pending if has_video.
    /// Caller should start a Timer for `video_delay_ms` then call `activate_video()`.
    pub fn request_video(&mut self) {
        if self.has_video_for_current {
            self.video_state = VideoState::Pending;
        } else {
            self.video_state = VideoState::Idle;
        }
    }

    /// Activate video after Timer delay (S16: playing after 100-300ms).
    pub fn activate_video(&mut self) {
        if self.video_state == VideoState::Pending && self.has_video_for_current {
            self.video_state = VideoState::Playing;
        }
    }

    /// Release video on blur/focus change (S17) or explicit cleanup.
    pub fn release_video(&mut self) {
        if self.video_state == VideoState::Playing || self.video_state == VideoState::Pending {
            self.video_state = VideoState::Released;
        }
    }

    /// Blur the current card: release video (S17). Called on focus change.
    pub fn on_blur(&mut self) {
        self.release_video();
    }

    /// Reset video state to idle (for reuse after Released).
    pub fn reset_video(&mut self) {
        self.video_state = VideoState::Idle;
    }

    /// Update count (when ThemeGalleryModel themes change).
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        if self.count == 0 {
            self.focused_index = 0;
        } else if self.focused_index >= self.count {
            self.focused_index = self.count - 1;
        }
    }
}

impl GalleryView for SliceView {
    fn style(&self) -> GalleryStyle {
        GalleryStyle::Slice
    }

    fn handle_key(&mut self, key: GalleryKey) -> bool {
        // delegate to inherent impl to keep single source of truth
        SliceView::handle_key(self, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 2.1 skwd geometry constants + cum_offset overlap (design D2) ────

    #[test]
    fn slice_width_collapsed_and_expanded_constants() {
        assert_eq!(SLICE_COLLAPSED_WIDTH, 135.0, "collapsed width must be 135");
        assert_eq!(SLICE_EXPANDED_WIDTH, 924.0, "expanded width must be 924");
        assert_eq!(SLICE_HEIGHT, 520.0, "slice height must be 520");
        assert_eq!(SLICE_SPACING_PX, 4.0, "spacing must be small positive gap");
        assert_eq!(SLICE_SKEW_PX, 35.0, "skew must be 35px X-shear");
        assert_eq!(SLICE_ANIM_DURATION_MS, 350, "anim duration 350ms OutCubic");
        assert_eq!(SLICE_ANIM_EASING, (0.215, 0.61, 0.355, 1.0), "OutCubic bezier");
    }

    #[test]
    fn cum_offset_focused_first_exact_px() {
        // focused = 0: the first card is expanded; later cards trail it by
        // expanded + gap steps (928), collapsed ones add 139 net each.
        assert_eq!(cum_offset(0, 0), 0.0);
        assert_eq!(cum_offset(1, 0), 928.0, "924 + 4");
        assert_eq!(cum_offset(2, 0), 1067.0);
        assert_eq!(cum_offset(3, 0), 1206.0, "924+2·135 + 3·4");
        assert_eq!(cum_offset(4, 0), 1345.0);
    }

    #[test]
    fn cum_offset_focused_mid_exact_px() {
        // focused = 2 in a 5-card row: expanded card sits at index 2.
        assert_eq!(cum_offset(0, 2), 0.0);
        assert_eq!(cum_offset(1, 2), 139.0);
        assert_eq!(cum_offset(2, 2), 278.0, "focused lands at 278");
        assert_eq!(cum_offset(3, 2), 1206.0, "135+135+924 + 3·4");
        assert_eq!(cum_offset(4, 2), 1345.0);
        // Crossing the focused edge advances by expanded+gap = 924+4.
        assert_eq!(cum_offset(3, 2) - cum_offset(2, 2), 928.0);
    }

    #[test]
    fn cum_offset_focused_last_exact_px() {
        // focused = last of 5: everything before it is collapsed.
        assert_eq!(cum_offset(0, 4), 0.0);
        assert_eq!(cum_offset(1, 4), 139.0);
        assert_eq!(cum_offset(3, 4), 417.0);
        assert_eq!(cum_offset(4, 4), 556.0);
        assert_eq!(cum_offset(5, 4), 1484.0, "4·135+924 + 5·4");
    }

    #[test]
    fn snap_x_exact_px_for_every_focused_index() {
        // snap_x = viewport_center − 924/2 − cum_offset(focused) (design D5).
        let center = 500.0;
        assert_eq!(snap_x(center, 0), 38.0, "500 − 462 − 0");
        assert_eq!(snap_x(center, 1), -101.0, "500 − 462 − 139");
        assert_eq!(snap_x(center, 2), -240.0);
        assert_eq!(snap_x(center, 3), -379.0);
        assert_eq!(snap_x(center, 4), -518.0, "500 − 462 − 556");
    }

    #[test]
    fn snap_x_pixel_centers_focused_card_for_every_index() {
        // Invariant: container_x + cum_offset(focused) + expanded/2 == center.
        let center = 1280.0;
        for focused in 0..7 {
            let x = snap_x(center, focused);
            let card_center = x + cum_offset(focused, focused) + SLICE_EXPANDED_WIDTH / 2.0;
            assert!(
                (card_center - center).abs() < 0.001,
                "focused {} must land dead-center: {} vs {}",
                focused,
                card_center,
                center
            );
        }
    }

    // ── 2.8 wheel gesture state machine (design D5) ─────────────────────

    #[test]
    fn wheel_step_advances_one_per_gesture() {
        let mut view = SliceView::new(6);
        assert_eq!(view.wheel_step(1), 1, "+1 notch → next");
        assert_eq!(view.wheel_step(1), 2);
        assert_eq!(view.wheel_step(-1), 1, "−1 notch → previous");
    }

    #[test]
    fn wheel_step_multi_notch_burst_moves_at_most_one() {
        let mut view = SliceView::new(6);
        assert_eq!(view.wheel_step(3), 1, "burst of +3 notches → still one step");
        assert_eq!(view.wheel_step(-5), 0, "burst of −5 notches → one step back");
        assert_eq!(view.wheel_step(120), 1);
    }

    #[test]
    fn wheel_step_clamped_no_wrap() {
        let mut view = SliceView::new(3);
        assert_eq!(view.wheel_step(-1), 0, "clamped at first, no wrap");
        assert_eq!(view.wheel_step(-9), 0);
        assert_eq!(view.wheel_step(1), 1);
        assert_eq!(view.wheel_step(1), 2);
        assert_eq!(view.wheel_step(1), 2, "clamped at last, no wrap");
        assert_eq!(view.wheel_step(9), 2);
    }

    #[test]
    fn wheel_step_zero_dir_and_empty_model_noop() {
        let mut view = SliceView::new(4);
        assert_eq!(view.wheel_step(0), 0, "dir 0 moves nothing");
        let mut empty = SliceView::new(0);
        assert_eq!(empty.wheel_step(1), 0, "empty model stays at 0");
    }

    #[test]
    fn wheel_target_free_fn_matches_method() {
        // Same contract as SliceView::wheel_step for callback use.
        assert_eq!(wheel_target(5, 0, 1), 1);
        assert_eq!(wheel_target(5, 4, 1), 4, "clamp at last");
        assert_eq!(wheel_target(5, 0, -1), 0, "clamp at first");
        assert_eq!(wheel_target(5, 2, -7), 1);
        assert_eq!(wheel_target(0, 0, 1), 0);
    }

    #[test]
    fn slice_width_focused_vs_collapsed() {
        let mut view = SliceView::new(5);
        // Focus 0 → 0 is expanded, others collapsed
        assert_eq!(view.card_width(0), SLICE_EXPANDED_WIDTH);
        assert_eq!(view.card_width(1), SLICE_COLLAPSED_WIDTH);
        assert_eq!(view.card_width(4), SLICE_COLLAPSED_WIDTH);
        view.set_focused_index(2);
        assert_eq!(view.card_width(2), SLICE_EXPANDED_WIDTH);
        assert_eq!(view.card_width(0), SLICE_COLLAPSED_WIDTH);
        assert_eq!(view.card_width(1), SLICE_COLLAPSED_WIDTH);
    }

    #[test]
    fn slice_width_out_of_bounds_returns_collapsed() {
        let view = SliceView::new(3);
        // Indices beyond count are treated as collapsed (no panic)
        assert_eq!(view.card_width(10), SLICE_COLLAPSED_WIDTH);
        assert_eq!(view.card_width(3), SLICE_COLLAPSED_WIDTH);
    }

    // ── 2.2 handle_key consumed ─────────────────────────────────────────

    #[test]
    fn slice_key_left_right_consumed_and_clamped() {
        let mut view = SliceView::new(3);
        assert_eq!(view.focused_index(), 0);
        // Right consumed, moves to 1
        assert!(view.handle_key(GalleryKey::Right), "Right must be consumed");
        assert_eq!(view.focused_index(), 1);
        assert!(view.handle_key(GalleryKey::Right));
        assert_eq!(view.focused_index(), 2);
        // Clamped at end, still consumed
        assert!(view.handle_key(GalleryKey::Right));
        assert_eq!(view.focused_index(), 2, "clamped at last, no wrap S13");
        // Left consumed
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 1);
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 0);
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 0, "clamped at first");
    }

    #[test]
    fn slice_key_up_down_not_consumed() {
        let mut view = SliceView::new(3);
        assert!(!view.handle_key(GalleryKey::Up), "Up not consumed for slice");
        assert!(!view.handle_key(GalleryKey::Down));
        assert!(!view.handle_key(GalleryKey::Enter));
        assert!(!view.handle_key(GalleryKey::Escape));
        assert!(!view.handle_key(GalleryKey::Other));
        assert_eq!(view.focused_index(), 0, "focus unchanged for non-consumed keys");
    }

    #[test]
    fn slice_key_gallery_view_trait_dispatch() {
        let view = SliceView::new(2);
        // Via trait object, Right still moves
        let mut boxed: Box<dyn GalleryView> = Box::new(view);
        assert!(boxed.handle_key(GalleryKey::Right));
        // Down not consumed
        assert!(!boxed.handle_key(GalleryKey::Down));
        assert_eq!(boxed.style(), GalleryStyle::Slice);
    }

    // ── 2.4 hit-test + flip 180 InOutQuad ───────────────────────────────

    #[test]
    fn slice_flip_toggle_and_angle() {
        let mut view = SliceView::new(1);
        assert!(!view.is_flipped());
        assert_eq!(view.flip_angle(), 0.0);
        assert_eq!(SLICE_FLIP_DURATION_MS, 400);
        assert_eq!(SLICE_FLIP_EASING, (0.455, 0.03, 0.515, 0.955));
        view.toggle_flip();
        assert!(view.is_flipped());
        assert_eq!(view.flip_angle(), 180.0, "flip 180° Y-axis");
        view.toggle_flip();
        assert!(!view.is_flipped());
        assert_eq!(view.flip_angle(), 0.0);
    }

    #[test]
    fn slice_flip_resets_on_focus_change() {
        let mut view = SliceView::new(3);
        view.toggle_flip();
        assert!(view.is_flipped());
        view.set_focused_index(1);
        assert!(!view.is_flipped(), "flip resets on blur/focus change");
        assert_eq!(view.flip_angle(), 0.0);
    }

    #[test]
    fn slice_flip_point_in_parallelogram_mask() {
        // Shape w=108 h=200 skew=28 (collapsed), w=768 h=200 skew=28 (expanded)
        // Hit-test mask non-rect (spec R2.1)
        // Center point should be inside
        assert!(SliceView::point_in_parallelogram(54.0, 100.0, 108.0, 200.0, 28.0));
        assert!(SliceView::point_in_parallelogram(384.0, 100.0, 768.0, 200.0, 28.0));
        // Top-left corner outside due to skew (0,0 is left of slanted edge)
        assert!(!SliceView::point_in_parallelogram(0.0, 0.0, 108.0, 200.0, 28.0));
        // Top edge at skew offset is inside
        assert!(SliceView::point_in_parallelogram(28.0, 0.0, 108.0, 200.0, 28.0));
        assert!(SliceView::point_in_parallelogram(30.0, 0.0, 108.0, 200.0, 28.0));
        // Far right top edge at w is inside (right == w at y=0)
        assert!(SliceView::point_in_parallelogram(108.0, 0.0, 108.0, 200.0, 28.0));
        // Beyond right should be outside
        assert!(!SliceView::point_in_parallelogram(109.0, 0.0, 108.0, 200.0, 28.0));
        // Bottom-left inside (left=0 at y=h)
        assert!(SliceView::point_in_parallelogram(0.0, 200.0, 108.0, 200.0, 28.0));
        assert!(SliceView::point_in_parallelogram(2.0, 200.0, 108.0, 200.0, 28.0));
        // Bottom right slanted inside edge: right = w - skew at y=h
        assert!(!SliceView::point_in_parallelogram(108.0, 200.0, 108.0, 200.0, 28.0));
        assert!(SliceView::point_in_parallelogram(80.0, 200.0, 108.0, 200.0, 28.0));
        // Below shape
        assert!(!SliceView::point_in_parallelogram(54.0, 201.0, 108.0, 200.0, 28.0));
        // contains_point wrapper uses default SKEW 28
        let view = SliceView::new(1);
        assert!(view.contains_point(54.0, 100.0, 108.0, 200.0));
        assert!(!view.contains_point(0.0, 0.0, 108.0, 200.0));
    }

    // ── 2.5 Video Timer 100-300ms release on blur S16/17 ────────────────

    #[test]
    fn slice_video_delay_bounds() {
        let mut view = SliceView::new(3);
        assert_eq!(view.video_delay_ms(), 200, "default 200 within 100-300");
        assert_eq!(SLICE_VIDEO_DELAY_MIN_MS, 100);
        assert_eq!(SLICE_VIDEO_DELAY_MAX_MS, 300);
        // Clamp below min
        assert_eq!(view.set_video_delay(50), 100);
        // Clamp above max
        assert_eq!(view.set_video_delay(500), 300);
        assert_eq!(view.set_video_delay(150), 150);
        assert_eq!(view.set_video_delay(300), 300);
        assert_eq!(view.set_video_delay(100), 100);
    }

    #[test]
    fn slice_video_pending_playing_and_release_on_blur() {
        let mut view = SliceView::new(3);
        view.set_has_video(true);
        assert_eq!(view.video_state(), VideoState::Idle);
        view.request_video();
        assert_eq!(view.video_state(), VideoState::Pending, "request → Pending");
        assert!(!view.is_video_playing());
        // Simulate Timer 100-300ms firing
        view.activate_video();
        assert_eq!(view.video_state(), VideoState::Playing);
        assert!(view.is_video_playing());
        // Blur/focus change releases (S17)
        view.on_blur();
        assert_eq!(view.video_state(), VideoState::Released);
        assert!(!view.is_video_playing());
        // Second blur stays Released
        view.on_blur();
        assert_eq!(view.video_state(), VideoState::Released);
    }

    #[test]
    fn slice_video_no_video_never_pending() {
        let mut view = SliceView::new(1);
        view.set_has_video(false);
        view.request_video();
        assert_eq!(view.video_state(), VideoState::Idle, "no video → Idle");
        view.activate_video();
        assert_eq!(view.video_state(), VideoState::Idle);
    }

    #[test]
    fn slice_video_release_on_focus_move() {
        let mut view = SliceView::new(3);
        view.set_has_video(true);
        view.request_video();
        view.activate_video();
        assert!(view.is_video_playing());
        // Move focus triggers release via set_focused_index blur
        view.set_focused_index(1);
        assert_eq!(view.video_state(), VideoState::Released, "release on blur S17");
    }

    #[test]
    fn slice_video_pending_released_on_blur_without_play() {
        let mut view = SliceView::new(2);
        view.set_has_video(true);
        view.request_video();
        assert_eq!(view.video_state(), VideoState::Pending);
        view.set_focused_index(1);
        assert_eq!(view.video_state(), VideoState::Released, "pending → released on blur");
    }

    // ── 3.1 depth cues: edge fade (design D4, skwd-wall exact) ──────────

    #[test]
    fn edge_fade_full_zone_skwd_formula() {
        // fullZone = min(0.6, (462 + 2·139) / halfView) — numerator 740 (gap 4).
        assert_eq!(edge_fade_full_zone(640.0), 0.6, "1280px viewport clamps at cap");
        assert_eq!(edge_fade_full_zone(1120.0), 0.6, "740/1120 still clamped");
        assert_eq!(edge_fade_full_zone(1480.0), 0.5, "2960px viewport: 740/1480");
        assert_eq!(edge_fade_full_zone(0.0), 0.6, "degenerate halfView safe");
        assert_eq!(EDGE_FADE_END, 1.2, "fade reaches 0 at normDist 1.2");
        assert_eq!(EDGE_FADE_FULL_ZONE_CAP, 0.6);
    }

    #[test]
    fn edge_norm_dist_uses_half_view_units() {
        assert_eq!(edge_norm_dist(1000.0, 640.0, 640.0), 0.5625, "|360|/640");
        assert_eq!(edge_norm_dist(300.0, 1000.0, 700.0), 1.0, "one half-view away");
        assert_eq!(edge_norm_dist(500.0, 500.0, 640.0), 0.0, "centered card");
        assert_eq!(edge_norm_dist(100.0, 900.0, 0.0), 0.0, "zero halfView safe");
    }

    #[test]
    fn fade_opacity_flat_inside_full_zone_linear_to_zero_at_1_2() {
        let fz = edge_fade_full_zone(640.0); // 0.6
        assert_eq!(fade_opacity(0.0, fz), 1.0);
        assert_eq!(fade_opacity(fz, fz), 1.0, "zone edge still full");
        assert!((fade_opacity(0.9, fz) - 0.5).abs() < 1e-5, "falloff midpoint");
        assert!(fade_opacity(1.199, fz) > 0.0, "still fading before end");
        assert_eq!(fade_opacity(1.2, fz), 0.0, "gone at normDist 1.2");
        assert_eq!(fade_opacity(2.0, fz), 0.0, "clamped beyond end");
    }

    #[test]
    fn fade_opacity_narrow_viewport_zone() {
        // fz = 0.5 (2960px wide, gap 4): falloff spans 0.5..1.2.
        let fz = edge_fade_full_zone(1480.0);
        assert_eq!(fade_opacity(0.5, fz), 1.0);
        assert!((fade_opacity(0.85, fz) - 0.5).abs() < 1e-5);
        assert_eq!(fade_opacity(1.2, fz), 0.0);
    }

    #[test]
    fn card_center_x_collapsed_and_expanded() {
        assert_eq!(card_center_x(0, 0), 462.0, "expanded card center");
        assert_eq!(card_center_x(1, 0), 995.5, "928 + 135/2");
        assert_eq!(card_center_x(2, 2), 740.0, "278 + 462");
    }

    // ── 3.1 depth cues: dim level + paint-layer rank (design D4) ────────

    #[test]
    fn dim_level_skwd_values() {
        assert_eq!(dim_level(SliceCardState::Current), 0.0);
        assert_eq!(dim_level(SliceCardState::Hovered), 0.15);
        assert_eq!(dim_level(SliceCardState::Idle), 0.4);
    }

    #[test]
    fn z_layer_current_above_hovered_above_idle() {
        assert_eq!(z_layer(2, 2, None), 2, "current paints topmost");
        assert_eq!(z_layer(2, 2, Some(2)), 2, "hover on current stays current layer");
        assert_eq!(z_layer(1, 2, Some(1)), 1, "hovered above idle");
        assert_eq!(z_layer(0, 2, None), 0, "idle bottom");
        assert_eq!(z_layer(0, 2, Some(3)), 0);
    }

    // ── S1 ring contracts (RED) ───────────────────────────────────────

    #[test]
    fn ring_centering_focused_slot_center_is_stage_center() {
        for stage_w in [800.0, 1200.0, 1920.0, 2560.0] {
            for focused in [0usize, 2, 5] {
                let count = 6;
                // ring_visible must contain focused and it must be centered
                let slots = ring_visible_slots(count, focused, stage_w);
                let focused_slot = slots.iter().find(|s| s.virtual_index == focused as isize).expect("focused must be visible");
                let center = focused_slot.x + focused_slot.width / 2.0;
                assert!((center - stage_w / 2.0).abs() < 0.001, "focused center {center} != stage center {} (stage {stage_w} focused {focused})", stage_w/2.0);
            }
        }
        // also direct ring_slot_x centering invariant
        for stage_w in [1200.0, 1920.0] {
            for focused in 0..6usize {
                let x = ring_slot_x(focused as isize, focused, stage_w);
                let center = x + SLICE_EXPANDED_WIDTH / 2.0;
                assert!((center - stage_w/2.0).abs() < 0.001, "ring_slot_x centering failed stage {stage_w} focused {focused}");
            }
        }
    }

    #[test]
    fn ring_contiguity_no_gap_between_collapsed_slats() {
        let stage_w = 1920.0;
        let focused = 3usize;
        let count = 8;
        let slots = ring_visible_slots(count, focused, stage_w);
        // slots sorted by virtual_index == sorted by x (left to right)
        let mut sorted = slots.clone();
        sorted.sort_by(|a, b| a.virtual_index.cmp(&b.virtual_index));
        for w in sorted.windows(2) {
            let a = w[0];
            let b = w[1];
            // if neither is the focused gap crossing, both collapsed → delta == collapsed width
            // if one is focused, the step from/to focused is special (collapsed vs expanded edge)
            if a.is_expanded || b.is_expanded {
                continue;
            }
            let delta = b.x - a.x;
            assert!((delta - SLICE_COLLAPSED_WIDTH).abs() < 0.001, "collapsed contiguity gap {} != {} between virt {} and {}", delta, SLICE_COLLAPSED_WIDTH, a.virtual_index, b.virtual_index);
        }
        // direct ring_slot_x contiguity both sides
        let fx = ring_focused_x(stage_w);
        let right1 = ring_slot_x(focused as isize + 1, focused, stage_w);
        let right2 = ring_slot_x(focused as isize + 2, focused, stage_w);
        assert!((right2 - right1 - SLICE_COLLAPSED_WIDTH).abs() < 0.001, "right side contiguity");
        let left1 = ring_slot_x(focused as isize - 1, focused, stage_w);
        let left2 = ring_slot_x(focused as isize - 2, focused, stage_w);
        assert!((left1 - left2 - SLICE_COLLAPSED_WIDTH).abs() < 0.001, "left side contiguity");
        let _ = fx;
    }

    #[test]
    fn ring_wrap_forward_and_backward() {
        assert_eq!(ring_step(5, 1, 6), 0, "forward wrap last→0");
        assert_eq!(ring_step(0, -1, 6), 5, "backward wrap 0→last");
        assert_eq!(ring_step(0, 1, 6), 1);
        assert_eq!(ring_step(2, -1, 6), 1);
        assert_eq!(ring_step(5, 2, 6), 1, "multi-step wraps modulo");
        assert_eq!(ring_step(0, -2, 6), 4);
        // wrapping via SliceView move_focus
        let mut view = SliceView::new(6);
        view.set_focused_index(5);
        assert_eq!(view.move_focus(1), 0);
        let mut view2 = SliceView::new(6);
        view2.set_focused_index(0);
        assert_eq!(view2.move_focus(-1), 5);
        // wheel_step must also wrap (S1 ring)
        let mut wv = SliceView::new(3);
        wv.set_focused_index(2);
        assert_eq!(wv.wheel_step(1), 0, "wheel wrap forward");
        let mut wv2 = SliceView::new(3);
        wv2.set_focused_index(0);
        assert_eq!(wv2.wheel_step(-1), 2, "wheel wrap backward");
        // wheel_target free fn wraps
        assert_eq!(wheel_target(6, 5, 1), 0);
        assert_eq!(wheel_target(6, 0, -1), 5);
    }

    #[test]
    fn ring_clone_mapping_real_index_in_range_and_cyclic() {
        let count = 6;
        // all visible slots map into [0, count)
        for stage_w in [1200.0, 1920.0, 3000.0] {
            for focused in [0usize, 3, 5] {
                let slots = ring_visible_slots(count, focused, stage_w);
                for s in &slots {
                    assert!(s.real_index < count, "real_index {} out of range", s.real_index);
                    assert_eq!(s.real_index, ring_real_index(s.virtual_index, count));
                }
            }
        }
        // explicit cyclic: virtual beyond ±count wraps
        assert_eq!(ring_real_index(6, 6), 0);
        assert_eq!(ring_real_index(7, 6), 1);
        assert_eq!(ring_real_index(-1, 6), 5);
        assert_eq!(ring_real_index(-7, 6), 5);
        assert_eq!(ring_real_index(12, 6), 0);
        assert_eq!(ring_real_index(-12, 6), 0);
        // slot right of last → theme 0 when focused near end
        let stage_w = 1920.0;
        let focused = 5usize; // last
        let right = ring_slot_x(focused as isize + 1, focused, stage_w);
        let real = ring_real_index(focused as isize + 1, count);
        assert_eq!(real, 0, "clone right of last maps to 0");
        let _ = right;
        // left of first → last
        assert_eq!(ring_real_index(-1, 6), 5);
    }

    #[test]
    fn ring_edge_to_edge_fill_no_hole() {
        for stage_w in [800.0, 1200.0, 1920.0, 2560.0] {
            for focused in [0usize, 3, 5] {
                let slots = ring_visible_slots(6, focused, stage_w);
                assert!(!slots.is_empty(), "must have visible slots");
                let min_x = slots.iter().map(|s| s.x).fold(f32::MAX, f32::min);
                let max_r = slots.iter().map(|s| s.x + s.width).fold(f32::MIN, f32::max);
                assert!(min_x <= 0.0 + 0.001, "left edge not covered min_x {min_x} stage {stage_w}");
                assert!(max_r >= stage_w - 0.001, "right edge not covered max_r {max_r} stage {stage_w}");
                // no hole larger than collapsed width between sorted slots
                let mut sorted = slots.clone();
                sorted.sort_by(|a,b| a.x.partial_cmp(&b.x).unwrap());
                for w in sorted.windows(2) {
                    let gap = w[1].x - (w[0].x + w[0].width);
                    assert!(gap <= 0.001, "hole {gap} between slots at stage {stage_w}");
                }
            }
        }
    }

    #[test]
    fn ring_degenerate_counts_and_tiny_stage_no_panic() {
        // count 0 empty no panic
        assert!(ring_visible_slots(0, 0, 1200.0).is_empty());
        assert_eq!(ring_step(0, 1, 0), 0);
        assert_eq!(ring_real_index(5, 0), 0);
        // count 1 every slot maps to 0
        let slots = ring_visible_slots(1, 0, 1920.0);
        assert!(!slots.is_empty());
        for s in &slots {
            assert_eq!(s.real_index, 0);
        }
        // tiny stage fewer than one slat width — no panic, still centered
        let tiny = ring_visible_slots(6, 2, 50.0);
        assert!(!tiny.is_empty(), "tiny stage should still have focused");
        let focused_slot = tiny.iter().find(|s| s.virtual_index==2).unwrap();
        assert!((focused_slot.x + focused_slot.width/2.0 - 25.0).abs()<0.001);
        // wheel/ move with count 1 wraps to 0
        let mut v1 = SliceView::new(1);
        assert_eq!(v1.move_focus(1), 0);
        assert_eq!(v1.move_focus(-1), 0);
        assert_eq!(v1.wheel_step(1), 0);
    }

    #[test]
    fn ring_determinism_same_inputs_same_model() {
        let a = ring_visible_slots(6, 3, 1920.0);
        let b = ring_visible_slots(6, 3, 1920.0);
        assert_eq!(a, b, "deterministic from geometry");
        let ax = ring_slot_x(5, 3, 1920.0);
        let bx = ring_slot_x(5, 3, 1920.0);
        assert_eq!(ax, bx);
        // after steps, model recomputed from index not accumulated drift
        let stage_w = 1200.0;
        let mut view = SliceView::new(6);
        view.set_focused_index(0);
        for _ in 0..12 { view.move_focus(1); }
        let back_to_zero = view.focused_index();
        assert_eq!(back_to_zero, 0, "12 steps wrap to start deterministically");
        let sx1 = ring_slot_x(0, back_to_zero, stage_w);
        let sx0 = ring_slot_x(0, 0, stage_w);
        assert!((sx1 - sx0).abs()<0.001, "no drift after cycles");
    }
}
