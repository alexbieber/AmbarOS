#!/usr/bin/env python3
"""Generates the Ambar Plasma theme from one palette.

Outputs (committed, but always reproducible from this script):
  color-schemes/AmbarLight.colors, AmbarDark.colors
  look-and-feel/os.ambar.desktop{,.dark}/...
  wallpapers/Ambar/...  (light + dark PNGs, original artwork drawn here)

Run: python3 desktop/plasma/generate.py
"""

from __future__ import annotations

import json
import math
import os
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# One palette for the desktop and the Automation Center (see ambar_center/qml/Main.qml).
PALETTE = {
    "light": {
        "window": "#EFE9E0", "view": "#FFFFFF", "alt": "#F7F4EF", "button": "#F4EFE8",
        "header": "#EFE9E0", "text": "#1F1A15", "muted": "#6F655A", "accent": "#B35C05",
        "on_accent": "#FFFFFF", "link": "#A35306", "visited": "#8A4A12",
        "good": "#21805A", "warn": "#9C6500", "bad": "#BE3A2E", "tooltip": "#241E18",
        "tooltip_text": "#F7F4EF", "complementary": "#1C1814", "complementary_text": "#F4EEE6",
    },
    "dark": {
        "window": "#1C1814", "view": "#231E19", "alt": "#2C261F", "button": "#2C261F",
        "header": "#1C1814", "text": "#F4EEE6", "muted": "#A89D90", "accent": "#F2A43A",
        "on_accent": "#1B1209", "link": "#F2A43A", "visited": "#D48A2E",
        "good": "#62C796", "warn": "#E8B54E", "bad": "#F07B6F", "tooltip": "#F4EEE6",
        "tooltip_text": "#1F1A15", "complementary": "#15120F", "complementary_text": "#F4EEE6",
    },
}


def rgb(hex_: str) -> tuple[int, int, int]:
    h = hex_.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def kde(hex_: str) -> str:
    return ",".join(str(c) for c in rgb(hex_))


def luminance(hex_: str) -> float:
    def ch(c: int) -> float:
        c = c / 255
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = rgb(hex_)
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a: str, b: str) -> float:
    la, lb = sorted([luminance(a), luminance(b)], reverse=True)
    return (la + 0.05) / (lb + 0.05)


def color_group(p: dict, bg: str, alt: str, fg: str, inactive: str | None = None) -> dict:
    return {
        "BackgroundNormal": kde(bg),
        "BackgroundAlternate": kde(alt),
        "DecorationFocus": kde(p["accent"]),
        "DecorationHover": kde(p["accent"]),
        "ForegroundNormal": kde(fg),
        "ForegroundInactive": kde(inactive or p["muted"]),
        "ForegroundActive": kde(p["accent"]),
        "ForegroundLink": kde(p["link"]),
        "ForegroundVisited": kde(p["visited"]),
        "ForegroundNegative": kde(p["bad"]),
        "ForegroundNeutral": kde(p["warn"]),
        "ForegroundPositive": kde(p["good"]),
    }


def color_scheme(mode: str) -> str:
    p = PALETTE[mode]
    name = f"Ambar{mode.capitalize()}"
    groups = {
        "General": {"ColorScheme": name, "Name": f"Ambar {mode.capitalize()}",
                    "shadeSortColumn": "true", "AccentColor": kde(p["accent"])},
        "KDE": {"contrast": "4"},
        "ColorEffects:Disabled": {"Color": kde(p["window"]), "ColorAmount": "0", "ColorEffect": "0",
                                  "ContrastAmount": "0.65", "ContrastEffect": "1",
                                  "IntensityAmount": "0.1", "IntensityEffect": "2"},
        "ColorEffects:Inactive": {"ChangeSelectionColor": "true", "Color": kde(p["muted"]),
                                  "ColorAmount": "0.025", "ColorEffect": "2", "ContrastAmount": "0.1",
                                  "ContrastEffect": "2", "Enable": "false", "IntensityAmount": "0",
                                  "IntensityEffect": "0"},
        "Colors:Window": color_group(p, p["window"], p["alt"], p["text"]),
        "Colors:View": color_group(p, p["view"], p["alt"], p["text"]),
        "Colors:Button": color_group(p, p["button"], p["alt"], p["text"]),
        "Colors:Header": color_group(p, p["header"], p["alt"], p["text"]),
        "Colors:Header][Inactive": color_group(p, p["header"], p["alt"], p["muted"]),
        "Colors:Selection": color_group(p, p["accent"], p["accent"], p["on_accent"], p["on_accent"]),
        "Colors:Tooltip": color_group(p, p["tooltip"], p["tooltip"], p["tooltip_text"], p["tooltip_text"]),
        "Colors:Complementary": color_group(p, p["complementary"], p["complementary"],
                                            p["complementary_text"]),
        "WM": {"activeBackground": kde(p["header"]), "activeBlend": kde(p["header"]),
               "activeForeground": kde(p["text"]), "inactiveBackground": kde(p["header"]),
               "inactiveBlend": kde(p["header"]), "inactiveForeground": kde(p["muted"])},
    }
    out = []
    for g, kv in groups.items():
        out.append(f"[{g}]")
        out.extend(f"{k}={v}" for k, v in kv.items())
        out.append("")
    return "\n".join(out)


LAYOUT_JS = r"""// Ambar desktop layout (Plasma 6), modelled on macOS:
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

var launchpad = add(dock, "org.kde.plasma.kickerdash");
if (launchpad) {
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
        app(["org.kde.korganizer.desktop"]),                                // Calendar
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
"""


def defaults(mode: str) -> str:
    scheme = f"Ambar{mode.capitalize()}"
    dark = mode == "dark"
    return f"""[kdeglobals][General]
ColorScheme={scheme}
AccentColor={kde(PALETTE[mode]["accent"])}
font=Inter,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1
menuFont=Inter,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1
toolBarFont=Inter,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1
smallestReadableFont=Inter,8,-1,5,400,0,0,0,0,0,0,0,0,0,0,1

[kdeglobals][WM]
activeFont=Inter,10,-1,5,600,0,0,0,0,0,0,0,0,0,0,1

[kdeglobals][KDE]
widgetStyle=Breeze
SingleClick=false

[kdeglobals][Icons]
Theme={"Papirus-Dark" if dark else "Papirus"}

[plasmarc][Theme]
name={"breeze-dark" if dark else "default"}

[kwinrc][org.kde.kdecoration2]
library=org.kde.kwin.aurorae.v2
theme=__aurorae__svg__{"AmbarDark" if dark else "Ambar"}
ButtonsOnLeft=XIA
ButtonsOnRight=

[KSplash][KSplash]
Theme=org.kde.breeze.desktop

[kcminputrc][Mouse]
cursorTheme=breeze_cursors
"""


def lnf_metadata(mode: str) -> dict:
    suffix = "" if mode == "light" else ".dark"
    title = "Ambar" if mode == "light" else "Ambar Dark"
    return {
        "KPackageStructure": "Plasma/LookAndFeel",
        "KPlugin": {
            "Id": f"os.ambar.desktop{suffix}",
            "Name": title,
            "Description": f"{title}: warm, minimal desktop with a top bar and a floating dock",
            "Authors": [{"Name": "Ambar Singh"}],
            "Category": "",
            "License": "Apache-2.0",
            "Version": "0.1.0",
            "Website": "https://github.com/alexbieber/AmbarOS",
        },
        "X-Plasma-APIVersion": "2",
        "X-Plasma-MainScript": "defaults",
    }


def draw_wallpaper(out: Path, w: int, h: int, mode: str) -> None:
    """Original artwork: a low sun over layered warm ridges."""
    from PySide6.QtCore import QPointF, Qt
    from PySide6.QtGui import QColor, QGuiApplication, QImage, QLinearGradient, QPainter, QPainterPath, QRadialGradient

    QGuiApplication.instance() or QGuiApplication(sys.argv[:1])
    img = QImage(w, h, QImage.Format.Format_RGB32)
    p = QPainter(img)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    if mode == "light":
        sky = [(0.0, "#FDF3E4"), (0.55, "#F7D9AE"), (1.0, "#EFA75A")]
        sun = ("#FFF6E8", "#F6B866")
        ridges = ["#E9964A", "#D9772A", "#BF5E1B", "#9C4612"]
    else:
        sky = [(0.0, "#120E0B"), (0.6, "#2A1A0E"), (1.0, "#5A3212")]
        sun = ("#F7B65A", "#B4621A")
        ridges = ["#3A2210", "#2C190C", "#201209", "#160C06"]
    g = QLinearGradient(0, 0, 0, h)
    for stop, c in sky:
        g.setColorAt(stop, QColor(c))
    p.fillRect(0, 0, w, h, g)
    # Sun with soft glow.
    cx, cy, r = w * 0.66, h * 0.56, h * 0.16
    glow = QRadialGradient(QPointF(cx, cy), r * 3.2)
    gc = QColor(sun[1]); gc.setAlpha(110 if mode == "light" else 90)
    glow.setColorAt(0, gc)
    gc2 = QColor(sun[1]); gc2.setAlpha(0)
    glow.setColorAt(1, gc2)
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(glow)
    p.drawEllipse(QPointF(cx, cy), r * 3.2, r * 3.2)
    sg = QRadialGradient(QPointF(cx - r * 0.3, cy - r * 0.3), r * 1.3)
    sg.setColorAt(0, QColor(sun[0]))
    sg.setColorAt(1, QColor(sun[1]))
    p.setBrush(sg)
    p.drawEllipse(QPointF(cx, cy), r, r)
    # Ridges: smooth layered sine curves.
    for i, col in enumerate(ridges):
        base = h * (0.62 + i * 0.1)
        amp = h * (0.05 - i * 0.008)
        ridge = QPainterPath(QPointF(0, h))
        steps = 120
        for s in range(steps + 1):
            x = w * s / steps
            y = base + amp * math.sin(2 * math.pi * (s / steps) * (1.2 + i * 0.35) + i * 1.7) \
                + amp * 0.45 * math.sin(2 * math.pi * (s / steps) * (3.1 + i) + i)
            ridge.lineTo(QPointF(x, y))
        ridge.lineTo(QPointF(w, h))
        ridge.closeSubpath()
        p.setBrush(QColor(col))
        p.drawPath(ridge)
    p.end()
    out.parent.mkdir(parents=True, exist_ok=True)
    if not img.save(str(out)):
        raise RuntimeError(f"could not write {out}")


def main() -> int:
    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    cs = HERE / "color-schemes"
    cs.mkdir(exist_ok=True)
    for mode in ("light", "dark"):
        (cs / f"Ambar{mode.capitalize()}.colors").write_text(color_scheme(mode))

    for mode in ("light", "dark"):
        suffix = "" if mode == "light" else ".dark"
        root = HERE / "look-and-feel" / f"os.ambar.desktop{suffix}"
        if root.exists():
            shutil.rmtree(root)
        (root / "contents" / "layouts").mkdir(parents=True)
        (root / "metadata.json").write_text(json.dumps(lnf_metadata(mode), indent=2) + "\n")
        (root / "contents" / "defaults").write_text(defaults(mode))
        (root / "contents" / "layouts" / "org.kde.plasma.desktop-layout.js").write_text(LAYOUT_JS)

    wp = HERE / "wallpapers" / "Ambar"
    (wp / "contents" / "images").mkdir(parents=True, exist_ok=True)
    (wp / "contents" / "images_dark").mkdir(parents=True, exist_ok=True)
    (wp / "metadata.json").write_text(json.dumps({
        "KPackageStructure": "Wallpaper/Images",
        "KPlugin": {"Id": "Ambar", "Name": "Ambar", "Authors": [{"Name": "Ambar Singh"}],
                    "License": "CC-BY-SA-4.0"},
    }, indent=2) + "\n")
    w, h = 3840, 2160
    draw_wallpaper(wp / "contents" / "images" / f"{w}x{h}.png", w, h, "light")
    draw_wallpaper(wp / "contents" / "images_dark" / f"{w}x{h}.png", w, h, "dark")
    for mode, dirname in (("light", "images"), ("dark", "images_dark")):
        src = wp / "contents" / dirname / f"{w}x{h}.png"
        from PySide6.QtGui import QImage
        from PySide6.QtCore import Qt
        prev = QImage(str(src)).scaled(1920, 1080, Qt.AspectRatioMode.KeepAspectRatio,
                                       Qt.TransformationMode.SmoothTransformation)
        suffix = "" if mode == "light" else ".dark"
        lnf = HERE / "look-and-feel" / f"os.ambar.desktop{suffix}" / "contents" / "previews"
        lnf.mkdir(parents=True, exist_ok=True)
        prev.scaled(480, 270, Qt.AspectRatioMode.KeepAspectRatio,
                    Qt.TransformationMode.SmoothTransformation).save(str(lnf / "preview.png"))
        prev.save(str(lnf / "fullscreenpreview.jpg"), quality=85)

    # Contrast report (WCAG AA needs ≥ 4.5 for body text).
    for mode, p in PALETTE.items():
        for fg, bg in [("text", "window"), ("text", "view"), ("muted", "view"),
                       ("on_accent", "accent"), ("tooltip_text", "tooltip")]:
            print(f"{mode:5} {fg:>12} on {bg:<8} {contrast(p[fg], p[bg]):5.2f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
