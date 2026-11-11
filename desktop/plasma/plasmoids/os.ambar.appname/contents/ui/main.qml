/*
 * The name of the app in front, in bold, next to the Ambar menu (like macOS).
 * Shows "Desktop" when no window is active.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PC3
import org.kde.taskmanager as TaskManager
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root
    preferredRepresentation: fullRepresentation
    Plasmoid.backgroundHints: "NoBackground"

    TaskManager.TasksModel {
        id: tasks
        sortMode: TaskManager.TasksModel.SortDisabled
        groupMode: TaskManager.TasksModel.GroupDisabled
    }

    readonly property string appName: {
        const i = tasks.activeTask;
        if (!i || !i.valid) return i18n("Desktop");
        const name = tasks.data(i, TaskManager.AbstractTasksModel.AppName);
        return name && name.length ? name : tasks.data(i, Qt.DisplayRole);
    }

    fullRepresentation: PC3.Label {
        Layout.minimumWidth: implicitWidth
        Layout.maximumWidth: Kirigami.Units.gridUnit * 14
        leftPadding: Kirigami.Units.smallSpacing
        rightPadding: Kirigami.Units.largeSpacing
        text: root.appName
        font.weight: Font.Bold
        elide: Text.ElideRight
        verticalAlignment: Text.AlignVCenter
    }
}
