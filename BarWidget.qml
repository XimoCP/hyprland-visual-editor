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

  // --- STANDARD CONFIGURATION LOGIC ---
  property var cfg: pluginApi?.pluginSettings || ({})
  property var defaults: pluginApi?.manifest?.metadata?.defaultSettings || ({})

  readonly property string iconColorKey: cfg.iconColor ?? defaults.iconColor ?? "onSurface"

  // --- PLUGIN DATA ---
  icon: "adjustments-horizontal"
  tooltipText: pluginApi?.tr("widget.tooltip") || "Hyprland Visual Editor"

  // --- SYSTEM STYLES ---
  tooltipDirection: BarService.getTooltipDirection(screen?.name)
  baseSize: Style.getCapsuleHeightForScreen(screen?.name)
  applyUiScale: false

  customRadius: Style.radiusL 

  colorBg: Style.capsuleColor
  colorFg: Color.resolveColorKey(iconColorKey)

  border.color: Style.capsuleBorderColor
  border.width: Style.capsuleBorderWidth

  // --- INTERACTION ---
  onClicked: {
    if (pluginApi) {
      pluginApi.openPanel(root.screen, this);
    }
  }

  // --- STANDARD CONTEXT MENU ---
  NPopupContextMenu {
    id: contextMenu

    model: [
      {
        "label": pluginApi?.tr("menu.settings") || "Settings",
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