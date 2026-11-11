/*
 * Ambar Launchpad: every app on full-screen pages, like macOS.
 *
 * Click the grid icon in the Dock (or press Escape / click outside to close).
 * Typing searches at once; Enter opens the first result. Swipe, scroll or use
 * the arrow keys to change page. The app list, search and the blurred full-screen
 * window come from Plasma's own launcher module (org.kde.plasma.private.kicker),
 * the same one Plasma's full-screen menu uses.
 */
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PC3
import org.kde.kirigami as Kirigami
import org.kde.plasma.private.kicker as Kicker

PlasmoidItem {
    id: root
    preferredRepresentation: compactRepresentation
    Plasmoid.icon: "view-app-grid-symbolic"
    toolTipMainText: i18n("Launchpad")
    toolTipSubText: i18n("All your apps")

    readonly property int columns: 7
    readonly property int rows: 5
    readonly property bool searching: search.text.length > 0

    function open() {
        search.text = "";
        pages.currentIndex = 0;
        dash.toggle();
        search.forceActiveFocus();
    }
    function close() { if (dash.visible) dash.toggle(); }
    function launch(model, row) {
        if (model && model.trigger(row, "", null)) close();
    }

    compactRepresentation: MouseArea {
        id: button
        hoverEnabled: true
        onClicked: root.open()
        Kirigami.Icon {
            anchors.fill: parent
            source: "view-app-grid-symbolic"
            active: button.containsMouse
        }
    }
    fullRepresentation: Item {}

    // Plasma's menu, flattened: row 0 is "All Applications", A to Z.
    Kicker.RootModel {
        id: rootModel
        appletInterface: root
        autoPopulate: true
        flat: true
        sorted: true
        showSeparators: false
        showTopLevelItems: true
        showAllApps: true
        showAllAppsCategorized: false
        showRecentApps: false
        showRecentDocs: false
        showPowerSession: false
        appNameFormat: 0          // just the name, like macOS
    }
    readonly property var apps: rootModel.count > 0 ? rootModel.modelForRow(0) : null
    readonly property int perPage: columns * rows
    readonly property int pageCount: apps ? Math.max(1, Math.ceil(apps.count / perPage)) : 0

    Kicker.RunnerModel {
        id: runner
        appletInterface: root
        runners: ["krunner_services", "krunner_systemsettings"]
        mergeResults: true
        query: search.text
    }

    Kicker.DashboardWindow {
        id: dash
        backgroundColor: "transparent"
        keyEventProxy: search
        onKeyEscapePressed: search.text ? search.text = "" : root.close()

        mainItem: Item {
            id: stage
            anchors.fill: parent

            // A warm dim over the blurred desktop; clicking it closes Launchpad.
            Rectangle {
                anchors.fill: parent
                color: Qt.rgba(0.07, 0.05, 0.03, 0.55)
                MouseArea { anchors.fill: parent; onClicked: root.close() }
            }

            readonly property real cell: Math.min((width * 0.78) / root.columns, (height * 0.72) / root.rows)
            readonly property real iconSize: Math.round(Math.min(96, cell * 0.56))

            // Search: a soft pill at the top, like macOS.
            PC3.TextField {
                id: search
                anchors.horizontalCenter: parent.horizontalCenter
                y: Math.round(stage.height * 0.06)
                width: Math.min(360, stage.width * 0.3)
                height: Kirigami.Units.gridUnit * 2
                placeholderText: i18n("Search")
                placeholderTextColor: Qt.alpha("white", 0.6)
                color: "white"
                horizontalAlignment: activeFocus || text ? Text.AlignLeft : Text.AlignHCenter
                leftPadding: Kirigami.Units.gridUnit * 1.8
                background: Rectangle {
                    radius: height / 2
                    color: Qt.alpha("white", search.activeFocus ? 0.22 : 0.16)
                    border.color: Qt.alpha("white", 0.18)
                    Kirigami.Icon {
                        source: "search"; color: Qt.alpha("white", 0.75); isMask: true
                        width: Kirigami.Units.iconSizes.small; height: width
                        anchors.verticalCenter: parent.verticalCenter
                        x: Kirigami.Units.gridUnit * 0.6
                    }
                }
                Keys.onReturnPressed: {
                    if (root.searching && results.count > 0) root.launch(results.model, 0);
                }
                Keys.onLeftPressed: event => { if (!root.searching) pages.decrementCurrentIndex(); else event.accepted = false }
                Keys.onRightPressed: event => { if (!root.searching) pages.incrementCurrentIndex(); else event.accepted = false }
                Accessible.name: i18n("Search apps")
            }

            // One app: a large icon and its name in white.
            component AppTile: Item {
                id: tile
                property var itemModel
                property int row
                property string label
                property var iconSource
                width: stage.cell; height: stage.cell
                Kirigami.Icon {
                    id: glyph
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: Math.round(parent.height * 0.08)
                    width: stage.iconSize; height: width
                    source: tile.iconSource
                    scale: press.pressed ? 0.92 : (press.containsMouse ? 1.06 : 1)
                    Behavior on scale { NumberAnimation { duration: Kirigami.Units.shortDuration } }
                }
                PC3.Label {
                    anchors.top: glyph.bottom
                    anchors.topMargin: Kirigami.Units.smallSpacing
                    width: parent.width - Kirigami.Units.smallSpacing * 2
                    anchors.horizontalCenter: parent.horizontalCenter
                    horizontalAlignment: Text.AlignHCenter
                    text: tile.label
                    elide: Text.ElideRight
                    maximumLineCount: 1
                    color: "white"
                    style: Text.Raised
                    styleColor: Qt.rgba(0, 0, 0, 0.35)
                }
                MouseArea {
                    id: press
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: root.launch(tile.itemModel, tile.row)
                }
                Accessible.role: Accessible.Button
                Accessible.name: tile.label
            }

            // All apps, a page at a time.
            ListView {
                id: pages
                visible: !root.searching
                anchors.horizontalCenter: parent.horizontalCenter
                y: search.y + search.height + Math.round(stage.height * 0.05)
                width: stage.cell * root.columns
                height: stage.cell * root.rows
                orientation: ListView.Horizontal
                snapMode: ListView.SnapOneItem
                highlightRangeMode: ListView.StrictlyEnforceRange
                highlightMoveDuration: Kirigami.Units.longDuration
                boundsBehavior: Flickable.StopAtBounds
                clip: true
                model: root.pageCount
                delegate: Grid {
                    id: page
                    required property int index
                    width: pages.width; height: pages.height
                    columns: root.columns
                    // Each page shows its slice of the full list; Grid skips the hidden tiles.
                    Repeater {
                        model: root.apps
                        delegate: AppTile {
                            required property int index
                            required property var model
                            visible: index >= page.index * root.perPage && index < (page.index + 1) * root.perPage
                            itemModel: root.apps
                            row: index
                            label: model.display
                            iconSource: model.decoration
                        }
                    }
                }
                WheelHandler {
                    property real acc: 0
                    onWheel: event => {
                        acc += event.angleDelta.y + event.angleDelta.x;
                        if (acc <= -120) { pages.incrementCurrentIndex(); acc = 0 }
                        else if (acc >= 120) { pages.decrementCurrentIndex(); acc = 0 }
                    }
                }
            }

            // Page dots.
            Row {
                visible: !root.searching && root.pageCount > 1
                anchors.horizontalCenter: parent.horizontalCenter
                y: pages.y + pages.height + Kirigami.Units.gridUnit
                spacing: Kirigami.Units.smallSpacing * 2
                Repeater {
                    model: root.pageCount
                    delegate: Rectangle {
                        required property int index
                        width: 8; height: 8; radius: 4
                        color: Qt.alpha("white", index === pages.currentIndex ? 0.95 : 0.35)
                        MouseArea { anchors.fill: parent; anchors.margins: -6; onClicked: pages.currentIndex = parent.index }
                    }
                }
            }

            // Search results, in the same grid style.
            Grid {
                id: results
                visible: root.searching
                readonly property var model: runner.count > 0 ? runner.modelForRow(0) : null
                readonly property int count: model ? model.count : 0
                anchors.horizontalCenter: parent.horizontalCenter
                y: pages.y
                columns: root.columns
                Repeater {
                    model: results.model
                    delegate: AppTile {
                        required property int index
                        required property var model
                        visible: index < root.columns * root.rows
                        itemModel: results.model
                        row: index
                        label: model.display
                        iconSource: model.decoration
                    }
                }
            }
            PC3.Label {
                visible: root.searching && results.count === 0 && !runner.querying
                anchors.horizontalCenter: parent.horizontalCenter
                y: pages.y + Kirigami.Units.gridUnit * 2
                text: i18n("No apps match “%1”", search.text)
                color: Qt.alpha("white", 0.75)
            }
        }
    }
}
