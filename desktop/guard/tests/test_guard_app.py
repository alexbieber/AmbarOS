"""The Ambar Guard window shows checks, fix buttons and connections (offscreen, own process)."""
from __future__ import annotations

import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

pytest.importorskip("PySide6")
GUARD = Path(__file__).resolve().parents[1]


def test_guard_window(tmp_path):
    # Reuse the insecure fake system from test_guard.py
    script = tmp_path / "check.py"
    script.write_text(textwrap.dedent(f"""
        import os, sys, json
        sys.path.insert(0, {str(GUARD / 'tests')!r})
        import test_guard as tg
        from pathlib import Path
        tmp = Path({str(tmp_path)!r})
        root, bins = tmp / "root", tmp / "bin"
        root.mkdir(); bins.mkdir()
        gen = tg.insecure.__wrapped__(tmp)
        os.environ.update(QT_QPA_PLATFORM="offscreen", AMBAR_GUARD_ROOT=str(root),
                          PATH=f"{{bins}}:/usr/bin:/bin", HOME=str(root / "home/user"))
        import importlib.machinery, importlib.util
        loader = importlib.machinery.SourceFileLoader("app", {str(GUARD / 'ambar-guard-app')!r})
        app_mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("app", loader)); loader.exec_module(app_mod)
        from PySide6.QtWidgets import QApplication, QPushButton
        app = QApplication([])
        ran = []
        w = app_mod.GuardWindow(runner=ran.append)
        assert "0" in w.score.text(), w.score.text()
        fixes = [b.property("check") for b in w.findChildren(QPushButton) if b.text() == "Fix"]
        assert "firewall" in fixes and "ssh" in fixes and "disk-encryption" not in fixes, fixes
        next(b for b in w.findChildren(QPushButton) if b.property("check") == "firewall").click()
        w.fix_all.click()
        assert ran == [["fix", "firewall"], ["fix", "--all"]], ran
        apps = sorted(w.listening.item(r, 0).text() for r in range(w.listening.rowCount()))
        assert apps == ["python3", "redis-server", "sshd"], apps
        reach = {{w.listening.item(r, 0).text(): w.listening.item(r, 4).text() for r in range(w.listening.rowCount())}}
        assert reach["redis-server"] == "network" and reach["python3"] == "this computer only"
        assert w.talking.item(0, 0).text() == "firefox" and w.talking.item(0, 2).text() == "93.12.11.10"
        w.grab().save({str(tmp_path / 'guard.png')!r})
        print("CHECK_OK")
    """))
    r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=120)
    assert "CHECK_OK" in r.stdout, r.stdout + r.stderr
