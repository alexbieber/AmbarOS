"""Ambar Clean and Ambar Share windows (offscreen, each in its own process)."""
from __future__ import annotations

import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

pytest.importorskip("PySide6")
UTILS = Path(__file__).resolve().parents[1]


def run_isolated(tmp_path: Path, body: str) -> None:
    script = tmp_path / "check.py"
    script.write_text(textwrap.dedent(body) + "\nprint('CHECK_OK')\n")
    r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=120)
    assert "CHECK_OK" in r.stdout, r.stdout + r.stderr


def test_clean_window(tmp_path):
    home = tmp_path / "home"
    (home / ".cache/pip").mkdir(parents=True)
    (home / ".cache/pip/x").write_bytes(b"x" * 400_000)
    (home / ".local/share/Trash/files").mkdir(parents=True)
    (home / ".local/share/Trash/files/t").write_bytes(b"t" * 100_000)
    run_isolated(tmp_path, f"""
        import os, importlib.machinery, importlib.util
        os.environ.update(QT_QPA_PLATFORM="offscreen", HOME={str(home)!r}, AMBAR_CLEAN_ROOT={str(tmp_path / 'sys')!r},
                          PATH="/nonexistent")
        l = importlib.machinery.SourceFileLoader("app", {str(UTILS / 'ambar-clean-app')!r})
        m = importlib.util.module_from_spec(importlib.util.spec_from_loader("app", l)); l.exec_module(m)
        from PySide6.QtWidgets import QApplication
        app = QApplication([])
        ran = []
        w = m.CleanWindow(runner=ran.append)
        assert w.boxes["caches"].isChecked() and not w.boxes["trash"].isChecked()
        assert "400.0 KB" in w.total.text(), w.total.text()
        w.boxes["trash"].setChecked(True)
        assert "500.0 KB" in w.total.text()
        w.clean()
        assert ran == [["run", "caches", "trash", "--yes"]], ran
    """)


def test_share_window(tmp_path):
    f = tmp_path / "report.pdf"
    f.write_bytes(b"%PDF-1.7 test")
    run_isolated(tmp_path, f"""
        import os, urllib.request, importlib.machinery, importlib.util
        from pathlib import Path
        os.environ["QT_QPA_PLATFORM"] = "offscreen"
        l = importlib.machinery.SourceFileLoader("app", {str(UTILS / 'ambar-share-app')!r})
        m = importlib.util.module_from_spec(importlib.util.spec_from_loader("app", l)); l.exec_module(m)
        from PySide6.QtWidgets import QApplication
        app = QApplication([])
        w = m.ShareWindow(host="127.0.0.1")
        w.add_files([Path({str(f)!r}), Path({str(f)!r}), Path("/nonexistent")])
        assert w.list.count() == 1
        w.toggle()
        url = w.url.text()
        assert url.startswith("http://127.0.0.1:") and url.endswith("/report.pdf"), url
        assert urllib.request.urlopen(url, timeout=5).read() == b"%PDF-1.7 test"
        w.tick()
        assert "1 download" in w.status.text(), w.status.text()
        w.toggle()
        assert w.start.text() == "Share" and w.server is None
        try:
            urllib.request.urlopen(url, timeout=2)
            raise AssertionError("still serving after Stop")
        except OSError:
            pass
    """)
