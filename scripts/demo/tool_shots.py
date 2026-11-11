#!/usr/bin/env python3
"""Screenshots of Ambar's own tools (Security, Workbench, Guard, Clean, Share), light and dark, at 2x.

Each app runs offscreen in its own process against fake data, so nothing on this
computer is scanned or changed.

    python3 scripts/demo/tool_shots.py                 # every app → docs/images/os/<app>.jpg (+ -dark.jpg)
    python3 scripts/demo/tool_shots.py guard --out /tmp/shots
"""
from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs/images/os"
FILES = {"security": "security-app"}  # file names the docs already use

COMMON = """
import os, sys, importlib.machinery, importlib.util
from pathlib import Path
os.environ.update(QT_QPA_PLATFORM="offscreen", QT_SCALE_FACTOR="2", AMBAR_THEME={theme!r})
def load(path, name):
    l = importlib.machinery.SourceFileLoader(name, path)
    m = importlib.util.module_from_spec(importlib.util.spec_from_loader(name, l)); l.exec_module(m); return m
from PySide6.QtWidgets import QApplication
app = QApplication([])
sys.path.insert(0, {ui!r}); import ambar_ui; ambar_ui.apply(app)
def save(w, size, page=None):
    w.resize(*size)
    if page is not None:
        (getattr(w, "shell", None) or w.tabs).setCurrentIndex(page)
    w.show()
    for _ in range(20): app.processEvents()
    img = w.grab().toImage()
    img.save({out!r}, "JPG", 90)
"""

APPS = {
    "guard": (1040, 740, """
        sys.path.insert(0, {root!r} + "/desktop/guard/tests")
        import test_guard as tg
        tmp = Path({tmp!r}); root, bins = tmp / "root", tmp / "bin"; root.mkdir(); bins.mkdir()
        tg.insecure.__wrapped__(tmp)
        os.environ.update(AMBAR_GUARD_ROOT=str(root), PATH=f"{{bins}}:/usr/bin:/bin", HOME=str(root / "home/user"))
        m = load({root!r} + "/desktop/guard/ambar-guard-app", "app")
        w = m.GuardWindow(runner=lambda a: None); w.timer.stop()
        save(w, (1040, 740))
    """),
    "clean": (860, 640, """
        home = Path({tmp!r}) / "home"
        for d, n in ((".cache/pip", 412_000_000), (".cache/thumbnails", 96_000_000), (".cache/mesa_shader_cache", 31_000_000),
                     (".local/share/Trash/files", 1_250_000_000)):
            (home / d).mkdir(parents=True); f = home / d / "data"; f.touch(); os.truncate(f, n)
        os.environ.update(HOME=str(home), AMBAR_CLEAN_ROOT={tmp!r} + "/sys", PATH="/nonexistent")
        m = load({root!r} + "/desktop/utils/ambar-clean-app", "app")
        w = m.CleanWindow(runner=lambda a: None)
        save(w, (860, 640))
    """),
    "share": (920, 620, """
        files = []
        for name, n in (("Holiday 2026.jpg", 4_200_000), ("Quarterly report.pdf", 860_000), ("Talk recording.mp4", 182_000_000)):
            f = Path({tmp!r}) / name; f.touch(); os.truncate(f, n); files.append(f)
        m = load({root!r} + "/desktop/utils/ambar-share-app", "app")
        w = m.ShareWindow(host="192.168.1.24")
        w.add_files(files)
        w.m.serve = lambda share, host, port: (type("S", (), dict(serve_forever=lambda s: None, shutdown=lambda s: None))(),
                                               f"http://{{host}}:8731/k3Xq9vR2mT7wLp0sYb4NcA/Holiday%202026.jpg")
        w.toggle(); w.timer.stop()
        w.status.setText("Link copied. Sharing for 30 minutes · 0 downloads")
        save(w, (920, 620))
    """),
    "security": (1120, 780, """
        os.environ["AMBAR_SECURITY_NO_PROBE"] = "1"
        m = load({root!r} + "/desktop/security/ambar-security", "app")
        w = m.Window(runner=lambda a: None)
        save(w, (1120, 780), int(os.environ.get("SHOT_PAGE", "0")))
    """),
    "workbench": (1180, 780, """
        import json
        E = Path({tmp!r}) / "E"; d = E / "acme-external"; d.mkdir(parents=True)
        (d / "scope.toml").write_text('[engagement]\\nname = "ACME external"\\nauthorised_by = "Jane Doe, CISO, letter 2026-11-10"\\n'
            'starts = 2026-11-12\\nends = 2026-11-30\\n[scope]\\ninclude = ["203.0.113.0/24", "acme.example", "*.acme.example"]\\n'
            'exclude = ["203.0.113.1", "vpn.acme.example"]\\n')
        F = [("high", "203.0.113.10", "Docker API exposed (tcp 2375)", "ambar-recon"),
             ("high", "shop.acme.example", "Default admin password", "manual"),
             ("medium", "203.0.113.10", "Redis reachable without auth (tcp 6379)", "ambar-recon"),
             ("medium", "https://shop.acme.example", "Missing Content-Security-Policy header", "ambar-recon"),
             ("low", "https://shop.acme.example", "Cookie session without Secure flag", "ambar-recon"),
             ("low", "mail.acme.example:443", "Certificate expires in 21 days", "ambar-recon"),
             ("info", "203.0.113.12", "OpenSSH 9.6p1 on tcp 22", "nmap"),
             ("info", "shop.acme.example", "nginx 1.26, PHP 8.3, WordPress", "ambar-recon")]
        (d / "findings.json").write_text(json.dumps([dict(severity=s, target=t, title=x, source=src, detail="", found="2026-11-12T10:42:00")
                                                     for s, t, x, src in F]))
        import datetime
        cl = {{t: {{"status": st, "updated": "2026-11-12T10:00:00"}} for t, st in (
            ("WSTG-INFO-01", "pass"), ("WSTG-INFO-02", "pass"), ("WSTG-INFO-03", "pass"), ("WSTG-INFO-06", "pass"),
            ("WSTG-CONF-02", "issue"), ("WSTG-CONF-06", "pass"), ("WSTG-CONF-07", "issue"), ("WSTG-CONF-11", "na"),
            ("WSTG-ATHN-02", "issue"), ("WSTG-SESS-02", "issue"), ("WSTG-CRYP-01", "pass"))}}
        cl["WSTG-CONF-11"]["note"] = "No cloud storage in scope"
        (d / "checklist.json").write_text(json.dumps(cl))
        fake = Path({tmp!r}) / "podman"
        fake.write_text(chr(10).join(["#!/bin/sh", '[ "$1" = ps ] && echo ambar-lab-juice-shop', "exit 0", ""])); fake.chmod(0o755)
        os.environ.update(AMBAR_ENGAGEMENTS=str(E), AMBAR_CONFIG={tmp!r} + "/cfg", AMBAR_PODMAN=str(fake))
        m = load({root!r} + "/desktop/workbench/ambar-workbench", "app")
        w = m.Workbench()
        w.findings.selectRow(0); w.show_detail(0, 0)
        w.refresh_labs()
        save(w, (1180, 780), int(os.environ.get("SHOT_PAGE", "2")))
    """),
}


def shot(app: str, theme: str, out: Path) -> None:
    w, h, body = APPS[app]
    with tempfile.TemporaryDirectory() as tmp:
        fmt = dict(theme=theme, ui=str(ROOT / "desktop/share/python"), out=str(out), root=str(ROOT), tmp=tmp)
        script = Path(tmp) / "shot.py"
        script.write_text(COMMON.format(**fmt) + textwrap.dedent(body).format(**fmt))
        r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=300, cwd=tmp)
        if r.returncode or not out.exists():
            raise SystemExit(f"{app} ({theme}) failed:\n{r.stdout}{r.stderr}")
    print(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("apps", nargs="*", default=list(APPS))
    ap.add_argument("--out", type=Path, default=OUT)
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    for app in a.apps:
        name = FILES.get(app, app)
        shot(app, "light", a.out / f"{name}.jpg")
        shot(app, "dark", a.out / f"{name}-dark.jpg")


if __name__ == "__main__":
    main()
