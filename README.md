# Hyprland Visual Editor (HVE)

> **Controlá visualmente animaciones, bordes y shaders de Hyprland.** Sin tocar configs a mano.

![Rust](https://img.shields.io/badge/Rust-1.75%2B-dea584?logo=rust)
![Slint](https://img.shields.io/badge/Slint-1.11-2379f4)
![License](https://img.shields.io/badge/License-MIT-green)
![Status](https://img.shields.io/badge/Status-Stable-brightgreen)
![Hyprland](https://img.shields.io/badge/Hyprland-0.40%2B-58b9ff)

---

## ¿Qué es HVE?

**Imaginate que Hyprland es una casa.** Sin HVE, para cambiar el color de una pared tenés que ir ladrillo por ladrillo editando archivos de config, acordarte qué variables van en cada archivo, y esperar que no se rompa nada al recargar.

**HVE es el arquitecto.** Tiene un tablero con botones: seleccionás una animación, elegís un borde, ajustás el grosor, y él se encarga de generar los planos correctos. No toca tus archivos originales. No rompe nada.

### Sistema Fragments & Assembly

HVE nunca modifica tu `hyprland.conf` o `hyprland.lua` directamente (excepto para inyectar un `source` o `dofile` al activar el sistema). El flujo es:

1. **Fragmentos**: cada preset que activás (una animación, un borde, un shader, un tamaño de borde) se copia como un archivo individual dentro de `~/.cache/hve/fragments/`.
2. **Ensamblado**: `assemble.sh` junta todos los fragmentos activos en un **overlay** único: `~/.cache/hve/overlay.{lua,conf}`.
3. **Inyección**: HVE le dice a Hyprland que incluya ese overlay con `hyprctl reload`. Nunca escribe sobre tu config original.
4. **Desactivación**: cuando desactivás HVE desde la UI, borra los marcadores de tu config y el overlay. Todo vuelve a como estaba.

No destructivo por diseño. Si cerrás HVE o se crashea, tu sesión de Hyprland sigue funcionando exactamente como antes.

---

## Capturas

> TODO: Agregá screenshots en `assets/screenshots/`.
>
> - `sidebar.png` — sidebar con módulos Home, Animations, Borders, Effects
> - `animations-tab.png` — grilla de presets de animación
> - `borders-tab.png` — selector de bordes con slider de grosor
> - `shaders-tab.png` — lista de shaders GLSL disponibles

---

## Quick Start

```bash
# Clonar
git clone https://github.com/XimoCP/hve.git
cd hve

# Instalación automática (compila + copia binario + assets + watcher)
./install.sh

# O manual:
cargo build --release
cp target/release/hve ~/.local/bin/
cp -r assets/ ~/.local/bin/assets/
```

Una vez instalado, ejecutalo con:

```bash
hve
```

O bindealo a una tecla (ver sección **Atajos de Teclado** más abajo).

---

## Features

- **Interfaz visual con Slint** — app Rust nativa, ~2MB de binary, sin Electron, sin web tech.
- **Sidebar data-driven** — los módulos de navegación se definen desde Rust como un array. Agregar un módulo nuevo es agregar una entrada al array y crear el componente. No tocar HTML, no tocar CSS.
- **4 módulos**: Home (activar/desactivar sistema), Animations (seleccionar estilo de animación), Borders (elegir borde + grosor), Effects (shaders GLSL en tiempo real).
- **Colores dinámicos con detección automática**: Noctalia → pywal → matugen → manual. HVE detecta cuál estás usando y extrae los colores automáticamente. No configurás nada.
- **Live refresh de colores**: cuando cambiás colores desde tu herramienta (Noctalia, pywal, matugen, o editando archivos a mano), HVE lo detecta via inotify + un signal file (`~/.cache/hve/colors.json`) y actualiza la UI al instante.
- **IPC listener**: HVE escucha eventos de Hyprland (`configreloaded`) via socket Unix y re-ensambla el overlay automáticamente cuando la configuración se recarga externamente.
- **No destructivo**: overlay separado, no toca tu config, watchdog de cleanup si desinstalás.
- **Sistema Fragments & Assembly**: cada preset activo es un fragmento individual. Se ensamblan en un overlay único. Limpio, modular, fácil de debuggear.
- **Detección de formato Lua/Conf**: funciona tanto con Hyprland legacy (`.conf`) como con Hyprland 0.55+ (`.lua`). Detecta automáticamente cuál usás y transpilea si es necesario.
- **~12 unit tests** en Rust (manipulación de colores: luminancia, lighten, darken, blend, parseo de hex).

---

## Arquitectura

```
┌──────────────────────────────────────────────────────────────────────┐
│  HVE — Hyprland Visual Editor                                        │
│                                                                       │
│  ┌───────────────────┐  ┌───────────────────┐  ┌──────────────────┐  │
│  │    Slint UI        │  │    Rust Core       │  │   Bash Scripts   │  │
│  │    (ui/*.slint)    │  │    (src/*.rs)      │  │ (assets/scripts/) │  │
│  │                    │  │                    │  │                  │  │
│  │  main.slint       │◄─┤  main.rs           ├─►│  assemble.sh     │  │
│  │   MainWindow      │  │  - orquestación    │  │  - ensamblador   │  │
│  │   sidebar for-in  │  │  - nav_modules[]   │  │  - valida sintax │  │
│  │                    │  │                    │  │                  │  │
│  │  components.slint │  │  callbacks.rs      │  │  apply_anim.sh   │  │
│  │  PresetCard       │  │  - toggle_system   │  │  - copia frag    │  │
│  │  NavButton        │  │  - apply_animation │  │  - llama assemble│  │
│  │  SectionHeader    │  │  - apply_border    │  │                  │  │
│  │  GeometrySlider   │  │  - apply_shader    │  │  border.sh       │  │
│  │  WelcomeCard      │  │  - apply_geometry  │  │  shader.sh       │  │
│  │  ActivationCard   │  │                    │  │  geometry.sh     │  │
│  │                    │  │  config.rs         │  │                  │  │
│  │  theme.slint      │  │  - load/save JSON  │  │  color_watcher.sh│  │
│  │  HveColors global │  │  - ~/.config/hve   │  │  - inotifywait   │  │
│  │  NavModule struct │  │                    │  │  - escribe signal│  │
│  │                    │  │  engine.rs         │  │                  │  │
│  │  modules.slint    │  │  - fachada bash    │  │  get_colors.sh   │  │
│  │  HomeModule       │  │  - run_script()    │  │  - JSON output   │  │
│  │  AnimationsModule │  │  - get_colors()    │  │  - colors → JSON │  │
│  │  BordersModule    │  │  - scan()          │  │                  │  │
│  │  ShadersModule    │  │                    │  │  scan.sh         │  │
│  │                    │  │  theme.rs          │  │  - lista presets │  │
│  │                    │  │  - apply_theme()   │  │  - @Title @Desc  │  │
│  │                    │  │  - parse_hex()     │  │                  │  │
│  │                    │  │  - luminance()     │  │  colors.sh       │  │
│  │                    │  │  - lighten/darken  │  │  - Noctalia/pywal│  │
│  │                    │  │  - blend()         │  │  - matugen/manual│  │
│  │                    │  │                    │  │                  │  │
│  │                    │  │  watcher.rs        │  │  init.sh         │  │
│  │                    │  │  - spawn_color_wat │  │  - enable/disable│  │
│  │                    │  │  - start_color_wat │  │  - inyecta marker│  │
│  │                    │  │  - signal colors   │  │                  │  │
│  │                    │  │                    │  │  detect_format.sh│  │
│  │                    │  │  hypr_ipc.rs       │  │  - lua vs conf   │  │
│  │                    │  │  - HyprIpc::new()  │  │  - cachea formato│  │
│  │                    │  │  - spawn_listener()│  │                  │  │
│  │                    │  │  - on_configreload │  │  hve_watchdog.sh │  │
│  │                    │  │                    │  │  - cleanup       │  │
│  │                    │  │  presets.rs        │  │                  │  │
│  │                    │  │  - populate_presets│  │  format_test.sh  │  │
│  │                    │  │  - scan y carga    │  │  - suite tests   │  │
│  └───────────────────┘  └─────────┬───────────┘  └────────┬─────────┘  │
│                                   │                        │            │
│                                   ▼                        ▼            │
│                          ┌───────────────────┐  ┌────────────────────┐  │
│                          │ ~/.config/hve/     │  │ ~/.cache/hve/      │  │
│                          │ config.json        │  │ overlay.lua/.conf  │  │
│                          │ (estado persist.)  │  │ colors.json        │  │
│                          └───────────────────┘  │ fragments/animation│  │
│                                                  │ fragments/border   │  │
│                          ┌───────────────────┐  │ fragments/shader   │  │
│                          │ Hyprland IPC       │  │ fragments/geometry │  │
│                          │ /tmp/hypr/*.sock   │  │ hve_format (cache) │  │
│                          │ eventos:           │  └────────────────────┘  │
│                          │ configreloaded     │                          │
│                          └───────────────────┘                          │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## Tour Archivo por Archivo

### Rust (`src/`)

---

#### `src/main.rs`

**Rol**: Orquestación principal. Arranca todo en orden: carga config, escanea presets, aplica tema, conecta callbacks, arranca listeners y watchers, y finalmente ejecuta la UI.

**Qué podés modificar**:
- **Agregar un servicio nuevo**: llamalo acá antes de `window.run()`. Por ejemplo, si querés un websocket listener, agregá `mi_socket::start_listener(&window)` después del color watcher.
- **Agregar un módulo al sidebar**: editá el array `nav_modules` (líneas 62-68). Cada `NavModule` tiene `label`, `icon`, `accent` (color) y `tab_index`. Agregá uno nuevo con el index siguiente al último.
- **Cambiar el orden de inicialización**: mové las secciones dentro de `main()`.

**Ejemplo**: agregar un módulo "Fonts" al sidebar:
```rust
let nav_modules = Vec::from([
    crate::NavModule { label: shared("Home"), icon: shared("⌂"), accent: parse_hex("#38bdf8"), tab_index: 0 },
    crate::NavModule { label: shared("Animations"), icon: shared("▶"), accent: parse_hex("#fbbf24"), tab_index: 1 },
    crate::NavModule { label: shared("Borders"), icon: shared("◻"), accent: parse_hex("#10b981"), tab_index: 2 },
    crate::NavModule { label: shared("Effects"), icon: shared("◆"), accent: parse_hex("#c084fc"), tab_index: 3 },
    // 👇 nuevo módulo
    crate::NavModule { label: shared("Fonts"), icon: shared("🔤"), accent: parse_hex("#f472b6"), tab_index: 4 },
]);
```

---

#### `src/config.rs`

**Rol**: Estado persistente. Guarda y carga `~/.config/hve/config.json` con Serde. Almacena: si el sistema está activo, tamaño de borde, y los archivos activos de animación/borde/shader.

**Qué podés modificar**:
- **Agregar un setting nuevo**: agregá un campo al struct `Config` con `serde::Serialize/Deserialize`. El `Default` impl define el valor inicial para usuarios nuevos. `save()` y `load()` manejan I/O automáticamente.
- **Cambiar la ruta del config**: editá `config_path()`.

**Ejemplo**: agregar un campo `opacity: f32`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub is_system_active: bool,
    pub border_size: i32,
    pub active_anim_file: String,
    pub active_border_file: String,
    pub active_shader_file: String,
    pub opacity: f32,          // 👈 nuevo campo
}

impl Default for Config {
    fn default() -> Self {
        Self {
            is_system_active: false,
            border_size: 2,
            active_anim_file: String::new(),
            active_border_file: String::new(),
            active_shader_file: String::new(),
            opacity: 1.0,       // 👈 valor por defecto
        }
    }
}
```

Luego en `callbacks.rs` lo usás como `cfg_c.opacity` y lo enviás a la UI.

---

#### `src/engine.rs`

**Rol**: Fachada hacia los scripts bash. Cada acción de la UI (activar sistema, aplicar animación, cambiar borde, etc.) llama a un método de `Engine` que ejecuta el script bash correspondiente. También expone `get_colors()` que parsea el JSON que devuelve `get_colors.sh`.

**Qué podés modificar**:
- **Agregar un script nuevo**: creá el método en `Engine` con `self.run_script("tu_script.sh", &[args])`.
- **Cambiar cómo se ejecutan los scripts**: editá `run_script()`. Actualmente usa `Command::new("bash")`.
- **Cambiar el parseo de colores**: editá el struct `ColorScheme` si querés más campos.

**Ejemplo**: agregar un método para aplicar fonts:
```rust
pub fn apply_font(&self, font_name: &str, font_size: &str) -> Result<String, String> {
    self.run_script("font.sh", &[font_name, font_size])
}
```

---

#### `src/callbacks.rs`

**Rol**: Conecta los botones/sliders de la UI con las acciones reales. Define 5 callbacks: `toggle_system`, `apply_animation`, `apply_border`, `apply_shader`, `apply_geometry`. Cada uno persiste el estado y ejecuta el script correspondiente via `Engine`.

**Qué podés modificar**:
- **Agregar un callback nuevo**: en `main.slint` declarás `callback mi-accion(tipos)`, y acá lo conectás con `window.on_mi_accion(move |args| { ... })`.
- **Cambiar qué pasa cuando se togglea algo**: editá el closure correspondiente.

**Ejemplo**: agregar un callback que se dispare al cambiar opacidad:
```rust
// En main.slint registrás: callback opacity-changed(float);
// En callbacks.rs:
window.on_opacity_changed(move |val| {
    cfg_c.opacity = val;
    let _ = cfg_c.save();
    // no ejecuta script, solo persiste
    if let Some(w) = weak.upgrade() {
        // la UI ya tiene el valor bindeado
    }
});
```

---

#### `src/theme.rs`

**Rol**: Manipulación de colores. Parsea hex, calcula luminancia relativa (sRGB), lighten, darken, blend entre colores. La función estrella es `apply_theme()`, que toma un `ColorScheme` y deriva todos los colores de la UI (texto, cards, hover, bordes, acentos) basándose en la luminancia del surface.

Incluye **12 unit tests** que cubren parseo de hex, luminancia, lighten, darken y blend.

**Qué podés modificar**:
- **Cambiar la fórmula de derivación**: editá `apply_theme()`. Por ejemplo, cambiá la línea 99 de `lighten(&surf, 0.08)` a `lighten(&surf, 0.12)` para que las cards sean más claras en modo oscuro.
- **Agregar un nuevo color derivado**: agregá un campo a `HveColors` en `theme.slint`, calculalo acá y setealo con `window.set_mi_color(valor)`.
- **Cambiar la detección de modo oscuro/claro**: editá la condición `let is_dark = luminance(&surf) < 0.5;`.

**Ejemplo**: hacer que el texto sea un poco más contrastado en modo oscuro:
```rust
// Cambiar línea 86 de #e6edf3 a #ffffff
let text = if is_dark {
    Color::from_rgb_u8(255, 255, 255) // blanco puro
} else {
    Color::from_rgb_u8(30, 33, 36)
};
```

---

#### `src/watcher.rs`

**Rol**: Dos watchers de colores:
1. `spawn_color_watcher()` — arranca `color_watcher.sh` (bash, inotify-based) como proceso hijo. Loggea a `~/.cache/hve/color_watcher.log`.
2. `start_color_watcher()` — thread Rust que monitorea `~/.cache/hve/colors.json` (escrito por el watcher bash) o hace fallback a los archivos de color directamente. Cuando detecta cambios, re-aplica el tema a la UI via `slint::invoke_from_event_loop`.

**Qué podés modificar**:
- **Cambiar la frecuencia de chequeo**: editá `Duration::from_secs(1)` (línea 86) a otro intervalo.
- **Agregar fuentes de color**: editá el array `fallback_sources` (líneas 61-64) para incluir más paths.
- **Cambiar el archivo de log**: editá `watcher_log` en `spawn_color_watcher()`.

**Ejemplo**: cambiar el intervalo de polling a 3 segundos:
```rust
// Línea 86, cambiar:
std::thread::sleep(Duration::from_secs(3));
```

---

#### `src/presets.rs`

**Rol**: Carga los presets (animaciones, bordes, shaders) llamando a `engine.scan()` y poblando los arrays de la UI (`anim_titles`, `anim_descs`, `anim_tags`, `anim_files`, etc.). También restaura el preset activo si estaba guardado en config.

**Qué podés modificar**:
- **Cambiar cómo se cargan los presets**: editá la lógica de `populate_presets()`. Por ejemplo, filtrar por tag o cambiar el orden.
- **Agregar una categoría nueva**: llamá a `engine.scan("tu_categoria")` y poblá arrays nuevos.

**Ejemplo**: filtrar solo presets con tag "OFFICIAL":
```rust
let animations = engine.scan("animations").unwrap_or_default();
let official: Vec<_> = animations.iter().filter(|a| a.tag == "OFFICIAL").collect();
// Poblá la UI con official en vez de animations
```

---

#### `src/hypr_ipc.rs`

**Rol**: Listener de eventos de Hyprland via Unix socket. Se conecta a `/run/user/1000/hypr/{INSTANCE_SIGNATURE}/.socket.sock`, se subscribe a eventos, y cuando recibe `configreloaded`, re-ejecuta `assemble.sh` y refresca los colores de la UI.

**Qué podés modificar**:
- **Cambiar los eventos escuchados**: en la línea 64-66, cambiá `b"subscribe\n"` por otro string, o agregá más event handlers en el `match line.starts_with(...)` (línea 73).
- **Cambiar el comportamiento en config reload**: editá el closure que se pasa a `spawn_listener()` (líneas 99-122).

**Ejemplo**: también reaccionar al evento `focusedmon`:
```rust
if line.starts_with("configreloaded") {
    on_config_reload();
} else if line.starts_with("focusedmon>>") {
    // hacer algo cuando cambia el monitor enfocado
    println!("[HVE] Focus changed: {}", line);
}
```

---

#### `TODO: src/ipc.rs`

**Rol** (planeado): Servidor Unix socket para recibir comandos externos (atajos de teclado). Escucharía en `/tmp/hve.sock`.

**Qué podrías modificar cuando exista**:
- **Agregar un comando nuevo**: agregá un `match` arm para el comando. Por ejemplo, `"toggle-theme" => toggle_theme(&window)`.
- **Cambiar el path del socket**: editá la constante.

**Mientras tanto**, los atajos de teclado se pueden manejar con el binario `hve-ipc` (ver sección **Atajos de Teclado**).

---

#### `TODO: src/tray.rs`

**Rol** (planeado): System tray icon que permite arrancar HVE minimizado y tener acceso rápido a toggle desde el tray.

**Qué podrías modificar cuando exista**:
- **Cambiar el menú**: editá los items del menú contextual.
- **Cambiar el icono**: reemplazá el path del icono.

---

### UI (`ui/`)

---

#### `ui/main.slint`

**Rol**: Ventana principal de la app. Declara el layout con sidebar a la izquierda (200px) y contenido a la derecha. El sidebar itera sobre `nav-modules` (array data-driven desde Rust). El contenido usa `if active-tab == N: Modulo { ... }`.

Propiedades in-out que se conectan con `HveColors` (tema) y con los datos de presets. Callbacks: `toggle-system`, `apply-animation`, `apply-border`, `apply-shader`, `apply-geometry`.

**Qué podés modificar**:
- **Cambiar el layout**: editá el `HorizontalLayout` o redimensioná el sidebar (`width: 200px`).
- **Agregar un módulo**: agregá un `if active-tab == N: TuModulo { ... }` en la sección de contenido (después de la línea 166), pasando las propiedades correspondientes.
- **Cambiar el logo**: editá los textos "HVE" y "Hyprland Visual Editor" (líneas 79-90).
- **Cambiar la versión**: editá la línea 115.

**Ejemplo**: agregar el módulo Fonts asumiendo tab_index=4:
```slint
if active-tab == 4: FontsModule {
    // properties aquí cuando existan
}
```

---

#### `ui/theme.slint`

**Rol**: Define tres cosas:
1. **Struct `Theme`** — todos los tokens visuales (bg-dark, bg-surface, text, accent-*, border, etc.)
2. **Global `HveTheme`** — tema oscuro por defecto (valores hardcodeados como fallback).
3. **Struct `NavModule`** — tipo de dato para los entries del sidebar (`label`, `icon`, `accent`, `tab-index`).
4. **Global `HveColors`** — colores globales accesibles desde cualquier componente y seteables desde Rust via `window.set_*`.

**Qué podés modificar**:
- **Agregar un campo nuevo**: agregalo al struct `Theme`, agregalo al global `HveColors` con valor default, y conectalo en `main.slint` con una propiedad `in-out property <color> mi-color <=> HveColors.mi-color`. Luego en `theme.rs::apply_theme()` calculalo y setealo con `window.set_mi_color(valor)`.
- **Cambiar el struct NavModule**: agregá campos como `badge`, `tooltip`, etc.

**Ejemplo**: agregar `accent-orange`:
```slint
// En el global HveColors:
export global HveColors {
    // ... existentes ...
    in-out property <color> accent-orange: #fb923c;
}

// En main.slint:
in-out property <color> accent-orange <=> HveColors.accent-orange;

// En theme.rs::apply_theme():
window.set_accent_orange(parse_hex("#fb923c"));
```

---

#### `ui/components.slint`

**Rol**: 6 componentes reutilizables:

| Componente | Uso |
|---|---|
| `PresetCard` | Cada preset de animación/borde/shader: muestra título, descripción, tag, toggle switch |
| `NavButton` | Botón del sidebar: icono + label, cambia de color cuando está seleccionado |
| `SectionHeader` | Título + subtítulo con color accent |
| `GeometrySlider` | Slider de grosor de borde (wrapper around `std-widgets.slint::Slider`) |
| `WelcomeCard` | Cards informativas en el Home: icono + título + descripción |
| `ActivationCard` | Toggle grande de activación/desactivación del sistema |

**Qué podés modificar**:
- **Cambiar el estilo de un componente**: editá las propiedades directas (height, border-radius, colors). Todos los colores usan `HveColors.XXX` — cambiá el global y se actualiza todo.
- **Agregar un componente nuevo**: crealo acá y exportalo.
- **Cambiar el toggle**: el `PresetCard` y `ActivationCard` tienen toggle switches dibujados manualmente con `Rectangle`. Cambiá colores, tamaños o animaciones.

**Ejemplo**: hacer que PresetCard sea más grande:
```slint
// Línea 13, cambiar:
height: 80px;  // antes era 64px
```

---

#### `ui/modules.slint`

**Rol**: Los 4 módulos de contenido que se muestran en el panel derecho:

| Módulo | Tab | Contenido |
|---|---|---|
| `HomeModule` | 0 | ActivationCard (toggle sistema) + WelcomeCards informativos |
| `AnimationsModule` | 1 | ScrollView con lista de PresetCards de animaciones |
| `BordersModule` | 2 | GeometrySlider + lista de PresetCards de bordes |
| `ShadersModule` | 3 | ScrollView con lista de PresetCards de shaders |

**Qué podés modificar**:
- **Agregar un módulo nuevo**: creá un nuevo componente acá (ej: `FontsModule`), importalo en `main.slint`, agregá el `if active-tab == N: FontsModule { ... }`.
- **Cambiar el contenido de un módulo**: editá el componente. Por ejemplo, agregá más WelcomeCards al HomeModule o cambiá los subtítulos.

**Ejemplo**: crear FontsModule:
```slint
export component FontsModule inherits ScrollView {
    VerticalLayout {
        padding: 24px;
        spacing: 12px;

        SectionHeader {
            title: "Typography";
            subtitle: "Choose your system font";
            accent: HveColors.accent-pink; // asumiento que existe
        }

        // ... selectores de fuente ...
    }
}
```

---

### Bash Scripts (`assets/scripts/`)

---

#### `assemble.sh`

**Rol**: El corazón del sistema Fragments & Assembly. Lee los fragmentos activos de `~/.cache/hve/fragments/` (animation.xxx, border.xxx, shader.xxx, geometry.xxx), los ensambla en un overlay único, valida que no haya mezcla de sintaxis Lua/Conf, y lo inyecta via `hyprctl reload`.

**Qué podés modificar**:
- **Cambiar el formato de salida**: editá cómo se escriben las variables de color (líneas 63-75) o el orden de ensamblado (array `MODULES` línea 91).
- **Agregar un nuevo módulo al overlay**: agregalo al array `MODULES` (ej: `"font"`) y asegurate de que `font.sh` escriba `font.{ext}` en fragments.
- **Cambiar la validación**: editá la sección "SYNTAX RECOGNITION AND VALIDATION" (líneas 111-128).

**Ejemplo**: agregar soporte para módulo "font":
```bash
MODULES=("animation" "border" "shader" "geometry" "font")
```

---

#### `color_watcher.sh`

**Rol**: Watcher bash que usa `inotifywait -e close_write` para monitorear archivos de color (Noctalia, pywal, matugen, o manual). Cuando detecta un cambio real (por hash MD5), re-ejecuta `assemble.sh` y escribe `~/.cache/hve/colors.json` para que la UI lo capte.

**Qué podés modificar**:
- **Cambiar qué archivos monitorea**: editá la función `find_watch_files()` (líneas 26-65). Agregá nuevas fuentes de color.
- **Cambiar el timeout de inotify**: editá `-t 30` (línea 115).
- **Cambiar el método de detección de cambios**: actualmente usa `md5sum`, podrías cambiarlo a `sha256sum` o diff.
- **Cambiar el archivo de log**: editá `LOG_FILE` (línea 18).

**Ejemplo**: agregar soporte para `wallust` (fork de pywal):
```bash
find_watch_files() {
    # ... existente ...
    local wallust_json="$HOME/.cache/wallust/colors.json"
    if [ -f "$wallust_json" ]; then
        files+=("$wallust_json")
    fi
    # ... resto ...
}
```

---

#### `scan.sh`

**Rol**: Escanea `assets/animations/`, `assets/borders/`, `assets/shaders/` y genera JSON con los metadatos de cada preset. Extrae `@Title`, `@Desc`, `@Tag`, `@Icon`, `@Color` de comentarios dentro de los archivos `.conf`, `.lua` o `.frag`. También respeta `HVE_FORMAT` para elegir la extensión correcta cuando un preset existe en ambos formatos.

**Qué podés modificar**:
- **Cambiar el formato de detección**: editá cómo se parsean los metadatos (función `get_meta` línea 76-77). Agregá nuevos campos como `@Author` o `@Version`.
- **Cambiar qué archivos incluye**: editá el `find` en la línea 57. Agregá más extensiones.
- **Cambiar el orden de los presets**: actualmente ordena por `sort`. Cambiá a otro criterio.

**Ejemplo**: extraer un campo `@Author`:
```bash
# Agregar después de TAG (línea 84):
AUTHOR=$(get_meta "Author")
[ -z "$AUTHOR" ] && AUTHOR="unknown"

# Incluirlo en el JSON (después de línea 99):
"author": "$AUTHOR",
```

---

#### `get_colors.sh`

**Rol**: Extrae los colores del sistema activo (Noctalia, pywal, matugen, manual) y los imprime como JSON. Es la interfaz entre `colors.sh` (bash) y `engine.rs` (Rust). Un script tiny que solo sourcea `colors.sh` y formatea la salida.

**Qué podés modificar**:
- **Agregar un campo nuevo al JSON**: agregalo al `printf` (línea 10). Por ejemplo, agregá `"error":"$HVE_ERROR"`.
- **Cambiar el formato de salida**: actualmente imprime JSON, podrías cambiarlo a TOML o YAML si adaptás el parseo en Rust.

**Ejemplo**: agregar el color `error`:
```bash
printf '{"primary":"%s","secondary":"%s","tertiary":"%s","surface":"%s","surface_lowest":"%s","accent":"%s","error":"%s"}' \
    "$HVE_PRIMARY" "$HVE_SECONDARY" "$HVE_TERTIARY" "$HVE_SURFACE" "$HVE_SURFACE_LOWEST" "$HVE_ACCENT" "$HVE_ERROR"
```

Y en `engine.rs`, agregá `pub error: String` al struct `ColorScheme`.

---

#### `colors.sh`

**Rol**: Biblioteca de extracción de colores. Detecta automáticamente qué herramienta de color estás usando (Noctalia → pywal → matugen → manual), parsea los archivos correspondientes y exporta las variables `HVE_PRIMARY`, `HVE_SECONDARY`, `HVE_TERTIARY`, `HVE_SURFACE`, `HVE_SURFACE_LOWEST`, `HVE_ACCENT`. Se sourcea desde `get_colors.sh`, `assemble.sh`, y otros scripts.

Soporta formatos: `#hex`, `rgb(r,g,b)`, `rgba(r,g,b,a)`, variables Lua (`primary = "..."`), variables conf (`$primary = ...`), y gradientes de borde.

**Qué podés modificar**:
- **Agregar una fuente de color nueva**: implementá una función `_hve_try_mi_fuente()` y agregala a la cadena de detección en `hve_load_colors()` (líneas 196-200).
- **Cambiar los fallbacks**: editá las líneas 203-208.
- **Agregar más variables de color**: exportá nuevas variables `HVE_*` y agregalas al parseo.

**Ejemplo**: agregar soporte para `flavours` (otro generador de esquemas):
```bash
_hve_try_flavours() {
    local flavours_file="$HOME/.cache/flavours/schemes/current"
    if [ -f "$flavours_file" ]; then
        echo "[HVE] Colors from: flavours" >&2
        # parsear y exportar HVE_* variables
        return 0
    fi
    return 1
}

# En hve_load_colors(), agregar antes de _hve_try_manual:
_hve_try_noctalia ||
_hve_try_pywal ||
_hve_try_matugen ||
_hve_try_flavours ||   # 👈 nueva
_hve_try_manual
```

---

#### `init.sh`

**Rol**: Activa o desactiva el sistema HVE en Hyprland. Cuando se activa (`enable`): asegura directorios, deploya el watchdog, ejecuta `assemble.sh` por primera vez, e inyecta las líneas necesarias en `hyprland.conf` o `hyprland.lua` (con marcadores `>>> HYPRLAND VISUAL EDITOR START/END <<<`). Cuando se desactiva (`disable`): limpia los marcadores y borra `~/.cache/hve/`.

**Qué podés modificar**:
- **Cambiar los marcadores**: editá las líneas 35-38. Si cambiás los marcadores, también actualizá `hve_watchdog.sh` que los busca para cleanup.
- **Cambiar qué se inyecta en la config**: editá las secciones `LUA MODE` (líneas 100-117) y `CONF MODE` (líneas 119-135).
- **Agregar pasos extras en enable/disable**: agregá código antes de `hyprctl reload`.

**Ejemplo**: que al desactivar también se detenga el watcher:
```bash
elif [ "$ACTION" == "disable" ]; then
    # 👇 nuevo: matar watcher
    pkill -f "color_watcher.sh" 2>/dev/null || true

    clean_hyprland_conf
    clean_hyprland_lua
    rm -rf "$HVE_SAFE_DIR"
    hyprctl reload
fi
```

---

#### `border.sh`, `shader.sh`, `geometry.sh`, `apply_animation.sh`

**Rol**: Cada uno maneja su tipo de preset. Flujo común:
1. Detectan formato (Lua/Conf) y asignan extensión.
2. Si el argumento es `"none"`, borran el fragmento correspondiente de `~/.cache/hve/fragments/`.
3. Si es un preset válido, copian el contenido del preset a `~/.cache/hve/fragments/{tipo}.{ext}`.
4. Llaman a `assemble.sh` para re-ensamblar el overlay.

| Script | Fragmento que genera | Fuente de presets |
|---|---|---|
| `apply_animation.sh` | `fragments/animation.{ext}` | `assets/animations/` |
| `border.sh` | `fragments/border.{ext}` | `assets/borders/` |
| `shader.sh` | `fragments/shader.{ext}` | `assets/shaders/` |
| `geometry.sh` | `fragments/geometry.{ext}` | genera inline, no copia preset |

**Qué podés modificar**:
- **Cambiar el formato del fragmento generado**: editá cómo se escribe el archivo. Por ejemplo, en `shader.sh` se genera un wrapper: podrías cambiar la sintaxis Lua/Conf (líneas 49-56).
- **Agregar validación**: antes de copiar el preset, validá su contenido.

**Ejemplo**: en `shader.sh`, cambiar el wrapper Lua para usar sintaxis más moderna:
```bash
# Línea 69, cambiar:
echo "hl.config({ decoration = { screen_shader = \"$SHADER_PATH\" } })" > "$TARGET_FRAGMENT"
```

> **Nota**: `apply_animation.sh` tiene código duplicado (parece que se pegó múltiples veces al editar). Funciona correctamente pero necesita una limpieza. Si tocás ese archivo, considera refactorizarlo.

---

#### `detect_format.sh`

**Rol**: Detecta si el usuario usa Hyprland con Lua (`.lua`, Hyprland 0.55+) o conf (`.conf`, legacy). Busca patrones como `hl.config(` o `require(` en `hyprland.lua`. Cachea el resultado en `~/.cache/hve/hve_format`.

**Qué podés modificar**:
- **Cambiar la prioridad**: actualmente prioriza Lua sobre conf. Podrías cambiar el orden.
- **Cambiar los patrones de detección**: editá el `grep -qE` de la línea 33.
- **Cambiar la ubicación del cache**: editá `FORMAT_CACHE` (línea 12).

**Ejemplo**: hacer que detecte también archivos en `~/.config/hypr/configs/`:
```bash
# Agregar después de línea 16:
LUA_ALT_CONFIG="$HVE_HYPR_DIR/configs/settings.lua"
```

---

#### `hve_watchdog.sh`

**Rol**: Script de cleanup automático. Se ejecuta en cada inicio de Hyprland (inyectado por `init.sh`). Si HVE fue desinstalado (el directorio del plugin no existe), remueve los marcadores de `hyprland.conf`/`hyprland.lua` y borra `~/.cache/hve/`. Esto asegura que no queden configs rotas si desinstalás HVE.

**Qué podés modificar**:
- **Cambiar los directorios que verifica**: si cambiás la estructura de directorios, actualizá `HVE_DIR`.
- **Agregar más limpieza**: por ejemplo, borrar archivos temporales.

---

#### `format_test.sh`

**Rol**: Suite de tests para el sistema de detección de formato y transpilación. Verifica que la detección Lua/Conf funcione, que los fragmentos se transpilen correctamente, y que el cacheo funcione. Útil para development.

**Qué podés modificar**:
- **Agregar más tests**: agregá más secciones `echo "Test N: ..."` con comandos de verificación.
- **Cambiar los tests existentes**: actualizalos si cambiás la lógica de formato.

---

#### `hve-ipc`

**Rol**: Cliente IPC para enviar comandos a HVE desde atajos de teclado. Escribe en un socket Unix (`$XDG_RUNTIME_DIR/hve.sock` o `/tmp/hve.sock`) y HVE responde.

Se instala automáticamente con `./install.sh`. Después podés bindear atajos en tu configuración de Hyprland:

```bash
# En hyprland.conf:
bind = $mainMod+Shift+T, exec, hve-ipc toggle-system
bind = $mainMod+Shift+A, exec, hve-ipc next-anim
bind = $mainMod+Shift+B, exec, hve-ipc next-border
bind = $mainMod+Shift+S, exec, hve-ipc next-shader
bind = $mainMod+Shift+W, exec, hve-ipc toggle-tray
```

```lua
-- En hyprland.lua (Hyprland 0.55+):
hl.bind("SUPER", "Shift", "T", "exec", "hve-ipc toggle-system")
hl.bind("SUPER", "Shift", "A", "exec", "hve-ipc next-anim")
hl.bind("SUPER", "Shift", "B", "exec", "hve-ipc next-border")
hl.bind("SUPER", "Shift", "S", "exec", "hve-ipc next-shader")
hl.bind("SUPER", "Shift", "W", "exec", "hve-ipc toggle-tray")
```

---

### Otros Archivos

#### `install.sh`

**Rol**: Instalador interactivo. Compila el binary, lo copia a `~/.local/bin/`, copia assets, genera un watcher personalizado según la herramienta de color detectada, y lo arranca.

**Qué podés modificar**:
- **Cambiar destinos**: editá `HVE_BIN`, `HVE_SCRIPTS`, etc.
- **Agregar pasos de instalación**: por ejemplo, crear un autostart `.desktop` en `~/.config/autostart/`.
- **Cambiar la detección de color tool**: editá la sección 4 (líneas 89-123).

#### `hve.desktop`

**Rol**: Desktop entry para mostrar HVE en el lanzador de aplicaciones. Instalado en `~/.local/share/applications/hve.desktop`.

---

## Guías Prácticas

### Cómo agregar un preset de animación

1. Creá el archivo de preset en `assets/animations/mi-anim.conf` (o `.lua` si usás Hyprland 0.55+).
2. Agregá metadatos como comentarios al inicio del archivo:
   ```conf
   # @Title: Mi Animación
   # @Desc: Una animación suave y elegante
   # @Tag: CUSTOM
   ```
3. Abrí HVE → pestaña Animations → debería aparecer automáticamente.
4. Si no aparece, ejecutá manualmente el scan para verificar:
   ```bash
   bash assets/scripts/scan.sh animations
   ```
   Debería mostrar tu preset en el JSON de salida.

### Cómo cambiar los colores

HVE detecta automáticamente qué herramienta de color estás usando:

- **Noctalia**: cambiá colores desde Noctalia → HVE los detecta automáticamente via `color_watcher.sh` (inotify). No necesitas hacer nada.
- **pywal**: ejecutá `wal -i imagen.jpg` → HVE detecta `~/.cache/wal/colors.json` y actualiza.
- **matugen**: ejecutá `matugen image imagen.jpg` → HVE busca el archivo de salida configurado en `~/.config/matugen/config.toml`.
- **Manual**: editá tus archivos de configuración de Hyprland que contengan variables `$primary`, `$secondary`, etc. → HVE hace un scan manual.

La derivación de colores (luminance → text contrast, lighten/darken para cards/hover, blend para accent_red) está en `src/theme.rs::apply_theme()`. Si querés cambiar cómo se derivan los colores, editá esa función.

Para forzar colores manualmente, corré:
```bash
bash assets/scripts/get_colors.sh
```
Esto imprime el JSON actual. Si no está detectando lo que esperás, revisá `assets/scripts/colors.sh` y verificá que tu fuente esté siendo detectada.

### Cómo agregar un módulo nuevo al sidebar

Ejemplo completo para agregar un módulo "Fonts":

**1. `src/main.rs`** — agregá el entry al array `nav_modules`:
```rust
crate::NavModule { label: shared("Fonts"), icon: shared("🔤"), accent: parse_hex("#f472b6"), tab_index: 4 },
```

**2. `ui/modules.slint`** — creá el componente:
```slint
export component FontsModule inherits ScrollView {
    VerticalLayout {
        padding: 24px;
        spacing: 12px;

        SectionHeader {
            title: "Typography";
            subtitle: "Choose your system font";
            accent: HveColors.accent-pink;
        }

        // ... contenido del módulo ...
    }
}
```

**3. `ui/main.slint`** — importalo y agregá el `if`:
```slint
import { ..., FontsModule } from "./modules.slint";

// En la sección de contenido:
if active-tab == 4: FontsModule {
    // connect properties here
}
```

**4. `src/callbacks.rs`** — si el módulo necesita acciones, agregá los callbacks:
```rust
window.on_apply_font(move |font_name| {
    // persistir y ejecutar script
});
```

**5. `ui/main.slint`** — declará el callback:
```slint
callback apply-font(string);
```

### Cómo modificar el estilo de un componente

Todos los componentes visuales están en `ui/components.slint`. Usan colores del global `HveColors`, así que cambiando el global se actualiza todo.

Ejemplos de modificaciones comunes:

- **PresetCard más grande**: en `components.slint`, línea 13, cambiá `height: 64px` a `height: 80px`.
- **Botón de nav más alto**: línea 118, cambiá `height: 44px` a `height: 52px`.
- **Toggle switch más grande**: en `PresetCard`, el toggle está en las líneas 92-106. Cambiá `width: 40px` y `height: 22px`.
- **Bordes más redondeados**: cambiá `border-radius` en cualquier componente.
- **Color de acento**: cambiá los valores en `theme.slint` global `HveColors`, o cambiá la lógica de derivación en `theme.rs::apply_theme()`.

### Cómo agregar un script bash nuevo

1. Creá el archivo en `assets/scripts/mi-script.sh`, con `source "$SCRIPT_DIR/utils.sh"` al inicio.
2. En `src/engine.rs`, agregá el método:
   ```rust
   pub fn mi_accion(&self, arg: &str) -> Result<String, String> {
       self.run_script("mi_script.sh", &[arg])
   }
   ```
3. En `src/callbacks.rs`, conectalo a un callback de UI o llamalo desde donde corresponda.

---

## Atajos de Teclado (IPC)

Agregá estos binds a tu `hyprland.conf`:

```conf
bind = $mainMod+Shift+T, exec, hve-ipc toggle-system
bind = $mainMod+Shift+A, exec, hve-ipc next-anim
bind = $mainMod+Shift+B, exec, hve-ipc next-border
bind = $mainMod+Shift+S, exec, hve-ipc next-shader
bind = $mainMod+Shift+W, exec, hve-ipc toggle-tray
```

O en `hyprland.lua` (Hyprland 0.55+):

```lua
hl.bind("SUPER", "Shift", "T", "exec", "hve-ipc toggle-system")
hl.bind("SUPER", "Shift", "A", "exec", "hve-ipc next-anim")
hl.bind("SUPER", "Shift", "B", "exec", "hve-ipc next-border")
hl.bind("SUPER", "Shift", "S", "exec", "hve-ipc next-shader")
hl.bind("SUPER", "Shift", "W", "exec", "hve-ipc toggle-tray")
```

Para abrir HVE directamente:

```conf
bind = $mainMod+V, exec, hve
```

```lua
hl.bind("SUPER", "V", "exec", "hve")
```

**Nota**: `hve-ipc` se instala automáticamente con `./install.sh`. Si no lo tenés, podés copiarlo manualmente desde `assets/scripts/hve-ipc` a `~/.local/bin/`.

---

## Troubleshooting

### "HVE no arranca"

**Causa más común**: `HYPRLAND_INSTANCE_SIGNATURE` no está seteada porque HVE no se ejecuta desde una sesión de Hyprland.

**Solución**: ejecutá HVE solo desde dentro de una sesión de Hyprland. Verificá con:
```bash
echo $HYPRLAND_INSTANCE_SIGNATURE
# Debería imprimir algo como "0x1234567890abcdef1234567890abcdef12345678_1234567890_1234567890"
```

Si está vacío, estás fuera de Hyprland o el socket no está disponible.

Otra causa: falta `libslint` o dependencias de Slint. Verificá con:
```bash
ldd target/release/hve | grep slint
```

### "Los colores no se actualizan"

1. Revisá el log del watcher:
   ```bash
   cat ~/.cache/hve/color_watcher.log
   ```
2. Verificá que `color_watcher.sh` esté corriendo:
   ```bash
   ps aux | grep color_watcher
   ```
3. Verificá que `inotifywait` esté instalado:
   ```bash
   which inotifywait
   ```
4. Forzá una regeneración manual:
   ```bash
   bash assets/scripts/get_colors.sh
   bash assets/scripts/assemble.sh
   ```
5. Si los colores se ven bien en terminal pero no en la UI, el problema está en `watcher.rs` o en la conexión entre el signal file (`~/.cache/hve/colors.json`) y el thread Rust. Revisá que el signal file se esté escribiendo:
   ```bash
   cat ~/.cache/hve/colors.json
   ```

### "El overlay no se aplica"

1. Verificá el formato detectado:
   ```bash
   cat ~/.cache/hve/hve_format
   # Debería ser "lua" o "conf"
   ```
2. Verificá que el overlay se generó correctamente:
   ```bash
   cat ~/.cache/hve/overlay.lua   # o overlay.conf
   ```
3. Si el overlay está vacío, corre `assemble.sh` manualmente para ver errores:
   ```bash
   bash assets/scripts/assemble.sh
   ```
4. Verificá que `hyprctl reload` funcione:
   ```bash
   hyprctl reload
   ```
5. Si ves `❌ [HVE ERROR] Validation failed!` en la terminal, hay una mezcla de sintaxis Lua y Conf. Revisá los fragmentos en `~/.cache/hve/fragments/` y asegurate de que todos usen el mismo formato.

### "No encuentro hve-ipc"

`hve-ipc` todavía no está implementado. Es un script/binario planeado. Por ahora usá los binds directos a `hve` (que abre la UI completa). Si necesitás control por teclado, considerá contribuir con la implementación o usá los workarounds descritos en la sección de Atajos.

### "Error de compilación"

**Dependencias necesarias**:
```bash
# Rust (si no lo tenés):
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Slint (dependencias de sistema en Linux):
# Debian/Ubuntu:
sudo apt install libfontconfig-dev libwayland-dev libxkbcommon-dev

# Arch:
sudo pacman -S fontconfig wayland libxkbcommon

# Inotify (para el watcher):
sudo pacman -S inotify-tools   # Arch
sudo apt install inotify-tools # Debian
```

**Errores comunes**:
- `error: failed to run custom build command for 'slint-build v1.11.0'` → falta la toolchain de Slint. Corré `rustup target list --installed` y asegurate de tener la target de tu sistema.
- `error: linker cc not found` → instalá `build-essential` (Debian) o `base-devel` (Arch).
- Slint 1.11 requiere Rust 1.75+. Verificá con `rustc --version`.

### "Los presets no aparecen"

Los presets se escanean de los directorios `assets/animations/`, `assets/borders/`, `assets/shaders/`. Si no aparecen:
1. Verificá que los archivos existan: `ls assets/animations/`
2. Ejecutá el scan manualmente: `bash assets/scripts/scan.sh animations`
3. Si el scan devuelve `[]`, revisá que los archivos tengan metadatos (`@Title:`).
4. Si estás en installed mode, los assets están en `~/.local/bin/assets/`. Verificá que existan ahí.

---

## Desarrollo

```bash
# Clonar
git clone https://github.com/XimoCP/hve.git
cd hve

# Tests (12 tests de manipulación de colores)
cargo test

# Verificar compilación (rápido, sin generar binary)
cargo check

# Build release
cargo build --release

# Correr en desarrollo
cargo run
```

### Estructura del proyecto

```
hve/
├── Cargo.toml              # Dependencias: slint 1.11, serde, serde_json, dirs
├── install.sh              # Instalador interactivo
├── hve.desktop             # Desktop entry
├── README.md               # Este archivo
│
├── src/
│   ├── main.rs             # Orquestación: init, nav_modules, startup
│   ├── config.rs           # Config persistente (JSON)
│   ├── engine.rs           # Fachada bash: run_script, get_colors, scan
│   ├── callbacks.rs        # UI callbacks: system, anim, border, shader, geometry
│   ├── theme.rs            # Colores: parse_hex, luminance, lighten, darken, blend, apply_theme
│   ├── watcher.rs          # Watcher: spawn bash watcher, signal thread Rust
│   ├── hypr_ipc.rs         # IPC listener: socket Hyprland, eventos configreloaded
│   └── presets.rs          # Carga de presets desde scan.sh
│
├── ui/
│   ├── main.slint          # Ventana principal + sidebar data-driven
│   ├── theme.slint         # HveColors global + NavModule struct + Theme
│   ├── components.slint    # 6 componentes reutilizables
│   └── modules.slint       # 4 módulos de contenido
│
├── assets/
│   ├── scripts/            # 14 scripts bash
│   │   ├── assemble.sh     # Fragments → overlay (ensamblador)
│   │   ├── color_watcher.sh# Watcher inotify
│   │   ├── colors.sh       # Extractor de colores (Noctalia/pywal/matugen/manual)
│   │   ├── get_colors.sh   # Colors → JSON
│   │   ├── scan.sh         # Scan de presets + metadatos
│   │   ├── init.sh         # Enable/disable HVE
│   │   ├── border.sh       # Aplicar preset de borde
│   │   ├── shader.sh       # Aplicar preset de shader
│   │   ├── geometry.sh     # Cambiar grosor de borde
│   │   ├── apply_animation.sh # Aplicar preset de animación
│   │   ├── detect_format.sh   # Detecta Lua vs Conf
│   │   ├── format_test.sh     # Suite de tests de formato
│   │   ├── hve_watchdog.sh    # Cleanup automático al desinstalar
│   │   └── utils.sh           # Paths, hve_resolve_preset, helpers
│   │
│   ├── animations/         # 18 presets de animación (Lua + Conf)
│   ├── borders/            # 14 presets de borde (Lua + Conf)
│   ├── shaders/            # 9 shaders GLSL
│   ├── fragments/          # Fragmentos activos (border.lua)
│   └── owl_neon.png        # Preview image
│
└── screenshots/            # TODO: agregar screenshots
```

---

## Uninstall

1. Abrí HVE → pestaña Home → **Disable system** (esto remueve los marcadores de tu config y borra el overlay).
2. Eliminá los archivos:
   ```bash
   rm -rf ~/.local/bin/hve ~/.config/hve ~/.cache/hve
   rm -rf ~/.local/bin/assets/   # si instalaste con ./install.sh
   ```
3. Eliminá el desktop entry:
   ```bash
   rm ~/.local/share/applications/hve.desktop
   ```
4. Si configuraste autostart:
   ```bash
   rm ~/.config/autostart/hve.desktop
   ```
5. Opcional: verificá que tu `hyprland.conf` o `hyprland.lua` no tenga marcadores residuales de HVE. Buscá líneas con `HYPRLAND VISUAL EDITOR` y eliminalas.

El watchdog (`hve_watchdog.sh`) que se inyectó en tu config limpia todo automáticamente si HVE no está instalado, pero siempre es mejor desactivar primero desde la UI.

---

## Licencia

MIT © XimoCP

---

*HVE nació como un plugin para Noctalia y evolucionó hasta convertirse en una app standalone. El sistema Fragments & Assembly permite que sea no destructivo por diseño: podés probar cualquier combinación de animaciones, bordes y shaders sabiendo que un solo `Disable system` vuelve todo a la normalidad.*
