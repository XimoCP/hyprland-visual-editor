import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons
import qs.Services.UI // Para ToastService

NScrollView {
    id: borderRoot

    property var pluginApi: null
    property var runHypr: null
    property var runScript: null

    // Base plugin directory using Quickshell Settings
    readonly property string pluginDir: Settings.configDir + "/plugins/hyprland-visual-editor"

    // --- OFFICIAL PERSISTENCE BOUND PROPERTIES ---
    property string activeBorderFile: pluginApi?.pluginSettings?.activeBorderFile || ""
    property int borderSize: pluginApi?.pluginSettings?.borderSize || 2

    Layout.fillWidth: true
    Layout.fillHeight: true
    contentHeight: mainLayout.implicitHeight + 50
    clip: true

    // --- SCANNER ---
    Process {
        id: scanner
        command: ["bash", pluginDir + "/assets/scripts/scan.sh", "borders"]
        property string outputData: ""
        stdout: SplitParser { onRead: function(data) { scanner.outputData += data; } }
        onExited: (code) => {
            if (code === 0) {
                try {
                    var data = JSON.parse(scanner.outputData);
                    borderModel.clear();
                    for (var i = 0; i < data.length; i++) { borderModel.append(data[i]); }
                } catch (e) { 
                    Logger.e("HVE", "JSON Parsing Error in Borders: " + e); 
                }
            }
        }
    }
    Component.onCompleted: scanner.running = true

    // --- DELEGATE ---
    Component {
        id: borderDelegate
        NBox {
            id: cardRoot
            Layout.fillWidth: true
            Layout.preferredHeight: 85 * Style.uiScaleRatio
            radius: Style.radiusM

            // Safe property bindings for ListModel data
            property string cTitleKey: model.title !== undefined ? model.title : ""
            property string cDescKey: model.desc !== undefined ? model.desc : ""
            property string cFile: model.file !== undefined ? model.file : ""
            property string cIcon: model.icon !== undefined ? model.icon : "help"
            property color cColor: model.color !== undefined ? model.color : "#888888"
            property string cTag: model.tag !== undefined ? model.tag : "USER"

            property bool isActive: borderRoot.activeBorderFile === cFile

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

                    // Script execution
                    if (borderRoot.runScript) {
                        borderRoot.runScript("border.sh", scriptArg)
                    }
                    
                    // Native state save
                    if (borderRoot.pluginApi) {
                        borderRoot.pluginApi.pluginSettings.activeBorderFile = settingArg
                        borderRoot.pluginApi.saveSettings()
                        
                        // Force UI update
                        borderRoot.activeBorderFile = settingArg
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
                            // Official safe translation call with fallback
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
                        pointSize: Style.fontSizeS
                        color: Color.mOnSurfaceVariant
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
                Item {
                    width: 40 * Style.uiScaleRatio; height: 20 * Style.uiScaleRatio
                    Rectangle {
                        anchors.fill: parent; radius: height / 2
                        color: cardRoot.isActive ? Color.mPrimary : "transparent"
                        border.color: cardRoot.isActive ? Color.mPrimary : Color.mOutline; border.width: 1
                        Rectangle {
                            width: parent.height - 6; height: width; radius: width / 2
                            color: cardRoot.isActive ? Color.mOnPrimary : Color.mOnSurfaceVariant
                            anchors.verticalCenter: parent.verticalCenter
                            x: cardRoot.isActive ? (parent.width - width - 3) : 3
                            Behavior on x { NumberAnimation { duration: 200 } }
                        }
                    }
                }
            }
        }
    }

    ListModel { id: borderModel }

    ColumnLayout {
        id: mainLayout
        width: borderRoot.availableWidth
        spacing: Style.marginS
        Layout.margins: Style.marginM

        ColumnLayout {
            Layout.fillWidth: true; spacing: 4; Layout.margins: Style.marginL
            NText {
                text: pluginApi?.tr("borders.header_title") || "Visual Styles"
                font.weight: Font.Bold; pointSize: Style.fontSizeL; color: Color.mPrimary
            }
            NText {
                text: pluginApi?.tr("borders.header_subtitle") || "Define the personality of your windows"
                pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant
            }
        }

        NDivider { Layout.fillWidth: true; opacity: 0.5 }

        // GEOMETRY SLIDER
        NBox {
            Layout.fillWidth: true
            implicitHeight: geoCol.implicitHeight + (Style.marginL * 2)
            color: Qt.alpha(Color.mSurface, 0.4)
            radius: Style.radiusM
            border.color: Color.mOutline; border.width: 1

            ColumnLayout {
                id: geoCol
                anchors.fill: parent; anchors.margins: Style.marginL; spacing: Style.marginM
                RowLayout {
                    spacing: Style.marginS
                    NIcon { icon: "maximize"; color: Color.mPrimary; pointSize: Style.fontSizeM }
                    NText {
                        text: pluginApi?.tr("borders.geometry.title") || "Border Thickness"
                        font.weight: Font.Bold; color: Color.mOnSurface
                    }
                    Item { Layout.fillWidth: true }
                    NText { text: thicknessSlider.value + "px"; color: Color.mPrimary; font.family: Style.fontMono; font.weight: Font.Bold }
                }
                NSlider {
                    id: thicknessSlider
                    Layout.fillWidth: true
                    from: 1; to: 5; stepSize: 1
                    value: borderRoot.borderSize
                    onMoved: {
                        // Native state save for slider
                        if (borderRoot.pluginApi) {
                            borderRoot.pluginApi.pluginSettings.borderSize = value
                            borderRoot.pluginApi.saveSettings()
                            borderRoot.borderSize = value
                        }
                        
                        if (borderRoot.runScript) {
                            borderRoot.runScript("geometry.sh", value.toString())
                        }
                    }
                }
            }
        }

        NDivider { Layout.fillWidth: true; Layout.topMargin: Style.marginM; Layout.bottomMargin: Style.marginS; opacity: 0.3 }

        Repeater {
            model: borderModel
            delegate: borderDelegate
        }
    }
}