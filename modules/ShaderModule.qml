import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons

NScrollView {
    id: shaderRoot

    property var pluginApi: null
    property var runHypr: null
    property var runScript: null

    readonly property string pluginDir: Settings.configDir + "/plugins/hyprland-visual-editor"

    // --- OFFICIAL PERSISTENCE BOUND PROPERTIES ---
    property string activeShaderFile: pluginApi?.pluginSettings?.activeShaderFile || ""

    Layout.fillWidth: true
    Layout.fillHeight: true
    contentHeight: mainLayout.implicitHeight + 50
    clip: true

    // --- SCANNER ---
    Process {
        id: scanner
        running: true
        command: ["bash", pluginDir + "/assets/scripts/scan.sh", "shaders"]
        property string outputData: ""
        stdout: SplitParser { onRead: function(data) { scanner.outputData += data; } }
        onExited: (code) => {
            if (code === 0) {
                try {
                    var data = JSON.parse(scanner.outputData);
                    shaderModel.clear();
                    for (var i = 0; i < data.length; i++) { shaderModel.append(data[i]); }
                } catch (e) { 
                    Logger.e("HVE", "JSON Parsing Error in Shaders: " + e); 
                }
            }
            // Memory management: clear accumulated string after parsing to prevent RAM leaks
            scanner.outputData = ""
        }
    }

    // --- DELEGATE ---
    Component {
        id: shaderDelegate
        NBox {
            id: cardRoot
            Layout.fillWidth: true
            Layout.preferredHeight: 85 * Style.uiScaleRatio
            radius: Style.radiusM

            // Safe property bindings for ListModel data
            property string cTitleKey: model.title !== undefined ? model.title : ""
            property string cDescKey: model.desc !== undefined ? model.desc : ""
            property string cFile: model.file !== undefined ? model.file : ""
            property string cTag: model.tag !== undefined ? model.tag : "USER"
            property color cColor: model.color !== undefined ? model.color : "#888888"
            property string cIcon: model.icon !== undefined ? model.icon : "help"

            property bool isActive: shaderRoot.activeShaderFile === cFile

            color: isActive ? Qt.alpha(cColor, 0.12) : (hoverArea.containsMouse ? Qt.alpha(cColor, 0.05) : "transparent")
            border.width: isActive ? 2 : 1
            border.color: isActive ? cColor : (hoverArea.containsMouse ? Qt.alpha(cColor, 0.4) : Color.mOutline)

            Behavior on color { ColorAnimation { duration: 150 } }
            Behavior on border.color { ColorAnimation { duration: 150 } }

            MouseArea {
                id: hoverArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                onClicked: {
                    var wasActive = isActive
                    var scriptArg = wasActive ? "none" : cardRoot.cFile
                    var settingArg = wasActive ? "" : cardRoot.cFile

                    if (shaderRoot.runScript) {
                        shaderRoot.runScript("shader.sh", scriptArg)
                    }
                    
                    // Native state save
                    if (shaderRoot.pluginApi) {
                        shaderRoot.pluginApi.pluginSettings.activeShaderFile = settingArg
                        shaderRoot.pluginApi.saveSettings()
                        shaderRoot.activeShaderFile = settingArg
                    }
                }
            }

            RowLayout {
                anchors.fill: parent; anchors.margins: Style.marginM; spacing: Style.marginM
                NIcon {
                    icon: cardRoot.cIcon
                    color: (cardRoot.isActive || hoverArea.containsMouse) ? cardRoot.cColor : Color.mOnSurfaceVariant
                    pointSize: Style.fontSizeL
                }
                ColumnLayout {
                    Layout.fillWidth: true; spacing: 2
                    RowLayout {
                        spacing: 8
                        NText {
                            // Direct translation call
                            text: cardRoot.cTitleKey !== "" ? pluginApi.tr(cardRoot.cTitleKey) : ""
                            font.weight: Font.Bold
                            color: cardRoot.isActive ? Color.mOnSurface : Color.mOnSurfaceVariant
                        }
                        Rectangle {
                            width: tagT.implicitWidth + 10; height: 16; radius: 4; color: Qt.alpha(cardRoot.cColor, 0.15)
                            NText { id: tagT; text: cardRoot.cTag; pointSize: 7; color: cardRoot.cColor; anchors.centerIn: parent; font.weight: Font.Bold }
                        }
                    }
                    NText {
                        // Direct translation call
                        text: cardRoot.cDescKey !== "" ? pluginApi.tr(cardRoot.cDescKey) : ""
                        pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant; elide: Text.ElideRight; Layout.fillWidth: true
                    }
                }
                NToggle {
                    checked: cardRoot.isActive
                    // Logic moved entirely to MouseArea
                }
            }
        }
    }

    ListModel { id: shaderModel }

    ColumnLayout {
        id: mainLayout
        width: shaderRoot.availableWidth
        spacing: Style.marginS
        Layout.margins: Style.marginM

        ColumnLayout {
            Layout.fillWidth: true; spacing: 4; Layout.margins: Style.marginL
            NText {
                text: pluginApi.tr("shaders.header_title")
                font.weight: Font.Bold; pointSize: Style.fontSizeL; color: Color.mPrimary
            }
            NText {
                text: pluginApi.tr("shaders.header_subtitle")
                pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant
            }
        }

        NDivider { Layout.fillWidth: true; opacity: 0.5 }

        Repeater {
            model: shaderModel
            delegate: shaderDelegate
        }
    }
}