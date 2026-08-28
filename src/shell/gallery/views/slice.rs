// HVE 2 — Slice carousel (gallery-immersive-redesign PR2 + S1 ring).
//
// Parallelogram carousel with skwd-wall exact geometry: collapsed 135 ↔
// expanded 924, height 520, skew 35px X-shear, radius 0, 350ms OutCubic.
// S1 infinite ring: focused slot centered at stage_width/2 (expanded 924),
// collapsed slats 135 contiguous (zero gap) on both sides, filling
// edge-to-edge and wrapping modulo count via clones. Parallelogram skew is
// visual-only (mask); layout stacks with zero gap. SLICE_SPACING_PX retained
// for edge-fade zone parity (skwd) but not used in ring layout.
// Manual x placement (D2): cum_offset sums widths (gap-free); snap_x
// pixel-centers focused. Wheel/key steps wrap rem_euclid.
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

/// Container x that pixel-centers the focused card (design D5 / S1 ring):
/// `viewport_center − expanded/2 − cum_offset(focused, focused)` with
/// gap-free cum_offset; focused center lands at viewport_center.
pub fn snap_x(viewport_center_x: f32, focused: usize) -> f32 {
    viewport_center_x - SLICE_EXPANDED_WIDTH / 2.0 - cum_offset(focused, focused)
}

/// Wheel gesture target index: advances `current` by **±1 max per gesture**
/// regardless of notch count (`dir` sign wins), wrapping modulo count (S1
/// infinite ring). Empty model stays at 0.
pub fn wheel_target(count: usize, current: usize, dir: isize) -> usize {
    if count == 0 {
        return 0;
    }
    ring_step(current, dir.signum(), count)
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
/// S1 ring (gap-free): `cum_offset(i, focused) = Σ_{j<i} width(j)` with
/// `width(j) = 924 if j == focused else 135`. Zero gap — collapsed slats are
/// contiguous; skew is visual-only. SLICE_SPACING_PX retained for edge-fade
/// parity but not used here.
pub fn cum_offset(i: usize, focused: usize) -> f32 {
    (0..i).map(|j| card_width_at(j, focused)).sum::<f32>()
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
/// `((virtual % count) + count) % count` — handles negatives via rem_euclid.
pub fn ring_real_index(virtual_index: isize, real_count: usize) -> usize {
    if real_count == 0 {
        return 0;
    }
    virtual_index.rem_euclid(real_count as isize) as usize
}

/// X of the focused (expanded) slot's left edge when centered.
pub fn ring_focused_x(stage_width: f32) -> f32 {
    stage_width / 2.0 - SLICE_EXPANDED_WIDTH / 2.0
}

/// Absolute x of slot at virtual_index when focused is centered.
/// Deterministic from geometry, no accumulated float drift.
/// Contiguous collapsed slats: zero gap on both sides.
pub fn ring_slot_x(virtual_index: isize, focused: usize, stage_width: f32) -> f32 {
    let fx = ring_focused_x(stage_width);
    let delta = virtual_index - focused as isize;
    if delta == 0 {
        fx
    } else if delta > 0 {
        fx + SLICE_EXPANDED_WIDTH + (delta - 1) as f32 * SLICE_COLLAPSED_WIDTH
    } else {
        fx + delta as f32 * SLICE_COLLAPSED_WIDTH
    }
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
/// `focused = (focused + delta).rem_euclid(real_count)` — infinite in one direction.
pub fn ring_step(focused: usize, delta: isize, real_count: usize) -> usize {
    if real_count == 0 {
        return 0;
    }
    (focused as isize + delta).rem_euclid(real_count as isize) as usize
}

/// All visible slots intersecting [0, stage_width], extending cyclically
/// outward from focused in both directions. Edge-to-edge fill via clones.
/// Deterministic: computed from focused + geometry, no running offsets.
pub fn ring_visible_slots(real_count: usize, focused: usize, stage_width: f32) -> Vec<RingSlot> {
    if real_count == 0 || !(stage_width > 0.0) {
        return Vec::new();
    }
    // Clamp focused to valid range (defensive; caller should ensure)
    let focused = focused.min(real_count.saturating_sub(1));
    let fx = ring_focused_x(stage_width);

    let mut slots: Vec<RingSlot> = Vec::new();

    // Focused slot
    slots.push(RingSlot {
        virtual_index: focused as isize,
        real_index: ring_real_index(focused as isize, real_count),
        x: fx,
        width: SLICE_EXPANDED_WIDTH,
        is_expanded: true,
    });

    // Expand left: contiguous collapsed slats until left edge covered
    let mut left: Vec<RingSlot> = Vec::new();
    let mut v = focused as isize - 1;
    // Guard against huge stage (e.g., 10k) needing many clones
    for _ in 0..10000 {
        let x = ring_slot_x(v, focused, stage_width);
        let w = SLICE_COLLAPSED_WIDTH;
        // Slot intersects stage iff x < stage_width && x+w > 0
        // For left side, once x+w <= 0 we have passed left edge; further left will be even more negative
        if x + w <= 0.0 {
            break;
        }
        if x < stage_width && x + w > 0.0 {
            left.push(RingSlot {
                virtual_index: v,
                real_index: ring_real_index(v, real_count),
                x,
                width: w,
                is_expanded: false,
            });
        }
        v -= 1;
    }
    // Expand right
    let mut right: Vec<RingSlot> = Vec::new();
    let mut v = focused as isize + 1;
    for _ in 0..10000 {
        let x = ring_slot_x(v, focused, stage_width);
        let w = SLICE_COLLAPSED_WIDTH;
        if x >= stage_width {
            break;
        }
        if x < stage_width && x + w > 0.0 {
            right.push(RingSlot {
                virtual_index: v,
                real_index: ring_real_index(v, real_count),
                x,
                width: w,
                is_expanded: false,
            });
        }
        v += 1;
    }

    left.reverse();
    let mut out = Vec::with_capacity(left.len() + 1 + right.len());
    out.extend(left);
    out.extend(slots);
    out.extend(right);
    // Sort by x to guarantee left-to-right order (also virtual order)
    out.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Slint-facing tile derived from RingSlot — includes subtle edge fade.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceUiTile {
    pub x: f32,
    pub w: f32,
    pub real_index: usize,
    pub is_expanded: bool,
    pub fade: f32,
    pub dist: i32,
}

/// Edge fade for a single ring slot at `stage_width` (skwd fullZone curve).
pub fn ring_slot_fade(slot: &RingSlot, stage_width: f32) -> f32 {
    if !(stage_width > 0.0) {
        return 1.0;
    }
    let half = stage_width / 2.0;
    let fz = edge_fade_full_zone(half);
    let center = slot.x + slot.width / 2.0;
    let nd = edge_norm_dist(center, half, half);
    fade_opacity(nd, fz)
}

/// Build Slint tiles from ring_visible_slots with per-slot fade (S2 WIRE).
pub fn slice_ui_tiles(real_count: usize, focused: usize, stage_width: f32) -> Vec<SliceUiTile> {
    ring_visible_slots(real_count, focused, stage_width)
        .into_iter()
        .map(|s| {
            let fade = ring_slot_fade(&s, stage_width);
            let dist = (s.virtual_index - focused as isize).abs() as i32;
            SliceUiTile { x: s.x, w: s.width, real_index: s.real_index, is_expanded: s.is_expanded, fade, dist }
        })
        .collect()
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

    /// Set focused index, wrapped modulo count for ring (S1) but clamped
    /// for direct jumps, resets flip + releases video on blur (S16/17).
    /// Wrapping is via `move_focus`/`ring_step`; direct `set` with out-of-range
    /// indices clamps (defensive) — the ring step is the infinite path.
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

    /// Move focus by delta, wrapping modulo count (S1 infinite ring).
    /// Deterministic: computed from index + delta, no accumulated offset drift.
    pub fn move_focus(&mut self, delta: isize) -> usize {
        if self.count == 0 {
            return 0;
        }
        let next = ring_step(self.focused_index, delta, self.count);
        if next != self.focused_index {
            self.release_video();
            self.flipped = false;
            self.flip_angle = 0.0;
        }
        self.focused_index = next;
        self.focused_index
    }

    /// Advance one step per wheel gesture toward `dir` (+1 next / −1 prev),
    /// wrapping (S1 ring). A multi-notch burst still moves ±1 (dir signum).
    pub fn wheel_step(&mut self, dir: isize) -> usize {
        if self.count == 0 {
            return 0;
        }
        let next = wheel_target(self.count, self.focused_index, dir);
        if next != self.focused_index {
            self.release_video();
            self.flipped = false;
            self.flip_angle = 0.0;
        }
        self.focused_index = next;
        self.focused_index
    }

    /// Ring slots for current focused index at given stage width — feeds Slint
    /// carousel positions (real_index + x + expanded flag) with clones and
    /// edge-to-edge fill.
    pub fn ring_slots(&self, stage_width: f32) -> Vec<RingSlot> {
        ring_visible_slots(self.count, self.focused_index, stage_width)
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
        // S1 ring gap-free: expanded 924, collapsed 135 contiguous (no gap).
        assert_eq!(cum_offset(0, 0), 0.0);
        assert_eq!(cum_offset(1, 0), 924.0, "924 contiguous");
        assert_eq!(cum_offset(2, 0), 1059.0, "924+135");
        assert_eq!(cum_offset(3, 0), 1194.0, "924+2·135");
        assert_eq!(cum_offset(4, 0), 1329.0);
    }

    #[test]
    fn cum_offset_focused_mid_exact_px() {
        // focused = 2 in a 5-card row: expanded card sits at index 2, gap-free.
        assert_eq!(cum_offset(0, 2), 0.0);
        assert_eq!(cum_offset(1, 2), 135.0);
        assert_eq!(cum_offset(2, 2), 270.0, "focused lands at 270 (2·135)");
        assert_eq!(cum_offset(3, 2), 1194.0, "135+135+924");
        assert_eq!(cum_offset(4, 2), 1329.0);
        // Crossing the focused edge advances by expanded = 924.
        assert_eq!(cum_offset(3, 2) - cum_offset(2, 2), 924.0);
    }

    #[test]
    fn cum_offset_focused_last_exact_px() {
        // focused = last of 5: everything before it is collapsed, gap-free.
        assert_eq!(cum_offset(0, 4), 0.0);
        assert_eq!(cum_offset(1, 4), 135.0);
        assert_eq!(cum_offset(3, 4), 405.0);
        assert_eq!(cum_offset(4, 4), 540.0);
        assert_eq!(cum_offset(5, 4), 1464.0, "4·135+924");
    }

    #[test]
    fn snap_x_exact_px_for_every_focused_index() {
        // S1 gap-free: snap_x = viewport_center − 462 − cum_offset(focused).
        let center = 500.0;
        assert_eq!(snap_x(center, 0), 38.0, "500 − 462 − 0");
        assert_eq!(snap_x(center, 1), -97.0, "500 − 462 − 135");
        assert_eq!(snap_x(center, 2), -232.0, "500 − 462 − 270");
        assert_eq!(snap_x(center, 3), -367.0, "500 − 462 − 405");
        assert_eq!(snap_x(center, 4), -502.0, "500 − 462 − 540");
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
        // S1 ring: wheel wraps modulo count (infinite)
        let mut view = SliceView::new(3);
        assert_eq!(view.wheel_step(-1), 2, "wrap backward 0→last");
        let mut view2 = SliceView::new(3);
        view2.set_focused_index(2);
        assert_eq!(view2.wheel_step(1), 0, "wrap forward last→0");
        // multi-step still one via wheel_target signum but wraps
        let mut view3 = SliceView::new(3);
        view3.set_focused_index(0);
        assert_eq!(view3.wheel_step(1), 1);
        assert_eq!(view3.wheel_step(1), 2);
        assert_eq!(view3.wheel_step(1), 0, "wraps again");
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
        // S1 ring wraps; matches SliceView::wheel_step signum+wrap.
        assert_eq!(wheel_target(5, 0, 1), 1);
        assert_eq!(wheel_target(5, 4, 1), 0, "wrap at last");
        assert_eq!(wheel_target(5, 0, -1), 4, "wrap at first");
        assert_eq!(wheel_target(5, 2, -7), 1, "burst signum wrap");
        assert_eq!(wheel_target(0, 0, 1), 0);
        assert_eq!(wheel_target(6, 5, 1), 0);
        assert_eq!(wheel_target(6, 0, -1), 5);
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
        // S1 ring wraps; keys move and wrap
        let mut view = SliceView::new(3);
        assert_eq!(view.focused_index(), 0);
        assert!(view.handle_key(GalleryKey::Right), "Right must be consumed");
        assert_eq!(view.focused_index(), 1);
        assert!(view.handle_key(GalleryKey::Right));
        assert_eq!(view.focused_index(), 2);
        // Wraps at end
        assert!(view.handle_key(GalleryKey::Right));
        assert_eq!(view.focused_index(), 0, "wrap last→0 S1 ring");
        // Left wraps backward
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 2, "wrap 0→last");
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 1);
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 0);
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
        // S1 gap-free contiguous
        assert_eq!(card_center_x(0, 0), 462.0, "expanded card center");
        assert_eq!(card_center_x(1, 0), 991.5, "924 + 135/2 gap-free");
        assert_eq!(card_center_x(2, 2), 732.0, "270 + 462 gap-free");
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

    // ── S2 ring → Slint model builder (WIRE) ──────────────────────────

    #[test]
    fn slice_ui_tiles_from_ring_focused_centered_and_fills_stage() {
        let stage_w = 1920.0;
        let tiles = slice_ui_tiles(6, 2, stage_w);
        assert!(!tiles.is_empty(), "tiles must fill stage");
        let expanded = tiles.iter().filter(|t| t.is_expanded).count();
        assert_eq!(expanded, 1, "exactly one expanded");
        let foc = tiles.iter().find(|t| t.is_expanded).unwrap();
        assert!((foc.x + foc.w / 2.0 - stage_w / 2.0).abs() < 0.001, "expanded dead-center");
        assert_eq!(foc.w, SLICE_EXPANDED_WIDTH);
        let min_x = tiles.iter().map(|t| t.x).fold(f32::MAX, f32::min);
        let max_r = tiles.iter().map(|t| t.x + t.w).fold(f32::MIN, f32::max);
        assert!(min_x <= 0.001 && max_r >= stage_w - 0.001, "edge-to-edge fill");
        // real_index always in range, clone mapping cyclic
        for t in &tiles {
            assert!(t.real_index < 6);
        }
        // different stage width yields different tile count / x, but still centered
        let narrow = slice_ui_tiles(6, 2, 800.0);
        assert!(narrow.len() < tiles.len(), "narrow stage fewer tiles");
        let foc2 = narrow.iter().find(|t| t.is_expanded).unwrap();
        assert!((foc2.x + foc2.w / 2.0 - 400.0).abs() < 0.001);
    }

    #[test]
    fn slice_ui_tiles_fade_subtle_and_bounded() {
        let tiles = slice_ui_tiles(6, 3, 1920.0);
        for t in &tiles {
            assert!((0.0..=1.0).contains(&t.fade), "fade {t:?} out of 0..1");
        }
        let foc_fade = tiles.iter().find(|t| t.is_expanded).unwrap().fade;
        assert!((foc_fade - 1.0).abs() < 0.001, "focused fully opaque");
        // farthest slat should be dimmer than center but not invisible unless beyond 1.2
        let far = tiles.iter().max_by(|a, b| (a.x).partial_cmp(&b.x).unwrap()).unwrap();
        assert!(far.fade <= 1.0 && far.fade >= 0.0);
        // degenerate inputs yield empty
        assert!(slice_ui_tiles(0, 0, 1920.0).is_empty());
        assert!(slice_ui_tiles(6, 0, 0.0).is_empty());
    }

    #[test]
    fn ring_slot_fade_center_and_edge_values() {
        let stage_w = 1920.0;
        let focused = 2usize;
        // centered expanded slot fully opaque
        let center_slot = RingSlot { virtual_index: focused as isize, real_index: focused, x: ring_focused_x(stage_w), width: SLICE_EXPANDED_WIDTH, is_expanded: true };
        assert!((ring_slot_fade(&center_slot, stage_w) - 1.0).abs() < 0.001);
        // far left slot dims toward edge but stays bounded
        let far_left = ring_visible_slots(6, focused, stage_w).into_iter().min_by(|a,b| a.x.partial_cmp(&b.x).unwrap()).unwrap();
        let f = ring_slot_fade(&far_left, stage_w);
        assert!((0.0..=1.0).contains(&f), "far fade {f}");
        // small stage still valid
        let tiny_slot = RingSlot { virtual_index: 0, real_index: 0, x: 0.0, width: SLICE_COLLAPSED_WIDTH, is_expanded: false };
        assert!((0.0..=1.0).contains(&ring_slot_fade(&tiny_slot, 50.0)));
    }
}
