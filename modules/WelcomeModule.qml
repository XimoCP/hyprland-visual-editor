import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Qt.labs.settings 1.0 as LabSettings
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons

NScrollView {
    id: welcomeRoot

    // CAMBIO: Renombrado a pluginApi
    property var pluginApi: null
    property var runHypr: null
    property var runScript: null

    Layout.fillWidth: true
    Layout.fillHeight: true
    contentHeight: mainLayout.implicitHeight + 100
    clip: true

    // Función helper segura para traducir (si pluginApi es null, devuelve key)
    function tr(key, defaultText) {
        if (welcomeRoot.pluginApi && welcomeRoot.pluginApi.tr) {
            return welcomeRoot.pluginApi.tr(key);
        }
        return defaultText || key;
    }

    // --- PERSISTENCIA ---
    LabSettings.Settings {
        id: welcomeSettings
        fileName: Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/assets/welcome.conf"
        property bool isSystemActive: false
    }

    ColumnLayout {
        id: mainLayout
        width: welcomeRoot.availableWidth
        spacing: Style.marginXL
        Layout.margins: Style.marginL

        // --- CABECERA ---
        ColumnLayout {
            Layout.fillWidth: true
            Layout.topMargin: Style.marginXL
            Layout.bottomMargin: Style.marginM
            Layout.alignment: Qt.AlignHCenter

            Image {
                source: "../assets/owl_neon.png"
                fillMode: Image.PreserveAspectFit
                Layout.preferredHeight: 400 * Style.uiScaleRatio
                Layout.preferredWidth: 600 * Style.uiScaleRatio
                Layout.alignment: Qt.AlignHCenter
                smooth: true
            }
        }

        NDivider { Layout.fillWidth: true }

        // --- ACTIVACIÓN ---
        ProCard {
            title: tr("welcome.activation_title", "Activación del Sistema")
            iconName: "power"
            accentColor: welcomeSettings.isSystemActive ? Color.mPrimary : "#ef4444"
            description: welcomeSettings.isSystemActive
            ? tr("welcome.system_active", "Sistema operativo.")
            : tr("welcome.system_inactive", "Sistema detenido.")

            extraContent: ColumnLayout {
                spacing: Style.marginM
                Layout.fillWidth: true

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 15
                    NText {
                        text: tr("welcome.enable_label", "Habilitar Visual Layer")
                        font.weight: Font.Bold
                        pointSize: Style.fontSizeL
                        color: Color.mOnSurface
                    }
                    Item { Layout.fillWidth: true }
                    VisualSwitch {
                        checked: welcomeSettings.isSystemActive
                        onToggled: {
                            welcomeSettings.isSystemActive = checked
                            if (welcomeRoot.runScript) {
                                welcomeRoot.runScript("init.sh", checked ? "enable" : "disable")
                            }
                        }
                    }
                }

                Rectangle {
                    visible: !welcomeSettings.isSystemActive
                    Layout.fillWidth: true
                    implicitHeight: warnCol.implicitHeight + 24
                    color: Qt.alpha("#ef4444", 0.08)
                    radius: Style.radiusM
                    border.color: Qt.alpha("#ef4444", 0.3)
                    border.width: 1
                    RowLayout {
                        id: warnCol
                        anchors.fill: parent; anchors.margins: 12; spacing: 12
                        NIcon { icon: "alert-circle"; color: "#ef4444"; pointSize: 20; Layout.alignment: Qt.AlignTop }
                        ColumnLayout {
                            Layout.fillWidth: true; spacing: 4
                            NText {
                                text: tr("welcome.warning.title", "CONTRATO IMPLÍCITO")
                                font.weight: Font.Bold; color: "#ef4444"; pointSize: Style.fontSizeS
                            }
                            NText {
                                text: tr("welcome.warning.text", "Se modificará hyprland.conf")
                                color: Color.mOnSurfaceVariant; wrapMode: Text.WordWrap; textFormat: Text.RichText; Layout.fillWidth: true; pointSize: Style.fontSizeS
                            }
                        }
                    }
                }
            }
        }

        // --- CARACTERÍSTICAS ---
        ProCard {
            title: tr("welcome.features.title", "Características")
            iconName: "star"; accentColor: "#fbbf24"
            description: tr("welcome.features.description", "La evolución estética.")
            extraContent: ColumnLayout {
                spacing: 6
                Repeater {
                    // Usamos las claves del JSON para la lista
                    model: [
                        tr("welcome.features.list.fluid_anim", "Animaciones"),
                        tr("welcome.features.list.smart_borders", "Bordes"),
                        tr("welcome.features.list.realtime_shaders", "Shaders"),
                        tr("welcome.features.list.non_destructive", "No Destructivo")
                    ]
                    delegate: RowLayout {
                        spacing: 8
                        NIcon { icon: "check"; color: Color.mPrimary; pointSize: 12 }
                        NText { text: modelData; color: Color.mOnSurfaceVariant; pointSize: 10; textFormat: Text.RichText }
                    }
                }
            }
        }
        // --- DOCUMENTACIÓN TÉCNICA ---
        ProCard {
            title: tr("welcome.docs.title", "Arquitectura y Documentación")
            iconName: "book"; accentColor: "#38bdf8"
            description: tr("welcome.docs.description", "Descubre cómo funciona NVL por debajo.")

            extraContent: ColumnLayout {
                spacing: 15

                // Resumen Técnico Elegante
                NText {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: "#a9b1d6"
                    font.pointSize: 10
                    textFormat: Text.RichText
                    text: tr("welcome.docs.summary", "<b>Noctalia Visual Layer</b> utiliza un sistema de <i>Fragmentos y Ensamblaje</i> en tiempo real. Nunca toca tu configuración principal. Todo se genera de forma segura en un archivo maestro <code>overlay.conf</code> aislado.")
                }

                // Fila de Botones de Acción
                RowLayout {
                    spacing: 10
                    Layout.fillWidth: true

                    NButton {
                        text: tr("welcome.docs.btn_readme", "Leer Manual Completo")
                        icon: "external-link"
                        Layout.fillWidth: true
                        onClicked: {
                            // Abre el LEEME.md (o README.md) con la aplicación por defecto del sistema
                            Qt.openUrlExternally("file://" + Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/LEEME.md")
                        }
                    }

                    NButton {
                        text: tr("welcome.docs.btn_folder", "Explorar Archivos")
                        icon: "folder"
                        Layout.fillWidth: true
                        onClicked: {
                            // Abre el gestor de archivos directamente en la carpeta del plugin
                            Qt.openUrlExternally("file://" + Quickshell.env("HOME") + "/.config/noctalia/plugins/noctalia-visual-layer/")
                        }
                    }
                }
            }
        }

        // --- CRÉDITOS ---
        ProCard {
            title: tr("welcome.credits.title", "Créditos")
            iconName: "heart"; accentColor: "#f472b6"
            description: tr("welcome.credits.description", "Gracias a HyDE.")

            extraContent: ColumnLayout {
                spacing: Style.marginM
                NButton {
                    text: tr("welcome.credits.btn_hyde", "Inspirado en HyDE")
                    icon: "brand-github"; Layout.fillWidth: true
                    onClicked: Qt.openUrlExternally("https://github.com/HyDE-Project/")
                }
                NDivider { Layout.fillWidth: true }
                RowLayout {
                    spacing: Style.marginM
                    NIcon { icon: "code"; color: Color.mOnSurfaceVariant; pointSize: Style.fontSizeL }
                    ColumnLayout {
                        spacing: 2
                        NText { text: tr("welcome.credits.ai_title", "IA"); font.weight: Font.Bold }
                        NText {
                            text: tr("welcome.credits.ai_desc", "Gracias a Gemini.");
                            color: Color.mOnSurfaceVariant; wrapMode: Text.Wrap; Layout.fillWidth: true; pointSize: Style.fontSizeS
                        }
                    }
                }
            }
        }
        Item { Layout.preferredHeight: 50 }
    }

    // --- COMPONENTES AUXILIARES ---
    component ProCard : NBox {
        id: cardRoot
        property string title; property string iconName; property string description
        property color accentColor; property Component extraContent: null
        Layout.fillWidth: true; Layout.leftMargin: Style.marginL; Layout.rightMargin: Style.marginL
        implicitHeight: cardCol.implicitHeight + (Style.marginL * 2)
        radius: Style.radiusM
        border.color: Qt.alpha(accentColor, 0.3); border.width: 1
        color: Qt.alpha(accentColor, 0.03)

        ColumnLayout {
            id: cardCol; anchors.fill: parent; anchors.margins: Style.marginL; spacing: Style.marginM
            RowLayout {
                spacing: Style.marginM
                NIcon { icon: iconName; color: accentColor; pointSize: Style.fontSizeL }
                NText { text: cardRoot.title; font.weight: Font.Bold; pointSize: Style.fontSizeL }
            }
            NDivider { Layout.fillWidth: true; opacity: 0.2 }
            NText { text: cardRoot.description; color: Color.mOnSurface; wrapMode: Text.WordWrap; Layout.fillWidth: true; textFormat: Text.RichText }
            Loader { active: extraContent !== null; sourceComponent: extraContent; Layout.fillWidth: true }
        }
    }

    component VisualSwitch : Item {
        id: sw; property bool checked: false; signal toggled()
        width: 46 * Style.uiScaleRatio; height: 24 * Style.uiScaleRatio
        Rectangle {
            anchors.fill: parent; radius: height / 2
            color: sw.checked ? Color.mPrimary : Color.mSurface
            border.color: sw.checked ? Color.mPrimary : Color.mOutline; border.width: 1
            Rectangle {
                width: parent.height - 8; height: width; radius: width / 2
                color: sw.checked ? Color.mOnPrimary : Color.mOnSurfaceVariant
                anchors.verticalCenter: parent.verticalCenter
                x: sw.checked ? (parent.width - width - 4) : 4
                Behavior on x { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
            }
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: { sw.checked = !sw.checked; sw.toggled() } }
    }
}
