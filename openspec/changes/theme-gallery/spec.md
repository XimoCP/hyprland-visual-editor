# Spec: Theme Gallery (Module 2)

## Change ID
`theme-gallery`

## Requirements

### R1 — ThemeCard Data Model
**The gallery shall present each saved theme as a complete look card.**
- Card carries: theme name, color scheme (primary/secondary/tertiary/accent/surface), border config (size, radius, color), background (wallpaper path or solid), shader name, saved timestamp, active flag, provider list
- Data sourced from `ThemeManager.list()` → `ThemeInfo` + provider capabilities
- Card is read-only view; no navigation state in card (nav state lives in `NavState`)

### R2 — Three Presentation Styles (1:1 skwd-wall)
**The gallery shall support three user-switchable presentation styles, visually matching skwd-wall V1.**

#### R2.1 — Slice/Parallelogram Carousel
- Skewed cards (skew offset ~28px), collapsed width ~108px, expanded width ~768px
- Horizontal ListView, current item centered and expanded
- 350ms `OutCubic` width animation on focus change
- Right-click → 180° Y-axis flip → back face with metadata, tags, actions (VIEW, RETAG, DELETE, STEAM)
- Video preview on current item after 100-300ms delay (configurable)
- Glow border: primary color when selected, subtle on hover
- Shadow shape matching parallelogram geometry
- Containment mask for non-rectangular hit testing

#### R2.2 — Hexagon Grid
- Honeycomb layout, hex radius ~140px
- Pull-out animation on selection (expands with dashed border)
- Subtle parallax on thumbnails
- Video preview on selected item
- Right-click → flipRequested signal with global coords for back face overlay
- Point-in-hexagon hit testing

#### R2.3 — Voronoi Mosaic
- Voronoi tessellation + Lloyd relaxation (configurable iterations, default 3)
- Organic cells with shard gap
- Kinetic scroll with friction 0.90
- Cloud opacity: circular viewport focus (inner radius 0.55, outer 1.05)
- Infinite horizontal scroll via dual-stripe tiling
- Staggered image reveal per cell (cellKey-based delay)
- Warmup preload of first N cells

### R3 — Style Switching
**User shall switch presentation styles at runtime without losing state.**
- Style selector in gallery chrome (toolbar or settings gear)
- Switch preserves: scroll position, focused card, expanded/collapsed state
- Transition between styles: cross-fade 200ms

### R4 — Instant Apply
**Clicking a theme card shall apply the complete look instantly.**
- Left-click on non-current card → `ThemeManager.apply(theme_name, reload_callback)`
- Two-pass apply: all providers write files → all post_apply hooks → reload callback
- Reload callback triggers watcher → UI refreshes colors/borders/shaders
- Perceived latency < 200ms from click to visual feedback
- Active theme indicator updates immediately

### R5 — Window Mutation (Card → Settings Expansion)
**Clicking a card when already expanded shall mutate the window into settings panel.**
- Precondition: Gallery is expanded (window at 1200×800), card is current
- Click current card → `Shell::dispatch` custom command → window animates to settings size
- Same `NavState` screen (Gallery), but slot swaps to `SettingsPanel` view
- `SizePolicy` computes settings target size; stepped animator walks there
- Back/Esc → collapse to Gallery view at expanded size
- Hyprland animates window frame (native compositor animation)

### R6 — Style Tokens (1:1 skwd-wall)
**Visual tokens shall match skwd-wall style.qml exactly.**
- Fonts: Roboto Condensed (UI), Roboto (headings), Roboto Mono (code), Material Icons, Symbols Nerd Font
- Radius scale: 2, 4, 8, 12, 16, 20, 40
- Spacing scale: 2, 4, 8, 12, 16, 20
- Animation durations: animVeryFast=100, animFast=150, animNormal=200, animEnter=250, animExpand=350, animSlow=400, animSpin=1000
- Easing: `OutCubic` for expand/collapse, `InOutQuad` for flips, `InCubic`/`OutCubic` for fades
- fallbackAccent: #4fc3f7
- Border widths: thin=1, medium=2, thick=3
- Opacity: dim=0.35, muted=0.5, subtle=0.6

### R7 — MIT Credit Attribution
**Gallery shall display MIT credit to skwd-wall (liixini) in footer.**
- Persistent footer: "Visual language translated from skwd-wall (MIT, © liixini)"
- Link to https://github.com/liixini/skwd-wall
- No GPL code from hyprmod used

### R8 — Performance Budgets
**Gallery shall meet performance targets for large theme collections.**
- 100+ themes: 60fps scroll, <16ms frame time
- Image preheat: 120ms timer before loading full thumb
- Thumbnail sourceSize capped at 400×720 (slice) / 1.3×hexRadius (hex) / mosaic tile bounds
- Virtualized ListView/Repeater: only visible + 1 buffer items rendered
- Voronoi rebuild only on filter change or theme count change (not on scroll)

### R9 — Integration with Module 1 Trunk
**Gallery slot shall integrate seamlessly with existing shell.**
- `GallerySlot` implements `Slot` trait (screen=Gallery, on_mount/on_unmount, label i18n)
- Registered in `main.rs` replacing `StubSlot::new(Screen::Gallery)`
- `Shell::dispatch(Expand(Gallery))` mounts slot, starts size animator
- `Shell::sync_global_after_show` restores expanded gallery after compositor hide/show
- Keyboard navigation: Home screen focus → Enter/Space → Expand(Gallery) → arrow keys move focus in gallery

---

## Scenarios

### S1 — Gallery Opens from Home (Happy Path)
**Given** HVE window at base size (900×680), Home screen mounted
**When** User presses Enter/Space on Gallery card (index 0)
**Then** Shell expands to 1200×800 (350ms OutCubic), Gallery slot mounts, first theme card focused

### S2 — Slice Carousel Navigation
**Given** Gallery open in Slice style, 5+ themes
**When** User presses Right/Left arrow keys
**Then** Focus moves to next/prev card (clamped), card expands to 768px, others collapse to 108px, 350ms OutCubic animation

### S3 — Slice Carousel Flip Back Face
**Given** Gallery in Slice style, card focused
**When** User right-clicks focused card
**Then** Card flips 180° Y-axis (400ms InOutQuad), back face shows metadata, tags, action buttons

### S4 — Hexagon Grid Selection
**Given** Gallery open in Hexagon style
**When** User clicks a hex cell
**Then** Cell pulls out with dashed border, parallax activates, video preview starts after delay

### S5 — Voronoi Mosaic Kinetic Scroll
**Given** Gallery open in Mosaic style, 50+ themes
**When** User wheel-scrolls horizontally
**Then** Kinetic scroll with friction 0.90, cloud opacity follows viewport center, cells fade at edges

### S6 — Style Switch Preserves State
**Given** Gallery in Slice style, scrolled to card 3, card 3 focused
**When** User switches to Hexagon style via toolbar
**Then** Cross-fade 200ms, card 3 remains focused/selected, scroll position mapped

### S7 — Instant Apply Theme
**Given** Gallery open, theme "Cyberpunk" not active
**When** User clicks "Cyberpunk" card
**Then** ThemeManager.apply("Cyberpunk") runs, watcher fires, UI colors/borders/shader update, active indicator moves to "Cyberpunk", <200ms perceived

### S8 — Apply Already Active Theme (No-Op)
**Given** Gallery open, theme "Nord" is active
**When** User clicks "Nord" card
**Then** No apply call, visual feedback only (brief pulse), active indicator stays

### S9 — Card Click Expands to Settings
**Given** Gallery expanded, card 1 focused, window at 1200×800
**When** User clicks focused card 1 (Enter/Space or left-click)
**Then** Shell dispatches custom command, window animates to settings size, Gallery slot swaps to SettingsPanel view, back hint shows "Esc collapses"

### S10 — Settings Back to Gallery
**Given** Settings panel open (expanded from Gallery card)
**When** User presses Esc or Back
**Then** Window animates back to 1200×800, SettingsPanel unmounts, Gallery slot remounts with same focused card

### S11 — Compositor Hide/Show Restores Gallery
**Given** Gallery expanded, window hidden via compositor (SUPER+C)
**When** Window shown again via composer
**Then** `Shell::sync_global_after_show` fires, window restores to 1200×800, Gallery slot mounted, focused card preserved

### S12 — Keyboard Navigation Home → Gallery → Card
**Given** Home screen, focus on Gallery card (index 0)
**When** User presses Enter → Right arrow → Enter
**Then** Expand Gallery → focus moves to card 1 → card 1 applies theme

### S13 — Focus Clamp at Gallery Bounds
**Given** Gallery open, focus on last card
**When** User presses Right arrow
**Then** Focus stays on last card (clamped), no wrap

### S14 — Unregistered Slot Falls Back
**Given** Gallery slot not registered (edge case)
**When** User tries to expand Gallery
**Then** NavState transitions but mount hook never fires, size still animates (Module 1 behavior)

### S15 — Rapid Navigation Queue
**Given** Home screen
**When** User rapidly presses: Enter (Gallery) → Enter (Workshop) → Back
**Then** Final state = Back (Home collapsed), queue drained FIFO

### S16 — Video Preview Activation
**Given** Gallery in Slice style, current card has video wallpaper
**When** Card becomes current (focused)
**Then** After 100-300ms delay, video preview starts playing muted, looped, cropped to card

### S17 — Video Preview Deactivation
**Given** Video preview playing on current card
**When** Focus moves to another card
**Then** Video preview stops, loader releases resources

### S18 — Empty Gallery State
**Given** No themes saved (`ThemeManager.list()` returns empty)
**When** Gallery opens
**Then** Empty state illustration + "No themes yet. Save your current setup as a theme." + Save button

### S19 — Theme Delete from Gallery
**Given** Gallery open, theme "Old" exists
**When** User flips card → clicks DELETE → confirms
**Then** ThemeManager.delete("Old"), gallery list refreshes, focus moves to next card or previous

### S20 — Theme Rename from Gallery
**Given** Gallery open, theme "Draft" exists
**When** User flips card → clicks RENAME → enters "Polished" → confirms
**Then** ThemeManager.rename("Draft", "Polished"), gallery list refreshes, active indicator updates if was active

### S21 — Shader Flicker Graceful Degradation
**Given** Theme with shader applied, Hyprland flicker detected (>2s frame delay)
**When** Shader change completes
**Then** Gallery shows transient "Applying shader…" overlay, no crash, UI responsive

### S22 — hyprmod Conflict Yield
**Given** hyprmod running and managing animations
**When** HVE gallery opens
**Then** HVE detects hyprmod (IPC or process check), yields animation control, gallery static preview still works

### S23 — Accessibility: Reduced Motion
**Given** System prefers-reduced-motion=true
**When** Gallery animations would run
**Then** All animations disabled (duration=0), instant transitions, no kinetic scroll, no flip animation