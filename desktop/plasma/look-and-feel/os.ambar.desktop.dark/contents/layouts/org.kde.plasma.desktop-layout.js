// Ambar desktop layout (Plasma 6), modelled on macOS:
//   top menu bar:  [Ambar menu] [App name] [App's menus …]   …   [status icons] [Control] [Search] [clock]
//   bottom dock:   [Launchpad] [pinned + running apps] | [Trash]
// Applied for every new user (kdeglobals LookAndFeelPackage), or by
// `plasma-apply-lookandfeel --apply os.ambar.desktop --resetLayout`.

var known = knownWidgetTypes;
function has(id) { return known.indexOf(id) !== -1; }
function add(panel, id) { return has(id) ? panel.addWidget(id) : null; }
function app(ids) {                       // first installed .desktop of a list
    for (var i = 0; i < ids.length; i++) {
        if (applicationExists(ids[i])) return "applications:" + ids[i];
    }
    return null;
}

// ------------------------------------------------------------ menu bar
var bar = new Panel;
bar.location = "top";
bar.height = Math.round(gridUnit * 1.5);
bar.floating = false;

var menu = add(bar, "os.ambar.menu") || add(bar, "org.kde.plasma.kickoff");
if (menu && menu.type === "org.kde.plasma.kickoff") {
    menu.currentConfigGroup = ["General"];
    menu.writeConfig("icon", "ambar-logo");
}
add(bar, "os.ambar.appname");
add(bar, "org.kde.plasma.appmenu");
add(bar, "org.kde.plasma.panelspacer");
var tray = add(bar, "org.kde.plasma.systemtray");
if (tray) {
    // Like macOS: only status that matters in the bar. Sound, brightness and displays live in
    // the Control Center, so they're left out here. "knownItems" lists every tray widget, so
    // Plasma doesn't switch the others back on as "new".
    tray.currentConfigGroup = ["General"];
    tray.writeConfig("extraItems", [
        "org.kde.plasma.networkmanagement", "org.kde.plasma.battery", "org.kde.plasma.bluetooth",
        "org.kde.plasma.notifications", "org.kde.plasma.devicenotifier", "org.kde.plasma.mediacontroller",
        "org.kde.plasma.keyboardlayout", "org.kde.plasma.manage-inputmethod", "org.kde.plasma.cameraindicator",
        "org.kde.plasma.printmanager", "org.kde.kdeconnect"].join(","));
    tray.writeConfig("knownItems", [
        "org.kde.kscreen", "org.kde.plasma.addons.katesessions", "org.kde.plasma.battery", "org.kde.plasma.bluetooth",
        "org.kde.plasma.brightness", "org.kde.plasma.cameraindicator", "org.kde.plasma.clipboard",
        "org.kde.plasma.devicenotifier", "org.kde.plasma.diskquota", "org.kde.plasma.keyboardindicator",
        "org.kde.plasma.keyboardlayout", "org.kde.plasma.manage-inputmethod", "org.kde.plasma.mediacontroller",
        "org.kde.plasma.networkmanagement", "org.kde.plasma.notifications", "org.kde.plasma.printmanager",
        "org.kde.plasma.trash", "org.kde.plasma.volume", "org.kde.plasma.weather", "org.kde.kdeconnect",
        "org.kde.kupapplet", "org.kde.plasma.vault", "org.kde.plasma.wacomtablet"].join(","));
}
add(bar, "os.ambar.controlcenter");
var clock = add(bar, "org.kde.plasma.digitalclock");
if (clock) {
    clock.currentConfigGroup = ["Appearance"];
    clock.writeConfig("showDate", "true");
    clock.writeConfig("dateDisplayFormat", "BesideTime");
    clock.writeConfig("dateFormat", "custom");
    clock.writeConfig("customDateFormat", "ddd d MMM");
    clock.writeConfig("use24hFormat", "0");
}

// ------------------------------------------------------------ dock
var dock = new Panel;
dock.location = "bottom";
dock.height = Math.round(gridUnit * 3.4);
dock.alignment = "center";
dock.lengthMode = "fit";
dock.floating = true;
dock.hiding = "dodgewindows";

var launchpad = add(dock, "os.ambar.launchpad");
if (!launchpad && (launchpad = add(dock, "org.kde.plasma.kickerdash"))) {  // Plasma's own, if ours is missing
    launchpad.currentConfigGroup = ["General"];
    launchpad.writeConfig("icon", "view-app-grid-symbolic");
    launchpad.writeConfig("showRecentApps", "false");   // open on the full app grid
    launchpad.writeConfig("showRecentDocs", "false");
    launchpad.writeConfig("alphaSort", "true");
    launchpad.writeConfig("showActionButtonCaptions", "false");
}
var tasks = add(dock, "org.kde.plasma.icontasks");
if (tasks) {
    var pinned = [
        app(["org.kde.dolphin.desktop"]),                                   // Finder
        app(["firefox.desktop", "org.kde.falkon.desktop"]),                // Browser
        app(["org.kde.kmail2.desktop", "org.kde.kontact.desktop"]),        // Mail
        app(["org.kde.merkuro.calendar.desktop", "org.kde.korganizer.desktop"]),  // Calendar
        app(["org.kde.marknote.desktop"]),                                  // Notes
        app(["org.kde.elisa.desktop"]),                                     // Music
        app(["org.kde.gwenview.desktop"]),                                  // Photos
        app(["libreoffice-writer.desktop"]),                                // Documents
        app(["org.kde.konsole.desktop"]),                                   // Terminal
        app(["org.kde.discover.desktop"]),                                  // Ambar Store
        app(["os.ambar.AutomationCenter.desktop"]),                         // Automations
        app(["systemsettings.desktop"])                                     // Settings
    ].filter(function (x) { return x !== null; });
    tasks.currentConfigGroup = ["General"];
    tasks.writeConfig("launchers", pinned.join(","));
    tasks.writeConfig("showOnlyCurrentDesktop", "false");
    tasks.writeConfig("indicateAudioStreams", "true");
    tasks.writeConfig("iconSpacing", "1");
}
add(dock, "org.kde.plasma.marginsseparator");
add(dock, "org.kde.plasma.trash");

// ------------------------------------------------------------ desktop
var all = desktops();
for (var i = 0; i < all.length; i++) {
    var d = all[i];
    d.wallpaperPlugin = "org.kde.image";
    d.currentConfigGroup = ["Wallpaper", "org.kde.image", "General"];
    d.writeConfig("Image", "file:///usr/share/wallpapers/Ambar/");
}
