<p align="center">
<img src="preview.png" alt="Hyprland Visual Editor Banner" width="800">
</p>

# 🦉 Hyprland Visual Editor (HVE)

### Dynamic Visual Control for Hyprland Customization

**Hyprland Visual Editor** is a professional-grade, non-destructive customization ecosystem for **Hyprland**, built as a native plugin for **Noctalia Shell**. It allows you to instantly change animations, borders, shaders, and geometry, without any risk of corrupting your main `hyprland.conf`.

---

## ✨ Key Features

| Feature | Description |
| --- | --- |
| **🛡️ Guardian Shield** | Deploys a secure external path. If the plugin is disabled, the system self-cleans on reboot. |
| **⚡ Native Integration** | Uses the official Noctalia Plugin API (3.6.0+) for settings and state persistence. |
| **🎬 Motion Library** | Swap between animation styles (Silk, Cyber Glitch, etc.) in milliseconds. |
| **🎨 Smart Borders** | Dynamic gradients and reactive effects tied to window focus. |
| **🕶️ Real-Time Shaders** | Post-processing filters (CRT, OLED, Night) applied on the fly via GLSL. |
| **🌍 Native i18n** | Full multilingual support using Noctalia's native translation engine via `i18n/`. |

---

## 📂 Project Structure

To ensure maximum stability, HVE follows the official Noctalia plugin architecture:

```text
~/.config/noctalia/
├── HVE/                        # 🛡️ THE SAFE REFUGE (Generated on activation)
│   ├── overlay.conf            # MASTER CONFIG: Sourced directly by Hyprland
│   └── hve_watchdog.sh         # Guardian script for passive auto-cleanup
│
└── plugins/hyprland-visual-editor/
    ├── manifest.json           # Plugin metadata and Entry Points
    ├── BarWidget.qml           # Entry Point: Taskbar trigger icon
    ├── Panel.qml               # Main UI & SmartPanel configuration
    │
    ├── modules/                # UI Components (QML)
    │   ├── WelcomeModule.qml   # Activation logic & Native Persistence
    │   ├── BorderModule.qml    # Style & Geometry selectors
    │   └── ...                 # Animation and Shader modules
    │
    ├── assets/                 # The "Engine" & Resources
    │   ├── borders/            # Style library (.conf)
    │   ├── animations/         # Movement library (.conf)
    │   ├── shaders/            # GLSL Post-processing filters (.frag)
    │   └── scripts/            # Bash Engine (Assembly and logic)
    │
    ├── i18n/                   # Official Translation Files (.json)
    └── settings.json           # Native Persistence (Managed by Noctalia)

```

---

## 🚀 Installation and Activation

1. Download this repository into `~/.config/noctalia/plugins/hyprland-visual-editor`.
2. Go to Noctalia Shell's **Settings > Plugins** and enable it.
3. Open the plugin panel and toggle the **"Enable Visual Editor"** switch.

> [!IMPORTANT]
> Ensure you have the following line in your `hyprland.conf` to allow HVE to inject styles:
> `source = ~/.config/noctalia/HVE/overlay.conf`

---

## ⌨️ IPC & Keybinds (Pro Features)

HVE supports native IPC calls. You can open the panel with a Hyprland keybind:

```bash
bind = $mainMod, V, exec, qs -c noctalia-shell ipc call plugin:hyprland-visual-editor openPanel

```

---

## 🧠 Technical Architecture

HVE uses a **dynamic construction** flow combined with Noctalia's native API:

1. **Native State:** All user preferences are handled via `pluginApi.pluginSettings`.
2. **Dynamic Scanning:** The `scan.sh` script extracts metadata from style headers.
3. **Assembly:** The engine unifies all active fragments into the external `HVE/overlay.conf`.
4. **Protection:** A watchdog script monitors the plugin state on every boot.

---

## 🛠️ Modding Guide (Metadata Protocol)

To add your own custom styles and have them automatically appear in the panel, use these formats:

### For Animations and Borders (`.conf`)

```ini
# @Title: My Epic Style
# @Icon: rocket
# @Color: #ff0000
# @Tag: CUSTOM
# @Desc: A brief description of your creation.

general {
    col.active_border = rgb(ff0000) rgb(00ff00) 45deg
}

```

### For Shaders (`.frag`)

```glsl
// @Title: Vision Filter
// @Icon: eye
// @Color: #4ade80
// @Tag: NIGHT
// @Desc: Post-processing description.

void main() { ... }

```

---

## ⚠️ Troubleshooting

**How to see debug logs?**
Launch Noctalia from the terminal to see HVE specific logs:

```bash
NOCTALIA_DEBUG=1 qs -c noctalia-shell | grep HVE

```

**Border animations freeze?**
This is a known Hyprland limitation during hot-reloads. Simply reopen the affected window to restore the loop effect.

---

## ❤️ Credits

* **Architecture & Core:** Ximo
* **Technical Assistance:** Co-programmed with AI
* **Inspiration:** HyDE Project & JaKooLit.
* **Community:** Thanks to all Noctalia users.