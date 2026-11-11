#!/usr/bin/env python3
"""Builds the Ambar OS tour video from real screenshots.

Every frame is a screenshot of the actual system: the built desktop (docs/images/os)
and the ISO booted and installed in QEMU (docs/images/os/booted). Nothing is mocked;
the captions only label what each shot shows.

    python3 scripts/demo/os_tour.py      # → docs/media/ambar-os-tour.mp4 (+ .gif, poster)

Needs: Pillow, ffmpeg, the Inter font (falls back to DejaVu).
"""
from __future__ import annotations

import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
IMG = ROOT / "docs" / "images" / "os"
OUT = ROOT / "docs" / "media"
W, H, FPS = 1600, 1000, 30
HOLD, FADE = 3.2, 0.6  # seconds on each shot, crossfade length

# (screenshot, title, subtitle). Order tells the story: boot → desktop → apps → install.
SHOTS = [
    ("desktop.jpg", "Ambar OS", "A full desktop OS on Ubuntu 26.04 LTS, designed like macOS"),
    ("ambar-menu.jpg", "The Ambar menu", "About, Settings, Store, Force Quit, Sleep, Restart, Shut Down"),
    ("control-center.jpg", "Control Center", "Wi-Fi, Bluetooth, Focus, dark mode, brightness and volume"),
    ("launchpad.jpg", "Launchpad", "Every app, one click away"),
    ("finder.jpg", "Files", "With its menus in the menu bar, as on a Mac"),
    ("browser.jpg", "A real web browser", "Falkon in this local build; Firefox in release builds"),
    ("mission-control.jpg", "Mission Control", "All windows at a glance"),
    ("store.jpg", "Ambar Store", "Install anything: Ubuntu apps, Flatpak, Snap, .deb, AppImage, Windows apps"),
    ("settings.jpg", "System Settings", "Displays, network, users, updates, everything"),
    ("dark-mode.jpg", "Light and dark", "One switch changes the whole desktop"),
    ("booted/desktop.jpg", "Booted for real", "The live ISO in a virtual machine (Plasma 6, Wayland)"),
    ("booted/search.jpg", "Search", "Alt+Space finds apps, files and automations"),
    ("booted/welcome.jpg", "Automations, built in", "The Ambar service runs on every boot"),
    ("booted/installer-done.jpg", "Installs to disk", "A full install: all 43 steps, on a blank 40 GB disk"),
    ("booted/installed-login.jpg", "Your own computer", "The installed system starts on its own"),
]


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    names = ["Inter-Bold.otf", "Inter-SemiBold.otf"] if bold else ["Inter-Regular.otf"]
    for d in [Path("/usr/share/fonts/opentype/inter"), Path("/usr/share/fonts/truetype/inter")]:
        for n in names:
            if (d / n).exists():
                return ImageFont.truetype(str(d / n), size)
    dejavu = "/usr/share/fonts/truetype/dejavu/DejaVuSans" + ("-Bold" if bold else "") + ".ttf"
    return ImageFont.truetype(dejavu, size)


def card(path: Path, title: str, sub: str) -> Image.Image:
    shot = Image.open(path).convert("RGB").resize((W, H), Image.LANCZOS)
    # Caption bar: a dark gradient at the bottom keeps text readable on any screenshot.
    shade = Image.new("L", (1, 220))
    for y in range(220):
        shade.putpixel((0, y), int(235 * (y / 219) ** 1.2))
    black = Image.new("RGB", (W, 220), (14, 10, 8))
    shot.paste(black, (0, H - 220), shade.resize((W, 220)))
    d = ImageDraw.Draw(shot)
    d.text((64, H - 132), title, font=font(52, bold=True), fill=(255, 255, 255))
    d.text((66, H - 66), sub, font=font(28), fill=(240, 205, 170))
    return shot


def build(dest: Path = OUT) -> Path:
    if not shutil.which("ffmpeg"):
        raise SystemExit("ffmpeg is needed")
    dest.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        frames = []
        for i, (name, title, sub) in enumerate(SHOTS):
            f = Path(tmp) / f"{i:02d}.png"
            card(IMG / name, title, sub).save(f)
            frames.append(f)
        inputs, chain = [], []
        for f in frames:
            inputs += ["-loop", "1", "-t", f"{HOLD + FADE}", "-i", str(f)]
        last = "[0:v]"
        for i in range(1, len(frames)):
            out = f"[v{i}]"
            chain.append(f"{last}[{i}:v]xfade=transition=fade:duration={FADE}:offset={i * HOLD:.2f}{out}")
            last = out
        chain.append(f"{last}format=yuv420p,fps={FPS}[out]")
        mp4 = dest / "ambar-os-tour.mp4"
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", *inputs, "-filter_complex", ";".join(chain),
                        "-map", "[out]", "-c:v", "libx264", "-preset", "veryfast", "-crf", "24",
                        "-movflags", "+faststart", str(mp4)], check=True)
        # A small looping GIF for the README, and a poster frame.
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(mp4), "-vf",
                        "fps=4,scale=640:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=bayer",
                        str(dest / "ambar-os-tour.gif")], check=True)
        card(IMG / SHOTS[0][0], SHOTS[0][1], SHOTS[0][2]).save(dest / "ambar-os-tour-poster.jpg", quality=85)
    return mp4


if __name__ == "__main__":
    print(build())
