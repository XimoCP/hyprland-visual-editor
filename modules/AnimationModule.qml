import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Qt.labs.settings 1.0 as LabSettings
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons

NScrollView {
    id: animRoot

    property var pluginApi: null
    property var runHypr: null
    property var runScript: null

    Layout.fillWidth: true
    Layout.fillHeight: true
    contentHeight: mainLayout.implicitHeight + 50
    clip: true

    // --- LÓGICA DE TRADUCCIÓN HÍBRIDA (ANTI-EXCLAMACIONES) ---
    function tr(key, fallback) {
        if (pluginApi && pluginApi.tr) {
            var t = pluginApi.tr(key);
            if (t !== key && t !== "" && t.indexOf("!!") === -1) {
                return t;
            }
        }
        return fallback || key;
    }

    // --- PERSISTENCIA ---
    LabSettings.Settings {
        id: animSettings
        fileName: Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/assets/animations/store.conf"
        property string activeAnimFile: ""
    }

    // --- ESCÁNER ---
    Process {
        id: scanner
        command: ["bash", Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/assets/scripts/scan.sh", "animations"]
        property string outputData: ""
        stdout: SplitParser { onRead: function(data) { scanner.outputData += data; } }
        onExited: (code) => {
            if (code === 0) {
                try {
                    var data = JSON.parse(scanner.outputData);
                    animModel.clear();
                    for (var i = 0; i < data.length; i++) { animModel.append(data[i]); }
                } catch (e) { console.error("JSON Error: " + e); }
            }
        }
    }
    Component.onCompleted: scanner.running = true

    // --- DELEGADO ---
    Component {
        id: animDelegate
        NBox {
            id: cardRoot
            Layout.fillWidth: true
            Layout.preferredHeight: 85 * Style.uiScaleRatio
            radius: Style.radiusM

            // 1. MAPEO DE PROPIEDADES
            property string cTitleKey: model.title || ""
            property string cDescKey: model.desc || ""
            property string cRawTitle: model.rawTitle || ""
            property string cRawDesc: model.rawDesc || ""

            property string cFile: model.file || ""
            property string cTag: model.tag || "USER"
            property color cColor: model.color || "#888888"
            property string cIcon: model.icon || "help"

            property bool isActive: animSettings.activeAnimFile === cFile

            color: isActive ? Qt.alpha(cColor, 0.12) : (hoverArea.containsMouse ? Qt.alpha(cColor, 0.05) : "transparent")
            border.width: isActive ? 2 : 1
            border.color: isActive ? cColor : (hoverArea.containsMouse ? Qt.alpha(cColor, 0.4) : Color.mOutline)

            Behavior on color { ColorAnimation { duration: 150 } }
            Behavior on border.color { ColorAnimation { duration: 150 } }

            MouseArea {
                id: hoverArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                onClicked: {
                    // 1. CAPTURAR EL ESTADO ACTUAL
                    var wasActive = isActive

                    // 2. CALCULAR LA INTENCIÓN
                    var scriptArg = wasActive ? "none" : cardRoot.cFile
                    var settingArg = wasActive ? "" : cardRoot.cFile

                    // 3. EJECUTAR EL SCRIPT PRIMERO
                    if (animRoot.runScript) animRoot.runScript("apply_animation.sh", scriptArg)

                        // 4. ACTUALIZAR LA UI AL FINAL
                        animSettings.activeAnimFile = settingArg
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
                        // 2. USO DE TRADUCCIÓN SEGURA
                        NText {
                            text: animRoot.tr(cardRoot.cTitleKey, cardRoot.cRawTitle)
                            font.weight: Font.Bold
                            color: cardRoot.isActive ? Color.mOnSurface : Color.mOnSurfaceVariant
                        }
                        Rectangle {
                            width: tagT.implicitWidth + 10; height: 16; radius: 4; color: Qt.alpha(cardRoot.cColor, 0.15)
                            NText { id: tagT; text: cardRoot.cTag; pointSize: 7; color: cardRoot.cColor; anchors.centerIn: parent; font.weight: Font.Bold }
                        }
                    }
                    NText {
                        text: animRoot.tr(cardRoot.cDescKey, cardRoot.cRawDesc)
                        pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant; elide: Text.ElideRight; Layout.fillWidth: true
                    }
                }
                // Switch visual para consistencia
                VisualSwitch {
                    checked: cardRoot.isActive
                    onToggled: hoverArea.clicked(null)
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
            // CABECERAS TRADUCIBLES
            NText {
                text: animRoot.tr("animations.header_title", "Biblioteca de Movimiento")
                font.weight: Font.Bold; pointSize: Style.fontSizeL; color: Color.mPrimary
            }
            NText {
                text: animRoot.tr("animations.header_subtitle", "Selecciona el estilo de animación")
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
