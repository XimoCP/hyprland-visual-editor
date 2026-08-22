# Design: Theme Gallery (Module 2)

## Change ID
`theme-gallery`

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Shell (Module 1)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────┐             │
│  │  NavState   │  │ SlotRegistry │  │ SizePolicy │             │
│  └──────┬──────┘  └──────┬───────┘  └─────┬──────┘             │
│         │                │                │                      │
│         ▼                ▼                ▼                      │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    GallerySlot (NEW)                     │    │
│  │  ┌─────────────────┐  ┌─────────────────────────────┐   │    │
│  │  │ ThemeGalleryModel │  │      GalleryView (enum)    │   │    │
│  │  │  (data layer)     │  │  ┌─────┐ ┌─────┐ ┌─────┐  │   │    │
│  │  │                   │  │  │Slice│ │ Hex │ │Mosaic│  │   │    │
│  │  └────────┬──────────┘  │  └─────┘ └─────┘ └─────┘  │   │    │
│  │           │             └──────────────┬──────────────┘   │    │
│  │           ▼                              │                │    │
│  │  ┌──────────────────────────────────────▼──────────────┐  │    │
│  │  │              ThemeCardDelegate (shared)             │  │    │
│  │  │  • Image + color swatches + border preview + shader │  │    │
│  │  │  • Click → apply | Right-click → flip/back-face     │  │    │
│  │  └─────────────────────────────────────────────────────┘  │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Engine (SEALED - Module 0)                 │
│  ┌──────────────────┐    ┌──────────────────────────────────┐  │
│  │  ThemeManager    │    │  Providers: Noctalia v4/v5,      │  │
│  │  • list()        │    │  HVE Presets, Hyprland Settings  │  │
│  │  • apply(name,   │    │                                 │  │
│  │    reload_cb)    │    │                                 │  │
│  └──────────────────┘    └──────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Watcher (bash inotify) → reload_cb → UI refresh          │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Component Design

### 1. ThemeGalleryModel (Data Layer)
**Location**: `src/shell/gallery/model.rs`

```rust
pub struct ThemeGalleryModel {
    theme_manager: Arc<Mutex<ThemeManager>>,
    current_style: GalleryStyle,
    themes: Vec<ThemeCard>,
    focused_index: usize,
}

pub struct ThemeCard {
    pub name: String,
    pub colors: ColorScheme,        // primary, secondary, tertiary, accent, surface
    pub border: BorderConfig,       // size, radius, color
    pub background: Background,     // Wallpaper(path) | Solid(color)
    pub shader: Option<String>,
    pub saved_at: String,
    pub is_active: bool,
    pub providers: Vec<String>,
    pub thumb_path: Option<PathBuf>, // cached thumbnail
}

pub enum GalleryStyle {
    Slice,   // Parallelogram carousel
    Hexagon, // Honeycomb grid
    Mosaic,  // Voronoi organic
}
```

**Responsibilities**:
- Wraps `ThemeManager.list()` → maps to `ThemeCard` vector
- Generates/caches thumbnails via `Engine::render_theme_preview()`
- Exposes `focused_index` for keyboard navigation
- Emits change signals for Slint model binding
- Style switch: `set_style(GalleryStyle)` → rebuilds view data

### 2. GalleryView Trait + 3 Implementations
**Location**: `src/shell/gallery/views/`

```rust
pub trait GalleryView: Send + Sync {
    fn style(&self) -> GalleryStyle;
    fn build_slint_model(&self, model: &ThemeGalleryModel) -> ModelRc<ThemeCard>;
    fn on_focus_change(&mut self, idx: usize);
    fn on_style_switch(&mut self, from: GalleryStyle);
    fn handle_key(&mut self, key: Key) -> bool; // true = consumed
    fn handle_click(&mut self, idx: usize, button: MouseButton) -> GalleryAction;
}
```

#### 2.1 SliceView (Parallelogram Carousel)
- **Slint**: Horizontal `ListView` with custom delegate
- **Delegate**: `SliceDelegate` (Canvas 2D parallelogram shape)
- **Animation**: `width` animate 108↔768, 350ms `OutCubic`
- **Flip**: 180° Y-rotation on right-click, `InOutQuad` 400ms
- **Video**: `Video` element loaded on current item after `Timer` delay
- **Hit-test**: Custom `containmentMask` (parallelogram geometry)

#### 2.2 HexagonView
- **Slint**: `GridView` with custom delegate, honeycomb positioning via `x`/`y` offsets
- **Delegate**: `HexDelegate` (Canvas 2D hexagon path)
- **Pull-out**: Scale + dashed border animate on selection
- **Parallax**: Thumbnail `x`/`y` offset based on mouse position
- **Flip**: Signal `flipRequested(data, global_x, global_y)` → overlay component

#### 2.3 MosaicView
- **Slint**: Custom `Repeater` over precomputed Voronoi cells
- **Voronoi**: Ported from skwd-wall `_buildTile()` + Lloyd relaxation
- **Cells**: Stored as polygon vertices + bbox + centroid + cloudR
- **Kinetic scroll**: `_velocity` + friction 0.90, 16ms timer
- **Cloud opacity**: `(dx²/cloudRx² + dy²/cloudRy²)` smoothstep
- **Stripe tiling**: Dual `_stripeA`/`_stripeB` for infinite scroll
- **Reveal**: Staggered `_imageAlpha` per cellKey

### 3. ThemeCardDelegate (Shared Visual)
**Location**: `ui/gallery/ThemeCardDelegate.slint`

```slint
export component ThemeCardDelegate inherits Item {
    in property <ThemeCard> card;
    in property bool is_current;
    in property bool is_hovered;
    in property <GalleryStyle> style;
    callback clicked();
    callback right_clicked();
    callback flip_requested(); // for hex/mosaic overlay

    // Visual: image + color dots (primary/secondary/tertiary/accent)
    //         + border preview line + shader badge
    //         + active checkmark overlay
}
```

### 4. GallerySlot (Mountable Module)
**Location**: `src/shell/gallery/slot.rs`

```rust
pub struct GallerySlot {
    screen: Screen::Gallery,
    model: Rc<RefCell<ThemeGalleryModel>>,
    view: Box<dyn GalleryView>,
    shell: Rc<RefCell<Shell>>, // for dispatch/size
}

impl Slot for GallerySlot {
    fn screen(&self) -> Screen { Screen::Gallery }
    fn on_mount(&self) {
        // Prewarm thumbnails, set initial focus
        self.model.borrow_mut().prewarm_visible();
        self.shell.borrow().dispatch(NavCommand::Expand(Screen::Gallery));
    }
    fn on_unmount(&self) {
        // Cancel timers, release video loaders
        self.view.cleanup();
    }
    fn label(&self, tr: &Tr) -> SharedString {
        tr.tr_shared("shell.slots.gallery", "Gallery")
    }
}
```

**Integration in `main.rs`**:
```rust
// Replace StubSlot with real GallerySlot
let gallery_slot = GallerySlot::new(shell.clone(), theme_manager.clone());
Shell::register_slot(&shell, Box::new(gallery_slot));
```

### 5. Settings Panel Expansion (Card → Settings)
**Flow**:
1. Gallery expanded, card focused → user clicks/presses Enter
2. `GallerySlot` emits `ExpandToSettings(theme_name)` custom event
3. `Shell` handles: `dispatch(Custom(ExpandToSettings))` → swaps mounted slot to `SettingsPanel`
4. `SizePolicy::target(expanded=true, settings=true)` computes new target
4. Stepped animator walks window to settings size
5. Back/Esc → `dispatch(Back)` → swaps back to Gallery view at expanded size

### 6. Style Tokens Port (1:1 skwd-wall)
**Location**: `ui/tokens.slint` (extend existing)

```slint
// Fonts
readonly property string font-family: "Roboto Condensed";
readonly property string font-family-heading: "Roboto";
readonly property string font-family-mono: "Roboto Mono";
readonly property string font-family-icons: "Material Design Icons";
readonly property string font-family-nerd: "Symbols Nerd Font";

// Radius scale
readonly property int radius-tiny: 2;
readonly property int radius-small: 4;
readonly property int radius-medium: 8;
readonly property int radius-large: 12;
readonly property int radius-xlarge: 16;
readonly property int radius-round: 20;
readonly property int radius-circle: 40;

// Spacing scale
readonly property int spacing-tiny: 2;
readonly property int spacing-small: 4;
readonly property int spacing-medium: 8;
readonly property int spacing-large: 12;
readonly property int spacing-xlarge: 16;
readonly property int spacing-xxlarge: 20;

// Animation durations
readonly property int anim-very-fast: 100;
readonly property int anim-fast: 150;
readonly property int anim-normal: 200;
readonly property int anim-enter: 250;
readonly property int anim-expand: 350;
readonly property int anim-slow: 400;
readonly property int anim-spin: 1000;

// Colors
readonly property color fallback-accent: #4fc3f7;
readonly property int border-thin: 1;
readonly property int border-medium: 2;
readonly property int border-thick: 3;
```

**Usage in delegates**:
```slint
Behavior on width { NumberAnimation { duration: anim-expand; easing.type: Easing.OutCubic } }
Behavior on angle { NumberAnimation { duration: anim-slow; easing.type: Easing.InOutQuad } }
```

### 7. Instant Apply Pipeline
```rust
// In GallerySlot / ThemeCardDelegate callback
fn on_card_clicked(&self, theme_name: String) {
    let shell = self.shell.clone();
    let tm = self.theme_manager.clone();
    
    tm.lock().unwrap().apply(theme_name, move || {
        // Reload callback runs after all providers post_apply
        // Trigger watcher refresh → UI updates
        if let Some(win) = shell.borrow().window.upgrade() {
            win.set_needs_repaint(); // or specific refresh call
        }
        Ok(())
    });
}
```

### 8. Performance Optimizations

| Technique | Implementation |
|-----------|----------------|
| Virtualization | Slint `ListView`/`Repeater` only renders visible + buffer |
| Image preheat | 120ms `Timer` before loading full thumbnail |
| SourceSize caps | `sourceSize.width: 400` / `sourceSize.height: 720` |
| Thumbnail cache | `cache: true`, `asynchronous: true` |
| Voronoi cache | Rebuild only on filter/theme count change |
| Warmup | Mosaic preloads first 64 cells on activate |
| Video lazy-load | `Loader` activates only on current/selected item |

## File Structure

```
src/
├── shell/
│   ├── gallery/
│   │   ├── mod.rs           // exports
│   │   ├── model.rs         // ThemeGalleryModel, ThemeCard
│   │   ├── slot.rs          // GallerySlot impl Slot
│   │   ├── views/
│   │   │   ├── mod.rs       // GalleryView trait + registry
│   │   │   ├── slice.rs     // SliceView
│   │   │   ├── hexagon.rs   // HexagonView
│   │   │   └── mosaic.rs    // MosaicView
│   │   └── delegate.rs      // shared card logic
│   └── ... (existing)
ui/
├── gallery/
│   ├── ThemeCardDelegate.slint
│   ├── SliceDelegate.slint
│   ├── HexDelegate.slint
│   ├── MosaicCell.slint
│   ├── MosaicView.slint
│   └── GalleryChrome.slint  // toolbar with style switcher
├── tokens.slint             // extended with style tokens
└── ...
```

## Integration Points

| Point | Module 1 Trunk | Module 2 Gallery |
|-------|----------------|------------------|
| Slot registration | `SlotRegistry::register` | `GallerySlot` replaces `StubSlot` |
| Navigation | `NavCommand::Expand(Gallery)` | Mounts GallerySlot, starts animator |
| Size | `SizePolicy::target(expanded=true)` | Adds `settings` variant for panel |
| Sync | `Shell::sync_global_after_show` | Restores Gallery expanded state |
| i18n | `Tr` system | Gallery labels via `tr_shared` |

## Risk Mitigations

| Risk | Design Mitigation |
|------|-------------------|
| Hyprland shader flicker | Detect via frame timing; show overlay; document |
| hyprmod conflict | Process check at startup; yield animations if detected |
| Canvas 2D perf | Profile on 100+ cards; fallback to Rectangle approx if needed |
| Voronoi CPU | Cache cells; rebuild async; max iterations configurable |
| Memory | Thumbnail cache bounded (LRU 200 items) |

## Testing Strategy

| Layer | Approach |
|-------|----------|
| Unit | `ThemeGalleryModel` mapping, Voronoi math, style tokens |
| Integration | Headless Slint: mount GallerySlot, dispatch Expand, verify size animator |
| UI | `i_slint_backend_testing`: click card → verify apply called, theme list refreshes |
| E2E | `cargo test -- --ignored` with real window: keyboard nav, style switch, flip |