import QtQuick
import Quickshell
import Quickshell.Io
import QtQuick.Layouts
import QtQuick.Controls
import qs.Commons
import qs.Widgets
import qs.Services.UI
import "./modules"

Item {
    id: root

    // IMPORTANT: Ensure standard naming for the API
    property var pluginApi: null
    property var runHypr: null
    readonly property int barHeight: 20

    // --- OFFICIAL SMARTPANEL PROPERTIES ---
    // Required by Noctalia for proper window sizing and API injection
    readonly property var geometryPlaceholder: panelContainer
    readonly property bool allowAttach: true

    // Centralized plugin directory path
    readonly property string pluginDir: Settings.configDir + "/plugins/hyprland-visual-editor"

    // --- SCRIPT ENGINE (FIXED MEMORY LEAK) ---
    Component {
        id: processFactory
        Process {
            id: tempProcess
            // Solo logueamos errores reales para no inflar la memoria
            onStderrChanged: if (stderr.trim() !== "") Logger.e("HVE", "Script Error: " + stderr)
            
            // Destrucción total al terminar para liberar la RAM
            onExited: tempProcess.destroy()
        }
    }

    function runScript(scriptName, args) {
        var scriptPath = pluginDir + "/assets/scripts/" + scriptName
        Logger.i("HVE", "Executing: " + scriptPath + " " + args)
        
        // Creamos una instancia desechable
        var p = processFactory.createObject(root)
        p.command = ["bash", scriptPath, args]
        p.running = true
    }

    property real contentPreferredWidth: 700 * Style.uiScaleRatio
    property real contentPreferredHeight: 700 * Style.uiScaleRatio

    anchors.fill: parent

    // Main container required by geometryPlaceholder
    Rectangle {
        id: panelContainer
        anchors.fill: parent
        anchors.topMargin: root.barHeight
        color: "transparent"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Style.marginL
            spacing: Style.marginM

            // 1. CENTERED HEADER
            RowLayout {
                Layout.fillWidth: true
                spacing: Style.marginS
                Layout.bottomMargin: Style.marginL

                Item { Layout.fillWidth: true }

                NIcon {
                    icon: "adjustments-horizontal"
                    color: Color.mPrimary
                    pointSize: Style.fontSizeXXL
                }

                ColumnLayout {
                    spacing: 0
                    Layout.alignment: Qt.AlignCenter
                    NText {
                        // Direct translation call without mitigation
                        text: root.pluginApi.tr("panel.header_title")
                        pointSize: Style.fontSizeXL
                        font.weight: Font.Bold
                        color: Color.mPrimary
                    }
                    NText {
                        // Direct translation call without mitigation
                        text: root.pluginApi.tr("panel.header_subtitle")
                        pointSize: Style.fontSizeS
                        color: Color.mOnSurfaceVariant
                    }
                }

                Item { Layout.fillWidth: true }
            }

            // 2. NAVIGATION BAR
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                TabItem {
                    label: root.pluginApi.tr("panel.tabs.home")
                    iconName: "home"
                    index: 0
                    accentColor: "#38bdf8"
                    isSelected: stackLayout.currentIndex === 0
                }
                TabItem {
                    label: root.pluginApi.tr("panel.tabs.animations")
                    iconName: "movie"
                    index: 1
                    accentColor: "#fbbf24"
                    isSelected: stackLayout.currentIndex === 1
                }
                TabItem {
                    label: root.pluginApi.tr("panel.tabs.borders")
                    iconName: "border-all"
                    index: 2
                    accentColor: "#10b981"
                    isSelected: stackLayout.currentIndex === 2
                }
                TabItem {
                    label: root.pluginApi.tr("panel.tabs.effects")
                    iconName: "wand"
                    index: 3
                    accentColor: "#c084fc"
                    isSelected: stackLayout.currentIndex === 3
                }
            }

            // 3. CONTENT AREA
            NBox {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: Color.mSurfaceVariant
                radius: Style.radiusM
                clip: true

                StackLayout {
                    id: stackLayout
                    anchors.fill: parent
                    anchors.margins: Style.marginS
                    currentIndex: 0

                    // Passing pluginApi to children
                    WelcomeModule   { pluginApi: root.pluginApi; runScript: root.runScript }
                    AnimationModule { pluginApi: root.pluginApi; runScript: root.runScript }
                    BorderModule    { pluginApi: root.pluginApi; runScript: root.runScript }
                    ShaderModule    { pluginApi: root.pluginApi; runScript: root.runScript }
                }
            }
        }
    }

    component TabItem : Rectangle {
        id: tabRoot
        property string label
        property string iconName
        property color accentColor: Color.mPrimary
        property int index
        property bool isSelected

        Layout.fillWidth: true
        height: 40 * Style.uiScaleRatio
        radius: Style.radiusM

        readonly property color currentAccent: isSelected ? Color.mPrimary : accentColor

        color: isSelected
            ? Qt.alpha(Color.mPrimary, 0.15)
            : (tabMouse.containsMouse ? Qt.alpha(accentColor, 0.1) : "transparent")

        border.width: 1
        border.color: isSelected
            ? Color.mPrimary
            : (tabMouse.containsMouse ? accentColor : Qt.alpha(accentColor, 0.2))

        Behavior on color { ColorAnimation { duration: 150 } }
        Behavior on border.color { ColorAnimation { duration: 150 } }

        MouseArea {
            id: tabMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: stackLayout.currentIndex = index
        }

        RowLayout {
            anchors.centerIn: parent
            spacing: 8

            NIcon {
                icon: iconName
                color: (isSelected || tabMouse.containsMouse) ? tabRoot.currentAccent : Color.mOnSurfaceVariant
                Behavior on color { ColorAnimation { duration: 150 } }
            }

            NText {
                text: label
                font.weight: isSelected ? Font.Bold : Font.Normal
                color: (isSelected || tabMouse.containsMouse) ? Color.mOnSurface : Color.mOnSurfaceVariant
                pointSize: Style.fontSizeS
            }
        }
    }
}