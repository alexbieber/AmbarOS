"""Static validation of the Plasma theme files (no Plasma session needed)."""
import configparser
import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
DESKTOP = HERE.parent
sys.path.insert(0, str(HERE))
import generate  # noqa: E402

REQUIRED_GROUPS = ["Colors:Window", "Colors:View", "Colors:Button", "Colors:Selection",
                   "Colors:Tooltip", "Colors:Complementary", "Colors:Header", "General", "WM"]
REQUIRED_KEYS = ["BackgroundNormal", "BackgroundAlternate", "ForegroundNormal", "ForegroundInactive",
                 "ForegroundActive", "ForegroundLink", "ForegroundNegative", "ForegroundNeutral",
                 "ForegroundPositive", "ForegroundVisited", "DecorationFocus", "DecorationHover"]


def ini(path):
    cp = configparser.ConfigParser(interpolation=None, strict=True)
    cp.optionxform = str
    cp.read(path)
    return cp


@pytest.mark.parametrize("mode", ["Light", "Dark"])
def test_color_schemes_are_complete_and_current(mode):
    path = HERE / "color-schemes" / f"Ambar{mode}.colors"
    assert path.read_text() == generate.color_scheme(mode.lower()), "re-run generate.py"
    cp = ini(path)
    for g in REQUIRED_GROUPS:
        assert cp.has_section(g), g
    for g in [s for s in cp.sections() if s.startswith("Colors:")]:
        for k in REQUIRED_KEYS:
            v = cp[g][k].split(",")
            assert len(v) == 3 and all(0 <= int(x) <= 255 for x in v), (g, k)
    assert cp["General"]["ColorScheme"] == f"Ambar{mode}"


@pytest.mark.parametrize("mode", ["light", "dark"])
def test_text_contrast_meets_wcag_aa(mode):
    p = generate.PALETTE[mode]
    for fg, bg in [("text", "window"), ("text", "view"), ("muted", "view"), ("muted", "window"),
                   ("on_accent", "accent"), ("tooltip_text", "tooltip"), ("bad", "view"),
                   ("good", "view"), ("warn", "view")]:
        assert generate.contrast(p[fg], p[bg]) >= 4.5, (mode, fg, bg)


@pytest.mark.parametrize("pkg", ["os.ambar.desktop", "os.ambar.desktop.dark"])
def test_look_and_feel_packages(pkg):
    root = HERE / "look-and-feel" / pkg
    meta = json.loads((root / "metadata.json").read_text())
    assert meta["KPlugin"]["Id"] == pkg
    assert meta["KPackageStructure"] == "Plasma/LookAndFeel"
    defaults = (root / "contents" / "defaults").read_text()
    scheme = "AmbarDark" if pkg.endswith("dark") else "AmbarLight"
    assert f"ColorScheme={scheme}" in defaults
    assert (root / "contents" / "previews" / "preview.png").stat().st_size > 1000
    layout = root / "contents" / "layouts" / "org.kde.plasma.desktop-layout.js"
    if shutil.which("node"):
        r = subprocess.run(["node", "--check", str(layout)], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr
    assert "os.ambar.AutomationCenter.desktop" in layout.read_text()


def test_wallpaper_package():
    root = HERE / "wallpapers" / "Ambar"
    meta = json.loads((root / "metadata.json").read_text())
    assert meta["KPlugin"]["Id"] == "Ambar"
    for d in ["images", "images_dark"]:
        f = root / "contents" / d / "3840x2160.png"
        assert f.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"


def test_krunner_and_autostart_entries():
    k = ini(DESKTOP / "share" / "krunner" / "dbusplugins" / "os.ambar.krunner.desktop")["Desktop Entry"]
    svc = ini(DESKTOP / "share" / "dbus-1" / "services" / "os.ambar.KRunner.service")["D-BUS Service"]
    assert k["X-Plasma-API"] == "DBus"
    assert k["X-Plasma-DBusRunner-Service"] == svc["Name"] == "os.ambar.KRunner"
    assert k["X-Plasma-DBusRunner-Path"] == "/runner"
    sys.path.insert(0, str(DESKTOP / "automation-center"))
    sys.path.insert(0, str(DESKTOP.parent / "sdk" / "python"))
    from ambar_center import krunner
    assert (krunner.BUS_NAME, krunner.OBJECT_PATH) == (svc["Name"], k["X-Plasma-DBusRunner-Path"])
    pyproject = (DESKTOP / "automation-center" / "pyproject.toml").read_text()
    assert 'ambar-krunner = "ambar_center.krunner:main"' in pyproject
    assert svc["Exec"] == "/usr/bin/ambar-krunner"
    a = DESKTOP / "share" / "autostart" / "os.ambar.Welcome.desktop"
    assert ini(a)["Desktop Entry"]["Exec"] == "ambar-center --welcome"
    if shutil.which("desktop-file-validate"):
        r = subprocess.run(["desktop-file-validate", str(a)], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout


def test_desktop_entry():
    f = DESKTOP / "share" / "applications" / "os.ambar.AutomationCenter.desktop"
    cp = ini(f)
    e = cp["Desktop Entry"]
    assert e["Type"] == "Application" and e["Exec"] == "ambar-center"
    assert (DESKTOP / "share" / "icons" / "hicolor" / "scalable" / "apps" / f"{e['Icon']}.svg").exists()
    if shutil.which("desktop-file-validate"):
        r = subprocess.run(["desktop-file-validate", str(f)], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout + r.stderr
