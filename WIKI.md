# HVE — Hyprland Visual Editor

<!-- NOTA DE MANTENIMIENTO: README.md y WIKI.md se mantienen en sincronía (mismo contenido). Si editas uno, aplica los mismos cambios al otro. -->

**HVE** es una aplicación gráfica para gestionar visualmente la estética de Hyprland: animaciones, bordes, shaders, geometría de ventanas y colores, todo desde una interfaz unificada.

Funciona con **cualquier desktop basado en Hyprland** (Noctalia Shell, Hyprland puro, etc.) y es **agnóstico al escritorio** — no depende de ningún shell en particular.

---

## Índice

- [Filosofía: Fragmentos y Ensamblado](#filosofía-fragmentos-y-ensamblado)
- [Instalación](#instalación)
  - [Qué se instala y dónde](#qué-se-instala-y-dónde)
  - [Dependencias del sistema](#dependencias-del-sistema)
  - [Pasos del instalador](#pasos-del-instalador)
- [Desinstalación](#desinstalación)
  - [Qué elimina y qué conserva](#qué-elimina-y-qué-conserva)
  - [Tranquilidad con la desinstalación](#tranquilidad-con-la-desinstalación)
- [Arquitectura](#arquitectura)
  - [Sistema de Fragmentos y Ensamblado](#sistema-de-fragmentos-y-ensamblado)
  - [Formato: Lua vs Conf](#formato-lua-vs-conf)
  - [Sistema de configuración](#sistema-de-configuración)
  - [Sistema de colores y temas](#sistema-de-colores-y-temas)
  - [Sistema de presets](#sistema-de-presets)
  - [Fondos animados (mpvpaper)](#fondos-animados-mpvpaper)
  - [IPC (Comunicación entre procesos)](#ipc-comunicación-entre-procesos)
  - [Bandeja del sistema (System Tray)](#bandeja-del-sistema-system-tray)
  - [Vigilante de colores (Color Watcher)](#vigilante-de-colores-color-watcher)
  - [Auto-minimizado y countdown](#auto-minimizado-y-countdown)
  - [Watchdog de seguridad](#watchdog-de-seguridad)
  - [Internacionalización (i18n)](#internacionalización-i18n)
- [Estructura del proyecto](#estructura-del-proyecto)
  - [Código fuente (src/)](#código-fuente-src)
  - [Scripts (assets/scripts/)](#scripts-assetsscripts)
  - [Presets de animación (assets/animations/)](#presets-de-animación-assetsanimations)
  - [Presets de bordes (assets/borders/)](#presets-de-bordes-assetsborders)
  - [Shaders (assets/shaders/)](#shaders-assetsshaders)
  - [Fragmentos dinámicos (assets/fragments/)](#fragmentos-dinámicos-assetsfragments)
  - [Archivos de runtime](#archivos-de-runtime)
- [Uso](#uso)
  - [Interfaz gráfica](#interfaz-gráfica)
  - [Línea de comandos](#línea-de-comandos)
  - [Bandeja del sistema](#bandeja-del-sistema)
  - [Atajos de teclado (IPC)](#atajos-de-teclado-ipc)
- [Preguntas frecuentes](#preguntas-frecuentes)

---

## Filosofía: Fragmentos y Ensamblado

HVE no toca directamente tus archivos de configuración de Hyprland. En lugar de eso, usa un sistema de **fragmentos** (snippets individuales) que se **ensamblan** en un archivo `overlay` que Hyprland carga además de tu configuración principal.

```
tus archivos hyprland  ←  inmutables, HVE nunca los modifica
      ┃
      ┃ (source / dofile)
      ┃
  ~/.cache/hve/overlay.{lua,conf}  ←  archivo único generado por HVE
      ┃
      ┃ (ensambla)
      ┃
  assets/fragments/*.{lua,conf}    ←  fragmentos individuales (animación, borde, shader, geometría)
  + colores detectados automáticamente
```

Esto significa que:

- **Tu configuración principal nunca se pierde** — HVE inyecta solo dos líneas (`source = ...` / `dofile(...)`) entre marcadores que puede revertir completamente.
- **Cada cambio es atómico** — el overlay se escribe en un temporal y se mueve con `mv` al destino.
- **La desinstalación deja tu sistema exactamente como estaba antes de instalar HVE.**

---

## Instalación

```bash
git clone https://github.com/tu-usuario/hve.git
cd hve
./install.sh
```

### Qué se instala y dónde

| Ruta | Contenido | Tipo |
|------|-----------|------|
| `~/.local/bin/hve` | Binario principal (Rust compilado) | Obligatorio |
| `~/.local/bin/assets/scripts/` | 15 scripts bash de soporte | Obligatorio |
| `~/.local/bin/assets/animations/` | 18 animaciones (formato dual .conf + .lua) | Obligatorio |
| `~/.local/bin/assets/borders/` | 14 bordes (formato dual .conf + .lua) | Obligatorio |
| `~/.local/bin/assets/shaders/` | 9 shaders GLSL (.frag) | Obligatorio |
| `~/.local/bin/assets/fragments/` | Fragmentos dinámicos (runtime) | Obligatorio |
| `~/.local/share/applications/hve.desktop` | Acceso directo para lanzadores | Obligatorio |
| `~/.local/bin/hve-ipc` | Script cliente IPC en Python | Opcional |
| `/usr/local/bin/hve-ipc` | Symlink del script IPC (requiere sudo) | Opcional |
| `~/.config/hve/config.json` (`auto_start: true`) | Inicio automático con el sistema via `exec-once` en hve-settings | Opcional |

### Dependencias del sistema

El instalador detecta tu distribución y las instala automáticamente:

| Familia | Gestor | Paquetes |
|---------|--------|----------|
| Arch / Manjaro / EndeavourOS / CachyOS / Artix | `pacman` | `inotify-tools pkg-config gtk3 glib2 cairo pango` |
| Debian / Ubuntu / Pop / Linux Mint / Elementary | `apt` | `inotify-tools pkg-config libgtk-3-dev libglib2.0-dev libcairo2-dev libpango1.0-dev` |
| Fedora | `dnf` | `inotify-tools pkgconfig gtk3-devel glib2-devel cairo-devel pango-devel` |
| openSUSE | `zypper` | `inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel` |
| Void | `xbps-install` | `inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel` |
| Gentoo | `emerge` | `inotify-tools pkg-config gtk3 glib2 cairo pango` |

### Pasos del instalador

1. **Verifica que no se ejecute como root** — el script pide sudo solo cuando es necesario.
2. **Detecta la distribución** por `ID` y `ID_LIKE` en `/etc/os-release`, con fallbacks a archivos clásicos.
3. **Instala dependencias** — intenta instalación por lote; si falla, prueba una por una.
4. **Verifica Rust** — si no está, lo instala via `rustup`.
5. **Compila** (`cargo build --release`) — genera el binario.
6. **Copia el binario** a `~/.local/bin/hve`.
7. **Copia los assets** (scripts, animaciones, bordes, shaders, fragmentos).
8. **Instala el acceso directo** — con la ruta absoluta al binario.
9. **Pregunta opcionales**:
   - ¿Instalar `hve-ipc`? (atajos de teclado en Hyprland)
   - ¿Symlink en `/usr/local/bin`? (rutas fijas en configs de Hyprland)
   - ¿Autostart? (inicia HVE en la bandeja al iniciar sesión)
   - ¿Iniciar ahora?
10. **Advierte si `~/.local/bin` no está en el PATH**.

---

## Desinstalación

```bash
./uninstall.sh
```

### Qué elimina y qué conserva

**Se elimina automáticamente:**

| Elemento | Ruta | Notas |
|----------|------|-------|
| Binario | `~/.local/bin/hve` | — |
| Assets completos | `~/.local/bin/assets/` | Scripts, animaciones, bordes, shaders |
| Acceso directo | `~/.local/share/applications/hve.desktop` | — |
| Autostart (legacy XDG) | `~/.config/autostart/hve.desktop` | Solo si existe de instalaciones anteriores |
| Script IPC | `~/.local/bin/hve-ipc` | Si se instaló |
| Symlink del sistema | `/usr/local/bin/hve-ipc` | Requiere sudo, solo si existe |

**Pregunta antes de eliminar:**

| Elemento | Ruta | Por qué pregunta |
|----------|------|------------------|
| Cache de runtime | `~/.cache/hve/` | Datos temporales (logs, overlays, backups). Se recrean si vuelves a ejecutar HVE. |
| Configuración de usuario | `~/.config/hve/` | **Tus ajustes**: tema elegido, animación activa, bordes, presets, keybinds. |

### Tranquilidad con la desinstalación

HVE está diseñado para **no dejar rastro**. Esto es posible porque:

1. **Tu configuración de Hyprland nunca se modifica directamente.** HVE solo añade dos líneas entre marcadores `# HVE START` / `# HVE END` (o `-- HVE START` / `-- HVE END` en Lua) en tu `hyprland.conf` o `hyprland.lua`. Si HVE no está, esas líneas no existen.

2. **El watchdog (`hve_watchdog.sh`) se asegura de esto.** Si el directorio de HVE deja de existir (desinstalación manual, borrado accidental), el watchdog —que se ejecuta en cada inicio de Hyprland— detecta la ausencia, remueve los marcadores de los archivos de configuración y limpia `~/.cache/hve/`.

3. **El desinstalador elimina todos los archivos que el instalador creó.** No hay archivos ocultos, ni registros en el sistema, ni configuraciones residuales.

4. **Tu configuración personal de HVE (`~/.config/hve/config.json`) se conserva a menos que explícitamente elijas borrarla.** Si después reinstalas, tus ajustes anteriores vuelven a funcionar.

5. **`~/.local/bin` no se elimina** (podrías tener otros programas allí). Solo se borran los archivos de HVE.

En resumen: **puedes instalar y desinstalar HVE cuantas veces quieras sin riesgo de dejar basura o romper tu configuración de Hyprland.**

---

## Arquitectura

### Sistema de Fragmentos y Ensamblado

El núcleo de HVE es `assemble.sh` (152 líneas), que:

1. **Obtiene los colores activos** (via `colors.sh`)
2. **Escribe un overlay temporal** con:
   - Cabecera (generación, fecha)
   - Variables de color (`$HVE_PRIMARY`, `$HVE_SECONDARY`, etc.)
   - Curvas bezier de animación
   - Fragmentos activos: `animation.{ext}`, `border.{ext}`, `geometry.{ext}`, `shader.{ext}`
3. **Valida la sintaxis** según el formato (Lua vs Conf)
4. **Mueve atómicamente** el temporal al destino (`mv`)
5. **Recarga Hyprland** (`hyprctl reload`)
6. **Crea/actualiza un symlink** `overlay.current` apuntando al overlay activo

```
assemble.sh
  ├── colors.sh        → variables de color
  ├── fragments/
  │   ├── animation.{ext}   ← write by apply_animation.sh
  │   ├── border.{ext}      ← write by border.sh
  │   ├── geometry.{ext}    ← write by geometry.sh
  │   └── shader.{ext}      ← write by shader.sh
  └── overlay.current  → symlink al overlay activo
```

### Formato: Lua vs Conf

Hyprland 0.55+ introdujo un sistema de configuración Lua con funciones como `hl.` y `require()`. HVE detecta automáticamente qué formato usa tu instalación mediante `detect_format.sh`:

- Busca `hyprland.lua` con patrones Lua (`hl.`, `require(`)
- Si lo encuentra, usa formato **Lua** en todos los fragmentos y overlays
- Si no, usa el formato **Conf** clásico
- El resultado se guarda en `~/.cache/hve/hve_format` para no detectar cada vez

Cada preset de animación y borde existe en **ambos formatos** (`.conf` + `.lua`), y HVE elige automáticamente el que corresponde.

### Sistema de configuración

`~/.config/hve/config.json` — versión actual: **7**

```json
{
  "config_version": 7,
  "is_system_active": true,
  "border_size": 2,
  "border_radius": 32,
  "gaps_in": 5,
  "gaps_out": 5,
  "active_anim_file": "01_relampago",
  "active_border_file": "01_cascade",
  "active_shader_file": "",
  "auto_start": false,
  "auto_minimize_enabled": true,
  "minimize_seconds": 10,
  "language": "es",
  "tiling_mode": false,
  "theme": "dark",
  "last_applied_theme": "",
  "keybinds_enabled": true,
  "disabled_providers": []
}
```

El sistema tiene **migraciones hacia adelante**: si una versión futura añade campos, HVE migra automáticamente desde cualquier versión anterior. Cadena de migración real:

| Paso | Campo(s) añadidos |
|------|-------------------|
| v0 → v1 | Marcado de versión (configs pre-versionado) |
| v1 → v2 | Panel de ajustes: `auto_minimize_enabled`, `minimize_seconds`, `language`, `tiling_mode`, `theme` |
| v2 → v3 | `keybinds_enabled` |
| v3 → v4 | `last_applied_theme` |
| v4 → v5 | `disabled_providers` |
| v5 → v6 | `border_radius` |
| v6 → v7 | `gaps_in`, `gaps_out` |

Usa `deny_unknown_fields` para detectar configs corruptas y caer a valores por defecto.

### Sistema de colores y temas

HVE obtiene colores de tu herramienta de theming actual y los aplica tanto a su propia interfaz como a los overlays de Hyprland.

**Detección automática** (en orden de prioridad):

1. **Noctalia** → `~/.config/hypr/noctalia/noctalia-colors.{conf,lua}`
2. **pywal** → `~/.cache/wal/colors.json`
3. **matugen** → `~/.config/matugen/config.toml` + archivo de salida
4. **Manual** → escanea `~/.config/hypr/*.{lua,conf}` buscando variables de color

**Variables exportadas** por `colors.sh`:
- `$HVE_PRIMARY`
- `$HVE_SECONDARY`
- `$HVE_TERTIARY`
- `$HVE_SURFACE`
- `$HVE_SURFACE_LOWEST`
- `$HVE_ACCENT`

**Tema de la interfaz**: El binario en Rust (theme.rs) resuelve el tema (oscuro/claro/sistema) y deriva 12 colores Slint usando luminance relativa: fondo, superficie, tarjetas, hover, texto, texto secundario, acentos (cyan, amber, verde, púrpura, rojo) y bordes.

### Sistema de presets

Cada preset (animación, borde, shader) es un archivo individual con metadatos en comentarios:

```bash
# @Title: Lightning
# @Icon: bolt
# @Color: #f87171
# @Tag: FAST
# @Desc: Maximum visual response.
```

`scan.sh` lee estos metadatos y genera JSON que la interfaz consume para mostrar tarjetas con icono, color y descripción. Los presets incluyen traducciones i18n por clave (ej. `animations.presets.01_relampago.title`) que tienen prioridad sobre los metadatos raw.

**Comportamiento toggle**: si haces clic en el preset ya activo, se **desactiva** (vuelve a "ninguno").

### Fondos animados (mpvpaper)

HVE integra fondos de pantalla animados (video) para temas de **Noctalia v5** a través del plugin oficial `noctalia/mpvpaper` (que supervisa una instancia de `mpvpaper` por salida y persiste las asignaciones en `~/.local/state/noctalia/mpvpaper/assignments.json`).

- **Manifiesto ligero por tema**: los temas guardan solo referencias, no bytes de video, en `{theme_dir}/providers/noctalia-v5/mpvpaper-assignments.json` — asignaciones por salida (`*` = todas, o un conector como `DP-3`) con `filename`, `local_path`, `url` y `sha256` opcional.
- **Cascada de resolución al aplicar**: (1) `local_path` existe en disco → se usa sin descarga; (2) `filename` ya está en `video_directory` → se usa; (3) hay `url` → se descarga con `curl` y se verifica el `sha256` si está presente; (4) si no → se advierte y se omite (nunca falla la aplicación del tema).
- **Aplicar/limpiar**: como el plugin solo lee `assignments.json` al arrancar, HVE escribe el archivo y "rebota" el plugin (`noctalia msg plugins disable/enable noctalia/mpvpaper`). Los temas **sin** manifiesto envían `clear-all` primero, de modo que un video previo no pueda secuestrar la pantalla.
- **Disponibilidad**: depende del provider activo (Noctalia v5). Noctalia v4 y los presets de HVE no usan video.

### IPC (Comunicación entre procesos)

HVE expone un **servidor Unix socket** en `$XDG_RUNTIME_DIR/hve.sock` (fallback `/tmp/hve.sock`).

**Cliente incluido**: `hve-ipc <comando>`

**Comandos disponibles:**

| Comando | Función |
|---------|---------|
| `toggle-tray` | Mostrar/ocultar la ventana principal |
| `pause-restart` | Activar/desactivar el sistema |
| `next-anim` | Siguiente preset de animación |
| `next-border` | Siguiente preset de borde |
| `next-shader` | Siguiente preset de shader |
| `status` | Devuelve JSON con el estado actual |
| `refresh-theme` | Relee colores y reaplica el tema |
| `quit` | Cierra HVE |

**Atajos de teclado predeterminados** (configurables via `hve-settings`):

| Atajo | Acción |
|-------|--------|
| `SUPER + H` | Mostrar/ocultar ventana |
| `SUPER + ALT + Q` | Pausar/reanudar sistema |
| `SUPER + ALT + N` | Siguiente animación |
| `SUPER + ALT + B` | Siguiente borde |
| `SUPER + ALT + S` | Siguiente shader |

### Bandeja del sistema (System Tray)

Usando el protocolo **KDE Status Notifier Item** (ksni), HVE muestra un icono programático en la bandeja del sistema. El icono se renderiza como imagen RGBA de 32×32 directamente desde Rust, sin depender de temas de iconos del sistema.

**Menú contextual:**

1. Título: "Hyprland Visual Editor"
2. *Separador*
3. **Toggle System** — activa/desactiva toda la gestión de HVE
4. **Next Animation** — cicla animaciones
5. **Next Border** — cicla bordes
6. **Next Shader** — cicla shaders
7. *Separador*
8. **Show/Hide Window** — muestra/oculta la ventana
9. *Separador*
10. **Quit** — cierra HVE

El icono cambia de color: **verde** cuando el sistema está activo, **gris** cuando está inactivo.

### Vigilante de colores (Color Watcher)

`color_watcher.sh` es un proceso en segundo plano que **observa cambios en los archivos de color** usando `inotifywait`:

- Detecta qué herramienta de color usas (Noctalia, pywal, matugen, manual)
- Vigila el archivo/fuente de color con `inotifywait -e close_write`
- Timeout de 30s para agrupar cambios masivos (ej. regeneración de pywal)
- Usa `md5sum` para detectar cambios reales (evita refrescos falsos)
- Al detectar un cambio: ejecuta `assemble.sh` + avisa a HVE via `hve-ipc refresh-theme`
- Logs en `~/.cache/hve/color_watcher.log`

### Auto-minimizado y countdown

Cuando pierdes el foco de la ventana de HVE (detectado via `activewindow` de Hyprland), si el auto-minimizado está activo:

1. Aparece una **cuenta regresiva circular** superpuesta en el botón de cerrar
2. El temporizador decrece cada segundo con barra de progreso
3. Si recuperas el foco, se **cancela** automáticamente
4. Al llegar a 0: la ventana se oculta y se mueve al workspace especial `special:minimized`
5. El botón de cerrar siempre minimiza **instantáneamente**

### Watchdog de seguridad

`hve_watchdog.sh` se ejecuta al inicio de Hyprland (vía `exec-once` / `hl.exec_cmd`). Su función:

1. Verifica que el directorio de HVE existe (`~/.local/bin/assets/scripts/`)
2. Si **no existe** (desinstalación manual, borrado):
   - Elimina los marcadores `HVE START/END` de `hyprland.conf` y `hyprland.lua`
   - Borra `~/.cache/hve/` completo
3. Si existe, no hace nada

### Internacionalización (i18n)

Traducciones completas en **inglés** y **español**, embebidas en el binario en tiempo de compilación (`include_str!`). Estructura JSON con claves por punto:

```json
{
  "panel": {
    "tabs": {
      "home": "Inicio",
      "animations": "Animaciones"
    }
  }
}
```

El idioma se detecta de la variable `$LANG` y se puede cambiar desde la configuración (requiere reinicio).

### Gallery image pipeline performance

Single decode → four artifacts: the worker decodes the source once and
derives thumb (≤400×720 cover), hero (≤1600×900 contain), and both
parallelogram slats (collapsed/expanded). All decode/resize/bake work
runs off the Slint thread; the UI callback only wraps `Image::from_rgba8`
handles and updates row data.

Content-keyed cache: artifacts are `<len>-<hash>-thumb.png`,
`<len>-<hash>-hero.png`, `<len>-<hash>-slat.png`, and
`<len>-<hash>-slat-exp.png` under `$XDG_CACHE_HOME/hve/thumbs`. Same
bytes share one key regardless of path/mtime; warm hits load slats from
small PNGs and skip the full-size decode. `prune_stale(cache_dir, 512)`
bounded sweep keeps the newest 512 artifacts by mtime and deletes the
oldest beyond the cap (off UI thread, best-effort, once per gallery open)
— prevents unbounded stale accumulation without reading every source.

In-place cards model: `sync_cards(&VecModel, new_rows)` diffs by name,
removes stale rows descending, updates changed rows via `set_row_data`,
and pushes additions—no `ModelRc` swap on theme apply, so delegates
are preserved.

Curtain: a single parent `curtain-phase` (0→1) driven by one 16 ms
Timer is passed to mosaic cells; each cell computes its wipe from pure
math `clamp((phase*600 - delay)/300)` and renders an opaque cover
Rectangle. No Timer lives inside repeater delegates; reduced-motion
falls back to the existing crossfade.

---

## Estructura del proyecto

```
hve/
├── Cargo.toml              ← Dependencias Rust
├── build.rs                ← Compilación Slint UI
├── install.sh              ← Instalador (458 líneas, bilingüe)
├── uninstall.sh            ← Desinstalador (207 líneas, bilingüe)
├── hve.desktop             ← Acceso directo (base)
├── i18n/
│   ├── en.json             ← Traducciones inglés
│   └── es.json             ← Traducciones español
├── ui/
│   ├── main.slint          ← Ventana principal (Slint)
│   ├── components.slint    ← Componentes reutilizables (botones, paneles)
│   ├── modules.slint       ← Módulos de pestañas (Home, Animaciones, Bordes, Efectos, Temas)
│   └── theme.slint         ← Tokens de color globales (HveColors), definición NavModule
├── src/
│   ├── main.rs             ← Punto de entrada, CLI, inicialización
│   ├── config.rs           ← Config persistente (JSON, migraciones v0→v7)
│   ├── engine.rs           ← Ejecución de scripts bash
│   ├── callbacks.rs        ← Eventos de la UI Slint
│   ├── tray.rs             ← Icono de bandeja (ksni)
│   ├── ipc.rs              ← Servidor Unix socket
│   ├── hypr_ipc.rs         ← Listener de eventos de Hyprland
│   ├── watcher.rs          ← Gestor del proceso color_watcher
│   ├── theme.rs            ← Sistema de colores y temas
│   ├── theme_manager.rs    ← Guardado/aplicación de temas completos
│   ├── tr.rs               ← Traducciones (i18n)
│   ├── presets.rs          ← Población de presets en la UI
│   ├── countdown.rs        ← Auto-minimizado con cuenta regresiva
│   ├── utils.rs            ← Utilidades varias
│   ├── composer/           ← Ensamblado de overlays y fragmentos
│   │   ├── mod.rs          ← Orquestación del compositor
│   │   └── hyprland.rs     ← Compose del overlay para Hyprland
│   └── providers/          ← Providers de temas (captura/restauración por shell)
│       ├── noctalia.rs     ← Provider Noctalia v4/v5 (colores, wallpapers, plantillas)
│       ├── shell.rs        ← Detección de shell activo + rutas por versión
│       ├── mpvpaper.rs     ← Fondos animados (video) para temas Noctalia v5
│       └── hve_presets.rs  ← Provider de presets HVE (anim/border/shader/geometría)
├── assets/
│   ├── hve_logo.svg        ← Icono vectorial
│   ├── scripts/            ← 15 scripts bash (ver abajo)
│   ├── animations/         ← 18 presets × 2 formatos = 36 archivos
│   ├── borders/            ← 14 presets × 2 formatos = 28 archivos
│   ├── shaders/            ← 9 shaders GLSL (.frag)
│   ├── fragments/          ← Fragmentos dinámicos (runtime)
│   └── owl_neon.png        ← Imagen decorativa
└── assets/scripts/
    ├── utils.sh            ← Funciones compartidas, paths
    ├── colors.sh           ← Detector/exportador de colores
    ├── detect_format.sh    ← Detecta Lua vs Conf
    ├── assemble.sh         ← Ensamblador de overlays
    ├── init.sh             ← Activar/desactivar HVE
    ├── apply_animation.sh  ← Aplicar preset de animación
    ├── border.sh           ← Aplicar preset de borde
    ├── shader.sh           ← Aplicar shader
    ├── geometry.sh         ← Aplicar geometría (border_size)
    ├── get_colors.sh       ← Obtener colores en JSON
    ├── color_watcher.sh    ← Vigilante de cambios en colores
    ├── scan.sh             ← Escanear y describir presets
    ├── hve-ipc             ← Cliente IPC en Python
    ├── hve_watchdog.sh     ← Watchdog de seguridad
    └── format_test.sh      ← Tests de formato (no producción)
```

### Código fuente (src/)

| Archivo | Líneas | Responsabilidad |
|---------|--------|-----------------|
| `main.rs` | 1729 | Punto de entrada, CLI (`--tray`, `-v`), inicialización de todos los sistemas, protección multinstancia (lock exclusivo), listeners, navegación |
| `config.rs` | 611 | Config JSON, migraciones v0→v7, `deny_unknown_fields`, rutas XDG |
| `engine.rs` | 699 | Interface tipada sobre los scripts bash, `EngineError`, `ColorScheme`, `PresetInfo`, `ScanEntry` |
| `tray.rs` | 277 | Icono programático ksni (SDF + letras HVE), menú contextual, colores activo/inactivo |
| `theme.rs` | 451 | Resolución de tema (oscuro/claro/sistema), paletas Tailwind, colores Slint derivados, render de logo |
| `theme_manager.rs` | 329 | Gestión de temas completos: listar, guardar, aplicar, renombrar, eliminar (con providers) |
| `ipc.rs` | 391 | Servidor Unix socket, comandos + `status` JSON, cleanup al salir |
| `hypr_ipc.rs` | 297 | Listener `socket2.sock`, evento `configreloaded`, refresco UI con throttle 3s |
| `presets.rs` | 221 | Escaneo y población de presets, traducción de metadatos, selección activa |
| `callbacks.rs` | 290 | Conexión UI ↔ lógica, toggle de presets, toggle de sistema, geometría |
| `tr.rs` | 223 | Carga de JSON embebido, resolución por clave con dot-notation, detección de idioma |
| `countdown.rs` | 328 | Timer thread-local, countdown UI, minimizado al perder foco |
| `watcher.rs` | 83 | Spawn de `color_watcher.sh`, logging |
| `utils.rs` | 101 | Utilidades varias |
| `composer/mod.rs` | 488 | Orquestación del ensamblado de overlays |
| `composer/hyprland.rs` | 367 | Compose del overlay para Hyprland |
| `providers/noctalia.rs` | 1189 | Provider Noctalia v4/v5: colores, wallpapers, template processor, manifest mpvpaper |
| `providers/shell.rs` | 574 | Detección de shell activo y rutas por versión (`ShellProvider`, `ShellDetector`) |
| `providers/mpvpaper.rs` | 758 | Fondos animados (video) para temas Noctalia v5 |
| `providers/hve_presets.rs` | 149 | Provider de presets HVE (animación, borde, shader, geometría) |

### Scripts (assets/scripts/)

| Script | Líneas | Función |
|--------|--------|---------|
| `utils.sh` | 96 | Rutas compartidas: `HVE_ASSETS_DIR`, `HVE_SCRIPTS_DIR`, `HVE_FRAGMENTS_DIR`, `HVE_SAFE_DIR`, `HVE_HYPR_DIR`; funciones `hve_resolve_preset()`, `hve_trim()`, `hve_unquote()` |
| `assemble.sh` | 152 | **Corazón del sistema**: ensambla colores + fragmentos → overlay atómico, valida sintaxis, recarga Hyprland |
| `init.sh` | 195 | Activar/desactivar HVE: inyecta o remueve marcadores en `hyprland.conf`/`hyprland.lua`, crea `hve-settings` con defaults |
| `colors.sh` | 407 | Detector inteligente de colores: Noctalia → pywal → matugen → manual, exporta 6 variables de color |
| `color_watcher.sh` | 217 | Vigilante `inotifywait` con hash-based detection, timeout 30s, refresh automático |
| `scan.sh` | 122 | Escáner de presets con parseo de metadatos (@Title, @Desc, @Tag, @Icon, @Color), output JSON |
| `shader.sh` | 99 | Aplica shader `.frag`, genera wrapper con `decoration.screen_shader`, "none" limpia via hyprctl |
| `geometry.sh` | 126 | Escribe fragmento de `general.border_size` |
| `apply_animation.sh` | 61 | Aplica preset de animación, copia a `fragments/animation.{ext}` |
| `border.sh` | 58 | Aplica preset de borde, copia a `fragments/border.{ext}` |
| `detect_format.sh` | 62 | Detecta Lua vs Conf, cachea resultado |
| `get_colors.sh` | 11 | Envuelve `colors.sh`, output JSON |
| `hve_watchdog.sh` | 28 | Seguridad: si HVE no existe, limpia marcadores y cache |
| `hve-ipc` | 26 | Cliente Python para el socket Unix |
| `format_test.sh` | 79 | Suite de tests de formato (no usada en producción) |

### Presets de animación (assets/animations/)

18 animaciones, cada una con archivos `.conf` y `.lua`:

01_relampago, 02_inercia_elastica, 03_seda_minimalista, 04_minimalismo_snappy,
05_material_moderno, 06_impacto_clasico, 07_lineal, 08_cristal, 09_seda_silk,
10_retro_arcade, 11_futurista, 12_rebote, 13_organico, 14_elastico,
15_desvanecido, 16_dinamico, 17_sutil, 18_energico

### Presets de bordes (assets/borders/)

14 bordes, cada uno con archivos `.conf` y `.lua`:

01_cascade hasta 14_looper

### Shaders (assets/shaders/)

9 shaders GLSL (`.frag`) — no tienen dualidad de formato porque Hyprland usa el mismo archivo independientemente del formato de configuración:

01_night, 02_mono, 03_vibrant, 04_sharp, 05_ink, 06_invert, 07_oled, 08_vision, 09_hybrid

### Fragmentos dinámicos (assets/fragments/)

Esta carpeta se crea en **runtime** dentro de `~/.local/bin/assets/fragments/` (o donde estén los assets). Contiene los fragmentos activos actuales:

- `animation.{lua,conf}` — animación activa
- `border.{lua,conf}` — borde activo
- `geometry.{lua,conf}` — tamaño de borde activo
- `shader.{lua,conf}` — shader activo (wrapper)

Son **sobrescritos** cada vez que cambias un preset.

### Archivos de runtime

| Ruta | Propósito |
|------|-----------|
| `~/.config/hve/config.json` | Configuración persistente del usuario |
| `~/.cache/hve/hve.lock` | Lock exclusivo (previene múltiples instancias) |
| `~/.cache/hve/hve_format` | Cache del formato detectado ("lua" o "conf") |
| `~/.cache/hve/hve-settings.{lua,conf}` | Fragmento gestionado con reglas de ventana, keybinds y autostart |
| `~/.cache/hve/overlay.{lua,conf}` | Overlay activo (ensamblado por assemble.sh) |
| `~/.cache/hve/overlay.current` | Symlink al overlay activo |
| `~/.cache/hve/colors.json` | Señal de colores (escrito por color_watcher) |
| `~/.cache/hve/color_watcher.log` | Logs del vigilante de colores |
| `~/.cache/hve/fragments/` | Fragmentos activos copiados por assemble.sh |

---

## Uso

### Interfaz gráfica

```
hve
```

Abre la ventana principal con 5 pestañas de navegación:

| Pestaña | Módulo |
|---------|--------|
| ⌂ **Inicio** | Estado del sistema, información general, sección About |
| ▶ **Animaciones** | Explora y aplica presets de animación |
| ◻ **Bordes** | Explora y aplica presets de bordes |
| ♦ **Efectos** | Shaders, geometría, ajustes |
| ✦ **Temas** | Guardar, aplicar, renombrar y eliminar temas completos |

La pestaña **Inicio** incluye una sección **About HVE** con la descripción del proyecto, características clave, árbol del proyecto y un enlace a la documentación.

La pestaña **Temas** gestiona configuraciones completas del escritorio: escribe un nombre, pulsa *Guardar* y el estado actual (animación, borde, shader, geometría y —según el provider activo— colores, wallpapers estáticos o animados de mpvpaper y reglas de ventana) se captura como un tema. Los temas se pueden **aplicar**, **renombrar**, **eliminar** (con confirmación) y **recargar** con la configuración actual. Solo se muestran los temas cuyos providers están activos en el sistema.

La ventana incluye un panel de **Ajustes** donde puedes configurar:
- Tema (oscuro/claro/sistema)
- Idioma (inglés/español, requiere reinicio)
- Auto-minimizado activable con tiempo configurable
- Modo tiling (flotante/embaldosado)
- Atajos de teclado activables/desactivables
- Tamaño de borde general

### Línea de comandos

```
hve [OPCIONES]

  --tray        Inicia minimizado en la bandeja del sistema
  -v            Log verboso (DEBUG)
  -vv           Log extremadamente verboso (TRACE)
```

### Bandeja del sistema

```
hve --tray
```

Inicia directamente en la bandeja. Todas las funciones están disponibles desde el menú contextual del icono.

### Atajos de teclado (IPC)

Requiere instalar `hve-ipc` durante la instalación. Una vez instalado:

```bash
hve-ipc toggle-tray      # Mostrar/ocultar ventana
hve-ipc pause-restart    # Pausar/reanudar sistema
hve-ipc next-anim        # Siguiente animación
hve-ipc next-border      # Siguiente borde
hve-ipc next-shader      # Siguiente shader
hve-ipc status           # Estado actual (JSON)
hve-ipc refresh-theme    # Refrescar colores
hve-ipc quit             # Cerrar HVE
```

Si los atajos de teclado están activos (por defecto: sí), puedes usar:

| Combinación | Acción |
|-------------|--------|
| `SUPER + H` | Mostrar/ocultar ventana |
| `SUPER + ALT + Q` | Pausar/reanudar |
| `SUPER + ALT + N` | Siguiente animación |
| `SUPER + ALT + B` | Siguiente borde |
| `SUPER + ALT + S` | Siguiente shader |

---

## Preguntas frecuentes

**¿HVE modifica mis archivos de configuración de Hyprland?**
No. Solo inyecta dos líneas entre marcadores `# HVE START` / `# HVE END` (o `--` en Lua). Al desinstalar o desactivar, esas líneas se eliminan y tu configuración queda exactamente como estaba.

**¿Funciona con Noctalia Shell?**
Sí. HVE es agnóstico al escritorio. Funciona con Hyprland puro, Noctalia Shell, o cualquier entorno basado en Hyprland.

**¿Qué pasa si borro los archivos de HVE manualmente?**
El watchdog `hve_watchdog.sh` —que se ejecuta al iniciar Hyprland— detecta la ausencia y limpia los marcadores automáticamente.

**¿Puedo tener dos instancias de HVE?**
No. HVE usa un lock exclusivo en `~/.cache/hve/hve.lock`. Si intentas abrir una segunda instancia, se cierra inmediatamente.

**¿Los cambios de animación/borde/shader son instantáneos?**
Sí. Cada cambio ejecuta `assemble.sh` que escribe el overlay y llama a `hyprctl reload`. Hyprland aplica los cambios al instante.

**¿Puedo usar HVE sin interfaz gráfica?**
Sí, con `hve --tray` (solo bandeja) y los comandos IPC desde la terminal o atajos de teclado.

**¿HVE consume muchos recursos?**
No. El binario es Rust compilado, liviano. El color_watcher usa `inotifywait` que no consume CPU mientras no hay cambios. Sin color_watcher activo, HVE solo usa recursos cuando interactúas con la interfaz.

**¿Qué pasa si cambio de herramienta de colores (pywal → matugen)?**
HVE lo detecta automáticamente. El color_watcher siempre elige la herramienta disponible con la prioridad: Noctalia > pywal > matugen > manual.
