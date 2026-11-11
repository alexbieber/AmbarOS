#!/usr/bin/env python3
"""Takes the README / docs screenshots from the real software at 2× scale.

Same setup as the demo video: a real ambard in a throwaway home, the real
Automation Center rendered offscreen, the AI reply replayed from a file.

    python3 scripts/demo/screenshots.py          # → docs/images/screens/*.png
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

os.environ.setdefault("DEMO_SCALE", "2")
sys.path.insert(0, str(Path(__file__).resolve().parent))
import record_demo as demo  # noqa: E402  (sets QT_SCALE_FACTOR from DEMO_SCALE)

ROOT = demo.ROOT
OUT = ROOT / "docs" / "images" / "screens"


def main():
    home = Path(os.environ.get("DEMO_HOME", "/home/alex"))
    if home.exists() and any(home.iterdir()):
        raise SystemExit(f"{home} exists and isn't empty; set DEMO_HOME")
    home.mkdir(parents=True, exist_ok=True)
    os.environ.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                      XDG_DATA_HOME=str(home / ".local/share"))
    sys.path.insert(0, str(ROOT / "sdk" / "python"))
    sys.path.insert(0, str(ROOT / "desktop" / "automation-center"))
    proc, data = demo.build_world(home)
    OUT.mkdir(parents=True, exist_ok=True)
    try:
        script = home / "ai-reply.txt"
        script.write_text("```toml\n" + demo.PDF_RULE.strip() + "\n```")
        os.environ["AMBAR_AI_SCRIPT"] = str(script)
        os.environ["AMBAR_MARKETPLACE"] = str(ROOT / "marketplace" / "index.json")
        from PySide6.QtCore import QObject
        from ambar_center.app import create
        app, engine, backend = create(["shots"], str(data), theme="light", poll_ms=0)
        backend._ambar = str(demo.BIN / "ambar")
        backend.refresh()
        win = engine.rootObjects()[0]

        def pump(s=0.5):
            end = time.time() + s
            while time.time() < end:
                app.processEvents()
                time.sleep(0.01)

        def shot(name, theme, page=None):
            win.setProperty("themeOverride", theme)
            if page:
                win.setProperty("page", page)
            pump(0.6)
            img = win.grabWindow()
            assert img.save(str(OUT / f"{name}.png")), name
            print(OUT / f"{name}.png", img.width(), "x", img.height())

        pump(0.5)
        for theme in ("light", "dark"):
            shot(f"overview-{theme}", theme, "overview")
            shot(f"automations-{theme}", theme, "automations")
        shot("activity-dark", "dark", "activity")
        shot("agents-light", "light", "agents")
        shot("proposals-light", "light", "proposals")
        shot("welcome-dark", "dark", "welcome")

        # AI draft review
        got = []
        backend.draftReady.connect(lambda d: got.append(d))
        win.setProperty("page", "overview")
        sentence = "When a PDF lands in Downloads, move it to Documents/PDFs and tell me"
        win.setProperty("askText", sentence)
        backend.describe(sentence)
        deadline = time.time() + 30
        while not got and time.time() < deadline:
            pump(0.1)
        shot("draft-review-light", "light")
        win.findChild(QObject, "draftPopup").close()
        win.setProperty("askText", "")
        pump(0.3)

        # Live approval
        from ambar_sdk import Client
        threading.Thread(target=lambda: Client(data, timeout=120).run_now("clean-old-logs"), daemon=True).start()
        deadline = time.time() + 15
        while not backend.approvals and time.time() < deadline:
            backend.refresh()
            pump(0.1)
        shot("approval-light", "light", "overview")
        shot("approval-dark", "dark", "overview")
        backend.decideApproval(backend.approvals[0]["id"], False)
        pump(0.5)
    finally:
        proc.terminate()
        proc.wait(timeout=30)
        import shutil
        shutil.rmtree(home, ignore_errors=True)

    installer()


INSTALLER = r"""
import importlib.util, json, sys
from PySide6.QtWidgets import QApplication
spec = importlib.util.spec_from_file_location("i", sys.argv[1]); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
app = QApplication([])
w = m.InstallerWindow("ghcr.io/alexbieber/ambar-os:latest", json.loads(sys.argv[2]))
w.disks.setCurrentRow(0); w.confirm.setText(w.selected())
w.show(); app.processEvents()
w.grab().save(sys.argv[3]); print(sys.argv[3])
"""


def installer():
    gb = 1024 ** 3
    tree = {"blockdevices": [
        {"name": "nvme0n1", "path": "/dev/nvme0n1", "size": 512 * gb, "type": "disk", "tran": "nvme",
         "rm": False, "ro": False, "model": "Example NVMe SSD", "mountpoints": [None]},
        {"name": "sdb", "path": "/dev/sdb", "size": 2000 * gb, "type": "disk", "tran": "sata", "rm": False,
         "ro": False, "model": "Example HDD", "mountpoints": [None]},
        {"name": "sda", "path": "/dev/sda", "size": 32 * gb, "type": "disk", "tran": "usb", "rm": True,
         "ro": False, "model": "USB stick", "mountpoints": [None],
         "children": [{"name": "sda1", "path": "/dev/sda1", "type": "part", "label": "AmbarOS-Live",
                       "mountpoints": ["/run/initramfs/live"]}]}]}
    subprocess.run([sys.executable, "-c", INSTALLER, str(ROOT / "os/live/installer/ambar_install.py"),
                    json.dumps(tree), str(OUT / "installer.png")], check=True,
                   env={**os.environ, "QT_QPA_PLATFORM": "offscreen", "QT_SCALE_FACTOR": "2",
                        "AMBAR_LOGO": str(ROOT / "desktop/brand/ambar-mark.svg")})


if __name__ == "__main__":
    main()
