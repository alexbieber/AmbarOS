#!/usr/bin/env python3
"""Records the Ambar demo video from the real software.

What you see is the actual Automation Center (QML, rendered offscreen) talking
to a real `ambard` over its socket: real workflows, a real agent proposal, a
real run that stops for approval. The one stand-in is the AI model: its reply
is replayed from a file (AMBAR_AI_SCRIPT), so the video needs no API key and
is the same every time. Captions say so.

    python3 scripts/demo/record_demo.py            # → docs/media/ambar-demo.mp4 (+ .gif, poster)

Needs: cargo build --release, PySide6, ffmpeg. Runs in a throwaway HOME.
"""
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get("AMBAR_TARGET_DIR", ROOT / "target" / "release"))
MARK = ROOT / "desktop" / "brand" / "ambar-mark.svg"
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["QT_SCALE_FACTOR"] = os.environ.get("DEMO_SCALE", "1.5")  # 1680×1110 frames

# ------------------------------------------------------------------ the world

PDF_RULE = '''
id = "file-pdfs"
name = "File PDFs from Downloads"
description = "Moves new PDFs into Documents/PDFs and tells me"

[trigger]
type = "folder_watch"
path = "~/Downloads"

[[conditions]]
type = "file_extension"
any_of = ["pdf"]

[[actions]]
type = "move_file"
to_dir = "~/Documents/PDFs"

[[actions]]
type = "notify"
title = "Filed {file_name}"

[permissions]
write = ["~/Downloads", "~/Documents/PDFs"]
notify = true
'''


def workflows(home: Path) -> list[str]:
    def wf(wid, name, trigger, action, perms):
        return f'id = "{wid}"\nname = "{name}"\n{trigger}\n{action}\n[permissions]\n{perms}\n'
    notes = home / "Documents" / "Notes"
    return [
        wf("daily-note", "Start today's note", '[trigger]\ntype = "schedule"\ncron = "0 9 * * 1-5"',
           f'[[actions]]\ntype = "write_file"\npath = "~/Documents/Notes/{{date}}.md"\ncontent = "# Notes\\n"',
           'write = ["~/Documents/Notes"]'),
        wf("tidy-screenshots", "Tidy screenshots", '[trigger]\ntype = "folder_watch"\npath = "~/Pictures/Screenshots"',
           '[[actions]]\ntype = "move_file"\nto_dir = "~/Pictures/Screenshots/{date}"',
           'write = ["~/Pictures/Screenshots"]'),
        wf("evening-report", "Evening work report", '[trigger]\ntype = "schedule"\ncron = "0 18 * * 1-5"',
           '[[actions]]\ntype = "write_file"\npath = "~/Documents/Reports/{date}.md"\ncontent = "Done today\\n"\nappend = true',
           'write = ["~/Documents/Reports"]'),
        wf("weekly-backup-list", "Weekly backup list", '[trigger]\ntype = "manual"',
           '[[actions]]\ntype = "write_file"\npath = "~/Documents/backup-list.txt"\ncontent = "projects\\n"\nappend = true',
           'write = ["~/Documents"]'),
    ], notes


def build_world(home: Path):
    from ambar_sdk import Client

    for d in ["Downloads", "Documents/Notes", "Documents/Reports", "Documents/PDFs", "Pictures/Screenshots",
              "Projects/old-logs"]:
        (home / d).mkdir(parents=True, exist_ok=True)
    (home / "Projects/old-logs/build-2025.log").write_text("x")
    data = home / ".local/share/ambar"
    env = {**os.environ, "AMBARD_NOTIFY": "log", "AMBARD_APPROVAL_TIMEOUT": "600"}
    proc = subprocess.Popen([str(BIN / "ambard"), "--data-dir", str(data)], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(200):
        if (data / "run" / "ambard.sock").exists():
            break
        time.sleep(0.05)
    wfs, _ = workflows(home)
    with Client(data) as c:
        for text in wfs:
            info = c.register(text, grant=True)
            c.enable(info["id"])
        c.run_now("weekly-backup-list")
        c.run_now("evening-report")
        c.run_now("daily-note")
        c.pause("evening-report")
        _, token = c.agent_create("planner", ["read", "propose"], "suggests automations")
        with Client(data, token=token) as a:
            a.propose('''id = "sort-invoices"
name = "Sort invoices by month"
[trigger]
type = "folder_watch"
path = "~/Downloads"
[[conditions]]
type = "file_name_matches"
glob = "invoice-*"
[[actions]]
type = "move_file"
to_dir = "~/Documents/Invoices/{date}"
[permissions]
write = ["~/Downloads", "~/Documents/Invoices"]
''', reason="12 invoices have piled up in Downloads this month")
        c.register(f'''id = "clean-old-logs"
name = "Clean up old build logs"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "~/Projects/old-logs/build-2025.log"
[permissions]
write = ["~/Projects/old-logs"]
''', grant=True)
        c.enable("clean-old-logs")
    return proc, data


# ------------------------------------------------------------------ recorder

class Recorder:
    """Grabs the window at ~20 fps with a fake cursor and captions painted on."""

    def __init__(self, app, win, frames: Path, size):
        from PySide6.QtCore import QPointF
        self.app, self.win, self.dir = app, win, frames
        self.w, self.h = size
        self.t = 0.0                       # timeline position (s)
        self.list: list[tuple[str, float]] = []
        self.cursor = QPointF(self.w * 0.62, self.h * 0.55)
        self.cursor_visible = False
        self.ripple = None                 # (pos, start_t)
        self.caption, self.caption_t = "", -10.0
        self.n = 0

    # -- painting
    def _paint_overlay(self, img):
        from PySide6.QtCore import QPointF, QRectF, Qt
        from PySide6.QtGui import QColor, QFont, QPainter, QPainterPath, QPen, QPolygonF
        p = QPainter(img)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        s = img.width() / self.w
        p.scale(s, s)
        if self.ripple:
            pos, t0 = self.ripple
            k = (self.t - t0) / 0.45
            if 0 <= k <= 1:
                c = QColor(242, 164, 58, int(150 * (1 - k)))
                p.setPen(QPen(c, 3))
                p.setBrush(Qt.BrushStyle.NoBrush)
                p.drawEllipse(pos, 8 + 22 * k, 8 + 22 * k)
        if self.cursor_visible:
            x, y = self.cursor.x(), self.cursor.y()
            arrow = QPolygonF([QPointF(x, y), QPointF(x, y + 21), QPointF(x + 5.2, y + 16.2),
                               QPointF(x + 9, y + 24.5), QPointF(x + 12.6, y + 23), QPointF(x + 8.9, y + 14.8),
                               QPointF(x + 15.5, y + 14.8)])
            p.setPen(QPen(QColor("#FFFFFF"), 1.6))
            p.setBrush(QColor("#111111"))
            p.drawPolygon(arrow)
        if self.caption:
            k = min(1.0, (self.t - self.caption_t) / 0.35)
            f = QFont("Inter", 15)
            f.setWeight(QFont.Weight.DemiBold)
            p.setFont(f)
            fm = p.fontMetrics()
            tw = fm.horizontalAdvance(self.caption)
            box = QRectF((self.w - tw) / 2 - 22, self.h - 124 + 8 * (1 - k), tw + 44, 42)  # above toasts
            path = QPainterPath()
            path.addRoundedRect(box, 21, 21)
            p.setOpacity(k)
            p.fillPath(path, QColor(17, 14, 11, 230))
            p.setPen(QPen(QColor(242, 164, 58, 170), 1.2))  # visible on dark pages too
            p.drawPath(path)
            p.setPen(QColor("#F7F4EF"))
            p.drawText(box, Qt.AlignmentFlag.AlignCenter, self.caption)
        p.end()

    def frame(self, img=None, dt=None):
        if img is None:
            img = self.win.grabWindow()
            self._paint_overlay(img)
        path = self.dir / f"f{self.n:05d}.jpg"
        img.save(str(path), "JPG", 92)
        self.n += 1
        self.list.append((path.name, dt))

    def hold(self, seconds: float, fps: float = 20.0):
        """Run the app for `seconds` of timeline, grabbing frames."""
        step = 1.0 / fps
        end = self.t + seconds
        while self.t < end - 1e-6:
            t_wall = time.time()
            while time.time() - t_wall < step * 0.6:
                self.app.processEvents()
                time.sleep(0.004)
            self.frame(dt=step)
            self.t += step

    def say(self, text: str):
        self.caption, self.caption_t = text, self.t

    def item_center(self, item):
        from PySide6.QtCore import QPointF
        pt = item.mapToScene(QPointF(item.width() / 2, item.height() / 2))
        return pt

    def move_to(self, target, seconds=0.7):
        from PySide6.QtCore import QPointF
        if not isinstance(target, QPointF):
            target = self.item_center(target)
        self.cursor_visible = True
        start = QPointF(self.cursor)
        steps = max(1, int(seconds * 20))
        for i in range(1, steps + 1):
            k = i / steps
            e = k * k * (3 - 2 * k)  # smoothstep
            self.cursor = start + (target - start) * e
            self.hold(1 / 20)

    def click(self, target=None, action=None, settle=0.4):
        if target is not None:
            self.move_to(target)
        self.ripple = (self.cursor, self.t)
        self.hold(0.12)
        if action:
            action()
        self.hold(settle)

    def card(self, title: str, subtitle: str, seconds: float, foot: str = ""):
        """A branded title card, faded in and out."""
        from PySide6.QtCore import QRectF, Qt
        from PySide6.QtGui import QColor, QFont, QImage, QLinearGradient, QPainter, QRadialGradient
        from PySide6.QtSvg import QSvgRenderer
        dpr = self.win.grabWindow().width() / self.w
        W, H = int(self.w * dpr), int(self.h * dpr)
        base = QImage(W, H, QImage.Format.Format_RGB32)
        p = QPainter(base)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.scale(dpr, dpr)
        g = QLinearGradient(0, 0, 0, self.h)
        g.setColorAt(0, QColor("#1A1612"))
        g.setColorAt(1, QColor("#07080C"))
        p.fillRect(QRectF(0, 0, self.w, self.h), g)
        glow = QRadialGradient(self.w / 2, self.h * 0.42, self.w * 0.45)
        glow.setColorAt(0, QColor(252, 161, 1, 46))
        glow.setColorAt(1, QColor(252, 161, 1, 0))
        p.fillRect(QRectF(0, 0, self.w, self.h), glow)
        QSvgRenderer(str(MARK)).render(p, QRectF(self.w / 2 - 80, self.h * 0.22, 160, 115))
        f = QFont("Inter Display", 46)
        f.setWeight(QFont.Weight.Bold)
        p.setFont(f)
        p.setPen(QColor("#FFFFFF"))
        p.drawText(QRectF(0, self.h * 0.22 + 135, self.w, 64), Qt.AlignmentFlag.AlignCenter, title)
        f2 = QFont("Inter", 19)
        p.setFont(f2)
        p.setPen(QColor("#F2A43A"))
        p.drawText(QRectF(0, self.h * 0.22 + 205, self.w, 34), Qt.AlignmentFlag.AlignCenter, subtitle)
        if foot:
            f3 = QFont("Inter", 13)
            p.setFont(f3)
            p.setPen(QColor(247, 244, 239, 150))
            p.drawText(QRectF(0, self.h - 70, self.w, 30), Qt.AlignmentFlag.AlignCenter, foot)
        p.end()
        fps = 20
        n = int(seconds * fps)
        for i in range(n):
            k = min(1.0, i / 8, (n - 1 - i) / 8)
            img = QImage(W, H, QImage.Format.Format_RGB32)
            img.fill(QColor("#07080C"))
            q = QPainter(img)
            q.setOpacity(max(0.0, k))
            q.drawImage(0, 0, base)
            q.end()
            self.frame(img, 1 / fps)
            self.t += 1 / fps


# ------------------------------------------------------------------ helpers

def find_text(item, text):
    """Depth-first search of the QML scene for a visible item whose text matches."""
    try:
        if item.isVisible() and item.property("text") == text and item.width() > 0:
            return item
    except RuntimeError:
        return None
    for c in item.childItems():
        hit = find_text(c, text)
        if hit is not None:
            return hit
    return None


def scene_root(win):
    root = win.contentItem()
    while root.parentItem() is not None:
        root = root.parentItem()
    return root


def find(win, text):
    hit = find_text(scene_root(win), text)
    if hit is None:
        raise SystemExit(f"demo: nothing on screen says {text!r}")
    return hit


# ------------------------------------------------------------------ script

def run(out_dir: Path, keep_frames: bool):
    # A tidy, throwaway home so paths on screen read /home/alex/…, not /tmp/xyz.
    home = Path(os.environ.get("DEMO_HOME", "/home/alex"))
    if home.exists() and any(home.iterdir()):
        raise SystemExit(f"{home} exists and isn't empty; set DEMO_HOME to a fresh path")
    home.mkdir(parents=True, exist_ok=True)
    os.environ.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                      XDG_DATA_HOME=str(home / ".local/share"))
    sys.path.insert(0, str(ROOT / "sdk" / "python"))
    sys.path.insert(0, str(ROOT / "desktop" / "automation-center"))
    proc, data = build_world(home)
    frames = Path(tempfile.mkdtemp(prefix="ambar-frames-"))
    try:
        script = home / "ai-reply.txt"
        script.write_text("```toml\n" + PDF_RULE.strip() + "\n```")
        os.environ["AMBAR_AI_SCRIPT"] = str(script)

        from PySide6.QtCore import QObject
        from ambar_center.app import create
        app, engine, backend = create(["demo"], str(data), theme="light", poll_ms=500)
        backend._ambar = str(BIN / "ambar")
        win = engine.rootObjects()[0]
        for _ in range(30):
            app.processEvents()
            time.sleep(0.02)
        r = Recorder(app, win, frames, (win.width(), win.height()))

        # 1. Title
        r.card("Ambar OS", "Designed for Humans. Built for AI. Open to Everyone.", 3.4)

        # 2. Overview
        win.setProperty("page", "overview")
        r.say("Your automations run quietly in the background")
        r.hold(3.2)
        r.click(find(win, "Automations"), lambda: win.setProperty("page", "automations"))
        r.say("Turn them on, pause them, run them now")
        r.hold(2.6)
        r.click(find(win, "Activity"), lambda: win.setProperty("page", "activity"))
        r.say("Every run is recorded, step by step")
        r.hold(2.4)

        # 3. Describe → draft → review → add
        r.click(find(win, "Overview"), lambda: win.setProperty("page", "overview"))
        ask = win.findChild(QObject, "askField")
        r.click(ask)
        r.say("Describe what you want in plain words")
        sentence = "When a PDF lands in Downloads, move it to Documents/PDFs and tell me"
        for i in range(1, len(sentence) + 1):
            win.setProperty("askText", sentence[:i])
            r.hold(0.05)
        r.hold(0.5)
        got = []
        backend.draftReady.connect(lambda d: got.append(d))
        r.click(win.findChild(QObject, "draftButton"), lambda: backend.describe(sentence), settle=0.2)
        deadline = time.time() + 30
        while not got and time.time() < deadline:
            r.hold(0.1)
        r.hold(0.6)
        r.say("Review the draft, the folders it needs, and its risk check")
        r.hold(4.2)
        r.say("Nothing runs until you approve it")
        r.click(find(win, "Grant & enable"), lambda: (
            backend.addDraft(got[0]["toml"], True),
            win.findChild(QObject, "draftPopup").close(),
            win.setProperty("askText", ""),
            win.setProperty("page", "automations")), settle=0.3)
        r.hold(2.6)

        # 4. Agent proposal
        r.click(find(win, "Proposals"), lambda: win.setProperty("page", "proposals"))
        r.say("Assistants can only propose. You decide.")
        r.hold(3.0)
        prop = next(p for p in backend.pendingProposals)
        r.click(find(win, "Approve"), lambda: backend.approve(prop["id"], True))
        r.hold(1.8)

        # 5. Live approval for a sensitive action
        r.click(find(win, "Overview"), lambda: win.setProperty("page", "overview"))
        from ambar_sdk import Client
        result = {}
        threading.Thread(target=lambda: result.update(
            Client(data, timeout=120).run_now("clean-old-logs")), daemon=True).start()
        deadline = time.time() + 15
        while not backend.approvals and time.time() < deadline:
            backend.refresh()
            r.hold(0.1)
        r.say("Sensitive actions, like moving to trash, wait for you")
        r.hold(3.4)
        r.click(find(win, "Allow"), lambda: backend.decideApproval(backend.approvals[0]["id"], True), settle=0.3)
        r.hold(2.0)

        # 6. Dark appearance + Welcome
        toggle = win.findChild(QObject, "themeToggle")
        r.say("Light and dark, matched to the whole desktop")
        r.click(toggle, lambda: win.setProperty("themeOverride", "dark"))
        r.hold(2.2)
        r.click(find(win, "Setup"), lambda: win.setProperty("page", "welcome"))
        r.say("First-run setup: service, appearance, AI and starter automations")
        r.cursor_visible = False
        r.hold(3.6)
        r.say("")

        # 7. Outro
        r.card("Ambar OS", "Automations you can trust. AI that asks first.", 4.0,
               foot="Open source · Apache-2.0 · github.com/alexbieber/AmbarOS · AI reply replayed from a file for this demo")

        out_dir.mkdir(parents=True, exist_ok=True)
        encode(r, frames, out_dir)
    finally:
        proc.terminate()
        proc.wait(timeout=30)
        shutil.rmtree(home, ignore_errors=True)
        if not keep_frames:
            shutil.rmtree(frames, ignore_errors=True)
        else:
            print("frames kept in", frames)


def encode(r: Recorder, frames: Path, out: Path):
    lst = frames / "list.txt"
    with lst.open("w") as f:
        for name, dt in r.list:
            f.write(f"file '{name}'\nduration {dt:.4f}\n")
        f.write(f"file '{r.list[-1][0]}'\n")
    mp4 = out / "ambar-demo.mp4"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst),
                    "-vf", "fps=30,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "21", "-movflags", "+faststart",
                    str(mp4)], check=True)
    # Poster: a frame from the draft review.
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-ss", "24", "-i", str(mp4), "-frames:v", "1",
                    str(out / "ambar-demo-poster.jpg")], check=True)
    # Short, small GIF for the README: the describe → review → approve part.
    pal = frames / "pal.png"
    vf = "fps=12,scale=760:-1:flags=lanczos"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-ss", "13", "-t", "20", "-i", str(mp4),
                    "-vf", f"{vf},palettegen=max_colors=128:stats_mode=diff", str(pal)], check=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-ss", "13", "-t", "20", "-i", str(mp4), "-i", str(pal),
                    "-lavfi", f"{vf}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle",
                    str(out / "ambar-demo.gif")], check=True)
    for f in ["ambar-demo.mp4", "ambar-demo.gif", "ambar-demo-poster.jpg"]:
        print(f"{out / f}  {(out / f).stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(ROOT / "docs" / "media"))
    ap.add_argument("--keep-frames", action="store_true")
    a = ap.parse_args()
    run(Path(a.out), a.keep_frames)
