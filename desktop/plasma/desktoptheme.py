#!/usr/bin/env python3
"""Generates the Ambar Plasma Style (desktop/plasma/desktoptheme/ambar).

What it changes from Breeze (anything not drawn here falls back to Breeze):
  - panels: frosted and translucent; the floating dock gets large rounded corners
    and a soft shadow, the full-width menu bar stays flat with a hairline
  - popups and menus: rounded corners, a hairline border and a soft, wide shadow
  - tooltips: rounded, translucent
  - dock items: rounded "pill" highlights instead of square blocks

Colours come from the system colour scheme (Ambar Light or Ambar Dark), so one
style serves both modes. Run: python3 desktop/plasma/desktoptheme.py
"""
from __future__ import annotations

import gzip
import json
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "desktoptheme" / "ambar"

STYLE = """<style type="text/css" id="current-color-scheme">
  .ColorScheme-Text { color:#1F1A15; stop-color:#1F1A15; }
  .ColorScheme-Background { color:#EFE9E0; stop-color:#EFE9E0; }
  .ColorScheme-Highlight { color:#B35C05; stop-color:#B35C05; }
</style>"""


def f(x: float) -> str:
    return f"{x:.3f}".rstrip("0").rstrip(".")


class Svg:
    """Collects elements on a canvas; each named element sits in its own cell."""

    def __init__(self):
        self.parts: list[str] = []
        self.defs: list[str] = []
        self.x = 0.0
        self.y = 0.0
        self.row_h = 0.0
        self.width = 0.0

    def place(self, w: float, h: float) -> tuple[float, float]:
        if self.x + w > 1200:
            self.x, self.y, self.row_h = 0.0, self.y + self.row_h + 4, 0.0
        pos = (self.x, self.y)
        self.x += w + 4
        self.row_h = max(self.row_h, h)
        self.width = max(self.width, self.x)
        return pos

    def add(self, eid: str, w: float, h: float, body: str) -> None:
        x, y = self.place(w, h)
        # Plasma sizes a piece by what is drawn in it, so an almost invisible filler keeps
        # empty parts (like the strip of a shadow under the frame) from being squashed away.
        filler = f'<rect width="{f(w)}" height="{f(h)}" style="fill:#000000;fill-opacity:0.0039;opacity:0.001"/>'
        self.parts.append(f'<g id="{eid}" transform="translate({f(x)},{f(y)})">{filler}{body}</g>')

    def hint(self, eid: str, w: float, h: float) -> None:
        x, y = self.place(max(w, 0.001), max(h, 0.001))
        self.parts.append(f'<rect id="{eid}" x="{f(x)}" y="{f(y)}" width="{f(max(w, 0.0001))}" '
                          f'height="{f(max(h, 0.0001))}" style="fill:#ff00ff"/>')

    def render(self) -> str:
        h = self.y + self.row_h + 4
        return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
                f'width="{f(self.width)}" height="{f(h)}" viewBox="0 0 {f(self.width)} {f(h)}">'
                f'<defs>{STYLE}{"".join(self.defs)}</defs>{"".join(self.parts)}</svg>')


def bg(opacity: float) -> str:
    return f'class="ColorScheme-Background" style="fill:currentColor;fill-opacity:{opacity}"'


def ink(opacity: float, cls: str = "ColorScheme-Text") -> str:
    return f'class="{cls}" style="fill:currentColor;fill-opacity:{opacity}"'


def corner_path(r: float, which: str) -> str:
    """A quarter disc of radius r filling the corner cell (r × r)."""
    return {
        "topleft": f"M 0,{f(r)} A {f(r)},{f(r)} 0 0 1 {f(r)},0 L {f(r)},{f(r)} Z",
        "topright": f"M 0,0 A {f(r)},{f(r)} 0 0 1 {f(r)},{f(r)} L 0,{f(r)} Z",
        "bottomright": f"M {f(r)},0 A {f(r)},{f(r)} 0 0 1 0,{f(r)} L 0,0 Z",
        "bottomleft": f"M {f(r)},{f(r)} A {f(r)},{f(r)} 0 0 1 0,0 L {f(r)},0 Z",
    }[which]


def ring_path(r: float, which: str, w: float = 1.0) -> str:
    """The outer hairline of a corner: the quarter disc minus a smaller one."""
    ri = r - w
    o = {
        "topleft": (f"M 0,{f(r)} A {f(r)},{f(r)} 0 0 1 {f(r)},0 L {f(r)},{f(w)} "
                    f"A {f(ri)},{f(ri)} 0 0 0 {f(w)},{f(r)} Z"),
        "topright": (f"M 0,0 A {f(r)},{f(r)} 0 0 1 {f(r)},{f(r)} L {f(ri)},{f(r)} "
                     f"A {f(ri)},{f(ri)} 0 0 0 0,{f(w)} Z"),
        "bottomright": (f"M {f(r)},0 A {f(r)},{f(r)} 0 0 1 0,{f(r)} L 0,{f(ri)} "
                        f"A {f(ri)},{f(ri)} 0 0 0 {f(ri)},0 Z"),
        "bottomleft": (f"M {f(r)},{f(r)} A {f(r)},{f(r)} 0 0 1 0,0 L {f(w)},0 "
                       f"A {f(ri)},{f(ri)} 0 0 0 {f(r)},{f(ri)} Z"),
    }
    return o[which]


EDGE = 8  # length of stretchable edge pieces


def frame(svg: Svg, prefix: str, r: float, fill: str, line: str | None, line_w: float = 1.0) -> None:
    """The nine pieces of a rounded rectangle: corners r × r, edges and centre stretch."""
    p = prefix
    for c in ("topleft", "topright", "bottomleft", "bottomright"):
        body = f'<path d="{corner_path(r, c)}" {fill}/>'
        if line:
            body += f'<path d="{ring_path(r, c, line_w)}" {line}/>'
        svg.add(p + c, r, r, body)
    lw = f(line_w)
    edges = {
        "top": (EDGE, r, f'<rect x="0" y="0" width="{EDGE}" height="{lw}"'),
        "bottom": (EDGE, r, f'<rect x="0" y="{f(r - line_w)}" width="{EDGE}" height="{lw}"'),
        "left": (r, EDGE, f'<rect x="0" y="0" width="{lw}" height="{EDGE}"'),
        "right": (r, EDGE, f'<rect x="{f(r - line_w)}" y="0" width="{lw}" height="{EDGE}"'),
    }
    for e, (w, h, ln) in edges.items():
        body = f'<rect x="0" y="0" width="{f(w)}" height="{f(h)}" {fill}/>'
        if line:
            body += f"{ln} {line}/>"
        svg.add(p + e, w, h, body)
    svg.add(p + "center", EDGE, EDGE, f'<rect x="0" y="0" width="{EDGE}" height="{EDGE}" {fill}/>')


def mask(svg: Svg, r: float) -> None:
    frame(svg, "mask-", r, 'style="fill:#000000"', None)


def margins(svg: Svg, prefix: str, top: float, bottom: float, left: float, right: float) -> None:
    for side, v in (("top", top), ("bottom", bottom), ("left", left), ("right", right)):
        if side in ("top", "bottom"):
            svg.hint(f"{prefix}hint-{side}-margin", 4, v)
        else:
            svg.hint(f"{prefix}hint-{side}-margin", v, 4)


def shadow(svg: Svg, r: float, size: float, strength: float, offset: float = 0.0) -> None:
    """A soft shadow around a rounded frame. Each piece is size + r, overlapping the frame by r.

    `offset` pushes the shadow down (a light source above), like macOS.
    """
    top, bottom = size - offset, size + offset
    sides = {"top": top, "bottom": bottom, "left": size, "right": size}
    n = len(svg.defs)

    def grad(gid: str, x1, y1, x2, y2):
        svg.defs.append(f'<linearGradient id="{gid}" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" gradientUnits="objectBoundingBox">'
                        f'<stop offset="0" stop-color="#000" stop-opacity="{strength}"/>'
                        f'<stop offset="0.35" stop-color="#000" stop-opacity="{strength * 0.45}"/>'
                        f'<stop offset="1" stop-color="#000" stop-opacity="0"/></linearGradient>')
        return f"url(#{gid})"

    # Edges: the gradient runs outward from the frame edge; the r-wide strip under the frame is empty.
    g = {"top": grad(f"st{n}", 0, 1, 0, 0), "bottom": grad(f"sb{n}", 0, 0, 0, 1),
         "left": grad(f"sl{n}", 1, 0, 0, 0), "right": grad(f"sr{n}", 0, 0, 1, 0)}
    s = sides
    svg.add("shadow-top", EDGE, s["top"] + r, f'<rect x="0" y="0" width="{EDGE}" height="{f(s["top"])}" style="fill:{g["top"]}"/>')
    svg.add("shadow-bottom", EDGE, s["bottom"] + r,
            f'<rect x="0" y="{f(r)}" width="{EDGE}" height="{f(s["bottom"])}" style="fill:{g["bottom"]}"/>')
    svg.add("shadow-left", s["left"] + r, EDGE, f'<rect x="0" y="0" width="{f(s["left"])}" height="{EDGE}" style="fill:{g["left"]}"/>')
    svg.add("shadow-right", s["right"] + r, EDGE,
            f'<rect x="{f(r)}" y="0" width="{f(s["right"])}" height="{EDGE}" style="fill:{g["right"]}"/>')
    svg.add("shadow-center", EDGE, EDGE, f'<rect width="{EDGE}" height="{EDGE}" style="fill:#000;fill-opacity:0"/>')

    # Corners: a radial fade centred on the corner's arc centre, with the frame's own corner cut out.
    for c in ("topleft", "topright", "bottomleft", "bottomright"):
        sx = s["left"] if "left" in c else s["right"]
        sy = s["top"] if "top" in c else s["bottom"]
        w, h = sx + r, sy + r
        # the frame's corner arc is centred on the piece's inner corner
        cx = w if "left" in c else 0.0
        cy = h if "top" in c else 0.0
        rad = max(sx, sy) + r
        gid = f"sc{n}{c}"
        stop0 = r / rad
        svg.defs.append(f'<radialGradient id="{gid}" cx="{f(cx)}" cy="{f(cy)}" r="{f(rad)}" gradientUnits="userSpaceOnUse">'
                        f'<stop offset="{f(stop0)}" stop-color="#000" stop-opacity="{strength}"/>'
                        f'<stop offset="{f(stop0 + (1 - stop0) * 0.35)}" stop-color="#000" stop-opacity="{strength * 0.45}"/>'
                        f'<stop offset="1" stop-color="#000" stop-opacity="0"/></radialGradient>')
        # whole piece, minus the quarter disc the frame covers (even-odd)
        if c == "topleft":
            cut = f"M {f(w - r)},{f(h)} A {f(r)},{f(r)} 0 0 1 {f(w)},{f(h - r)} L {f(w)},{f(h)} Z"
        elif c == "topright":
            cut = f"M 0,{f(h - r)} A {f(r)},{f(r)} 0 0 1 {f(r)},{f(h)} L 0,{f(h)} Z"
        elif c == "bottomleft":
            cut = f"M {f(w)},{f(r)} A {f(r)},{f(r)} 0 0 1 {f(w - r)},0 L {f(w)},0 Z"
        else:
            cut = f"M {f(r)},0 A {f(r)},{f(r)} 0 0 1 0,{f(r)} L 0,0 Z"
        body = (f'<path d="M 0,0 H {f(w)} V {f(h)} H 0 Z {cut}" style="fill:url(#{gid});fill-rule:evenodd"/>')
        svg.add(f"shadow-{c}", w, h, body)

    for side, v in sides.items():
        if side in ("top", "bottom"):
            svg.hint(f"shadow-hint-{side}-margin", 4, v)
        else:
            svg.hint(f"shadow-hint-{side}-margin", v, 4)


def write(rel: str, svg: Svg) -> None:
    path = OUT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=9, mtime=0) as fh:
        fh.write(svg.render().encode())  # mtime=0: the same input gives the same bytes


# ------------------------------------------------------------------ the pieces

PANEL_R, DIALOG_R, TOOLTIP_R, TASK_R = 16, 12, 8, 10
VARIANTS = {"": 0.86, "translucent/": 0.72, "solid/": 1.0, "opaque/": 1.0}


def panel(fill_opacity: float, with_shadow: bool) -> Svg:
    s = Svg()
    frame(s, "", PANEL_R, bg(fill_opacity), ink(0.10))
    margins(s, "", 4, 4, 6, 6)
    for side in ("top", "bottom", "left", "right"):  # floating panels sit this far from the frame edge
        s.hint(f"hint-{side}-inset", 0, 0)
    mask(s, PANEL_R)
    if with_shadow:
        shadow(s, PANEL_R, 14, 0.16)
    return s


def dialog(fill_opacity: float, with_shadow: bool) -> Svg:
    s = Svg()
    frame(s, "", DIALOG_R, bg(fill_opacity), ink(0.12))
    margins(s, "", 8, 8, 8, 8)
    mask(s, DIALOG_R)
    if with_shadow:
        shadow(s, DIALOG_R, 20, 0.20)
    return s


def tooltip(fill_opacity: float, with_shadow: bool) -> Svg:
    s = Svg()
    frame(s, "", TOOLTIP_R, bg(fill_opacity), ink(0.12))
    margins(s, "", 6, 6, 8, 8)
    mask(s, TOOLTIP_R)
    if with_shadow:
        shadow(s, TOOLTIP_R, 10, 0.16)
    return s


# Dock items: a soft pill behind running apps, stronger for the active one, accent when asking for attention.
TASK_STATES = {"normal": ("ColorScheme-Text", 0.07), "minimized": ("ColorScheme-Text", 0.04),
               "hover": ("ColorScheme-Text", 0.12), "focus": ("ColorScheme-Text", 0.16),
               "attention": ("ColorScheme-Highlight", 0.30), "progress": ("ColorScheme-Highlight", 0.22)}


def tasks() -> Svg:
    s = Svg()
    for state, (cls, op) in TASK_STATES.items():
        for loc in ("", "north-", "west-", "east-"):
            frame(s, f"{loc}{state}-", TASK_R, ink(op, cls), None)
    margins(s, "normal-", 4, 4, 4, 4)
    return s


def main() -> int:
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)
    (OUT / "metadata.json").write_text(json.dumps({
        "KPlugin": {"Id": "ambar", "Name": "Ambar", "Description": "Frosted panels, rounded popups and a pill-style dock",
                    "Authors": [{"Name": "Ambar Singh"}], "Category": "", "EnabledByDefault": True,
                    "License": "LGPL", "Version": "0.2", "Website": "https://github.com/alexbieber/AmbarOS"},
        "X-Plasma-API": "5.0",
    }, indent=2) + "\n")
    (OUT / "plasmarc").write_text("[ContrastEffect]\nenabled=true\ncontrast=0.9\nintensity=1.1\nsaturation=1.7\n\n"
                                  "[BlurBehindEffect]\nenabled=true\n\n[AdaptiveTransparency]\nenabled=true\n")
    for prefix, opacity in VARIANTS.items():
        shadowed = prefix not in ("opaque/",)  # no compositor, no shadows
        write(f"{prefix}widgets/panel-background.svgz", panel(opacity, shadowed))
        write(f"{prefix}dialogs/background.svgz", dialog(min(1.0, opacity + 0.08), shadowed))
        write(f"{prefix}widgets/tooltip.svgz", tooltip(min(1.0, opacity + 0.04), shadowed))
    write("widgets/tasks.svgz", tasks())
    print(f"wrote {sum(1 for _ in OUT.rglob('*.svgz'))} SVGs to {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
