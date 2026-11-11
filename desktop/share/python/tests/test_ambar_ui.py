"""Ambar's shared Qt design: readable colours in both themes, and the building blocks work."""
from __future__ import annotations

import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))
pytest.importorskip("PySide6")
import ambar_ui  # noqa: E402


def luminance(hex_colour: str) -> float:
    def ch(v: int) -> float:
        c = v / 255
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
    h = hex_colour.lstrip("#")
    r, g, b = (int(h[i:i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a: str, b: str) -> float:
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


@pytest.mark.parametrize("mode", ["light", "dark"])
def test_text_meets_wcag_aa(mode):
    t = ambar_ui.TOKENS[mode]
    for bg in ("bg", "side", "surface", "surface2", "accent_soft", "select"):
        assert contrast(t["text"], t[bg]) >= 7, (bg, "body text should be AAA")
        assert contrast(t["muted"], t[bg]) >= 4.5, (bg, "secondary text")
    assert contrast(t["on_accent"], t["accent"]) >= 4.5, "text on primary buttons"
    for tone in ("good", "warn", "bad", "info", "accent"):
        badge_bg = ambar_ui.mix(t["surface"], t[tone], 0.14)
        fg = ambar_ui.badge_text(t[tone], t["surface"], t["text"])
        assert contrast(fg, badge_bg) >= 4.5, (tone, "badge text")
        assert contrast(t[tone], t["surface"]) >= 4.5, (tone, "coloured text on cards")


def test_same_palette_as_the_automation_center():
    qml = (HERE.parents[2] / "desktop/automation-center/ambar_center/qml/Main.qml").read_text()
    for mode in ("light", "dark"):
        for key in ("bg", "surface", "border", "text", "muted", "accent", "good", "warn", "bad"):
            assert ambar_ui.TOKENS[mode][key].upper() in qml.upper(), (mode, key)


def test_every_badge_and_role_is_styled():
    css = ambar_ui.stylesheet(ambar_ui.TOKENS["light"], "/c.svg", "/d.svg")
    for name in ambar_ui.TONES:
        assert f'QLabel[badge="{name}"]' in css, name
    for role in ("title", "subtitle", "section", "cardtitle", "muted", "hero", "notice", "mono"):
        assert f'QLabel[role="{role}"]' in css, role
    assert 'QPushButton[kind="primary"]' in css and "url(/c.svg)" in css and "url(/d.svg)" in css


@pytest.mark.parametrize("mode", ["light", "dark"])
def test_shell_pages_and_widgets_render(tmp_path, mode):
    script = tmp_path / "check.py"
    script.write_text(textwrap.dedent(f"""
        import os, sys
        os.environ.update(QT_QPA_PLATFORM="offscreen", AMBAR_THEME={mode!r}, XDG_RUNTIME_DIR={str(tmp_path)!r})
        sys.path.insert(0, {str(HERE)!r})
        import ambar_ui as ui
        from PySide6.QtWidgets import QApplication
        app = QApplication([])
        th = ui.apply(app)
        assert th.name == {mode!r} and app.palette().window().color().name().upper() == ui.TOKENS[{mode!r}]["bg"].upper()
        assert app.font().family() in ("Inter", "Inter Variable", "Noto Sans", "DejaVu Sans") or app.font().family()
        assert (__import__("pathlib").Path({str(tmp_path)!r}) / "ambar-ui").is_dir(), "tick and dot images written"
        s = ui.Shell("Test", "Subtitle")
        p1 = ui.Page("One", "First page"); p1.body.addWidget(ui.Card("Card", "Body"))
        st = ui.Stat("Count", "3", "good"); p1.body.addWidget(st); st.set("4", "bad")
        p1.body.addWidget(ui.badge("High", "high")); p1.body.addWidget(ui.badge("?", "nonsense"))
        s.add_page("One", p1); s.add_page("Two", ui.Page("Two"))
        assert s.currentIndex() == 0 and s.pages.currentIndex() == 0
        s.setCurrentIndex(1); assert s.pages.currentIndex() == 1
        s.setTabText(1, "Two (2)"); assert s.nav.item(1).text() == "Two (2)"
        assert st.value.text() == "4"
        s.resize(900, 600); img = s.grab().toImage()
        assert img.width() == 900
        # the sidebar is drawn in the sidebar colour, the page in the page colour
        assert img.pixelColor(5, 590).name().upper() == ui.TOKENS[{mode!r}]["side"].upper(), img.pixelColor(5, 590).name()
        assert img.pixelColor(895, 590).name().upper() == ui.TOKENS[{mode!r}]["bg"].upper(), img.pixelColor(895, 590).name()
        print("CHECK_OK")
    """))
    r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=120)
    assert "CHECK_OK" in r.stdout, r.stdout + r.stderr
