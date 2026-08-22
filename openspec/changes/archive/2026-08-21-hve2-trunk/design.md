# Design: HVE 2 Trunk — skwd-wall Shell Translation

## Technical Approach

State-driven shell as a 1:1 concept translation of skwd-wall (MIT) into Slint. `ShellRoot` → `MainWindow`; `cardContainer` animated `cardWidth/cardHeight` (350ms OutCubic) → `ExpansionState` + animated `set_size`; QML `Loader`-per-screen → Rust `SlotRegistry` (Slint lacks runtime Loader; mounted-screen enum + `if` replaces it). `NavState`, one Rust owner, drives the UI read-only (nav-shell spec). `style.qml`/`Colors.qml` tokens land now in `ui/tokens.slint`; gallery/workshop land modules 2–4 on the seams below. `Composer` untouched; hide/show preserves state (base-window spec).

## Translation Map

`shell.qml` ShellRoot, `Colors.qml`, `style.qml` → `MainWindow` + `HveColors` (existing) + `SkwdTokens` global (**Trunk**). `WallpaperSelector` Scope + cardContainer → `ui/slots/gallery.slint` (module 2 seam; theme arrays + apply callbacks). `SliceDelegate` (skewed carousel) → `ui/gallery/slice-delegate.slint`, Canvas parallelogram (module 3 seam). `MosaicView/Cell` (voronoi) → `ui/gallery/mosaic.slint`, voronoi math in Rust, Canvas polys (module 3). `HexDelegate` (hex columns) → `ui/gallery/hex-delegate.slint`, Canvas hexagon (module 3). `SettingsPanel` (tabs, `_settingsShift`) → `ui/slots/workshop.slint`, panel shift → `set_size` growth (module 4 seam). `components/*`, `FilterButton` → `ui/components/skwd-*.slint`, Canvas shapes, hover stripes (module 4). PanelWindow + card-in-window → single `MainWindow` + `SizePolicy`, window-as-card (**Trunk**).

Idioms: `Behavior{NumberAnimation}` → `animate{duration;easing}` (OutCubic ≈ `cubic-bezier(0.215,0.61,0.355,1)`); `Shape`/`ShapePath` → `Canvas` 2D; `Keys.on*` → `KeyBinding`; `Loader` → `if`+enum; `Repeater` → `for`; singleton → `export global`.

## Design Tokens (port to `ui/tokens.slint`)

Fonts: Roboto Condensed (body), Roboto (heading), Roboto Mono (code). Sizes: 9, 11, 12, 13, 14, 16, 18, 24, 32, 36. Radii: 2, 4, 8, 12, 16, 20, 40. Spacing: 2, 4, 8, 12, 16, 20. Anim (ms): 100, 150, 200, 250, 300, 350 (expand), 400, 1000, OutCubic default. Border: 1, 2, 3. Opacity: .35, .5, .6. Colors (→ `HveColors`): primary #ffb4ab, tertiary #8bceff, surface #1d100e, surfaceVariant #5a413e, surfaceContainer #2c1f1d, outline #a98986, fallbackAccent #4fc3f7. Gallery geometry: slice 135/924/520/−30/skew 35/visible 12; hex 140/3/7; grid 6×3, thumb 300×169; mosaic 48, relax 2, 1500×800; uiScale 1.0.

## Architecture Decisions

**D1 State owner**: `NavState` in Rust (`Arc<Mutex<>>` in `Shell`) — spec forbids UI-held state.
**D2 Rapid-nav**: FIFO `VecDeque<NavCommand>`; one transition per `process_next`; final = last; same-target = no-op.
**D3 Slot contract**: `trait Slot` + `HashMap<Screen, Box<dyn Slot>>`; register = one insert. Rejected: hardcoded conditions (module add edits shell); component-returning trait (impossible). Hooks + i18n label; `if`+enum replaces `Loader`.
**D4 Window sizing**: stepped `Timer` 16ms, skwd `animExpand` 350ms OutCubic cadence. Rejected: one `set_size` jump (layout jumpiness). Clamp: Slint `min-width/height` + `SizePolicy::clamp`.
**D5 Composer**: `Shell::sync_after_show` from existing show paths; `prewarm_tabs` body re-pointed.
**D6 Tokens**: `export global SkwdTokens` (the `pragma Singleton` analog) over Rust consts (verbose).
**D7 Shapes**: `Canvas` 2D + skwd geometry math verbatim over border-image (can't skew + chamfer).
**D8 Keyboard = mouse**: both emit `NavCommand` via `card-activated`/`back-activated`.

## Requirement Traceability

nav-shell: State Model → `NavState` (D1); Expansion → `process_next` + FIFO (D2); Slot Mounting → `SlotRegistry` + `StubSlot` (D3); Keyboard/Mouse → single `NavCommand` (D8); i18n → `shell.*` keys + `tr.rs`. base-window: Single Window → one `MainWindow` 900×680; Growth → `SizePolicy` + timer (D4); Minimum Size → min props + clamp (D4); Composer → `sync_after_show` (D5).

## Data Flow

Input (mouse click / keyboard Return on a card) → `card-activated(screen)` → `Shell::dispatch` → `NavState.push(Expand)` → FIFO `process_next` → `NavTransition { mount, unmount, expand }` → registry mount/unmount + stepped `set_size` + UI props (`mounted-screen`, `expanded`).

## File Changes

`src/shell/mod.rs` Create `Shell` root (NavState, SlotRegistry, SizePolicy, Tr; dispatch, sync_after_show). `src/shell/nav.rs` Create Screen, ExpansionState, NavCommand, NavTransition, NavState (pure). `src/shell/slots.rs` Create `Slot` trait, `SlotRegistry`, `StubSlot` (cfg test). `src/shell/size.rs` Create `SizePolicy` (base/expanded/min; target, clamp). `ui/tokens.slint` Create `SkwdTokens` global. `ui/shell.slint` Create chrome, slot area, FocusScope/KeyBinding. `ui/main.slint` Modify: mount shell, remove clip-switch block. `src/main.rs` Modify: build `Shell`, register stubs, wire callbacks/sizes/sync, port `nav_logic`. `src/callbacks.rs` Modify: `card-activated`, `back-activated`, `nav-move`. `i18n/en.json` + `i18n/es.json` Modify: `shell.*` keys. `src/tr.rs` unchanged.

## Interfaces / Contracts

```rust
// src/shell/nav.rs — pure, unit-testable without Slint
pub enum Screen { Home, Gallery, Workshop }
pub enum ExpansionState { Collapsed, Expanded(Screen) }
pub enum NavCommand { Expand(Screen), Collapse, Back }
pub struct NavTransition { mount: Option<Screen>, unmount: Option<Screen>, expand: bool }
pub struct NavState;  // screen, expansion, queue VecDeque<NavCommand>, focused_card
// methods: new (Home+Collapsed, focus 0), screen, expansion, focused_card, move_focus, push,
// process_next -> Option<NavTransition>   // None = same/unknown target or drained

pub trait Slot: Send + Sync { fn screen(&self) -> Screen; fn on_mount(&self); fn on_unmount(&self); fn label(&self, tr: &Tr) -> SharedString; }
pub struct SlotRegistry;  // HashMap<Screen, Box<dyn Slot>>; register, get
pub struct SizePolicy;    // base/expanded/min (f32,f32); target, clamp
```

Sizes: base `900×680`, expanded `1200×800`, min `800×580` (skwd geometry scaled; matches current min). `set_size(LogicalSize)` stepped 16ms via `slint::Timer`.

## Testing Strategy

Unit (pure `cargo test`): `NavState` — initial, expand happy path, no-op same target, unknown target, queue order (final = last), Back, `move_focus`; `SizePolicy::target/clamp`; `SlotRegistry` + `StubSlot` hooks. Integration (`i-slint-backend-testing`, `#[ignore]`, `--test-threads=1`): base size; `card-activated` → `mounted-screen` + `set_size`; collapse shrinks; keyboard == mouse; Escape; `toggle_tray` restores state; port `nav_logic`. E2E: none (per `config.yaml`).

## Migration / Rollout

No migration. Additive: `src/shell/`, `ui/shell.slint`, `ui/tokens.slint`; old panel logic kept until module 2. Rollback = revert. **MIT credit**: visual language, geometry, tokens translated from skwd-wall (MIT, © liixini, https://github.com/liixini/skwd-wall) — credit header in every translated file. No GPL (hyprmod).

## Open Questions

- [ ] Stepped cadence and expanded size (16ms fixed vs 350ms OutCubic-driven; `1200×800` vs skwd 924-wide) — verify empirically in apply.
- [ ] `focused_card` in `NavState` vs per-module focus owned by gallery — confirm split.