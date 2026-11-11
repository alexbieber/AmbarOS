"""ambar-icons: every visible app gets the same squircle tile; small sizes and folders are right."""
from __future__ import annotations

import importlib.machinery
import importlib.util
import os
import sys
from pathlib import Path

import pytest

pytest.importorskip("PySide6")
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
loader = importlib.machinery.SourceFileLoader("ambar_icons", str(HERE / "ambar-icons"))
icons = importlib.util.module_from_spec(importlib.util.spec_from_loader("ambar_icons", loader))
sys.modules["ambar_icons"] = icons
loader.exec_module(icons)

GLYPH = '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><circle cx="32" cy="32" r="18" fill="#2f80ed"/></svg>'
TILE = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect x="4" y="4" width="56" height="56" rx="6" '
        'fill="#4caf50"/></svg>')


def desktop(d: Path, name: str, icon: str, hidden: bool = False) -> None:
    d.mkdir(parents=True, exist_ok=True)
    (d / f"{name}.desktop").write_text(f"[Desktop Entry]\nType=Application\nName={name}\nExec={name}\nIcon={icon}\n"
                                       + ("NoDisplay=true\n" if hidden else ""))


@pytest.fixture
def root(tmp_path):
    r = tmp_path / "root"
    for theme in ("Papirus", "Papirus-Dark"):
        apps = r / f"usr/share/icons/{theme}/64x64/apps"
        apps.mkdir(parents=True)
        for n in ("glyphapp", "tileapp", "hiddenapp", "overridden", "preferences-desktop-font"):
            (apps / f"{n}.svg").write_text(TILE if n == "tileapp" else GLYPH)
        small = r / f"usr/share/icons/{theme}/16x16/apps"
        small.mkdir(parents=True)
        (small / "glyphapp.svg").write_text(GLYPH)
        places = r / f"usr/share/icons/{theme}/64x64/places"
        places.mkdir(parents=True)
        for n in ("folder-orange", "folder-orange-documents", "user-orange-home", "folder-blue"):
            (places / f"{n}.svg").write_text(GLYPH)
    a = r / "usr/share/applications"
    desktop(a, "glyphapp", "glyphapp")
    desktop(a, "tileapp", "tileapp")
    desktop(a, "hiddenapp", "hiddenapp", hidden=True)
    desktop(a, "overridden", "overridden")
    desktop(r / "usr/local/share/applications", "overridden", "overridden", hidden=True)  # hidden by an override
    desktop(a, "fonts", "com.github.FontManager.FontManager")  # its own icon is replaced (alias)
    return r


def test_builds_both_themes_for_visible_apps_only(root, tmp_path):
    out = tmp_path / "icons"
    icons.main(["build", "--root", str(root), "--out", str(out)])
    for theme, parent in (("Ambar", "Papirus"), ("Ambar-Dark", "Papirus-Dark")):
        t = out / theme
        made = {p.stem for p in (t / "apps/scalable").glob("*.svg")}
        assert {"glyphapp", "tileapp", "com.github.FontManager.FontManager"} <= made
        assert not {"hiddenapp", "overridden"} & made, "hidden apps (and /usr/local overrides) are skipped"
        index = (t / "index.theme").read_text()
        assert f"Inherits={parent}," in index and "apps/16x16" in index and "places/64x64" in index
        assert "MinSize=28" in index, "small sizes come from Papirus, not the tile"
        assert (t / "apps/16x16/glyphapp.svg").read_text() == GLYPH
        assert (t / "places/64x64/folder.svg").exists() and (t / "places/64x64/folder-documents.svg").exists()
        assert (t / "places/64x64/user-home.svg").exists()
        assert not (t / "places/64x64/folder-blue.svg").exists()


def render(path: Path):
    from PySide6.QtCore import QRectF, Qt
    from PySide6.QtGui import QGuiApplication, QImage, QPainter
    from PySide6.QtSvg import QSvgRenderer
    QGuiApplication.instance() or QGuiApplication([])
    r = QSvgRenderer(str(path))
    assert r.isValid(), path
    img = QImage(128, 128, QImage.Format.Format_ARGB32)
    img.fill(Qt.GlobalColor.transparent)
    p = QPainter(img)
    r.render(p, QRectF(0, 0, 128, 128))
    p.end()
    return img


@pytest.mark.parametrize("theme", ["Ambar", "Ambar-Dark"])
@pytest.mark.parametrize("app", ["glyphapp", "tileapp"])
def test_every_icon_is_the_same_squircle(root, tmp_path, theme, app):
    out = tmp_path / "icons"
    icons.main(["build", "--root", str(root), "--out", str(out)])
    img = render(out / theme / "apps/scalable" / f"{app}.svg")
    alpha = lambda x, y: img.pixelColor(x, y).alphaF()  # noqa: E731
    assert alpha(64, 64) > 0.99, "the centre is filled"
    assert alpha(64, 12) > 0.9 and alpha(12, 64) > 0.9, "edges of the tile are filled"
    assert alpha(7, 7) < 0.35 and alpha(120, 120) < 0.35, "the corners are rounded away (squircle)"
    assert alpha(1, 64) < 0.3, "a small margin around the tile"


def test_tile_detection_and_tint(root):
    base = root / "usr/share/icons/Papirus/64x64/apps"
    is_tile, colour = icons.analyse(base / "tileapp.svg")
    assert is_tile and colour[1] > colour[0]  # green
    is_tile, colour = icons.analyse(base / "glyphapp.svg")
    assert not is_tile and colour[2] > colour[0]  # blue


def test_squircle_is_closed_and_inside_the_box():
    d = icons.squircle(8, 8, 112, 112)
    pts = [tuple(map(float, xy.split(","))) for xy in d[2:-2].split(" L ")]
    assert d.endswith(" Z") and len(pts) == 96
    assert all(8 - 0.01 <= x <= 120.01 and 8 - 0.01 <= y <= 120.01 for x, y in pts)


def test_installed_and_used_everywhere():
    build = (REPO / "os/ubuntu/build.sh").read_text()
    hook = (REPO / "os/ubuntu/hooks/46-icons.chroot").read_text()
    assert '"$ROOT/usr/lib/ambar/ambar-icons"' in build
    assert "/usr/lib/ambar/ambar-icons build" in hook and "DPkg::Post-Invoke" in hook and "ambar-icons.path" in hook
    assert "[Icons]\nTheme=Ambar" in (REPO / "os/ubuntu/hooks/20-desktop.chroot").read_text()
    assert "gtk-icon-theme-name=Ambar" in (REPO / "os/ubuntu/hooks/20-desktop.chroot").read_text()
    for pkg, theme in (("os.ambar.desktop", "Ambar"), ("os.ambar.desktop.dark", "Ambar-Dark")):
        d = (REPO / f"desktop/plasma/look-and-feel/{pkg}/contents/defaults").read_text()
        assert f"[kdeglobals][Icons]\nTheme={theme}" in d
    assert '"Ambar-Dark" if dark_on else "Ambar"' in (REPO / "desktop/plasma/control-center/ambar-control-center").read_text()
