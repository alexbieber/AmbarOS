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
        Layout.preferredWidth: Kirigami.Units.gridUnit * 20
        Layout.minimumWidth: Layout.preferredWidth
        spacing: Kirigami.Units.largeSpacing

        component Tile: QQC2.AbstractButton {
            id: tile
            property string key
            property string iconName
            property string label
            readonly property var info: root.cc[key]
            readonly property bool on: info ? info.on : false
            visible: info !== null && info !== undefined
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 3.2
            onClicked: {
                const want = !on;                       // show the change at once, then confirm by polling
                const next = Object.assign({}, root.cc);
                next[key] = Object.assign({}, info, { on: want });
                root.cc = next;
                root.set(key, want ? "on" : "off");
            }
            Accessible.name: label
            Accessible.checked: on
            background: Rectangle {
                radius: Kirigami.Units.cornerRadius * 2
                color: Kirigami.Theme.backgroundColor
                border.color: Qt.alpha(Kirigami.Theme.textColor, 0.12)
                Rectangle { anchors.fill: parent; radius: parent.radius; color: Kirigami.Theme.textColor
                            opacity: tile.hovered ? 0.06 : 0 }
            }
            contentItem: RowLayout {
                spacing: Kirigami.Units.smallSpacing * 2
                Rectangle {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 1.8; Layout.preferredHeight: Layout.preferredWidth
                    radius: width / 2
                    color: tile.on ? Kirigami.Theme.highlightColor : Qt.alpha(Kirigami.Theme.textColor, 0.12)
                    Kirigami.Icon { anchors.centerIn: parent; width: Kirigami.Units.iconSizes.small; height: width
                                    source: tile.iconName; color: tile.on ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                                    isMask: true }
                }
                ColumnLayout {
                    spacing: 0
                    Layout.fillWidth: true
                    PC3.Label { text: tile.label; font.weight: Font.DemiBold; Layout.fillWidth: true; elide: Text.ElideRight }
                    PC3.Label { text: tile.info && tile.info.detail ? tile.info.detail : (tile.on ? i18n("On") : i18n("Off"))
                                opacity: 0.65; font: Kirigami.Theme.smallFont; Layout.fillWidth: true; elide: Text.ElideRight }
                }
            }
        }

        component SliderRow: ColumnLayout {
            property string key
            property string label
            property string iconName
            readonly property var info: root.cc[key]
            visible: info !== null && info !== undefined
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            PC3.Label { text: label; font.weight: Font.DemiBold }
            RowLayout {
                Kirigami.Icon { source: parent.parent.iconName; Layout.preferredWidth: Kirigami.Units.iconSizes.small; Layout.preferredHeight: Layout.preferredWidth }
                PC3.Slider {
                    Layout.fillWidth: true
                    from: 0; to: 100; stepSize: 1
                    value: parent.parent.info ? parent.parent.info.value : 0
                    onMoved: root.set(parent.parent.key, Math.round(value))
                    Accessible.name: parent.parent.label
                }
            }
        }

        GridLayout {
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.largeSpacing
            Layout.fillWidth: true
            Tile { key: "wifi"; iconName: "network-wireless-symbolic"; label: i18n("Wi-Fi") }
            Tile { key: "bluetooth"; iconName: "network-bluetooth-symbolic"; label: i18n("Bluetooth") }
            Tile { key: "airplane"; iconName: "network-flightmode-on-symbolic"; label: i18n("Airplane Mode") }
            Tile { key: "dnd"; iconName: "notifications-disabled-symbolic"; label: i18n("Do Not Disturb") }
            Tile { key: "dark"; iconName: "weather-clear-night-symbolic"; label: i18n("Dark Mode") }
            Tile { key: "nightlight"; iconName: "redshift-status-on"; label: i18n("Night Light") }
        }

        SliderRow { key: "brightness"; label: i18n("Display"); iconName: "brightness-high-symbolic" }
        SliderRow { key: "volume"; label: i18n("Sound"); iconName: "audio-volume-high-symbolic" }

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
