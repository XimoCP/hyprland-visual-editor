import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Widgets
import qs.Commons
import qs.Services.UI // Added for ToastService

NScrollView {
    id: welcomeRoot

    property var pluginApi: null
    property var runHypr: null
    property var runScript: null

    // Official and clean route for Noctalia
    readonly property string pluginDir: Settings.configDir + "/plugins/hyprland-visual-editor"

    // Property bound directly to the official pluginSettings
    property bool isSystemActive: pluginApi?.pluginSettings?.isSystemActive || false

    Layout.fillWidth: true
    Layout.fillHeight: true
    contentHeight: mainLayout.implicitHeight + 100
    clip: true

    ColumnLayout {
        id: mainLayout
        width: welcomeRoot.availableWidth
        spacing: Style.marginXL
        Layout.margins: Style.marginL

        // --- HEADER ---
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

        // --- ACTIVATION SECTION ---
        ProCard {
            title: pluginApi?.tr("welcome.activation_title") || "System Activation"
            iconName: "power"
            accentColor: welcomeRoot.isSystemActive ? Color.mPrimary : "#ef4444"
            description: welcomeRoot.isSystemActive
                ? (pluginApi?.tr("welcome.system_active") || "System Active")
                : (pluginApi?.tr("welcome.system_inactive") || "System Inactive")

            extraContent: ColumnLayout {
                spacing: Style.marginM
                Layout.fillWidth: true

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 15
                    NText {
                        text: pluginApi?.tr("welcome.enable_label") || "Enable Visual Editor"
                        font.weight: Font.Bold
                        pointSize: Style.fontSizeL
                        color: Color.mOnSurface
                    }
                    Item { Layout.fillWidth: true }
                    
                    // ACTIVATION BUTTON
                    VisualSwitch {
                        checked: welcomeRoot.isSystemActive
                        onToggled: {
                            var newState = !welcomeRoot.isSystemActive
                            welcomeRoot.isSystemActive = newState
                            
                            // Official native save
                            if (welcomeRoot.pluginApi) {
                                welcomeRoot.pluginApi.pluginSettings.isSystemActive = newState
                                welcomeRoot.pluginApi.saveSettings()
                                
                                var statusMsg = newState ? "Visual Editor Enabled" : "Visual Editor Disabled"
                                ToastService.showNotice(statusMsg)
                            }

                            if (welcomeRoot.runScript) {
                                welcomeRoot.runScript("init.sh", newState ? "enable" : "disable")
                            }
                        }
                    }
                }

                Rectangle {
                    visible: !welcomeRoot.isSystemActive
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
                                text: pluginApi?.tr("welcome.warning.title") || "WARNING"
                                font.weight: Font.Bold; color: "#ef4444"; pointSize: Style.fontSizeS
                            }
                            NText {
                                text: pluginApi?.tr("welcome.warning.text") || "Safety Notice"
                                color: Color.mOnSurfaceVariant; wrapMode: Text.WordWrap; textFormat: Text.RichText; Layout.fillWidth: true; pointSize: Style.fontSizeS
                            }
                        }
                    }
                }
            }
        }

        // --- FEATURES ---
        ProCard {
            title: pluginApi?.tr("welcome.features.title") || "Features & Benefits"
            iconName: "star"; accentColor: "#fbbf24"
            description: pluginApi?.tr("welcome.features.description") || "Evolution of your desktop"
            extraContent: ColumnLayout {
                spacing: 6
                Repeater {
                    model: [
                        pluginApi?.tr("welcome.features.list.fluid_anim") || "Fluid Animations",
                        pluginApi?.tr("welcome.features.list.smart_borders") || "Smart Borders",
                        pluginApi?.tr("welcome.features.list.realtime_shaders") || "Real-Time Shaders",
                        pluginApi?.tr("welcome.features.list.non_destructive") || "Non-Destructive"
                    ]
                    delegate: RowLayout {
                        spacing: 8
                        NIcon { icon: "check"; color: Color.mPrimary; pointSize: 12 }
                        NText { text: modelData; color: Color.mOnSurfaceVariant; pointSize: 10; textFormat: Text.RichText }
                    }
                }
            }
        }
        
        // --- TECHNICAL DOCUMENTATION ---
        ProCard {
            title: pluginApi?.tr("welcome.docs.title") || "Architecture & Documentation"
            iconName: "book"; accentColor: "#38bdf8"
            description: pluginApi?.tr("welcome.docs.description") || "How HVE works"

            extraContent: ColumnLayout {
                spacing: 15

                NText {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: "#a9b1d6"
                    font.pointSize: 10
                    textFormat: Text.RichText
                    text: pluginApi?.tr("welcome.docs.summary") || "Uses fragments system"
                }

                RowLayout {
                    spacing: 10
                    Layout.fillWidth: true

                    NButton {
                        text: pluginApi?.tr("welcome.docs.btn_readme") || "Read Manual"
                        icon: "external-link"
                        Layout.fillWidth: true
                        onClicked: Qt.openUrlExternally("file://" + pluginDir + "/README.md")
                    }

                    NButton {
                        text: pluginApi?.tr("welcome.docs.btn_folder") || "Browse Files"
                        icon: "folder"
                        Layout.fillWidth: true
                        onClicked: Qt.openUrlExternally("file://" + pluginDir + "/")
                    }
                }
            }
        }

        // --- CREDITS ---
        ProCard {
            title: pluginApi?.tr("welcome.credits.title") || "Credits"
            iconName: "heart"; accentColor: "#f472b6"
            description: pluginApi?.tr("welcome.credits.description") || "Thanks to HyDE"

            extraContent: ColumnLayout {
                spacing: Style.marginM
                NButton {
                    text: pluginApi?.tr("welcome.credits.btn_hyde") || "Inspired by HyDE"
                    icon: "brand-github"; Layout.fillWidth: true
                    onClicked: Qt.openUrlExternally("https://github.com/HyDE-Project/")
                }
                NDivider { Layout.fillWidth: true }
                RowLayout {
                    spacing: Style.marginM
                    NIcon { icon: "code"; color: Color.mOnSurfaceVariant; pointSize: Style.fontSizeL }
                    ColumnLayout {
                        spacing: 2
                        NText { text: pluginApi?.tr("welcome.credits.ai_title") || "AI Co-Programmed"; font.weight: Font.Bold }
                        NText {
                            text: pluginApi?.tr("welcome.credits.ai_desc") || "Thanks to Gemini"
                            color: Color.mOnSurfaceVariant; wrapMode: Text.Wrap; Layout.fillWidth: true; pointSize: Style.fontSizeS
                        }
                    }
                }
            }
        }
        Item { Layout.preferredHeight: 50 }
    }

    // --- HELPER COMPONENTS ---
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
        MouseArea { 
            anchors.fill: parent; 
            cursorShape: Qt.PointingHandCursor; 
            onClicked: { sw.toggled() } // Swapped state logic handled in parent to ensure sync
        }
    }
}