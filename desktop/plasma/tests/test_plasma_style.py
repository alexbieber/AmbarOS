"""The Ambar Plasma Style, terminal colours, splash and menu-bar layout (no Plasma session needed)."""
from __future__ import annotations

import configparser
import gzip
import json
import os
import re
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
STYLE = HERE / "desktoptheme" / "ambar"
sys.path.insert(0, str(HERE))
import desktoptheme  # noqa: E402
import generate  # noqa: E402

FRAME = ["topleft", "top", "topright", "left", "center", "right", "bottomleft", "bottom", "bottomright"]
SHADOW = [f"shadow-{p}" for p in FRAME if p != "center"] + ["shadow-center"]
HINTS = [f"hint-{s}-margin" for s in ("top", "bottom", "left", "right")]


def ids(rel: str) -> set[str]:
    return set(re.findall(r'id="([^"]+)"', gzip.open(STYLE / rel).read().decode()))


def test_style_is_generated_and_current(tmp_path, monkeypatch):
    monkeypatch.setattr(desktoptheme, "OUT", tmp_path / "ambar")
    desktoptheme.main()
    fresh = {p.relative_to(tmp_path / "ambar"): p.read_bytes() for p in (tmp_path / "ambar").rglob("*") if p.is_file()}
    committed = {p.relative_to(STYLE): p.read_bytes() for p in STYLE.rglob("*") if p.is_file()}
    assert fresh == committed, "run python3 desktop/plasma/desktoptheme.py"


def test_metadata_and_follows_the_colour_scheme():
    meta = json.loads((STYLE / "metadata.json").read_text())
    assert meta["KPlugin"]["Id"] == "ambar" and meta["X-Plasma-API"] == "5.0"
    assert not (STYLE / "colors").exists(), "no colours file: the style follows Ambar Light/Dark"
    rc = (STYLE / "plasmarc").read_text()
    assert "[ContrastEffect]" in rc and "enabled=true" in rc


@pytest.mark.parametrize("variant", ["", "translucent/", "solid/", "opaque/"])
@pytest.mark.parametrize("name", ["widgets/panel-background", "dialogs/background", "widgets/tooltip"])
def test_frames_have_every_piece(variant, name):
    got = ids(f"{variant}{name}.svgz")
    missing = [e for e in FRAME + HINTS + ["mask-" + p for p in FRAME] if e not in got]
    assert not missing, missing
    if variant != "opaque/":
        assert not [e for e in SHADOW if e not in got], "translucent styles need shadows"
        assert all(f"shadow-hint-{s}-margin" in got for s in ("top", "bottom", "left", "right"))


def test_svgs_parse_and_use_the_colour_scheme():
    import xml.dom.minidom
    for p in STYLE.rglob("*.svgz"):
        text = gzip.open(p).read().decode()
        xml.dom.minidom.parseString(text)
        assert 'id="current-color-scheme"' in text and "ColorScheme-Background" in text or "tasks" in p.name, p


def test_dock_highlights_are_pills_for_every_state_and_edge():
    got = ids("widgets/tasks.svgz")
    for state in ("normal", "focus", "hover", "attention", "minimized", "progress"):
        for loc in ("", "north-", "west-", "east-"):
            missing = [p for p in FRAME if f"{loc}{state}-{p}" not in got]
            assert not missing, (loc, state, missing)
    assert all(f"normal-{h}" in got for h in HINTS)


def test_pieces_keep_their_size_when_partly_empty():
    """Plasma sizes a piece by what's drawn; empty strips must still count (the filler rect)."""
    from PySide6.QtSvg import QSvgRenderer
    r = QSvgRenderer(gzip.open(STYLE / "translucent/dialogs/background.svgz").read())
    margin = r.boundsOnElement("shadow-hint-top-margin").height()
    radius = r.boundsOnElement("topleft").height()
    assert r.boundsOnElement("shadow-top").height() == pytest.approx(margin + radius, abs=0.01)
    assert r.boundsOnElement("shadow-bottom").height() == pytest.approx(margin + radius, abs=0.01)
    assert radius == desktoptheme.DIALOG_R


def test_look_and_feel_uses_the_style_and_the_ambar_splash():
    for pkg in ("os.ambar.desktop", "os.ambar.desktop.dark"):
        root = HERE / "look-and-feel" / pkg / "contents"
        d = (root / "defaults").read_text()
        assert "[plasmarc][Theme]\nname=ambar" in d
        assert f"[KSplash][KSplash]\nTheme={pkg}" in d
        assert (root / "splash" / "Splash.qml").exists() and (root / "splash" / "images" / "ambar-mark.svg").exists()
    hook = (HERE.parents[1] / "os/ubuntu/hooks/20-desktop.chroot").read_text()
    assert "[Theme]\nname=ambar" in hook and "Theme=os.ambar.desktop" in hook
    assert "DefaultProfile=Ambar.profile" in hook
    assert "Font=Inter" in hook.split("90-ambar.conf", 1)[1], "login screen font"


def test_splash_loads(tmp_path):
    qml = HERE / "look-and-feel/os.ambar.desktop/contents/splash/Splash.qml"
    script = tmp_path / "s.py"
    script.write_text(textwrap.dedent(f"""
        import os; os.environ["QT_QPA_PLATFORM"] = "offscreen"
        from PySide6.QtCore import QUrl
        from PySide6.QtGui import QGuiApplication
        from PySide6.QtQuick import QQuickView
        app = QGuiApplication([]); v = QQuickView(); v.setSource(QUrl.fromLocalFile({str(qml)!r}))
        assert not v.errors(), v.errors()
        v.rootObject().setProperty("stage", 3)
        print("CHECK_OK")
    """))
    r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=60)
    assert "CHECK_OK" in r.stdout, r.stdout + r.stderr


@pytest.mark.parametrize("mode", ["light", "dark"])
def test_terminal_colours_are_readable(mode):
    t = generate.TERMINAL[mode]
    for c in t["ansi"][1:] + [t["fg"]]:
        assert generate.contrast(c, t["bg"]) >= 4.5, (mode, c)
    cp = configparser.ConfigParser(interpolation=None)
    cp.optionxform = str
    cp.read(HERE / "konsole" / f"Ambar{mode.capitalize()}.colorscheme")
    assert {"Background", "Foreground", "General"} <= set(cp.sections())
    assert all(f"Color{i}" in cp and f"Color{i}Intense" in cp for i in range(8))
    profile = (HERE / "konsole" / "Ambar.profile").read_text()
    assert "ColorScheme=AmbarDark" in profile and "JetBrains Mono" in profile


def test_menu_bar_tray_is_trimmed():
    js = generate.LAYOUT_JS
    extra = re.search(r'"extraItems", \[(.*?)\]', js, re.S).group(1)
    known = re.search(r'"knownItems", \[(.*?)\]', js, re.S).group(1)
    assert "networkmanagement" in extra and "battery" in extra and "notifications" in extra
    for gone in ("org.kde.plasma.volume", "org.kde.plasma.brightness", "org.kde.kscreen"):
        assert gone not in extra and gone in known, gone
    assert set(re.findall(r'"([\w.-]+)"', extra)) <= set(re.findall(r'"([\w.-]+)"', known))


def test_control_center_modules():
    qml = (HERE / "plasmoids/os.ambar.controlcenter/contents/ui/main.qml").read_text()
    for part in ("component Module", "component ToggleRow", "component Square", "component SliderModule"):
        assert part in qml, part
    for key in ("wifi", "bluetooth", "airplane", "dnd", "dark", "nightlight", "brightness", "volume"):
        assert f'key: "{key}"' in qml, key
    assert "panel." not in qml, "inline components reach shared state through `root`, which is known to work"
