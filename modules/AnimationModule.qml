import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons
import qs.Services.UI // Para ToastService

NScrollView {
    id: animRoot

    property var pluginApi: null
    property var runHypr: null
    property var runScript: null

    // Centralized plugin directory path
    readonly property string pluginDir: Settings.configDir + "/plugins/hyprland-visual-editor"

    // --- OFFICIAL PERSISTENCE BOUND PROPERTIES ---
    property string activeAnimFile: pluginApi?.pluginSettings?.activeAnimFile || ""

    Layout.fillWidth: true
    Layout.fillHeight: true
    contentHeight: mainLayout.implicitHeight + 50
    clip: true

    // --- SCANNER ---
    Process {
        id: scanner
        command: ["bash", pluginDir + "/assets/scripts/scan.sh", "animations"]
        property string outputData: ""
        stdout: SplitParser { onRead: function(data) { scanner.outputData += data; } }
        onExited: (code) => {
            if (code === 0) {
                try {
                    var data = JSON.parse(scanner.outputData);
                    animModel.clear();
                    for (var i = 0; i < data.length; i++) { animModel.append(data[i]); }
                } catch (e) { 
                    Logger.e("HVE", "JSON Parsing Error in Animations: " + e); 
                }
            }
        }
    }
    Component.onCompleted: scanner.running = true

    // --- DELEGATE ---
    Component {
        id: animDelegate
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

            property bool isActive: animRoot.activeAnimFile === cFile

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

                    if (animRoot.runScript) {
                        animRoot.runScript("apply_animation.sh", scriptArg)
                    }
                    
                    // Native state save
                    if (animRoot.pluginApi) {
                        animRoot.pluginApi.pluginSettings.activeAnimFile = settingArg
                        animRoot.pluginApi.saveSettings()
                        animRoot.activeAnimFile = settingArg
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
                        // Official safe translation call with fallback
                        NText {
                            text: cardRoot.cTitleKey !== "" ? (pluginApi?.tr(cardRoot.cTitleKey) || cardRoot.cTitleKey) : ""
                            font.weight: Font.Bold
                            color: cardRoot.isActive ? Color.mOnSurface : Color.mOnSurfaceVariant
                        }
                        Rectangle {
                            width: tagT.implicitWidth + 10; height: 16; radius: 4; color: Qt.alpha(cardRoot.cColor, 0.15)
                            NText { id: tagT; text: cardRoot.cTag; pointSize: 7; color: cardRoot.cColor; anchors.centerIn: parent; font.weight: Font.Bold }
                        }
                    }
                    NText {
                        // Official safe translation call with fallback
                        text: cardRoot.cDescKey !== "" ? (pluginApi?.tr(cardRoot.cDescKey) || cardRoot.cDescKey) : ""
                        pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant; elide: Text.ElideRight; Layout.fillWidth: true
                    }
                }
                VisualSwitch {
                    checked: cardRoot.isActive
                    // Logic moved entirely to MouseArea to avoid infinite loops and maintain single source of truth
                }
            }
        }
    }

    ListModel { id: animModel }

    ColumnLayout {
        id: mainLayout
        width: animRoot.availableWidth
        spacing: Style.marginS
        Layout.margins: Style.marginM

        ColumnLayout {
            Layout.fillWidth: true; spacing: 4; Layout.margins: Style.marginL
            NText {
                text: pluginApi?.tr("animations.header_title") || "Animations"
                font.weight: Font.Bold; pointSize: Style.fontSizeL; color: Color.mPrimary
            }
            NText {
                text: pluginApi?.tr("animations.header_subtitle") || "Motion dynamics for your desktop"
                pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant
            }
        }

        NDivider { Layout.fillWidth: true; opacity: 0.5 }

        Repeater {
            model: animModel
            delegate: animDelegate
        }
    }

    component VisualSwitch : Item {
        id: sw; property bool checked: false; signal toggled()
        width: 40 * Style.uiScaleRatio; height: 20 * Style.uiScaleRatio
        Rectangle {
            anchors.fill: parent; radius: height / 2
            color: sw.checked ? Color.mPrimary : "transparent"
            border.color: sw.checked ? Color.mPrimary : Color.mOutline; border.width: 1
            Rectangle {
                width: parent.height - 6; height: width; radius: width / 2
                color: sw.checked ? Color.mOnPrimary : Color.mOnSurfaceVariant
                anchors.verticalCenter: parent.verticalCenter
                x: sw.checked ? (parent.width - width - 3) : 3
                Behavior on x { NumberAnimation { duration: 200 } }
            }
        }
    }
}