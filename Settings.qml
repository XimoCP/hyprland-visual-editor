import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Widgets

ColumnLayout {
  id: root

  property var pluginApi: null

  property var cfg: pluginApi?.pluginSettings || ({})
  property var defaults: pluginApi?.manifest?.metadata?.defaultSettings || ({})

  property string valueOverlayPath: cfg.overlayPath ?? defaults.overlayPath ?? "~/.cache/noctalia/HVE/overlay.conf"
  property bool valueAutoApply: cfg.autoApply ?? defaults.autoApply ?? true

  spacing: Style.marginL

  Component.onCompleted: {
    Logger.d("HVE", "Settings UI loaded");
  }

  ColumnLayout {
    spacing: Style.marginM
    Layout.fillWidth: true

    NTextInput {
      Layout.fillWidth: true
      label: pluginApi?.tr("settings.path.label") || ""
      description: pluginApi?.tr("settings.path.desc") || ""
      text: root.valueOverlayPath
      onTextChanged: root.valueOverlayPath = text
      readOnly: true
    }

    NToggle {
      Layout.fillWidth: true
      label: pluginApi?.tr("settings.autoapply.label") || ""
      description: pluginApi?.tr("settings.autoapply.desc") || ""
      checked: root.valueAutoApply
      onToggled: root.valueAutoApply = !root.valueAutoApply
    }
  }

  function saveSettings() {
    if (!pluginApi) {
      Logger.e("HVE", "Cannot save settings: pluginApi is null");
      return;
    }

    pluginApi.pluginSettings.overlayPath = root.valueOverlayPath;
    pluginApi.pluginSettings.autoApply = root.valueAutoApply;
    pluginApi.saveSettings();

    Logger.d("HVE", "Settings saved successfully");
  }
}