/*
 * Ambar menu: the logo at the left of the menu bar, like the Apple menu.
 * About This Computer, System Settings, Ambar Store, Force Quit, then
 * Sleep / Restart / Shut Down and Lock / Log Out.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PC3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.private.sessions as Sessions
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root

    preferredRepresentation: compactRepresentation
    Plasmoid.icon: "ambar-logo"
    toolTipMainText: i18n("Ambar menu")
    toolTipSubText: ""

    Sessions.SessionManagement { id: session }

    P5Support.DataSource {
        id: shell
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => disconnectSource(source)
        function run(cmd) { connectSource(cmd) }
    }

    function act(fn) { root.expanded = false; fn() }

    compactRepresentation: MouseArea {
        id: button
        Layout.minimumWidth: Kirigami.Units.iconSizes.small + Kirigami.Units.largeSpacing * 2
        hoverEnabled: true
        onClicked: root.expanded = !root.expanded
        Kirigami.Icon {
            anchors.centerIn: parent
            width: Kirigami.Units.iconSizes.small + 2
            height: width
            source: "ambar-logo"
            active: button.containsMouse || root.expanded
        }
    }

    fullRepresentation: ColumnLayout {
        id: menu
        spacing: 0
        Layout.preferredWidth: Kirigami.Units.gridUnit * 15
        Layout.minimumWidth: Layout.preferredWidth

        component Entry: PC3.ItemDelegate {
            property string shortcut: ""
            Layout.fillWidth: true
            contentItem: RowLayout {
                PC3.Label { text: parent.parent.text; Layout.fillWidth: true; elide: Text.ElideRight }
                PC3.Label { text: parent.parent.shortcut; opacity: 0.6; visible: text.length > 0 }
            }
        }
        component Line: Kirigami.Separator {
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.smallSpacing
            Layout.bottomMargin: Kirigami.Units.smallSpacing
        }

        Entry { text: i18n("About This Computer"); onClicked: act(() => shell.run("kinfocenter")) }
        Line {}
        Entry { text: i18n("System Settings…"); onClicked: act(() => shell.run("systemsettings")) }
        Entry { text: i18n("Ambar Store…"); onClicked: act(() => shell.run("plasma-discover")) }
        Entry { text: i18n("Automations…"); onClicked: act(() => shell.run("ambar-center")) }
        Line {}
        Entry { text: i18n("Force Quit…"); shortcut: "Ctrl+Alt+Esc"
                onClicked: act(() => shell.run("busctl --user call org.kde.KWin /KWin org.kde.KWin killWindow")) }
        Line {}
        Entry { text: i18n("Sleep"); enabled: session.canSuspend; onClicked: act(() => session.suspend()) }
        Entry { text: i18n("Restart…"); enabled: session.canReboot; onClicked: act(() => session.requestReboot()) }
        Entry { text: i18n("Shut Down…"); enabled: session.canShutdown; onClicked: act(() => session.requestShutdown()) }
        Line {}
        Entry { text: i18n("Lock Screen"); shortcut: "Meta+L"; enabled: session.canLock; onClicked: act(() => session.lock()) }
        Entry { text: i18n("Log Out…"); enabled: session.canLogout; onClicked: act(() => session.requestLogout()) }
    }
}
