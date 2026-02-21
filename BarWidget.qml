import Quickshell
import qs.Commons
import qs.Services.UI
import qs.Widgets

NIconButton {
  id: root

  property var pluginApi: null

  property ShellScreen screen
  property string widgetId: ""
  property string section: ""

  // --- LÓGICA ESTÁNDAR DE CONFIGURACIÓN (1:1 Hello World) ---
  property var cfg: pluginApi?.pluginSettings || ({})
  property var defaults: pluginApi?.manifest?.metadata?.defaultSettings || ({})

  // Aquí definimos el color. Si no hay config, usa "primary" por defecto.
  readonly property string iconColorKey: cfg.iconColor ?? defaults.iconColor ?? "onSurface"

  // --- DATOS PROPIOS DEL PLUGIN ---
  icon: "adjustments-horizontal"
  tooltipText: pluginApi?.tr("widget.tooltip") || "Noctalia Visual Layer"

  // --- ESTILOS DEL SISTEMA (1:1 Hello World) ---
  // Usamos las variables globales para máxima compatibilidad
  tooltipDirection: BarService.getTooltipDirection(screen?.name)
  baseSize: Style.getCapsuleHeightForScreen(screen?.name)
  applyUiScale: false

  customRadius: Style.radiusL // El estándar usa Radius L

  // Colores del sistema (Cápsula sólida)
  colorBg: Style.capsuleColor
  colorFg: Color.resolveColorKey(iconColorKey)

  border.color: Style.capsuleBorderColor
  border.width: Style.capsuleBorderWidth

  // --- INTERACCIÓN ---
  onClicked: {
    if (pluginApi) {
      pluginApi.openPanel(root.screen, this);
    }
  }

  // --- MENÚ CONTEXTUAL ESTÁNDAR ---
  NPopupContextMenu {
    id: contextMenu

    model: [
      {
        "label": pluginApi?.tr("menu.settings") || "Ajustes",
        "action": "settings",
        "icon": "settings"
      },
    ]

    onTriggered: function (action) {
      contextMenu.close();
      PanelService.closeContextMenu(screen);
      if (action === "settings") {
        BarService.openPluginSettings(root.screen, pluginApi.manifest);
      }
    }
  }

  onRightClicked: {
    PanelService.showContextMenu(contextMenu, root, screen);
  }
}
