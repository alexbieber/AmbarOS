"""ambar-lab and ambar-depcheck, with a stand-in podman that records what it's asked to do."""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]

FAKE = """#!/bin/sh
echo "$@" >> "$STATE/calls"
case "$1" in
  ps) cat "$STATE/running" 2>/dev/null ;;
  run) for a; do case "$a" in ambar-lab-*) echo "$a" >> "$STATE/running";; esac; done ;;
  stop) grep -v "^$2$" "$STATE/running" > "$STATE/r2"; mv "$STATE/r2" "$STATE/running" ;;
esac
exit 0
"""


@pytest.fixture
def env(tmp_path):
    state = tmp_path / "state"
    state.mkdir()
    fake = tmp_path / "podman"
    fake.write_text(FAKE)
    fake.chmod(0o755)
    return {**os.environ, "AMBAR_PODMAN": str(fake), "STATE": str(state), "XDG_CACHE_HOME": str(tmp_path / "cache"),
            "PATH": "/usr/bin:/bin", "TZ": "Asia/Kolkata"}, state


def run(tool, env, *args):
    return subprocess.run([sys.executable, str(HERE / tool), *args], capture_output=True, text=True, env=env, timeout=30)


def test_labs_start_on_localhost_only_and_stop(env):
    e, state = env
    r = run("ambar-lab", e, "list")
    assert r.returncode == 0 and "juice-shop" in r.stdout and "webgoat" in r.stdout and "stopped" in r.stdout
    r = run("ambar-lab", e, "start", "juice-shop")
    assert r.returncode == 0 and "http://127.0.0.1:3000/" in r.stdout, r.stderr
    start = [l for l in (state / "calls").read_text().splitlines() if l.startswith("run")][-1].split()
    assert "docker.io/bkimminich/juice-shop:latest" in start and "TZ=Asia/Kolkata" in start
    ports = [start[i + 1] for i, a in enumerate(start) if a == "-p"]
    assert ports == ["127.0.0.1:3000:3000"], "published on this computer only, never on the network"
    assert "--rm" in start, "nothing is left behind when stopped"

    r = run("ambar-lab", e, "start", "webgoat")
    start = [l for l in (state / "calls").read_text().splitlines() if l.startswith("run")][-1].split()
    assert [start[i + 1] for i, a in enumerate(start) if a == "-p"] == ["127.0.0.1:8080:8080", "127.0.0.1:9090:9090"]
    assert "WebWolf: http://127.0.0.1:9090/WebWolf/" in r.stdout

    assert "already running" in run("ambar-lab", e, "start", "juice-shop").stdout
    status = json.loads(run("ambar-lab", e, "status", "--json").stdout)
    assert {x["id"]: x["running"] for x in status} == {"juice-shop": True, "webgoat": True}
    assert run("ambar-lab", e, "stop", "juice-shop").returncode == 0
    status = json.loads(run("ambar-lab", e, "status", "--json").stdout)
    assert {x["id"]: x["running"] for x in status} == {"juice-shop": False, "webgoat": True}
    r = run("ambar-lab", e, "open", "juice-shop")
    assert r.returncode == 2 and "isn't running" in r.stderr
    assert run("ambar-lab", e, "start", "dvwa").returncode == 2, "only the listed labs"


def test_every_lab_binds_to_loopback():
    import importlib.machinery
    import importlib.util
    loader = importlib.machinery.SourceFileLoader("ambar_lab", str(HERE / "ambar-lab"))
    m = importlib.util.module_from_spec(importlib.util.spec_from_loader("ambar_lab", loader))
    loader.exec_module(m)
    os.environ.setdefault("AMBAR_PODMAN", "/bin/true")
    for name in m.LABS:
        cmd = m.run_command(name)
        ports = [cmd[i + 1] for i, a in enumerate(cmd) if a == "-p"]
        assert ports and all(p.startswith("127.0.0.1:") for p in ports), (name, ports)
        assert m.LABS[name]["url"].startswith("http://127.0.0.1:")


def test_depcheck_mounts_the_project_read_only(env, tmp_path):
    e, state = env
    project = tmp_path / "my-app"
    project.mkdir()
    (project / "package.json").write_text('{"dependencies": {"lodash": "4.17.10"}}')
    r = run("ambar-depcheck", e, str(project))
    assert r.returncode == 0, r.stderr
    call = (state / "calls").read_text().split()
    assert f"{project}:/src:ro,Z" in call, "the project is mounted read-only"
    assert "docker.io/owasp/dependency-check:latest" in call
    assert call[call.index("--scan") + 1] == "/src" and call[call.index("--project") + 1] == "my-app"
    assert (project / "dependency-check-report").is_dir()
    assert (tmp_path / "cache/ambar/dependency-check").is_dir(), "the database is cached between runs"
    assert "--nvdApiKey" not in call and "NVD API key" in r.stdout

    (state / "calls").unlink()
    e2 = {**e, "NVD_API_KEY": "test-key-not-real"}
    assert run("ambar-depcheck", e2, str(project), "--format", "JSON", "--out", str(tmp_path / "out")).returncode == 0
    call = (state / "calls").read_text().split()
    assert call[call.index("--nvdApiKey") + 1] == "test-key-not-real" and call[call.index("--format") + 1] == "JSON"
    assert run("ambar-depcheck", e, str(tmp_path / "missing")).returncode == 2
