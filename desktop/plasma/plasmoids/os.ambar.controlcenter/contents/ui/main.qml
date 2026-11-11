/*
 * Ambar Control Center: the panel behind the toggle icon in the menu bar,
 * like macOS's Control Center. Tiles for Wi-Fi, Bluetooth, Airplane mode,
 * Do Not Disturb, Dark Mode and Night Light; sliders for display and sound.
 * All reading and switching goes through `ambar-control-center` (tested
 * separately), so this file only draws.
 */
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PC3
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root
    preferredRepresentation: compactRepresentation
    Plasmoid.icon: "ambar-control-center-symbolic"
    toolTipMainText: i18n("Control Center")

    property var cc: ({})   // latest `ambar-control-center status`
    readonly property string helper: "ambar-control-center"

    P5Support.DataSource {
        id: shell
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => {
            if (source === root.helper + " status") {
                try { root.cc = JSON.parse(data["stdout"]) } catch (e) {}
            } else {
                refresh.restart()
            }
            disconnectSource(source)
        }
        function run(cmd) { connectSource(cmd) }
    }
    function poll() { shell.run(root.helper + " status") }

    readonly property color moduleColor: Qt.alpha(Kirigami.Theme.textColor, 0.07)
    readonly property color moduleHover: Qt.alpha(Kirigami.Theme.textColor, 0.11)
    readonly property real moduleRadius: Kirigami.Units.cornerRadius * 3
    function has(key) { return root.cc[key] !== null && root.cc[key] !== undefined }
    function toggle(key) {
        const info = root.cc[key];
        const want = !(info && info.on);                  // show the change at once, then confirm by polling
        const next = Object.assign({}, root.cc);
        next[key] = Object.assign({}, info, { on: want });
        root.cc = next;
        root.set(key, want ? "on" : "off");
    }
    function set(key, value) { shell.run(root.helper + " set " + key + " " + value) }
    Timer { id: refresh; interval: 400; onTriggered: root.poll() }
    Timer { interval: 3000; repeat: true; running: root.expanded; triggeredOnStart: true; onTriggered: root.poll() }
    Component.onCompleted: poll()

    compactRepresentation: MouseArea {
        id: button
        Layout.minimumWidth: Kirigami.Units.iconSizes.small + Kirigami.Units.largeSpacing * 2
        hoverEnabled: true
        onClicked: root.expanded = !root.expanded
        Kirigami.Icon {
            anchors.centerIn: parent
            width: Kirigami.Units.iconSizes.small; height: width
            source: "ambar-control-center-symbolic"
            active: button.containsMouse || root.expanded
        }
    }

    fullRepresentation: ColumnLayout {
        id: panel
        Layout.preferredWidth: Kirigami.Units.gridUnit * 21
        Layout.minimumWidth: Layout.preferredWidth
        spacing: Kirigami.Units.smallSpacing * 3

        // A rounded, softly filled block, like the modules of macOS's Control Center.
        component Module: Rectangle {
            radius: root.moduleRadius
            color: root.moduleColor
            Layout.fillWidth: true
        }

        // A round icon that turns accent-coloured when the setting is on.
        component Glyph: Rectangle {
            property string iconName
            property bool on
            implicitWidth: Kirigami.Units.gridUnit * 1.9; implicitHeight: implicitWidth
            radius: width / 2
            color: on ? Kirigami.Theme.highlightColor : Qt.alpha(Kirigami.Theme.textColor, 0.12)
            Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
            Kirigami.Icon {
                anchors.centerIn: parent
                width: Kirigami.Units.iconSizes.small; height: width
                source: parent.iconName; isMask: true
                color: parent.on ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
            }
        }

        // One row inside a module: glyph, name and state. The whole row is the button.
        component ToggleRow: QQC2.AbstractButton {
            id: row
            property string key
            property string iconName
            property string label
            readonly property var info: root.cc[key]
            readonly property bool on: info ? info.on : false
            visible: root.has(key)
            Layout.fillWidth: true
            implicitHeight: Kirigami.Units.gridUnit * 2.6
            onClicked: root.toggle(key)
            Accessible.name: label
            Accessible.checked: on
            background: Rectangle { radius: root.moduleRadius - 4; color: row.hovered ? root.moduleHover : "transparent" }
            contentItem: RowLayout {
                spacing: Kirigami.Units.smallSpacing * 2.5
                Glyph { iconName: row.iconName; on: row.on }
                ColumnLayout {
                    spacing: 0
                    Layout.fillWidth: true
                    PC3.Label { text: row.label; font.weight: Font.DemiBold; Layout.fillWidth: true; elide: Text.ElideRight }
                    PC3.Label { text: row.info && row.info.detail ? row.info.detail : (row.on ? i18n("On") : i18n("Off"))
                                opacity: 0.6; font: Kirigami.Theme.smallFont; Layout.fillWidth: true; elide: Text.ElideRight }
                }
            }
        }

        // A small square module: glyph above a short name.
        component Square: QQC2.AbstractButton {
            id: sq
            property string key
            property string iconName
            property string label
            readonly property bool on: root.cc[key] ? root.cc[key].on : false
            visible: root.has(key)
            Layout.fillWidth: true
            implicitHeight: Kirigami.Units.gridUnit * 4.2
            onClicked: root.toggle(key)
            Accessible.name: label
            Accessible.checked: on
            background: Rectangle { radius: root.moduleRadius; color: sq.hovered ? root.moduleHover : root.moduleColor }
            contentItem: ColumnLayout {
                spacing: Kirigami.Units.smallSpacing
                Glyph { iconName: sq.iconName; on: sq.on; Layout.alignment: Qt.AlignHCenter }
                PC3.Label { text: sq.label; font: Kirigami.Theme.smallFont; Layout.alignment: Qt.AlignHCenter
                            horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true; elide: Text.ElideRight }
            }
        }

        // A module with a title and a full-width slider, like Display and Sound on macOS.
        component SliderModule: Module {
            id: sm
            property string key
            property string label
            property string iconName
            readonly property var info: root.cc[key]
            visible: root.has(key)
            implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2
            ColumnLayout {
                id: col
                anchors.fill: parent
                anchors.margins: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.smallSpacing
                PC3.Label { text: sm.label; font.weight: Font.DemiBold }
                RowLayout {
                    spacing: Kirigami.Units.smallSpacing * 2
                    Kirigami.Icon { source: sm.iconName; Layout.preferredWidth: Kirigami.Units.iconSizes.small
                                    Layout.preferredHeight: Layout.preferredWidth; opacity: 0.75 }
                    PC3.Slider {
                        Layout.fillWidth: true
                        from: 0; to: 100; stepSize: 1
                        value: sm.info ? sm.info.value : 0
                        onMoved: root.set(sm.key, Math.round(value))
                        Accessible.name: sm.label
                    }
                    PC3.Label { text: sm.info ? Math.round(sm.info.value) + "%" : ""; opacity: 0.6
                                font: Kirigami.Theme.smallFont; Layout.minimumWidth: Kirigami.Units.gridUnit * 2
                                horizontalAlignment: Text.AlignRight }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing * 3

            // Connectivity: shown only when the computer has Wi-Fi, Bluetooth or an airplane switch.
            Module {
                visible: root.has("wifi") || root.has("bluetooth") || root.has("airplane")
                Layout.alignment: Qt.AlignTop
                Layout.preferredWidth: 1
                implicitHeight: conn.implicitHeight + Kirigami.Units.smallSpacing * 3
                ColumnLayout {
                    id: conn
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.smallSpacing * 1.5
                    spacing: 0
                    ToggleRow { key: "wifi"; iconName: "network-wireless-symbolic"; label: i18n("Wi-Fi") }
                    ToggleRow { key: "bluetooth"; iconName: "network-bluetooth-symbolic"; label: i18n("Bluetooth") }
                    ToggleRow { key: "airplane"; iconName: "network-flightmode-on-symbolic"; label: i18n("Airplane Mode") }
                    Item { Layout.fillHeight: true }
                }
            }

            ColumnLayout {
                Layout.alignment: Qt.AlignTop
                Layout.preferredWidth: 1
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing * 3
                Module {
                    visible: root.has("dnd")
                    implicitHeight: focusRow.implicitHeight + Kirigami.Units.smallSpacing * 3
                    ToggleRow { id: focusRow; key: "dnd"; iconName: "notifications-disabled-symbolic"; label: i18n("Do Not Disturb")
                          anchors.fill: parent; anchors.margins: Kirigami.Units.smallSpacing * 1.5 }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing * 3
                    Square { key: "dark"; iconName: "weather-clear-night-symbolic"; label: i18n("Dark Mode") }
                    Square { key: "nightlight"; iconName: "redshift-status-on"; label: i18n("Night Light") }
                }
            }
        }

        SliderModule { key: "brightness"; label: i18n("Display"); iconName: "brightness-high-symbolic" }
        SliderModule { key: "volume"; label: i18n("Sound"); iconName: "audio-volume-high-symbolic" }

        RowLayout {
            Layout.fillWidth: true
            PC3.ToolButton { icon.name: "spectacle"; text: i18n("Screenshot")
                             onClicked: { root.expanded = false; shell.run("spectacle") } }
            Item { Layout.fillWidth: true }
            PC3.ToolButton { icon.name: "configure"; text: i18n("Settings…")
                             onClicked: { root.expanded = false; shell.run("systemsettings") } }
        }
    }
}
