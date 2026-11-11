#!/usr/bin/env python3
"""Generates the Ambar window decorations for KWin's Aurorae engine.

macOS-style: close / minimise / maximise as red, yellow and green circles on
the left, a centred title, a soft shadow (corners are square: Aurorae sizes corners like the borders). Light and dark
variants, matching the AmbarLight / AmbarDark colour schemes.

    python3 desktop/plasma/aurorae.py      # → desktop/plasma/aurorae/{Ambar,AmbarDark}

Aurorae draws the frame from 9-slice SVG pieces (decoration.svg) and each
button from its own SVG with one element per state.
"""
from __future__ import annotations

from pathlib import Path

OUT = Path(__file__).resolve().parent / "aurorae"

P = 14      # shadow padding around the window
T = 30      # title bar height
B = 1       # side/bottom border

THEMES = {
    "Ambar": {"bg": "#EFE9E0", "bg_in": "#F4F0EA", "edge": "#D6CDC1", "text": "31,26,21",
              "text_in": "150,140,128", "shadow": 0.13, "dot_in": "#D3CBC0", "glyph": "#4A2A0C"},
    "AmbarDark": {"bg": "#1C1814", "bg_in": "#221D18", "edge": "#3A3129", "text": "247,244,239",
                  "text_in": "140,130,120", "shadow": 0.40, "dot_in": "#4A423A", "glyph": "#2A1608"},
}
BUTTONS = {"close": "#FF5F57", "minimize": "#FEBC2E", "maximize": "#28C840", "restore": "#28C840"}


def frame(prefix: str, bg: str, edge: str, shadow: float) -> str:
    """The nine frame pieces for one state. Aurorae draws every corner at the
    size of the side borders, so corners are square: (P+B) wide."""
    s = shadow
    W = 4                      # stretchable middle pieces
    defs, parts = [], []
    n = [0]

    def lin(x1, y1, x2, y2):   # transparent → shadow, in user space
        n[0] += 1
        gid = f"{prefix}-g{n[0]}"
        defs.append(f'<linearGradient id="{gid}" gradientUnits="userSpaceOnUse" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">'
                    f'<stop offset="0" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="{s}"/></linearGradient>')
        return f"url(#{gid})"

    def rad(cx, cy):           # shadow at (cx, cy) fading out over P
        n[0] += 1
        gid = f"{prefix}-g{n[0]}"
        defs.append(f'<radialGradient id="{gid}" gradientUnits="userSpaceOnUse" cx="{cx}" cy="{cy}" r="{P}">'
                    f'<stop offset="0" stop-color="#000" stop-opacity="{s}"/><stop offset="1" stop-color="#000" stop-opacity="0"/></radialGradient>')
        return f"url(#{gid})"

    def piece(id_, x, y, w, h, body):
        parts.append(f'<g id="{prefix}-{id_}"><rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none"/>{body}</g>')

    L = P + B                  # side piece width
    TH = P + T                 # top piece height
    # --- top row (y 0 .. TH)
    x = 0
    piece("topleft", x, 0, L, TH,
          f'<rect x="{x}" y="0" width="{P}" height="{P}" fill="{rad(x + P, P)}"/>'
          f'<rect x="{x}" y="{P}" width="{P}" height="{T}" fill="{lin(x, 0, x + P, 0)}"/>'
          f'<rect x="{x + P}" y="{P}" width="{B}" height="{T}" fill="{edge}"/>')
    x = 40
    piece("top", x, 0, W, TH,
          f'<rect x="{x}" y="0" width="{W}" height="{P}" fill="{lin(0, 0, 0, P)}"/>'
          f'<rect x="{x}" y="{P}" width="{W}" height="{T}" fill="{bg}"/>'
          f'<rect x="{x}" y="{P}" width="{W}" height="{B}" fill="{edge}"/>')
    x = 80
    piece("topright", x, 0, L, TH,
          f'<rect x="{x + B}" y="0" width="{P}" height="{P}" fill="{rad(x + B, P)}"/>'
          f'<rect x="{x + B}" y="{P}" width="{P}" height="{T}" fill="{lin(x + L, 0, x + B, 0)}"/>'
          f'<rect x="{x}" y="{P}" width="{B}" height="{T}" fill="{edge}"/>')
    # --- middle row (y 100 .. 100+W)
    y = 100
    piece("left", 0, y, L, W, f'<rect x="0" y="{y}" width="{P}" height="{W}" fill="{lin(0, 0, P, 0)}"/>'
                              f'<rect x="{P}" y="{y}" width="{B}" height="{W}" fill="{edge}"/>')
    piece("center", 40, y, W, W, f'<rect x="40" y="{y}" width="{W}" height="{W}" fill="{bg}"/>')
    piece("right", 80, y, L, W, f'<rect x="80" y="{y}" width="{B}" height="{W}" fill="{edge}"/>'
                                f'<rect x="{80 + B}" y="{y}" width="{P}" height="{W}" fill="{lin(80 + L, 0, 80 + B, 0)}"/>')
    # --- bottom row (y 140 .. 140+L)
    y = 140
    piece("bottomleft", 0, y, L, L, f'<rect x="0" y="{y + B}" width="{P}" height="{P}" fill="{rad(P, y + B)}"/>'
                                    f'<rect x="0" y="{y}" width="{P}" height="{B}" fill="{lin(0, 0, P, 0)}"/>'
                                    f'<rect x="{P}" y="{y}" width="{B}" height="{B}" fill="{edge}"/>')
    piece("bottom", 40, y, W, L, f'<rect x="40" y="{y}" width="{W}" height="{B}" fill="{edge}"/>'
                                 f'<rect x="40" y="{y + B}" width="{W}" height="{P}" fill="{lin(0, y + L, 0, y + B)}"/>')
    piece("bottomright", 80, y, L, L, f'<rect x="{80 + B}" y="{y + B}" width="{P}" height="{P}" fill="{rad(80 + B, y + B)}"/>'
                                      f'<rect x="{80 + B}" y="{y}" width="{P}" height="{B}" fill="{lin(80 + L, 0, 80 + B, 0)}"/>'
                                      f'<rect x="80" y="{y}" width="{B}" height="{B}" fill="{edge}"/>')
    return "".join(defs), "".join(parts)


def decoration_svg(t: dict) -> str:
    g1, p1 = frame("decoration", t["bg"], t["edge"], t["shadow"])
    g2, p2 = frame("decoration-inactive", t["bg_in"], t["edge"], t["shadow"] * 0.55)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">'
            f'<defs>{g1}{g2}</defs>{p1}{p2}</svg>')


GLYPHS = {
    "close": "M5,5 L11,11 M11,5 L5,11",
    "minimize": "M4.5,8 L11.5,8",
    "maximize": "M5,11 L5,6.5 L9.5,11 Z M11,5 L11,9.5 L6.5,5 Z",
    "restore": "M8,8 L8,4.5 L11.5,8 Z M8,8 L8,11.5 L4.5,8 Z",
}


def button_svg(name: str, t: dict) -> str:
    color = BUTTONS[name]
    glyph = GLYPHS[name]
    fill_glyph = name in ("maximize", "restore")
    gl = (f'<path d="{glyph}" fill="{t["glyph"]}" fill-opacity="0.75"/>' if fill_glyph else
          f'<path d="{glyph}" stroke="{t["glyph"]}" stroke-opacity="0.8" stroke-width="1.4" stroke-linecap="round"/>')

    def state(id_, fill, glyph_on, dim=1.0):
        return (f'<g id="{id_}-center"><rect x="0" y="0" width="16" height="16" fill="none"/>'
                f'<circle cx="8" cy="8" r="6.2" fill="{fill}" fill-opacity="{dim}" stroke="#000" stroke-opacity="0.12" stroke-width="0.6"/>'
                f'{gl if glyph_on else ""}</g>')
    body = "".join([
        state("active", color, False),
        state("hover", color, True),
        state("pressed", color, True, 0.75),
        state("inactive", t["dot_in"], False),
        state("deactivated", t["dot_in"], False, 0.6),
    ])
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">{body}</svg>'


def rc(t: dict) -> str:
    return f"""[General]
ActiveTextColor={t["text"]}
InactiveTextColor={t["text_in"]}
TitleAlignment=Center
TitleVerticalAlignment=Center
Animation=120
ActiveHighlightColor=0,0,0,0
InactiveHighlightColor=0,0,0,0
Shadow=true

[Layout]
BorderLeft={B}
BorderRight={B}
BorderBottom={B}
TitleEdgeTop={7}
TitleEdgeBottom={7}
TitleEdgeLeft={12}
TitleEdgeRight={12}
TitleBorderLeft=8
TitleBorderRight=8
TitleHeight={T - 14}
ButtonWidth=16
ButtonHeight=16
ButtonSpacing=6
ButtonMarginTop=0
ExplicitButtonSpacer=10
PaddingTop={P}
PaddingBottom={P}
PaddingLeft={P}
PaddingRight={P}
"""


def metadata(name: str) -> str:
    pretty = "Ambar" if name == "Ambar" else "Ambar Dark"
    return f"""[Desktop Entry]
Name={pretty}
Comment=macOS-style window buttons for Ambar OS
X-KDE-PluginInfo-Author=Ambar OS
X-KDE-PluginInfo-Name={name}
X-KDE-PluginInfo-Version=1.0
X-KDE-PluginInfo-License=Apache-2.0
X-KDE-PluginInfo-Website=https://github.com/alexbieber/AmbarOS
"""


def main() -> int:
    for name, t in THEMES.items():
        d = OUT / name
        d.mkdir(parents=True, exist_ok=True)
        (d / "decoration.svg").write_text(decoration_svg(t))
        for b in BUTTONS:
            (d / f"{b}.svg").write_text(button_svg(b, t))
        (d / f"{name}rc").write_text(rc(t))
        (d / "metadata.desktop").write_text(metadata(name))
        print(d)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
