# Delta for gallery-immersive

> **Changelog 2026-08-23:** Hexagon removed per user decision #632 — gallery now Slice (principal) + Mosaic only. Requirement Hexagon Grid Refinement marked REMOVED below; thumbnail wording updated from 3 views to 2.

New capability — no archived spec exists. MODIFIED supersedes inherited theme-gallery requirements (never archived; its spec.md is the lineage reference). PRESERVED restates still-valid contracts.

## MODIFIED Requirements

### Requirement: Slice Carousel Geometry & Navigation
(Previously: theme-gallery R2.1/R6 — 108/768px cards, positive gaps, 28px skew, static layout, swapped tokens.)

Slices MUST render at skwd-wall metrics: collapsed width 135px, expanded width 924px, height 520px, spacing −30px (overlap), 35px horizontal X-shear parallelogram, corner radius 0 (opt-in 16). Tokens MUST read `gallery-slice-h=520px`, `gallery-slice-expanded-w=924px` (currently swapped). Overlap layout MUST use manual x placement. Current card MUST snap pixel-centered. Wheel MUST step index ±1 with 400ms debounce-snap; hover MUST select after >2px travel; arrow keys stay clamped, no wrap.

#### Scenario: Exact geometry

- GIVEN Slice view laid out with N themes
- WHEN positions compute
- THEN widths 135/924, height 520, 30px overlap, 35px shear, radius 0, current centered

#### Scenario: Wheel debounce-snap

- GIVEN current index 3
- WHEN user wheels 3 detents within 300ms
- THEN index advances exactly 1, snapping to center after 400ms idle

#### Scenario: Hover select threshold

- GIVEN pointer over a non-current slice
- WHEN cursor travels >2px onto it and settles
- THEN that card becomes current

#### Scenario: Focus clamp (preserved)

- GIVEN focus on last card
- WHEN Right arrow pressed
- THEN focus stays clamped, no wrap

### Requirement: Floating FilterBar Chrome
(Previously: theme-gallery R3 — selector in permanent toolbar.)

Permanent toolbar MUST be removed. Style/filter selection MUST live in a floating top-center bar: top-margin 30px, maxWidth parent−20, hidden state a 24px hover-reveal strip, auto-hide after 250ms without hover. Buttons MUST be skewed pills h24, skew 10px, −10px row overlap, active filled primary. Style switches cross-fade 200ms preserving focused card.

#### Scenario: Hover reveal

- GIVEN FilterBar hidden with strip visible
- WHEN pointer enters the strip
- THEN the full pill bar expands

#### Scenario: Auto-hide

- GIVEN FilterBar revealed
- WHEN pointer leaves for 250ms
- THEN bar collapses to the 24px strip

### Requirement: Real Flip With Metadata Back Face
(Previously: theme-gallery R2.1 — crossfade pseudo-flip, placeholder back face; STEAM action removed.)

Right-click MUST flip Y-axis 0↔180° over 400ms InOutQuad, faces swapping at 90°. Back face MUST show: name 13px bold tertiary, ext·dims·size row 10px, favourite toggle (parallelogram 48×24, knob 22×18, 150ms), tag pills h26/11px, actions VIEW/RETAG/DELETE h30 uppercase.

#### Scenario: Flip reveals metadata sheet

- GIVEN focused current card
- WHEN right-clicked
- THEN faces swap at 90°; back face renders name, meta row, tags, three actions

#### Scenario: Favourite toggle animates

- GIVEN back face visible
- WHEN favourite toggled
- THEN parallelogram knob slides within 150ms, state persists

### Requirement: Hexagon Grid Refinement — REMOVED (Hex dropped 2026-08-23)
> **Removed per 2026-08-23 decision #632: Hexagon dropped, Slice+Mosaic only.** Former content kept for traceability — not in scope, no implementation, no tests.

<!--
Former Requirement (retired):
(Previously: theme-gallery R2.2 — radius ~140, dashed pull-out, no arc/falloff.)
Hex view MUST use radius 140, cell 280×243, gap 6, staggered columns (odd offset +h/2).
Arc MUST bow columns by yOffset = −normDist²·radius·1.2. Pull-out MUST grow to bigRadius = radius·3 from source cell position over 400ms.
Scenarios: Arc offsets (yOffset = −normDist²·140·1.2) and Pull-out origin (bigR 420, 400ms) — both retired.
-->

### Requirement: Mosaic Refinement
(Previously: theme-gallery R2.3 — Lloyd default 3, unparameterized stagger.)

Mosaic MUST relax sites with exactly 2 Lloyd iterations per activation; cells reveal staggered by 20+(key%7)·14ms, alpha 0→1 over 200ms OutCubic. Idle grading brightness −0.05 / saturation −0.1; hover +0.05 brightness, primary@0.18 tint, 2px stroke. Kinetic friction 0.90 preserved.

#### Scenario: Stagger reveal bounds

- GIVEN rebuild after filter change
- WHEN cells appear
- THEN delays span 20–104ms by key%7, each fading 200ms

## ADDED Requirements

### Requirement: Depth Cues

Carousel MUST layer by distance: opacity fades linearly to 0 at 1.2 normalized distance; z-order current 100 / hovered 90 / others 50−min(|i−cur|,50). Each slice casts a shaped parallelogram shadow — current x4/y10 α0.5, others x2/y5 α0.3 (200ms animation). Dim overlay 0 current / 0.15 hover / 0.4 idle (200ms). Glow stroke: current primary w3, hover primary@0.4 w1, idle black@0.6 w1.

#### Scenario: Edge fade falloff

- GIVEN carousel wider than viewport
- WHEN a slice reaches 1.2 normalized distance
- THEN its opacity reaches 0 linearly

#### Scenario: Light stack by state

- GIVEN current, hovered, idle slices visible
- WHEN states evaluate
- THEN z=100/90/(50−d), dims 0/.15/.4, shadows x4y10 α0.5 vs x2y5 α0.3

### Requirement: Real Wallpaper Thumbnails

Both views (Slice + Mosaic) MUST render real wallpaper images decoded with sourceSize 400×720 cover-cropped, fading in 200ms OutCubic over a skeleton shimmer placeholder. (Previously "All three views" — Hex removed 2026-08-23.)

#### Scenario: Thumbnail pipeline

- GIVEN themes with image wallpapers
- WHEN any view builds delegates
- THEN thumbs decode ≤400×720 source size, cover-fit, shimmer until ready

### Requirement: Entry Choreography

On open, backdrop MUST fade 300ms leaving live desktop visible (default alpha 0); items animate in staggered, opacity 0→1 + scale 0.85→1, 250ms OutCubic.

#### Scenario: Staggered entrance

- GIVEN gallery opening fullscreen
- WHEN items mount
- THEN each fades/scales 250ms OutCubic staggered; backdrop fades 300ms

## PRESERVED Requirements

Carried from theme-gallery unchanged.

### Requirement: Instant Apply

Click applies the complete look via ThemeManager, <200ms perceived; active indicator updates immediately; clicking active theme pulses visually only.

#### Scenario: Apply latency (preserved)

- GIVEN inactive theme focused
- WHEN clicked
- THEN look applies and UI reflects it within 200ms perceived

### Requirement: Settings Mutation & Esc Collapse

Clicking current card mutates window into settings panel; Esc/Back returns to Gallery with same focused card.

#### Scenario: Esc collapse (preserved)

- GIVEN settings panel open from Gallery
- WHEN Esc pressed
- THEN Gallery remounts with same focused card

### Requirement: Empty State

With zero themes, gallery shows empty-state illustration, guidance copy, Save action.

#### Scenario: Empty gallery (preserved)

- GIVEN ThemeManager.list() empty
- WHEN gallery opens
- THEN empty state renders with Save button
