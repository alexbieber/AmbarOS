"""ambar-sec: catalogue integrity, target resolution, and real installs against stand-ins.

The GitHub path runs against a local HTTP server that serves a release the way
GitHub does (release JSON, archive, checksums file), so the checksum check is
exercised for real, including a tampered download that must be refused.
"""
from __future__ import annotations

import hashlib
import importlib.machinery
import importlib.util
import io
import json
import os
import subprocess
import sys
import tarfile
import threading
import tomllib
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

SEC = Path(__file__).resolve().parents[1]
CLI = SEC / "ambar-sec"
CATALOG = SEC / "catalog.toml"


def load_module(root: Path | None = None):
    if root is not None:
        os.environ["AMBAR_SEC_ROOT"] = str(root)
    loader = importlib.machinery.SourceFileLoader("ambar_sec", str(CLI))
    spec = importlib.util.spec_from_loader("ambar_sec", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


@pytest.fixture()
def sec(tmp_path, monkeypatch):
    monkeypatch.setenv("AMBAR_SEC_CATALOG", str(CATALOG))
    monkeypatch.setenv("AMBAR_SEC_ASSUME_ROOT", "1")
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    mod = load_module(tmp_path / "root")
    yield mod
    os.environ.pop("AMBAR_SEC_ROOT", None)


# ----------------------------------------------------------------- catalogue

def catalog() -> dict:
    return tomllib.loads(CATALOG.read_text())


def test_catalog_is_well_formed():
    c = catalog()
    names = [t["name"] for t in c["tool"]]
    assert len(names) == len(set(names)), "duplicate tool names"
    for t in c["tool"]:
        assert t["cat"] in c["categories"], t
        assert t["tier"] in ("top10", "default"), t  # large/everything come from the Kali container
        assert t["src"] in ("apt", "pipx", "github", "kali"), t
        assert t.get("about"), t
        if t["src"] == "github":
            assert "/" in t["pkg"], t
    assert {"large", "everything"} <= set(c["kali"])
    assert c["kali"]["image"].startswith("docker.io/kalilinux/kali-rolling")


def test_every_kali_default_tool_is_covered_or_excluded_with_a_reason():
    c = catalog()
    have = set(c["excluded"])
    for t in c["tool"]:
        have.update(t.get("pkg", t["name"]).split())
        have.add(t["name"])
        have.update(r.split()[0] for r in t.get("replaces", []))
    kali = {l.strip() for l in (SEC / "kali-default.txt").read_text().splitlines()
            if l.strip() and not l.startswith("#")}
    assert len(kali) > 190
    assert not sorted(kali - have), "Kali default tools missing from the catalogue"
    assert all(len(reason) > 20 for reason in c["excluded"].values())


def test_kali_top10_is_the_top10_tier():
    top = {t["name"] for t in catalog()["tool"] if t["tier"] == "top10"}
    # kali-tools-top10, with netexec as Kali itself now ships it
    assert top == {"nmap", "wireshark", "aircrack-ng", "hydra", "john", "sqlmap", "netexec",
                   "msfconsole", "burpsuite", "responder"}


def test_modern_replacements_are_recorded():
    rep = {t["name"]: t.get("replaces", []) for t in catalog()["tool"]}
    assert "crackmapexec" in rep["netexec"]
    assert "enum4linux" in rep["enum4linux-ng"]
    assert "dirbuster" in rep["ffuf"] and "dirbuster" in rep["feroxbuster"]
    assert "hash-identifier" in rep["name-that-hash"]


def test_network_services_stay_off():
    disable = set(catalog()["services"]["disable"])
    assert {"apache2", "smbd", "snmpd", "mariadb", "atftpd", "inetsim"} <= disable


# ----------------------------------------------------------------- resolution

def test_tiers_are_cumulative_and_large_adds_kali_meta(sec):
    cat = sec.load(CATALOG)
    top, meta = sec.resolve(cat, ["top10"])
    default, _ = sec.resolve(cat, ["default"])
    large, lmeta = sec.resolve(cat, ["large"])
    assert len(top) == 10 and not meta
    assert {t["name"] for t in top} < {t["name"] for t in default}
    assert {t["name"] for t in large} == {t["name"] for t in default}
    assert lmeta == ["kali-linux-large"]
    assert sec.resolve(cat, ["everything"])[1] == ["kali-linux-everything"]


def test_category_and_tool_targets(sec):
    cat = sec.load(CATALOG)
    wifi, _ = sec.resolve(cat, ["wireless"])
    assert wifi and all(t["cat"] == "wireless" for t in wifi)
    assert [t["name"] for t in sec.resolve(cat, ["nuclei"])[0]] == ["nuclei"]
    with pytest.raises(sec.Fail, match="did you mean wireless"):
        sec.resolve(cat, ["wireles"])


# ----------------------------------------------------------------- stand-in tools

def stubs(tmp_path: Path) -> tuple[Path, Path]:
    """apt-get, pipx, systemctl, dpkg-query, distrobox that record their arguments."""
    bin_dir, calls = tmp_path / "bin", tmp_path / "calls.log"
    bin_dir.mkdir()
    for name in ["apt-get", "pipx", "systemctl", "distrobox", "sudo"]:
        p = bin_dir / name
        p.write_text(f'#!/bin/sh\necho "{name} $*" >> "{calls}"\n'
                     + ('[ "$1" = list ] && exit 0\n' if name == "distrobox" else "")
                     + "exit 0\n")
        p.chmod(0o755)
    return bin_dir, calls


def run_cli(tmp_path, *args, env=None):
    bin_dir, calls = stubs(tmp_path) if not (tmp_path / "bin").exists() else (tmp_path / "bin", tmp_path / "calls.log")
    e = {**os.environ, "PATH": f"{bin_dir}:/usr/bin:/bin", "AMBAR_SEC_CATALOG": str(CATALOG),
         "AMBAR_SEC_ROOT": str(tmp_path / "root"), "HOME": str(tmp_path / "home"),
         "AMBAR_SEC_ASSUME_ROOT": "0", **(env or {})}
    r = subprocess.run([sys.executable, str(CLI), *args], capture_output=True, text=True, env=e)
    return r, (calls.read_text() if calls.exists() else "")


def test_install_category_as_root_runs_apt_pipx_and_disables_services(tmp_path):
    r, calls = run_cli(tmp_path, "install", "passwords", "--system-only", env={"AMBAR_SEC_ASSUME_ROOT": "1"})
    assert r.returncode == 0, r.stderr + r.stdout
    assert "Use these tools only on systems you own" in r.stdout
    assert "apt-get install -y" in calls and "hashcat" in calls and "john" in calls
    assert "pipx install --global name-that-hash" in calls
    assert "distrobox" not in calls  # --system-only
    launchers = list((tmp_path / "root/usr/share/applications").glob("ambar-sec-*.desktop"))
    assert any(l.name == "ambar-sec-hashcat.desktop" for l in launchers)
    entry = (tmp_path / "root/usr/share/applications/ambar-sec-hashcat.desktop").read_text()
    assert "X-Ambar-Security-passwords" in entry and "Terminal=true" in entry


def test_kali_tools_go_into_the_container_as_the_user(tmp_path):
    r, calls = run_cli(tmp_path, "install", "msfconsole", "burpsuite")
    assert r.returncode == 0, r.stderr + r.stdout
    assert "distrobox create --yes --name ambar-kali --image docker.io/kalilinux/kali-rolling" in calls
    assert "apt-get install -y burpsuite metasploit-framework" in calls
    assert "distrobox-export --bin /usr/bin/msfconsole" in calls
    assert "distrobox-export --app burpsuite" in calls
    assert "sudo" not in calls.replace("-- sudo", "")  # the container part never asks for host root


def test_non_root_install_elevates_only_the_system_part(tmp_path):
    r, calls = run_cli(tmp_path, "install", "nmap", "responder")
    assert r.returncode == 0, r.stderr + r.stdout
    sudo_lines = [l for l in calls.splitlines() if l.startswith("sudo /usr/bin/env")]
    assert len(sudo_lines) == 1 and "_system nmap" in sudo_lines[0] and "responder" not in sudo_lines[0]


def test_root_skips_the_kali_container(tmp_path):
    r, calls = run_cli(tmp_path, "install", "top10", env={"AMBAR_SEC_ASSUME_ROOT": "1"})
    assert "skipped when running as root" in r.stdout
    assert "distrobox" not in calls


# ----------------------------------------------------------------- github releases

def release(name: str, binary: bytes, kind: str) -> tuple[str, bytes]:
    if kind == "zip":
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("README.md", "x")
            z.writestr(name, binary)
        return f"{name}_1.2.3_linux_amd64.zip", buf.getvalue()
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as t:
        info = tarfile.TarInfo(name)
        info.size = len(binary)
        t.addfile(info, io.BytesIO(binary))
    return f"{name}_1.2.3_linux_amd64.tar.gz", buf.getvalue()


@pytest.fixture()
def fake_github():
    files: dict[str, bytes] = {}

    class H(BaseHTTPRequestHandler):
        def do_GET(self):
            body = files.get(self.path)
            self.send_response(200 if body is not None else 404)
            self.end_headers()
            if body is not None:
                self.wfile.write(body)

        def log_message(self, *a):
            pass

    srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{srv.server_address[1]}"

    def publish(repo: str, name: str, binary: bytes, kind="zip", tamper=False, checksums=True):
        asset, data = release(name, binary, kind)
        digest = hashlib.sha256(data).hexdigest()
        files[f"/dl/{asset}"] = data + (b"evil" if tamper else b"")
        assets = [{"name": asset, "browser_download_url": f"{base}/dl/{asset}"},
                  {"name": f"{name}_1.2.3_linux_arm64.zip", "browser_download_url": f"{base}/dl/other"}]
        if checksums:
            files[f"/dl/{name}_1.2.3_checksums.txt"] = f"{digest}  {asset}\n".encode()
            assets.append({"name": f"{name}_1.2.3_checksums.txt",
                           "browser_download_url": f"{base}/dl/{name}_1.2.3_checksums.txt"})
        files[f"/repos/{repo}/releases/latest"] = json.dumps({"tag_name": "v1.2.3", "assets": assets}).encode()

    yield base, publish
    srv.shutdown()


def test_github_binary_installs_after_checksum_check(sec, fake_github, monkeypatch):
    base, publish = fake_github
    monkeypatch.setattr(sec, "GITHUB_API", base)
    monkeypatch.setattr(sec, "arch", lambda: ("amd64", "x86_64"))
    publish("projectdiscovery/nuclei", "nuclei", b"#!/bin/sh\necho nuclei 1.2.3\n")
    tool = next(t for t in sec.load(CATALOG)["tool"] if t["name"] == "nuclei")
    sec.github_install(tool, dry=False)
    link = sec.LOCAL_BIN / "nuclei"
    assert link.is_symlink() and os.access(link, os.X_OK)
    assert subprocess.run([str(link)], capture_output=True, text=True).stdout.strip() == "nuclei 1.2.3"
    state = json.loads((sec.STATE / "github.json").read_text())
    assert state["nuclei"]["version"] == "v1.2.3" and len(state["nuclei"]["sha256"]) == 64


def test_github_tar_gz_release(sec, fake_github, monkeypatch):
    base, publish = fake_github
    monkeypatch.setattr(sec, "GITHUB_API", base)
    monkeypatch.setattr(sec, "arch", lambda: ("amd64", "x86_64"))
    publish("trufflesecurity/trufflehog", "trufflehog", b"#!/bin/sh\necho th\n", kind="tar")
    tool = next(t for t in sec.load(CATALOG)["tool"] if t["name"] == "trufflehog")
    sec.github_install(tool, dry=False)
    assert (sec.OPT / "trufflehog").read_bytes() == b"#!/bin/sh\necho th\n"


def test_tampered_download_is_refused(sec, fake_github, monkeypatch):
    base, publish = fake_github
    monkeypatch.setattr(sec, "GITHUB_API", base)
    monkeypatch.setattr(sec, "arch", lambda: ("amd64", "x86_64"))
    publish("projectdiscovery/nuclei", "nuclei", b"good", tamper=True)
    tool = next(t for t in sec.load(CATALOG)["tool"] if t["name"] == "nuclei")
    with pytest.raises(sec.Fail, match="SHA-256 mismatch"):
        sec.github_install(tool, dry=False)
    assert not (sec.OPT / "nuclei").exists() and not (sec.LOCAL_BIN / "nuclei").exists()


def test_release_without_checksums_is_refused(sec, fake_github, monkeypatch):
    base, publish = fake_github
    monkeypatch.setattr(sec, "GITHUB_API", base)
    monkeypatch.setattr(sec, "arch", lambda: ("amd64", "x86_64"))
    publish("projectdiscovery/nuclei", "nuclei", b"x", checksums=False)
    tool = next(t for t in sec.load(CATALOG)["tool"] if t["name"] == "nuclei")
    with pytest.raises(sec.Fail, match="publishes no checksums"):
        sec.github_install(tool, dry=False)


def test_remove_github_tool(sec, fake_github, monkeypatch, tmp_path):
    base, publish = fake_github
    monkeypatch.setattr(sec, "GITHUB_API", base)
    monkeypatch.setattr(sec, "arch", lambda: ("amd64", "x86_64"))
    publish("projectdiscovery/nuclei", "nuclei", b"x")
    cat = sec.load(CATALOG)
    sec.github_install(next(t for t in cat["tool"] if t["name"] == "nuclei"), dry=False)
    sec.main(["remove", "nuclei"])
    assert not (sec.LOCAL_BIN / "nuclei").exists() and "nuclei" not in sec.github_state()


def test_menu_has_a_submenu_per_category():
    menu = (SEC / "menu/ambar-security.menu").read_text()
    for c in catalog()["categories"]:
        if c != "support":
            assert f"X-Ambar-Security-{c}" in menu, c
            assert (SEC / f"menu/ambar-security-{c}.directory").exists(), c


def test_binary_runtime_libraries_and_pipx_come_with_apt(tmp_path):
    r, calls = run_cli(tmp_path, "install", "naabu", "sslyze", "--system-only",
                       env={"AMBAR_SEC_ASSUME_ROOT": "1", "AMBAR_SEC_GITHUB_API": "http://127.0.0.1:9"})
    install = next(l for l in calls.splitlines() if l.startswith("apt-get install"))
    assert "libpcap0.8t64" in install
    assert "pipx install --global sslyze" in calls


def test_only_services_the_install_enabled_are_turned_off(tmp_path):
    """apache2 and ssh.socket get enabled by the install; smbd was enabled by the user before."""
    bin_dir, calls = stubs(tmp_path)
    marker = tmp_path / "installed"
    (bin_dir / "apt-get").write_text(f'#!/bin/sh\necho "apt-get $*" >> "{calls}"\n'
                                     f'[ "$1" = install ] && touch "{marker}"\nexit 0\n')
    (bin_dir / "systemctl").write_text(f'''#!/bin/sh
echo "systemctl $*" >> "{calls}"
if [ "$1" = is-enabled ]; then
  case "$2" in
    smbd.service) echo enabled ;;
    apache2.service|ssh.socket) [ -e "{marker}" ] && echo enabled || echo disabled ;;
    *) echo not-found ;;
  esac
fi
exit 0
''')
    r, calls_text = run_cli(tmp_path, "install", "lab-servers", "--system-only", env={"AMBAR_SEC_ASSUME_ROOT": "1"})
    assert r.returncode == 0, r.stderr + r.stdout
    disabled = {l.split()[2] for l in calls_text.splitlines() if l.startswith("systemctl disable")}
    assert disabled == {"apache2.service", "ssh.socket"}, disabled
    assert "smbd.service" not in disabled  # the user's own choice stays
