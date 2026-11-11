"""Static and harness tests for the OS image recipe (no container build here)."""
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
OS = ROOT / "os"
sys.path.insert(0, str(OS))
import smoke_test  # noqa: E402


def containerfile():
    return (OS / "Containerfile").read_text()


def test_copy_sources_exist():
    for line in containerfile().splitlines():
        m = re.match(r"COPY\s+(?!--from)(\S+(?:\s+\S+)*)\s+(\S+)$", line.strip())
        if m:
            for src in m.group(1).split():
                assert (ROOT / src).exists(), f"COPY source missing: {src}"


def test_pinned_versions_and_lint():
    cf = containerfile()
    assert "ARG FEDORA_VERSION=45" in cf
    assert cf.rstrip().endswith("RUN bootc container lint")
    assert "--locked" in cf, "reproducible Rust build"
    assert "/var/" not in cf.replace("/var/lib/containers", ""), "nothing may be baked into /var"


def test_configs_parse():
    q = tomllib.loads((OS / "config/qcow2.toml").read_text())
    assert q["customizations"]["user"][0]["name"] == smoke_test.USER
    tomllib.loads((OS / "config/iso.toml").read_text())
    k = tomllib.loads((OS / "files/usr/lib/bootc/kargs.d/10-ambar-console.toml").read_text())
    assert "console=ttyS0,115200n8" in k["kargs"]
    m = tomllib.loads((OS / "files/etc/xdg/ambar/marketplace.toml").read_text())
    assert m["index"].startswith("/usr/share/ambar/marketplace/")
    assert "/usr/share/ambar/marketplace/index.json" in containerfile()


def test_user_unit_verifies():
    if not shutil.which("systemd-analyze"):
        pytest.skip("systemd-analyze not installed")
    ambard = ROOT / "target" / "debug" / "ambard"
    if not ambard.exists():
        pytest.skip("build ambard first")
    unit = (OS / "files/usr/lib/systemd/user/ambard.service").read_text()
    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "ambard.service"
        p.write_text(unit.replace("/usr/bin/ambard", str(ambard)))
        r = subprocess.run(["systemd-analyze", "verify", "--man=no", str(p)], capture_output=True, text=True)
        assert r.returncode == 0 and not r.stderr.strip(), r.stderr
    preset = (OS / "files/usr/lib/systemd/user-preset/80-ambar.preset").read_text().split()
    assert preset == ["enable", "ambard.service"]


def test_branding_script(tmp_path):
    f = tmp_path / "os-release"
    f.write_text('NAME="Fedora Linux"\nID=fedora\nVERSION_ID=45\nPRETTY_NAME="Fedora Linux 45 (Kinoite)"\n')
    subprocess.run([str(OS / "branding.sh"), str(f), "45"], check=True)
    t = f.read_text()
    assert 'PRETTY_NAME="Ambar OS 0.1 (Fedora 45)"' in t
    assert "ID=fedora\n" in t and "VERSION_ID=45" in t, "Fedora identity kept"
    assert "IMAGE_ID=ambar-os" in t
    subprocess.run([str(OS / "branding.sh"), str(f), "45"], check=True)
    assert f.read_text() == t, "idempotent"


def test_shell_scripts():
    scripts = [OS / "build.sh", OS / "test-qemu.sh", OS / "branding.sh"]
    for s in scripts:
        subprocess.run(["bash", "-n", str(s)], check=True)
    if shutil.which("shellcheck"):
        r = subprocess.run(["shellcheck", "-S", "warning", *map(str, scripts)], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout


def fake_console(path, good=True):
    """A pretend serial console: login prompt, password, shell, canned output."""
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(path)
    srv.listen(1)

    def serve():
        c, _ = srv.accept()
        c.sendall(b"\r\nAmbar OS 0.1 (Fedora 45)\r\nambar-os login: ")
        assert c.recv(100).startswith(b"ambar")
        c.sendall(b"Password: ")
        c.recv(100)
        c.sendall(b"[ambar@ambar-os ~]$ ")
        data = b""
        while b"echo __END__" not in data:
            data += c.recv(65536)
        out = [
            "__BEGIN__", 'PRETTY_NAME="Ambar OS 0.1 (Fedora 45)"', "ambar 0.1.0", "active",
            "ambard is running (ok)",
            "dev.ambar.daily-notes 1.0.0  — Starts a dated notes file",
            "2026-11-11 10:00:00  succeeded  smoke  1a2b3c4d  manual", "ok", "__END__",
        ]
        c.sendall(("\r\n".join(out) + "\r\n").encode())
        data = b""
        while b"echo __DEND__" not in data:
            chunk = c.recv(65536)
            if not chunk:
                break
            data += chunk
        dout = ["__DBEGIN__", "PLASMA_OK", "WELCOME_OK",
                'a(sssida{sv}) 1 "create:tidy my desktop" "Create an automation: tidy my desktop"',
                "os.ambar.desktop", "os.ambar.desktop.dark", "__DEND__"]
        if not good:
            dout.remove("WELCOME_OK")
        c.sendall(("\r\n".join(dout) + "\r\n").encode())
        c.close()

    threading.Thread(target=serve, daemon=True).start()
    return srv


@pytest.mark.parametrize("good", [True, False])
def test_smoke_harness(tmp_path, good, capsys):
    sock = str(tmp_path / "s.sock")
    fake_console(sock, good)
    code = smoke_test.run(sock, str(tmp_path / "serial.log"), boot_timeout=10)
    assert code == (0 if good else 1)
    out = capsys.readouterr().out
    assert ("PASSED" in out) == good
    if not good:
        assert "welcome autostart" in out
    assert "ambar-os login:" in (tmp_path / "serial.log").read_text()
