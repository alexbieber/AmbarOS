import QtQuick

Rectangle {
    id: root
    color: "#15120F"
    property int stage   // set by ksplash, 1 … 6

    Image {
        id: mark
        anchors.centerIn: parent
        anchors.verticalCenterOffset: -height * 0.25
        width: Math.round(Math.min(root.width, root.height) * 0.09)
        height: width
        sourceSize.width: width * 2
        sourceSize.height: height * 2
        source: "images/ambar-mark.svg"
        smooth: true
        opacity: 0
        Component.onCompleted: opacity = 1
        Behavior on opacity { NumberAnimation { duration: 600; easing.type: Easing.OutCubic } }
    }

    Rectangle {
        id: track
        anchors.top: mark.bottom
        anchors.topMargin: mark.height * 0.7
        anchors.horizontalCenter: parent.horizontalCenter
        width: mark.width * 1.6
        height: 4
        radius: 2
        color: "#3B332A"
        Rectangle {
            height: parent.height
            radius: 2
            color: "#F2A43A"
            width: parent.width * Math.min(1, Math.max(0.08, root.stage / 6))
            Behavior on width { NumberAnimation { duration: 400; easing.type: Easing.OutCubic } }
        }
    }
}
