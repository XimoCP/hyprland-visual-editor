<p align="center">
<img src="preview.png" alt="Noctalia Visual Layer Banner" width="800">
</p>

# 🦉 Noctalia Visual Layer

### El Controlador Estético Definitivo para Hyprland

**Noctalia Visual Layer (NVL)** es un ecosistema de personalización dinámica y no destructiva para **Hyprland** y **Noctalia Shell**, desarrollado con **Quickshell (QML)** y **Bash**. Permite cambiar animaciones, bordes, shaders y geometría al instante, sin riesgo de corromper la configuración principal del usuario.

---

## ✨ Características Principales

| Característica | Descripción |
| --- | --- |
| **🛡️ Escudo Guardián (Watchdog)** | NVL despliega una ruta externa segura y un script de autolimpieza. Si desinstalas el plugin, el sistema se autolimpia al reiniciar sin romper Hyprland. |
| **⚡ Aplicación Instantánea** | La lógica reactiva aplica cualquier cambio en milisegundos, sin necesidad de recargar. |
| **🎬 Biblioteca de Movimiento** | Desde la suavidad de *Seda* hasta la agresividad de *Cyber Glitch*. |
| **🎨 Bordes Inteligentes** | Degradados dinámicos y efectos reactivos al foco de la ventana. |
| **🕶️ Shaders en Tiempo Real** | Filtros de post-procesado (Noche, CRT, Monocromo, OLED) aplicados al vuelo. |
| **🌍 Internacionalización** | Soporte nativo multilingüe. El sistema se adapta a tu idioma automáticamente. |

---

## 📂 Estructura del Proyecto

Para garantizar la máxima estabilidad, NVL separa la lógica del plugin de la configuración que se inyecta en el sistema:

```text
~/.config/noctalia/
├── NVL/                        # 🛡️ REFUGIO SEGURO (Generado al activar)
│   ├── overlay.conf            # ARCHIVO MAESTRO: Sourced directamente por Hyprland
│   └── nvl_watchdog.sh         # Script guardián para autolimpieza pasiva
│
└── plugins/noctalia-visual-layer/
    ├── manifest.json           # Metadatos y definición del plugin
    ├── BarWidget.qml           # Punto de entrada: Botón disparador en la barra
    ├── Panel.qml               # Interfaz principal (Contenedor de módulos)
    │
    ├── modules/                # Lógica de la Interfaz (QML)
    │   ├── WelcomeModule.qml   # Panel de bienvenida y persistencia
    │   ├── BorderModule.qml    # Selector de estilos y colores
    │   └── ...                 # Otros módulos (Animation, Shader, etc.)
    │
    ├── assets/                 # El "Motor" y Recursos
    │   ├── nvl-colors.conf     # DINÁMICO: Colores procesados con Alpha (Mustache)
    │   ├── borders/            # Biblioteca de estilos (.conf)
    │   ├── animations/         # Biblioteca de curvas de movimiento (.conf)
    │   ├── shaders/            # Filtros de post-procesado (.frag)
    │   ├── fragments/          # Estado actual (Simlinks de los estilos activos)
    │   └── scripts/            # Bash Engine (Ensamblado y lógica de aplicación)
    │
    └── i18n/                   # Traducciones (Soporte para más de 16 idiomas)

```

---

## 🚀 Instalación y Activación

Es necesario tener **Noctalia Shell** y **Hyprland** para poder utilizar este plugin. Aquí tienes los pasos exactos para su correcta instalación:

1. Descarga este repositorio en la ruta `~/.config/noctalia/plugins/`.
2. Una vez tengas el plugin en la ruta correcta, debes ir a la **Configuración** de Noctalia Shell e ir al apartado de **Plugins**, donde debería aparecer en la lista de instalados para poder activarlo. Una vez activo, debe aparecer en la barra de Noctalia.
3. Una vez dentro del panel, para que las modificaciones funcionen, debes activar el interruptor **"Habilitar Visual Layer"**.

> [!NOTE]
> Al activarlo, NVL desplegará automáticamente el escudo guardián e inyectará una ruta externa segura (`source = ~/.config/noctalia/NVL/overlay.conf`) en tu `hyprland.conf`. Al apagarlo, limpiará tu configuración y eliminará el refugio seguro, dejándolo en su estado original inmaculado.

---

## 🧠 Arquitectura Técnica (El Sistema de Fragmentos)

A diferencia de otros gestores que editan archivos estáticos, NVL utiliza un flujo de **construcción dinámica** combinado con un recolector de basura pasivo:

1. **Escaneo Dinámico:** El script `scan.sh` extrae metadatos directamente de los comentarios en los archivos de `assets/`.
2. **Generación de Fragmentos:** Al seleccionar un estilo en QML, se clona en `assets/fragments/`.
3. **Ensamblaje:** `assemble.sh` unifica todos los fragmentos activos y los escribe en la ruta externa segura (`NVL/overlay.conf`).
4. **Inyección y Protección:** Hyprland recarga el nuevo overlay externo, mientras `nvl_watchdog.sh` vigila silenciosamente la existencia del plugin en cada arranque.

```mermaid
graph LR
    A[UI QML] -->|Calcula Intención| B(Script Bash)
    B -->|Genera| C[Fragmento .conf]
    B -->|Despliega| W[nvl_watchdog.sh]
    C -->|assemble.sh| D[NVL/overlay.conf]
    D -->|reload| E[Hyprland Core]
    W -->|Protege| E

```

---

## 🛠️ Guía de Modding (Protocolo de Metadatos)

Para añadir tus propios archivos y que aparezcan en el panel automáticamente, usa este formato en la cabecera:

### Para Animaciones y Bordes (`.conf`)

```ini
# @Title: Mi Estilo Épico
# @Icon: rocket
# @Color: #ff0000
# @Tag: CUSTOM
# @Desc: Una descripción breve de tu creación.

general {
    col.active_border = rgb(ff0000) rgb(00ff00) 45deg
}

```

### Para Shaders (`.frag`)

```glsl
// @Title: Filtro Vision
// @Icon: eye
// @Color: #4ade80
// @Tag: NIGHT
// @Desc: Descripción del post-procesado.

void main() { ... }

```

### 🎨 Iconografía

El sistema utiliza **Tabler Icons**. Para añadir nuevos iconos, consulta el catálogo en [tabler-icons.io](https://tabler-icons.io/) y usa el nombre exacto (ej. `brand-github`, `bolt`).

---

## ⚠️ Solución de Problemas

**El panel muestra exclamaciones `!!text!!` en un estilo.**

* El sistema no encuentra la traducción oficial. Si persiste, el sistema usará el texto de respaldo de tu archivo automáticamente (Fallback seguro).

**He creado un estilo propio y Hyprland da error.**

* NVL aísla los errores en `overlay.conf`. Si un estilo no carga, revisa la sintaxis de código de tu archivo personal.

**Las animaciones de los bordes se detienen y no giran en bucle.**

* Es una limitación conocida del motor de Hyprland al recargar la configuración en caliente sobre ventanas que ya están dibujadas en pantalla. Para solucionarlo de inmediato, basta con reabrir la ventana afectada. De todos modos, este detalle se irá disipando por sí solo a medida que abras nuevas ventanas durante tu flujo de trabajo, y funcionará de manera impecable y global la próxima vez que inicies sesión.

---

## ❤️ Créditos y Autoría

* **Arquitectura & Core:** Ximo
* **Asistencia Técnica:** Co-programado con IA (Gemini - Google)
* **Inspiración:** HyDE Project & JaKooLit.
* **Comunidad:** Gracias a todos los usuarios de Noctalia.
