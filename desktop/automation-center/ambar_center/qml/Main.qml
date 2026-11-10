// Ambar Automation Center
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: win
    width: 1120
    height: 740
    minimumWidth: 880
    minimumHeight: 580
    visible: true
    title: "Ambar Automation Center"

    property string themeOverride: initialTheme
    property bool dark: themeOverride === "dark"
                        || (themeOverride === "" && Qt.styleHints.colorScheme === Qt.ColorScheme.Dark)
    property string page: initialPage
    property var draft: null

    QtObject {
        id: t
        readonly property color bg: win.dark ? "#15120F" : "#F7F4EF"
        readonly property color side: win.dark ? "#1C1814" : "#EFE9E0"
        readonly property color surface: win.dark ? "#231E19" : "#FFFFFF"
        readonly property color surface2: win.dark ? "#2C261F" : "#F4EFE8"
        readonly property color border: win.dark ? "#3B332A" : "#E3DACD"
        readonly property color text: win.dark ? "#F4EEE6" : "#1F1A15"
        readonly property color muted: win.dark ? "#A89D90" : "#6F655A"
        readonly property color accent: win.dark ? "#F2A43A" : "#B35C05"
        readonly property color accentSoft: win.dark ? "#3B2B16" : "#FBE9D1"
        readonly property color onAccent: win.dark ? "#1B1209" : "#FFFFFF"
        readonly property color good: win.dark ? "#62C796" : "#21805A"
        readonly property color warn: win.dark ? "#E8B54E" : "#9C6500"
        readonly property color bad: win.dark ? "#F07B6F" : "#BE3A2E"
        readonly property int radius: 12
    }

    color: t.bg
    font.pixelSize: 14

    function stateTone(s) {
        return s === "active" ? t.good : s === "paused" ? t.warn : t.muted
    }
    function stateLabel(s) {
        return s === "inactive" ? "Disabled" : s.charAt(0).toUpperCase() + s.slice(1)
    }
    function statusTone(s) {
        return s === "succeeded" ? t.good
             : (s === "failed" || s === "denied") ? t.bad
             : s === "cancelled" ? t.warn
             : s === "running" ? t.accent : t.muted
    }
    function riskTone(level) {
        return level.indexOf("high") === 0 ? t.bad : level.indexOf("warning") === 0 ? t.warn : t.muted
    }
    function triggerLabel(k) {
        return k === "folder_watch" ? "Folder" : k === "schedule" ? "Schedule" : "Manual"
    }

    // ------------------------------------------------------------ components

    component Label2: Text {
        color: t.text
        font.pixelSize: 14
        wrapMode: Text.WordWrap
    }

    component Muted: Text {
        color: t.muted
        font.pixelSize: 13
        wrapMode: Text.WordWrap
    }

    component Pill: Rectangle {
        id: pill
        property string label
        property color tone: t.muted
        implicitHeight: 22
        implicitWidth: pillText.implicitWidth + 18
        radius: 11
        color: Qt.rgba(tone.r, tone.g, tone.b, win.dark ? 0.18 : 0.12)
        Text {
            id: pillText
            anchors.centerIn: parent
            text: pill.label
            color: pill.tone
            font.pixelSize: 12
            font.weight: Font.DemiBold
        }
    }

    component Btn: Button {
        id: b
        property bool primary: false
        property bool danger: false
        implicitHeight: 32
        leftPadding: 14
        rightPadding: 14
        font.pixelSize: 13
        font.weight: Font.Medium
        hoverEnabled: true
        contentItem: Text {
            text: b.text
            font: b.font
            color: !b.enabled ? t.muted : b.primary ? t.onAccent : b.danger ? t.bad : t.text
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            opacity: b.enabled ? 1 : 0.7
        }
        background: Rectangle {
            radius: 8
            color: !b.enabled ? (b.primary ? t.surface2 : "transparent")
                 : b.primary ? (b.down ? Qt.darker(t.accent, 1.15) : b.hovered ? Qt.lighter(t.accent, 1.06) : t.accent)
                             : (b.down ? t.border : b.hovered ? t.surface2 : "transparent")
            border.width: b.primary && b.enabled ? 0 : 1
            border.color: t.border
        }
    }

    component Check: CheckBox {
        id: cb
        spacing: 8
        indicator: Rectangle {
            x: cb.leftPadding
            anchors.verticalCenter: parent.verticalCenter
            width: 18; height: 18; radius: 5
            color: cb.checked ? t.accent : t.surface
            border.color: cb.checked ? t.accent : t.muted
            border.width: 1.5
            Text { anchors.centerIn: parent; text: "✓"; visible: cb.checked; color: t.onAccent; font.pixelSize: 12; font.weight: Font.Bold }
        }
        contentItem: Text {
            text: cb.text
            color: t.text
            font.pixelSize: 13
            verticalAlignment: Text.AlignVCenter
            leftPadding: cb.indicator.width + cb.spacing
        }
    }

    component Card: Rectangle {
        color: t.surface
        radius: t.radius
        border.color: t.border
        border.width: 1
    }

    component SectionTitle: Text {
        color: t.muted
        font.pixelSize: 12
        font.weight: Font.DemiBold
        font.letterSpacing: 0.6
        font.capitalization: Font.AllUppercase
    }

    component Empty: Card {
        property string heading
        property string detail
        Layout.fillWidth: true
        implicitHeight: emptyCol.implicitHeight + 48
        ColumnLayout {
            id: emptyCol
            anchors.centerIn: parent
            width: parent.width - 64
            spacing: 6
            Label2 { text: heading; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
            Muted { text: detail; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
        }
    }

    component RunRow: Rectangle {
        property var run
        Layout.fillWidth: true
        implicitHeight: 52
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 16
            anchors.rightMargin: 16
            spacing: 14
            Pill { label: run.status; tone: win.statusTone(run.status); Layout.preferredWidth: 88 }
            ColumnLayout {
                spacing: 1
                Layout.fillWidth: true
                Label2 { text: run.workflow_id; font.weight: Font.Medium; elide: Text.ElideRight; wrapMode: Text.NoWrap; Layout.fillWidth: true }
                Muted {
                    text: run.error ? run.error : (run.steps_count + (run.steps_count === 1 ? " step" : " steps") + " · " + run.trigger)
                    elide: Text.ElideRight
                    wrapMode: Text.NoWrap
                    Layout.fillWidth: true
                    color: run.error && run.status !== "skipped" ? t.bad : t.muted
                }
            }
            Muted { text: run.when }
        }
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: t.border; opacity: 0.6 }
    }

    // ------------------------------------------------------------ layout

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Sidebar
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 236
            color: t.side

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 16
                spacing: 4

                RowLayout {
                    spacing: 11
                    Layout.topMargin: 8
                    Layout.bottomMargin: 22
                    Layout.leftMargin: 6
                    Rectangle {
                        width: 32; height: 32; radius: 10
                        gradient: Gradient {
                            GradientStop { position: 0; color: "#F2A43A" }
                            GradientStop { position: 1; color: "#C9620A" }
                        }
                        Text { anchors.centerIn: parent; text: "A"; color: "white"; font.pixelSize: 18; font.weight: Font.Bold }
                    }
                    ColumnLayout {
                        spacing: 0
                        Text { text: "Ambar"; color: t.text; font.pixelSize: 16; font.weight: Font.DemiBold }
                        Text { text: "Automation Center"; color: t.muted; font.pixelSize: 12 }
                    }
                }

                Repeater {
                    model: [
                        { key: "overview", label: "Overview" },
                        { key: "automations", label: "Automations" },
                        { key: "proposals", label: "Proposals" },
                        { key: "agents", label: "Agents" },
                        { key: "activity", label: "Activity" }
                    ]
                    delegate: Rectangle {
                        id: nav
                        required property var modelData
                        property bool selected: win.page === modelData.key
                        Layout.fillWidth: true
                        implicitHeight: 38
                        radius: 9
                        color: selected ? t.accentSoft : navMouse.containsMouse ? t.surface2 : "transparent"
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 10
                            Rectangle {
                                width: 3; height: 16; radius: 2
                                color: t.accent
                                visible: nav.selected
                            }
                            Text {
                                text: nav.modelData.label
                                color: nav.selected ? t.text : t.muted
                                font.pixelSize: 14
                                font.weight: nav.selected ? Font.DemiBold : Font.Normal
                                Layout.fillWidth: true
                            }
                            Rectangle {
                                visible: nav.modelData.key === "proposals" && backend.pendingProposals.length > 0
                                implicitWidth: Math.max(20, badge.implicitWidth + 10)
                                implicitHeight: 20
                                radius: 10
                                color: t.accent
                                Text { id: badge; anchors.centerIn: parent; text: backend.pendingProposals.length; color: t.onAccent; font.pixelSize: 11; font.weight: Font.Bold }
                            }
                        }
                        MouseArea {
                            id: navMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: win.page = nav.modelData.key
                        }
                    }
                }

                Item { Layout.fillHeight: true }

                Card {
                    Layout.fillWidth: true
                    implicitHeight: svcCol.implicitHeight + 24
                    color: t.surface
                    ColumnLayout {
                        id: svcCol
                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 3
                        RowLayout {
                            spacing: 8
                            Rectangle {
                                width: 8; height: 8; radius: 4
                                color: !backend.connected ? t.bad : backend.health.status === "ok" ? t.good : t.warn
                            }
                            Text {
                                text: !backend.connected ? "Service stopped"
                                      : backend.health.status === "ok" ? "Service running" : "Needs attention"
                                color: t.text
                                font.pixelSize: 13
                                font.weight: Font.Medium
                            }
                        }
                        Muted {
                            text: backend.connected ? ("ambard " + backend.health.version + " · pid " + backend.health.pid) : "Start it to run automations"
                            font.pixelSize: 12
                        }
                    }
                }
                Btn {
                    objectName: "themeToggle"
                    Layout.fillWidth: true
                    text: win.dark ? "Use light appearance" : "Use dark appearance"
                    onClicked: win.themeOverride = win.dark ? "light" : "dark"
                }
            }
        }

        // Content
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Service not running
            ColumnLayout {
                anchors.centerIn: parent
                width: Math.min(440, parent.width - 64)
                visible: !backend.connected
                spacing: 12
                Rectangle {
                    Layout.alignment: Qt.AlignHCenter
                    width: 56; height: 56; radius: 18
                    color: t.accentSoft
                    Text { anchors.centerIn: parent; text: "⏻"; color: t.accent; font.pixelSize: 26 }
                }
                Label2 { text: "The Ambar service isn't running"; font.pixelSize: 20; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
                Muted { text: "Automations run in the background service, ambard. Start it to see and control your automations."; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
                Btn {
                    Layout.alignment: Qt.AlignHCenter
                    primary: true
                    text: backend.busy ? "Starting…" : "Start service"
                    enabled: !backend.busy
                    onClicked: backend.startService()
                }
            }

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 32
                anchors.topMargin: 28
                spacing: 20
                visible: backend.connected

                // Header
                RowLayout {
                    Layout.fillWidth: true
                    ColumnLayout {
                        spacing: 2
                        Text {
                            text: ({ overview: "Overview", automations: "Automations", proposals: "Proposals",
                                     agents: "Agents", activity: "Activity" })[win.page]
                            color: t.text
                            font.pixelSize: 26
                            font.weight: Font.DemiBold
                        }
                        Muted {
                            text: ({ overview: "What's running, and a quick way to automate something new.",
                                     automations: "Everything Ambar runs for you. Nothing runs without the permissions you approved.",
                                     proposals: "Automations your agents suggested. They do nothing until you approve them.",
                                     agents: "Assistants that can talk to Ambar, and exactly what each one may do.",
                                     activity: "Every run, with its outcome." })[win.page]
                        }
                    }
                    Item { Layout.fillWidth: true }
                    Btn { text: "Refresh"; onClicked: backend.refresh() }
                }

                StackLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    currentIndex: ["overview", "automations", "proposals", "agents", "activity"].indexOf(win.page)

                    // ---------------------------------------------- Overview
                    ScrollView {
                        contentWidth: availableWidth
                        clip: true
                        ColumnLayout {
                            width: parent.width
                            spacing: 20

                            Card {
                                Layout.fillWidth: true
                                implicitHeight: askCol.implicitHeight + 40
                                ColumnLayout {
                                    id: askCol
                                    anchors.fill: parent
                                    anchors.margins: 20
                                    spacing: 12
                                    Label2 { text: "Describe an automation"; font.pixelSize: 16; font.weight: Font.DemiBold }
                                    Rectangle {
                                        Layout.fillWidth: true
                                        implicitHeight: 76
                                        radius: 10
                                        color: t.surface2
                                        border.color: askField.activeFocus ? t.accent : t.border
                                        TextArea {
                                            id: askField
                                            objectName: "askField"
                                            anchors.fill: parent
                                            anchors.margins: 4
                                            wrapMode: TextArea.Wrap
                                            color: t.text
                                            placeholderText: "e.g. When a PDF lands in Downloads, move it to Documents/PDFs and let me know"
                                            placeholderTextColor: t.muted
                                            background: null
                                            font.pixelSize: 14
                                        }
                                    }
                                    RowLayout {
                                        Layout.fillWidth: true
                                        Muted { text: "You'll see the draft, its permissions and a risk review before anything is added."; Layout.fillWidth: true }
                                        Btn {
                                            objectName: "draftButton"
                                            primary: true
                                            text: backend.busy === "drafting an automation" ? "Drafting…" : "Create draft"
                                            enabled: askField.text.trim().length > 0 && !backend.busy
                                            onClicked: backend.describe(askField.text)
                                        }
                                    }
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 14
                                Repeater {
                                    model: [
                                        { label: "Active", value: backend.health.workflows ? backend.health.workflows.active : 0, tone: t.good },
                                        { label: "Paused", value: backend.health.workflows ? backend.health.workflows.paused : 0, tone: t.warn },
                                        { label: "Running now", value: backend.health.running || 0, tone: t.accent },
                                        { label: "Awaiting approval", value: backend.pendingProposals.length, tone: t.accent }
                                    ]
                                    delegate: Card {
                                        required property var modelData
                                        Layout.fillWidth: true
                                        implicitHeight: 92
                                        ColumnLayout {
                                            anchors.fill: parent
                                            anchors.margins: 16
                                            spacing: 4
                                            Muted { text: modelData.label }
                                            Text { text: modelData.value; color: t.text; font.pixelSize: 28; font.weight: Font.DemiBold }
                                        }
                                        Rectangle { width: 28; height: 3; radius: 2; color: modelData.tone; anchors.left: parent.left; anchors.leftMargin: 16; anchors.bottom: parent.bottom; anchors.bottomMargin: 14 }
                                    }
                                }
                            }

                            Repeater {
                                model: backend.health.problems || []
                                delegate: Rectangle {
                                    required property string modelData
                                    Layout.fillWidth: true
                                    implicitHeight: probText.implicitHeight + 24
                                    radius: 10
                                    color: Qt.rgba(t.warn.r, t.warn.g, t.warn.b, 0.12)
                                    border.color: Qt.rgba(t.warn.r, t.warn.g, t.warn.b, 0.4)
                                    Label2 { id: probText; anchors.fill: parent; anchors.margins: 12; text: "⚠  " + modelData; color: t.text }
                                }
                            }

                            SectionTitle { text: "Recent activity" }
                            Card {
                                Layout.fillWidth: true
                                implicitHeight: Math.max(64, recentCol.implicitHeight)
                                visible: backend.activity.length > 0
                                ColumnLayout {
                                    id: recentCol
                                    width: parent.width
                                    spacing: 0
                                    Repeater {
                                        model: backend.activity.slice(0, 6)
                                        delegate: RunRow { required property var modelData; run: modelData }
                                    }
                                }
                            }
                            Empty {
                                visible: backend.activity.length === 0
                                heading: "Nothing has run yet"
                                detail: "Runs show up here as soon as an automation fires."
                            }
                            Item { implicitHeight: 8 }
                        }
                    }

                    // ---------------------------------------------- Automations
                    ScrollView {
                        contentWidth: availableWidth
                        clip: true
                        ColumnLayout {
                            width: parent.width
                            spacing: 10
                            Empty {
                                visible: backend.workflows.length === 0
                                heading: "No automations yet"
                                detail: "Describe one on the Overview page, or add a workflow file with: ambar workflow add <file>"
                            }
                            Repeater {
                                model: backend.workflows
                                delegate: Card {
                                    id: wfCard
                                    required property var modelData
                                    property var w: modelData
                                    Layout.fillWidth: true
                                    implicitHeight: 76
                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: 18
                                        anchors.rightMargin: 14
                                        spacing: 14
                                        ColumnLayout {
                                            spacing: 3
                                            Layout.fillWidth: true
                                            RowLayout {
                                                spacing: 8
                                                Label2 { text: wfCard.w.name; font.weight: Font.DemiBold; wrapMode: Text.NoWrap }
                                                Pill { label: win.stateLabel(wfCard.w.state); tone: win.stateTone(wfCard.w.state) }
                                                Pill { visible: wfCard.w.running; label: "Running"; tone: t.accent }
                                                Pill { visible: !wfCard.w.granted; label: "No permission"; tone: t.bad }
                                            }
                                            Muted {
                                                text: win.triggerLabel(wfCard.w.trigger) + " · " + wfCard.w.id + " · "
                                                      + (wfCard.w.last_status ? "last run " + wfCard.w.last_status + " " + wfCard.w.last_when : "never run")
                                                elide: Text.ElideRight
                                                wrapMode: Text.NoWrap
                                                Layout.fillWidth: true
                                            }
                                        }
                                        Btn {
                                            text: "Run now"
                                            visible: wfCard.w.state === "active" && wfCard.w.trigger !== "folder_watch"
                                            enabled: !wfCard.w.running && !backend.busy
                                            onClicked: backend.runNow(wfCard.w.id)
                                        }
                                        Btn { text: "Pause"; visible: wfCard.w.state === "active"; onClicked: backend.control("pause", wfCard.w.id) }
                                        Btn { text: "Resume"; visible: wfCard.w.state === "paused"; onClicked: backend.control("resume", wfCard.w.id) }
                                        Btn { text: "Disable"; visible: wfCard.w.state === "active" || wfCard.w.state === "paused"; onClicked: backend.control("disable", wfCard.w.id) }
                                        Btn { text: "Enable"; primary: true; visible: wfCard.w.state === "inactive" || wfCard.w.state === "cancelled"; onClicked: backend.control("enable", wfCard.w.id) }
                                        Btn { text: "Cancel"; danger: true; visible: wfCard.w.running; onClicked: backend.control("cancel", wfCard.w.id) }
                                    }
                                }
                            }
                        }
                    }

                    // ---------------------------------------------- Proposals
                    ScrollView {
                        contentWidth: availableWidth
                        clip: true
                        ColumnLayout {
                            width: parent.width
                            spacing: 14
                            Empty {
                                visible: backend.pendingProposals.length === 0
                                heading: "No proposals waiting"
                                detail: "When an agent suggests an automation, it appears here for you to review."
                            }
                            Repeater {
                                model: backend.pendingProposals
                                delegate: Card {
                                    id: pCard
                                    required property var modelData
                                    property var p: modelData
                                    Layout.fillWidth: true
                                    implicitHeight: pCol.implicitHeight + 36
                                    ColumnLayout {
                                        id: pCol
                                        anchors.fill: parent
                                        anchors.margins: 18
                                        spacing: 10
                                        RowLayout {
                                            Layout.fillWidth: true
                                            Label2 { text: pCard.p.workflow_id; font.pixelSize: 16; font.weight: Font.DemiBold; wrapMode: Text.NoWrap }
                                            Pill { label: pCard.p.high ? "High risk" : pCard.p.risks.length ? "Review" : "Low risk"; tone: pCard.p.high ? t.bad : pCard.p.risks.length ? t.warn : t.good }
                                            Item { Layout.fillWidth: true }
                                            Muted { text: "from " + pCard.p.agent_name + " · " + pCard.p.when }
                                        }
                                        Label2 { visible: pCard.p.reason.length > 0; text: "“" + pCard.p.reason + "”"; color: t.muted; Layout.fillWidth: true }
                                        Repeater {
                                            model: pCard.p.risks
                                            delegate: Text {
                                                required property string modelData
                                                text: "•  " + modelData.replace(/^(high|warning|info): /, "")
                                                color: win.riskTone(modelData)
                                                font.pixelSize: 13
                                                wrapMode: Text.WordWrap
                                                Layout.fillWidth: true
                                            }
                                        }
                                        Rectangle {
                                            Layout.fillWidth: true
                                            implicitHeight: Math.min(190, code.implicitHeight + 20)
                                            radius: 8
                                            color: t.surface2
                                            clip: true
                                            Text {
                                                id: code
                                                x: 12; y: 10
                                                width: parent.width - 24
                                                text: pCard.p.definition.trim()
                                                color: t.text
                                                font.family: "monospace"
                                                font.pixelSize: 12
                                                wrapMode: Text.WrapAnywhere
                                            }
                                        }
                                        RowLayout {
                                            Layout.fillWidth: true
                                            Check {
                                                id: ack
                                                visible: pCard.p.high
                                                text: "I've reviewed the high-risk items"
                                            }
                                            Item { Layout.fillWidth: true }
                                            Btn { text: "Reject"; danger: true; onClicked: backend.reject(pCard.p.id) }
                                            Btn { text: "Approve"; enabled: !pCard.p.high || ack.checked; onClicked: backend.approve(pCard.p.id, false) }
                                            Btn { text: "Approve & enable"; primary: true; enabled: !pCard.p.high || ack.checked; onClicked: backend.approve(pCard.p.id, true) }
                                        }
                                    }
                                }
                            }
                            SectionTitle { visible: backend.decidedProposals.length > 0; text: "Decided" }
                            Repeater {
                                model: backend.decidedProposals
                                delegate: RowLayout {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    Pill { label: modelData.status; tone: modelData.status === "approved" ? t.good : t.muted }
                                    Label2 { text: modelData.workflow_id; wrapMode: Text.NoWrap }
                                    Muted { text: "from " + modelData.agent_name; Layout.fillWidth: true }
                                }
                            }
                        }
                    }

                    // ---------------------------------------------- Agents
                    ScrollView {
                        contentWidth: availableWidth
                        clip: true
                        ColumnLayout {
                            width: parent.width
                            spacing: 10
                            Empty {
                                visible: backend.agents.length === 0
                                heading: "No agents"
                                detail: "Give an assistant limited access with: ambar agent create <name> --scope read --scope propose"
                            }
                            Repeater {
                                model: backend.agents
                                delegate: Card {
                                    id: aCard
                                    required property var modelData
                                    property var a: modelData
                                    Layout.fillWidth: true
                                    implicitHeight: 82
                                    opacity: a.revoked ? 0.6 : 1
                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: 18
                                        anchors.rightMargin: 14
                                        spacing: 14
                                        ColumnLayout {
                                            spacing: 5
                                            Layout.fillWidth: true
                                            RowLayout {
                                                spacing: 8
                                                Label2 { text: aCard.a.name; font.weight: Font.DemiBold; wrapMode: Text.NoWrap }
                                                Pill { label: aCard.a.revoked ? "Revoked" : "Active"; tone: aCard.a.revoked ? t.muted : t.good }
                                                Muted { text: aCard.a.id }
                                            }
                                            Flow {
                                                spacing: 6
                                                Layout.fillWidth: true
                                                Repeater {
                                                    model: aCard.a.scope_list
                                                    delegate: Pill { required property string modelData; label: modelData; tone: t.accent }
                                                }
                                            }
                                        }
                                        ColumnLayout {
                                            spacing: 2
                                            Muted { text: aCard.a.requests + " requests · " + aCard.a.denied + " refused"; Layout.alignment: Qt.AlignRight }
                                            Muted { text: "last seen " + aCard.a.when; Layout.alignment: Qt.AlignRight }
                                        }
                                        Btn { text: "Revoke"; danger: true; visible: !aCard.a.revoked; onClicked: backend.revokeAgent(aCard.a.id) }
                                    }
                                }
                            }
                        }
                    }

                    // ---------------------------------------------- Activity
                    ScrollView {
                        contentWidth: availableWidth
                        clip: true
                        ColumnLayout {
                            width: parent.width
                            spacing: 0
                            Empty {
                                visible: backend.activity.length === 0
                                heading: "No runs yet"
                                detail: "Every run will be listed here with its result."
                            }
                            Card {
                                Layout.fillWidth: true
                                visible: backend.activity.length > 0
                                implicitHeight: actCol.implicitHeight
                                ColumnLayout {
                                    id: actCol
                                    width: parent.width
                                    spacing: 0
                                    Repeater {
                                        model: backend.activity
                                        delegate: RunRow { required property var modelData; run: modelData }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ draft review

    Popup {
        id: draftPopup
        objectName: "draftPopup"
        modal: true
        anchors.centerIn: Overlay.overlay
        width: Math.min(720, win.width - 80)
        height: Math.min(640, win.height - 60)
        padding: 0
        closePolicy: Popup.CloseOnEscape
        background: Rectangle { color: t.surface; radius: 16; border.color: t.border }
        Overlay.modal: Rectangle { color: win.dark ? "#99000000" : "#55201810" }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 24
            spacing: 12
            RowLayout {
                Layout.fillWidth: true
                Label2 { text: "Review draft"; font.pixelSize: 20; font.weight: Font.DemiBold }
                Pill {
                    visible: win.draft !== null
                    label: win.draft && win.draft.high ? "High risk" : win.draft && win.draft.risks.length ? "Review" : "Low risk"
                    tone: win.draft && win.draft.high ? t.bad : win.draft && win.draft.risks.length ? t.warn : t.good
                }
                Item { Layout.fillWidth: true }
                Muted { text: win.draft ? win.triggerLabel(win.draft.trigger) + " · " + win.draft.id : "" }
            }
            Muted {
                Layout.fillWidth: true
                text: win.draft ? "This automation will be able to: "
                      + (win.draft.permissions.write.length ? "change files in " + win.draft.permissions.write.join(", ") : "")
                      + (win.draft.permissions.read.length ? (win.draft.permissions.write.length ? "; " : "") + "read " + win.draft.permissions.read.join(", ") : "")
                      + (win.draft.permissions.notify ? "; show notifications" : "")
                      + (!win.draft.permissions.write.length && !win.draft.permissions.read.length && !win.draft.permissions.notify ? "nothing outside itself" : "")
                      : ""
            }
            Repeater {
                model: win.draft ? win.draft.risks : []
                delegate: Text {
                    required property var modelData
                    text: "•  " + modelData.message
                    color: win.riskTone(modelData.level)
                    font.pixelSize: 13
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                radius: 10
                color: t.surface2
                border.color: draftEdit.activeFocus ? t.accent : t.border
                ScrollView {
                    anchors.fill: parent
                    anchors.margins: 4
                    TextArea {
                        id: draftEdit
                        objectName: "draftEdit"
                        text: win.draft ? win.draft.toml : ""
                        color: t.text
                        font.family: "monospace"
                        font.pixelSize: 12
                        wrapMode: TextArea.NoWrap
                        background: null
                        selectByMouse: true
                    }
                }
            }
            Muted { text: "You can edit the TOML. It is validated again when added."; Layout.fillWidth: true }
            RowLayout {
                Layout.fillWidth: true
                Check { id: draftAck; visible: win.draft !== null && win.draft.high; text: "I've reviewed the high-risk items" }
                Item { Layout.fillWidth: true }
                Btn { text: "Discard"; onClicked: { draftPopup.close(); win.draft = null } }
                Btn {
                    text: "Grant & add"
                    enabled: !(win.draft && win.draft.high) || draftAck.checked
                    onClicked: { backend.addDraft(draftEdit.text, false); draftPopup.close(); win.page = "automations" }
                }
                Btn {
                    primary: true
                    text: "Grant & enable"
                    enabled: !(win.draft && win.draft.high) || draftAck.checked
                    onClicked: { backend.addDraft(draftEdit.text, true); draftPopup.close(); win.page = "automations" }
                }
            }
        }
    }

    // ------------------------------------------------------------ toasts

    Rectangle {
        id: toastBox
        property string kind: "ok"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 24
        implicitWidth: Math.min(win.width - 80, toastText.implicitWidth + 40)
        implicitHeight: toastText.implicitHeight + 22
        radius: 10
        color: win.dark ? "#F4EEE6" : "#241E18"
        opacity: 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 180 } }
        Row {
            anchors.centerIn: parent
            spacing: 8
            Rectangle { width: 8; height: 8; radius: 4; anchors.verticalCenter: parent.verticalCenter; color: toastBox.kind === "error" ? t.bad : toastBox.kind === "ok" ? t.good : t.accent }
            Text { id: toastText; color: win.dark ? "#1F1A15" : "#F7F4EF"; font.pixelSize: 13; text: "" }
        }
        Timer { id: toastTimer; interval: 3500; onTriggered: toastBox.opacity = 0 }
    }

    Connections {
        target: backend
        function onToast(kind, message) {
            toastBox.kind = kind
            toastText.text = message
            toastBox.opacity = 1
            toastTimer.restart()
        }
        function onDraftReady(d) {
            win.draft = d
            draftAck.checked = false
            draftPopup.open()
        }
        function onDraftFailed(message) {
            toastBox.kind = "error"
            toastText.text = message
            toastBox.opacity = 1
            toastTimer.restart()
        }
    }
}
