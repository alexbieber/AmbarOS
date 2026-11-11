"""ambar-guard against a fake system: files under a fake root, stand-in commands on PATH."""
from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

GUARD = Path(__file__).resolve().parents[1] / "ambar-guard"


def load(root: Path, monkeypatch, bin_dir: Path):
    monkeypatch.setenv("AMBAR_GUARD_ROOT", str(root))
    monkeypatch.setenv("PATH", f"{bin_dir}:/usr/bin:/bin")
    monkeypatch.setenv("HOME", str(root / "home/user"))
    loader = importlib.machinery.SourceFileLoader("ambar_guard", str(GUARD))
    mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("ambar_guard", loader))
    sys.modules["ambar_guard"] = mod  # dataclasses look their module up here
    loader.exec_module(mod)
    return mod


def write(root: Path, path: str, text: str) -> None:
    f = root / path.lstrip("/")
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(text)


def stub(bin_dir: Path, name: str, out: str = "", code: int = 0) -> None:
    bin_dir.mkdir(exist_ok=True)
    s = bin_dir / name
    s.write_text(f"#!/bin/sh\ncat <<'EOF'\n{out}\nEOF\nexit {code}\n")
    s.chmod(0o755)


@pytest.fixture()
def insecure(tmp_path):
    """A computer with most things wrong."""
    root, bins = tmp_path / "root", tmp_path / "bin"
    write(root, "/etc/ufw/ufw.conf", "# managed\nENABLED=no\nLOGLEVEL=low\n")
    write(root, "/usr/sbin/sshd", "")
    write(root, "/etc/ssh/sshd_config", "Include /etc/ssh/sshd_config.d/*.conf\nPermitRootLogin yes\n#PasswordAuthentication no\n")
    write(root, "/etc/apt/apt.conf.d/20auto-upgrades", 'APT::Periodic::Update-Package-Lists "1";\nAPT::Periodic::Unattended-Upgrade "0";\n')
    write(root, "/usr/bin/unattended-upgrade", "")
    write(root, "/etc/sudoers", "root ALL=(ALL) ALL\nalex ALL=(ALL) NOPASSWD: ALL\n")
    write(root, "/home/user/.config/kscreenlockerrc", "[Daemon]\nAutolock=false\n")
    write(root, "/proc/sys/kernel/kptr_restrict", "0\n")
    write(root, "/proc/sys/kernel/dmesg_restrict", "1\n")
    write(root, "/proc/sys/kernel/yama/ptrace_scope", "1\n")
    # /proc/net/tcp: redis listening on 0.0.0.0:6379 (inode 111), a loopback-only service, one outgoing connection
    write(root, "/proc/net/tcp",
          "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n"
          "   0: 00000000:18EB 00000000:0000 0A 00000000:00000000 00:00000000 00000000   112        0 111 1\n"
          "   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 222 1\n"
          "   2: 0F02000A:D431 0A0B0C5D:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 333 1\n")
    write(root, "/proc/net/tcp6",
          "  sl  local_address rem_address   st\n"
          "   0: 00000000000000000000000000000000:0016 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000 0 0 444 1\n")
    for pid, name, inode in [(100, "redis-server", 111), (200, "python3", 222), (300, "firefox", 333), (400, "sshd", 444)]:
        write(root, f"/proc/{pid}/comm", name + "\n")
        (root / f"proc/{pid}/fd").mkdir(parents=True)
        os.symlink(f"socket:[{inode}]", root / f"proc/{pid}/fd/3")
    stub(bins, "apt", "Listing...\nopenssl/resolute-security 3.5.1 amd64 [upgradable from: 3.5.0]\nvim/resolute-updates 9.1 amd64 [upgradable from: 9.0]")
    stub(bins, "lsblk", json.dumps({"blockdevices": [{"name": "nvme0n1", "type": "disk", "mountpoints": [None], "children": [
        {"name": "nvme0n1p2", "type": "part", "mountpoints": ["/"]}]}]}))
    stub(bins, "mokutil", "SecureBoot disabled")
    return root, bins


def test_connections_are_read_from_proc(insecure, monkeypatch):
    root, bins = insecure
    g = load(root, monkeypatch, bins)
    conns = {(c["process"], c["state"]): c for c in g.connections()}
    redis = conns[("redis-server", "LISTEN")]
    assert (redis["local_ip"], redis["local_port"]) == ("0.0.0.0", 6379)
    out = conns[("firefox", "ESTABLISHED")]
    assert (out["local_ip"], out["remote_ip"], out["remote_port"]) == ("10.0.2.15", "93.12.11.10", 443)
    assert conns[("sshd", "LISTEN")]["local_ip"] == "::" and conns[("sshd", "LISTEN")]["local_port"] == 22
    assert conns[("python3", "LISTEN")]["local_ip"] == "127.0.0.1"


def test_insecure_computer_fails_the_right_checks(insecure, monkeypatch):
    root, bins = insecure
    g = load(root, monkeypatch, bins)
    checks = {c.id: c for c in g.all_checks()}
    status = {k: c.status for k, c in checks.items()}
    assert status == {
        "firewall": "fail", "exposed-services": "fail", "ssh": "fail", "pending-updates": "fail",
        "auto-updates": "fail", "disk-encryption": "fail", "screen-lock": "fail", "path-writable": "pass",
        "sudo-nopasswd": "fail", "kernel-hardening": "fail", "secure-boot": "fail",
    }, status
    assert "redis-server (tcp 6379)" in checks["exposed-services"].detail
    assert "python3" not in checks["exposed-services"].detail, "loopback-only services are fine"
    assert "root can log in" in checks["ssh"].detail.lower() and "password logins" in checks["ssh"].detail
    assert "openssl" in checks["pending-updates"].detail and "vim" not in checks["pending-updates"].detail
    assert "kernel.kptr_restrict=0" in checks["kernel-hardening"].detail
    assert g.score(list(checks.values())) == 0


def test_sshd_drop_in_wins_over_main_config(insecure, monkeypatch):
    root, bins = insecure
    write(root, "/etc/ssh/sshd_config.d/50-ambar-guard.conf", "PermitRootLogin no\nPasswordAuthentication no\n")
    g = load(root, monkeypatch, bins)
    assert g.check_ssh().status == "pass"


def test_secure_computer_scores_100(tmp_path, monkeypatch):
    root, bins = tmp_path / "root", tmp_path / "bin"
    write(root, "/etc/ufw/ufw.conf", "ENABLED=yes\n")
    write(root, "/etc/apt/apt.conf.d/20auto-upgrades", 'APT::Periodic::Update-Package-Lists "1";\nAPT::Periodic::Unattended-Upgrade "1";\n')
    write(root, "/usr/bin/unattended-upgrade", "")
    write(root, "/etc/sudoers", "root ALL=(ALL) ALL\n%sudo ALL=(ALL:ALL) ALL\n")
    for k, v in [("kptr_restrict", "1"), ("dmesg_restrict", "1"), ("yama/ptrace_scope", "1")]:
        write(root, f"/proc/sys/kernel/{k}", v)
    write(root, "/proc/net/tcp", "header\n")
    stub(bins, "apt", "Listing...")
    stub(bins, "lsblk", json.dumps({"blockdevices": [{"name": "nvme0n1", "type": "disk", "children": [
        {"name": "p3", "type": "part", "children": [{"name": "luks", "type": "crypt", "mountpoints": ["/"]}]}]}]}))
    stub(bins, "mokutil", "SecureBoot enabled")
    g = load(root, monkeypatch, bins)
    checks = g.all_checks()
    assert {c.id: c.status for c in checks if c.status == "fail"} == {}
    assert g.score(checks) == 100


def test_fix_runs_the_described_commands(insecure, tmp_path):
    root, bins = insecure
    calls = tmp_path / "calls"
    for name in ["ufw", "install", "systemctl", "sysctl", "apt-get", "kwriteconfig6", "sudo"]:
        s = bins / name
        s.write_text(f'#!/bin/sh\necho "{name} $*" >> "{calls}"\n[ "{name}" = install ] && cat >> "{calls}.stdin"\nexit 0\n')
        s.chmod(0o755)
    env = {**os.environ, "AMBAR_GUARD_ROOT": str(root), "PATH": f"{bins}:/usr/bin:/bin", "HOME": str(root / "home/user"),
           "AMBAR_GUARD_ASSUME_ROOT": "1"}
    r = subprocess.run([sys.executable, str(GUARD), "fix", "firewall", "ssh", "kernel-hardening", "disk-encryption"],
                       capture_output=True, text=True, env=env)
    assert r.returncode == 0, r.stdout + r.stderr
    log = calls.read_text()
    assert "ufw default deny incoming" in log and "ufw --force enable" in log
    assert "install -m 644 /dev/stdin /etc/ssh/sshd_config.d/50-ambar-guard.conf" in log and "systemctl reload ssh" in log
    stdin = Path(f"{calls}.stdin").read_text()
    assert "PermitRootLogin no" in stdin and "kernel.kptr_restrict = 1" in stdin
    assert "can't be fixed automatically" in r.stdout and "reinstall" in r.stdout  # disk encryption: advice only


def test_fix_dry_run_changes_nothing(insecure, tmp_path):
    root, bins = insecure
    env = {**os.environ, "AMBAR_GUARD_ROOT": str(root), "PATH": f"{bins}:/usr/bin:/bin", "HOME": str(root / "home/user"),
           "AMBAR_GUARD_ASSUME_ROOT": "0"}
    r = subprocess.run([sys.executable, str(GUARD), "fix", "--all", "--dry-run"], capture_output=True, text=True, env=env)
    assert r.returncode == 0
    assert "$ sudo ufw --force enable" in r.stdout, "system fixes go through sudo"
    assert "$ kwriteconfig6" in r.stdout and "$ sudo kwriteconfig6" not in r.stdout, "the screen lock is your own setting"


def test_json_output(insecure, tmp_path):
    root, bins = insecure
    env = {**os.environ, "AMBAR_GUARD_ROOT": str(root), "PATH": f"{bins}:/usr/bin:/bin", "HOME": str(root / "home/user")}
    r = subprocess.run([sys.executable, str(GUARD), "check", "--json"], capture_output=True, text=True, env=env)
    data = json.loads(r.stdout)
    assert data["score"] == 0 and len(data["checks"]) == 11
    r = subprocess.run([sys.executable, str(GUARD), "net", "--listening", "--json"], capture_output=True, text=True, env=env)
    assert {c["process"] for c in json.loads(r.stdout)} == {"redis-server", "python3", "sshd"}
