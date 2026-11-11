"""The Workbench window drives ambar-engage: create, scan, findings, report (offscreen, own process)."""
from __future__ import annotations

import os
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

pytest.importorskip("PySide6")
ROOT = Path(__file__).resolve().parents[3]
RECON = ROOT / "target/debug/ambar-recon"
pytestmark = pytest.mark.skipif(not RECON.exists(), reason="build ambar-recon first")


def test_workbench_flow(tmp_path):
    script = tmp_path / "check.py"
    script.write_text(textwrap.dedent(f"""
        import importlib.machinery, importlib.util, os, socket, threading
        os.environ.update(QT_QPA_PLATFORM="offscreen", AMBAR_ENGAGEMENTS={str(tmp_path / 'E')!r},
                          AMBAR_CONFIG={str(tmp_path / 'cfg')!r}, AMBAR_RECON={str(RECON)!r})
        srv = socket.socket(); srv.bind(("127.0.0.1", 0)); srv.listen()
        def serve():
            while True:
                c, _ = srv.accept(); c.sendall(b"SSH-2.0-OpenSSH_9.6\\r\\n"); c.close()
        threading.Thread(target=serve, daemon=True).start()
        port = srv.getsockname()[1]
        loader = importlib.machinery.SourceFileLoader("wb", {str(ROOT / 'desktop/workbench/ambar-workbench')!r})
        wb = importlib.util.module_from_spec(importlib.util.spec_from_loader("wb", loader)); loader.exec_module(wb)
        from PySide6.QtWidgets import QApplication
        app = QApplication([])
        w = wb.Workbench()
        assert "No engagements yet" in w.scope_view.text()

        def wait():
            while w.proc and w.proc.state() != w.proc.ProcessState.NotRunning:
                app.processEvents(); w.proc.waitForFinished(50)
            app.processEvents()

        dlg = wb.NewDialog(w)
        dlg.name.setText("Lab"); dlg.include.setText("192.0.2.0/24, *.lab.example")
        dlg.auth.setText("Jane Doe, CISO, 2026-11-10"); dlg.ends.setText("2099-01-01")
        w.create(dlg.args()); wait()
        assert w.list.count() == 1 and "Jane Doe" in w.scope_view.text(), w.output.toPlainText()

        w.target.setText("127.0.0.1"); w.extra.setText(f"-p {{port}}")
        w.run_tool(); wait()
        assert w.findings.rowCount() == 1, w.output.toPlainText()
        assert "ssh" in w.findings.item(0, 2).text()

        w.target.setText("198.51.100.9"); w.run_tool(); wait()
        assert "refused" in w.output.toPlainText() and w.findings.rowCount() == 1

        # OWASP checklist page: changing a status in the table records it for the engagement.
        w.tabs.setCurrentIndex(3); app.processEvents()
        assert w.checklist.rowCount() == 109, w.checklist.rowCount()
        w.area.setCurrentIndex(w.area.findData("CONF")); app.processEvents()
        assert w.checklist.item(0, 0).text() == "WSTG-CONF-01"
        box = w.checklist.cellWidget(6, 2)
        assert box.property("test") == "WSTG-CONF-07"
        box.setCurrentIndex(1); app.processEvents()   # Passed
        import json as _j
        from pathlib import Path
        cl = _j.loads((Path({str(tmp_path / 'E')!r}) / "lab/checklist.json").read_text())
        assert cl["WSTG-CONF-07"]["status"] == "pass", cl
        assert w.cov_done.value.text() == "1/109"

        w.report("pdf"); wait()
        assert "report.pdf" in w.report_status.text(), w.report_status.text()
        w.resize(1180, 760); w.tabs.setCurrentIndex(2); app.processEvents()
        w.grab().save({str(tmp_path / 'wb.png')!r})
        print("CHECK_OK")
    """))
    r = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=180)
    assert "CHECK_OK" in r.stdout, r.stdout + r.stderr
    assert (tmp_path / "E/lab/report.pdf").exists()
