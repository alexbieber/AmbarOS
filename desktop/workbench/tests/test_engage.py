"""ambar-engage end to end, with the real ambar-recon binary and real local services."""
from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
ENGAGE = ROOT / "desktop/workbench/ambar-engage"
RECON = ROOT / "target/debug/ambar-recon"
pytestmark = pytest.mark.skipif(not RECON.exists(), reason="build ambar-recon first: cargo build -p ambar-recon")


def engage(tmp_path, *args):
    env = {**os.environ, "AMBAR_ENGAGEMENTS": str(tmp_path / "Engagements"), "AMBAR_CONFIG": str(tmp_path / "cfg"),
           "AMBAR_RECON": str(RECON), "QT_QPA_PLATFORM": "offscreen"}
    return subprocess.run([sys.executable, str(ENGAGE), *args], capture_output=True, text=True, env=env, timeout=120)


@pytest.fixture()
def services():
    """A Redis-like server (speaks first) and a web server, on free local ports."""
    redis = socket.socket()
    redis.bind(("127.0.0.1", 0))
    redis.listen()

    def serve_redis():
        while True:
            try:
                c, _ = redis.accept()
            except OSError:
                return
            c.sendall(b"-ERR unknown command\r\n")
            c.close()

    threading.Thread(target=serve_redis, daemon=True).start()

    class H(BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header("Server", "nginx")
            self.send_header("Set-Cookie", "sid=1; Path=/")
            self.end_headers()
            self.wfile.write(b"<title>Lab</title>")

        def log_message(self, *a):
            pass

    web = ThreadingHTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=web.serve_forever, daemon=True).start()
    yield redis.getsockname()[1], web.server_address[1]
    web.shutdown()
    redis.close()


def new(tmp_path):
    r = engage(tmp_path, "new", "Lab Test", "--include", "192.0.2.0/24", "*.lab.example", "--exclude", "192.0.2.1",
               "--authorised-by", "Jane Doe, CISO, 2026-11-10", "--ends", "2099-12-31")
    assert r.returncode == 0, r.stderr
    return tmp_path / "Engagements/lab-test"


def test_new_engagement_needs_an_authorisation_and_a_valid_scope(tmp_path):
    r = engage(tmp_path, "new", "x", "--include", "192.0.2.0/24", "--authorised-by", "me")
    assert r.returncode == 2 and "who authorised" in r.stderr
    r = engage(tmp_path, "new", "y", "--include", "not a host!", "--authorised-by", "Jane Doe, CISO")
    assert r.returncode == 2 and "isn't valid" in r.stderr
    assert not (tmp_path / "Engagements/y").exists()
    d = new(tmp_path)
    scope = (d / "scope.toml").read_text()
    assert 'authorised_by = "Jane Doe, CISO, 2026-11-10"' in scope and "ends = 2099-12-31" in scope


def test_scans_become_findings_and_a_report(tmp_path, services):
    redis_port, web_port = services
    d = new(tmp_path)
    r = engage(tmp_path, "run", "recon-ports", "127.0.0.1", "--", "-p", f"{redis_port},{web_port}")
    assert r.returncode == 0, r.stdout + r.stderr
    r = engage(tmp_path, "run", "recon-http", f"http://127.0.0.1:{web_port}/")
    assert r.returncode == 0, r.stdout + r.stderr
    # Running the same scan again doesn't duplicate findings.
    engage(tmp_path, "run", "recon-ports", "127.0.0.1", "--", "-p", f"{redis_port},{web_port}")
    f = json.loads((d / "findings.json").read_text())
    titles = [(x["severity"], x["title"]) for x in f]
    assert ("medium", "Redis exposed") not in titles  # a random port isn't 6379: it's reported as an open port
    assert ("low", "Missing security headers") in titles
    assert ("low", "Cookies without HttpOnly/Secure") in titles
    assert sum(x["title"].startswith("Open port") for x in f) == 2
    assert [x["severity"] for x in f] == sorted((x["severity"] for x in f), key=["critical", "high", "medium", "low", "info"].index)
    r = engage(tmp_path, "report", "--format", "pdf")
    assert r.returncode == 0, r.stderr
    md = (d / "report.md").read_text()
    assert "Jane Doe, CISO" in md and "Missing security headers" in md and "| Low | 2 |" in md
    assert (d / "report.pdf").read_bytes()[:5] == b"%PDF-"
    assert "<h1>" in (d / "report.html").read_text()


def test_out_of_scope_targets_are_refused_and_logged(tmp_path):
    d = new(tmp_path)
    for target in ["198.51.100.7", "192.0.2.1", "evil.example"]:
        r = engage(tmp_path, "run", "nmap", target)
        assert r.returncode == 2 and "refused" in r.stderr, (target, r.stderr)
    log = (d / "activity.log").read_text()
    assert log.count("REFUSED nmap") == 3
    assert not list((d / "runs").iterdir()), "nothing ran"
    r = engage(tmp_path, "findings", "--add", "x", "--target", "198.51.100.7")
    assert r.returncode == 2


def test_nmap_and_nuclei_output_is_imported(tmp_path):
    sys.path.insert(0, str(ENGAGE.parent))
    import importlib.machinery
    import importlib.util
    loader = importlib.machinery.SourceFileLoader("ambar_engage", str(ENGAGE))
    mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("ambar_engage", loader))
    loader.exec_module(mod)
    out = tmp_path / "run"
    out.with_suffix(".xml").write_text("""<?xml version="1.0"?><nmaprun><host><address addr="192.0.2.5" addrtype="ipv4"/>
      <ports><port protocol="tcp" portid="2375"><state state="open"/><service name="docker"/></port>
      <port protocol="tcp" portid="443"><state state="open"/><service name="https" product="nginx" version="1.27"/></port>
      <port protocol="tcp" portid="25"><state state="closed"/></port></ports></host></nmaprun>""")
    f = mod.import_results("nmap", "192.0.2.5", out)
    assert [(x["severity"], x["target"]) for x in f] == [("high", "192.0.2.5:2375"), ("info", "192.0.2.5:443")]
    assert f[1]["detail"] == "nginx 1.27"
    out.with_suffix(".jsonl").write_text(json.dumps({"template-id": "git-config", "matched-at": "https://x.lab.example/.git/config",
                                                     "info": {"name": "Exposed .git config", "severity": "medium", "description": "d"}}) + "\n")
    f = mod.import_results("nuclei", "x.lab.example", out)
    assert f[0]["severity"] == "medium" and f[0]["source"] == "nuclei:git-config"
