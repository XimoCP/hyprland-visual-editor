import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Qt.labs.settings 1.0 as LabSettings
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons

NScrollView {
    id: borderRoot

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
            var translated = pluginApi.tr(key);

            // Verificamos DOS cosas:
            // 1. Que no sea igual a la clave
            // 2. Que NO contenga "!!" (que es como Noctalia marca los errores)
            if (translated !== key && translated.indexOf("!!") === -1) {
                return translated;
            }
        }
        // Si falla la traducción o devuelve error (!!), usamos el texto original del archivo
        return fallback || key;
    }
    // --- PERSISTENCIA ---
    LabSettings.Settings {
        id: borderSettings
        fileName: Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/assets/borders/store.conf"
        property string activeBorderFile: ""
    }
    LabSettings.Settings {
        id: geomSettings
        category: "VisualLayer_Geometry"
        fileName: Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/assets/borders/store.conf"
        property int borderSize: 2
    }

    // --- ESCÁNER ---
    Process {
        id: scanner
        command: ["bash", Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/assets/scripts/scan.sh", "borders"]
        property string outputData: ""
        stdout: SplitParser { onRead: function(data) { scanner.outputData += data; } }
        onExited: (code) => {
            if (code === 0) {
                try {
                    var data = JSON.parse(scanner.outputData);
                    borderModel.clear();
                    for (var i = 0; i < data.length; i++) { borderModel.append(data[i]); }
                } catch (e) { console.error("JSON Error: " + e); }
            }
        }
    }
    Component.onCompleted: scanner.running = true

    // --- DELEGADO ---
    Component {
        id: borderDelegate
        NBox {
            id: cardRoot
            Layout.fillWidth: true
            Layout.preferredHeight: 85 * Style.uiScaleRatio
            radius: Style.radiusM

            // 1. MAPEO DE PROPIEDADES (Ahora leemos rawTitle y rawDesc del JSON)
            property string cTitleKey: model.title || ""
            property string cDescKey: model.desc || ""
            property string cRawTitle: model.rawTitle || ""
            property string cRawDesc: model.rawDesc || ""

            property string cFile: model.file || ""
            property string cIcon: model.icon || "help" // Evita exclamaciones si falta icono
            property color cColor: model.color || "#888888" // Evita exclamaciones si falta color
            property string cTag: model.tag || "USER"

            property bool isActive: borderSettings.activeBorderFile === cFile

            color: isActive ? Qt.alpha(cColor, 0.12) : (hoverArea.containsMouse ? Qt.alpha(cColor, 0.05) : "transparent")
            border.width: isActive ? 2 : 1
            border.color: isActive ? cColor : (hoverArea.containsMouse ? Qt.alpha(cColor, 0.4) : Color.mOutline)

            Behavior on color { ColorAnimation { duration: 150 } }
            Behavior on border.color { ColorAnimation { duration: 150 } }

            MouseArea {
                id: hoverArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                onClicked: {
                    // 1. CAPTURAR EL ESTADO ACTUAL (Antes de que cambie)
                    var wasActive = isActive

                    // 2. CALCULAR LA INTENCIÓN
                    var scriptArg = wasActive ? "none" : cardRoot.cFile
                    var settingArg = wasActive ? "" : cardRoot.cFile

                    // 3. EJECUTAR EL SCRIPT PRIMERO
                    if (borderRoot.runScript) borderRoot.runScript("border.sh", scriptArg)

                        // 4. ACTUALIZAR LA UI AL FINAL
                        borderSettings.activeBorderFile = settingArg
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
                        // 2. USO DE LA TRADUCCIÓN HÍBRIDA
                        NText {
                            // "Intenta traducir la Key, si no puedes, dame el RawTitle"
                            text: borderRoot.tr(cardRoot.cTitleKey, cardRoot.cRawTitle)
                            font.weight: Font.Bold
                            color: cardRoot.isActive ? Color.mOnSurface : Color.mOnSurfaceVariant
                        }
                        Rectangle {
                            width: tagT.implicitWidth + 10; height: 16; radius: 4; color: Qt.alpha(cardRoot.cColor, 0.15)
                            NText { id: tagT; text: cardRoot.cTag; pointSize: 7; color: cardRoot.cColor; anchors.centerIn: parent; font.weight: Font.Bold }
                        }
                    }
                    NText {
                        // "Intenta traducir la Key, si no puedes, dame el RawDesc"
                        text: borderRoot.tr(cardRoot.cDescKey, cardRoot.cRawDesc)
                        pointSize: Style.fontSizeS
                        color: Color.mOnSurfaceVariant
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
                // Switch visual (opcional, para coherencia)
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

        // CABECERA (También usa la función tr segura)
        ColumnLayout {
            Layout.fillWidth: true; spacing: 4; Layout.margins: Style.marginL
            NText {
                text: borderRoot.tr("borders.header_title", "Estilos de Borde")
                font.weight: Font.Bold; pointSize: Style.fontSizeL; color: Color.mPrimary
            }
            NText {
                text: borderRoot.tr("borders.header_subtitle", "Personaliza los colores y degradados de las ventanas")
                pointSize: Style.fontSizeS; color: Color.mOnSurfaceVariant
            }
        }

        NDivider { Layout.fillWidth: true; opacity: 0.5 }

        // SLIDER DE GEOMETRÍA
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
                        text: borderRoot.tr("borders.geometry.title", "Grosor del Borde")
                        font.weight: Font.Bold; color: Color.mOnSurface
                    }
                    Item { Layout.fillWidth: true }
                    NText { text: thicknessSlider.value + "px"; color: Color.mPrimary; font.family: Style.fontMono; font.weight: Font.Bold }
                }
                NSlider {
                    id: thicknessSlider
                    Layout.fillWidth: true
                    from: 1; to: 5; stepSize: 1
                    value: geomSettings.borderSize
                    onMoved: {
                        geomSettings.borderSize = value
                        if (borderRoot.runScript) borderRoot.runScript("geometry.sh", value.toString())
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
