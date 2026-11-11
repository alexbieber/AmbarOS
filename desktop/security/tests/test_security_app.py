"""The Ambar Security window builds the right ambar-sec command (offscreen Qt).

Each check runs in its own Python process: a QApplication can't be created where
another test already made a QGuiApplication (the Automation Center tests do).
"""
from __future__ import annotations

import os
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

pytest.importorskip("PySide6")
SEC = Path(__file__).resolve().parents[1]

PRELUDE = f"""
import importlib.machinery, importlib.util, os
os.environ["QT_QPA_PLATFORM"] = "offscreen"
os.environ["AMBAR_SECURITY_NO_PROBE"] = "1"
os.environ["AMBAR_SEC_CATALOG"] = {str(SEC / "catalog.toml")!r}
loader = importlib.machinery.SourceFileLoader("ambar_security", {str(SEC / "ambar-security")!r})
app_mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("ambar_security", loader))
loader.exec_module(app_mod)
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication, QLabel
app = QApplication([])
"""


def run_isolated(body: str, tmp_path: Path) -> None:
    script = tmp_path / "check.py"
    script.write_text(PRELUDE + textwrap.dedent(body) + "\nprint('CHECK_OK')\n")
    r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=120)
    assert "CHECK_OK" in r.stdout, r.stdout + r.stderr


def test_window_builds_install_command(tmp_path):
    run_isolated(f"""
    ran = []
    w = app_mod.Window(runner=ran.append)
    # Default tier is preselected
    assert w.targets() == ["default"], w.targets()
    assert "tools" in w.summary.text() and w.install_btn.isEnabled()
    # Pick "Everything", add the Wireless category and one tool by its checkbox
    for b in w.tier.buttons():
        if b.property("tier") == "everything":
            b.setChecked(True)
    w.cats["wireless"].setChecked(True)
    row = next(r for r in range(w.table.rowCount()) if w.table.item(r, 0).data(Qt.UserRole) == "nuclei")
    w.table.item(row, 0).setCheckState(Qt.Checked)
    assert w.targets() == ["everything", "wireless", "nuclei"], w.targets()
    assert "kali-linux-everything" in w.summary.text()
    w.install()
    assert ran == [["install", "everything", "wireless", "nuclei"]], ran
    # No tier and nothing ticked: nothing to install
    w.cats["wireless"].setChecked(False)
    w.table.item(row, 0).setCheckState(Qt.Unchecked)
    for b in w.tier.buttons():
        if b.property("tier") == "none":
            b.setChecked(True)
    assert not w.install_btn.isEnabled()
    # Search filters the table
    w.filter.setText("hashcat")
    rows = [r for r in range(w.table.rowCount()) if not w.table.isRowHidden(r)]
    assert "hashcat" in [w.table.item(r, 1).text() for r in rows]
    assert all("hashcat" in " ".join(w.table.item(r, c).text() for c in range(1, 5)).lower() for r in rows)
    assert len(rows) < 5
    grab = w.grab()
    grab.save({str(tmp_path / "security.png")!r})
    assert grab.width() > 500
    """, tmp_path)
    assert (tmp_path / "security.png").stat().st_size > 10_000


def test_first_run_explains_the_kali_container(tmp_path):
    run_isolated("""
    w = app_mod.Window(runner=lambda a: None, first_run=True)
    text = " ".join(l.text() for l in w.findChildren(QLabel))
    assert "Welcome to Ambar OS Security" in text and "Kali container" in text
    """, tmp_path)
