"""Live USB, installer, ARM64 / Raspberry Pi and hardware-report checks.

Nothing here boots anything: the real build and boot are in the os-image CI
job. These check the recipes, the safety rules of the installer, and the
privacy of the hardware report.
"""
import importlib.util
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import tomllib
from importlib.machinery import SourceFileLoader
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
OS = ROOT / "os"
LIVE = OS / "live"
sys.path.insert(0, str(OS))
import smoke_test  # noqa: E402

spec = importlib.util.spec_from_file_location("ambar_install", LIVE / "installer" / "ambar_install.py")
inst = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inst)

loader = SourceFileLoader("ambar_hwreport", str(OS / "files/usr/bin/ambar-hwreport"))
hw_spec = importlib.util.spec_from_loader("ambar_hwreport", loader)


def hwreport(root):
    os.environ["AMBAR_HWREPORT_ROOT"] = str(root)
    try:
        m = importlib.util.module_from_spec(hw_spec)
        loader.exec_module(m)
        return m
    finally:
        del os.environ["AMBAR_HWREPORT_ROOT"]


# ---------------------------------------------------------------- live image

def test_live_containerfile_builds_on_the_os_image():
    cf = (LIVE / "Containerfile").read_text()
    assert "ARG BASE=localhost/ambar-os:latest" in cf and "FROM ${BASE}" in cf
    assert "dracut-live" in cf and "lsinitrd" in cf, "initramfs must be checked for dmsquash-live"
    assert "systemctl mask bootloader-update.service" in cf
    assert "visudo -cf" in cf
    conf = (LIVE / "files/usr/lib/dracut/dracut.conf.d/40-ambar-live.conf").read_text()
    assert re.search(r'add_dracutmodules\+=".*\bdmsquash-live\b', conf)


def test_iso_config():
    y = yaml.safe_load((LIVE / "files/usr/lib/image-builder/bootc/iso.yaml").read_text())
    label = y["label"]
    assert len(label) <= 32 and " " not in label, "ISO 9660 volume label"
    assert "rd.live.image" in y["kernel_args"]
    assert any(a.startswith("console=ttyS0") for a in y["kernel_args"]), "smoke test needs serial"
    names = [e["name"] for e in y["grub2"]["entries"]]
    assert names[0] == "Try Ambar OS" and any("safe graphics" in n for n in names)
    for e in y["grub2"]["entries"]:
        assert "${kernelpath}" in e["linux"] and "${root}" in e["linux"] and e["initrd"] == "${initrdpath}"


def test_live_user_is_live_only():
    unit = (LIVE / "files/usr/lib/systemd/system/ambar-live-setup.service").read_text()
    assert "ConditionKernelCommandLine=rd.live.image" in unit, "must never run on an installed system"
    assert "Before=display-manager.service" in unit
    sudo = (LIVE / "files/etc/sudoers.d/ambar-live").read_text()
    rules = [l for l in sudo.splitlines() if l.strip() and not l.startswith("#")]
    assert rules == ["liveuser ALL=(ALL) NOPASSWD: ALL"]
    if shutil.which("visudo"):
        subprocess.run(["visudo", "-cf", str(LIVE / "files/etc/sudoers.d/ambar-live")], check=True,
                       capture_output=True)
    sddm = (LIVE / "files/etc/sddm.conf.d/ambar-live.conf").read_text()
    assert "User=liveuser" in sddm and "Session=plasma" in sddm
    # Live-only files must not leak into the installable image.
    assert not (OS / "files/etc/sudoers.d").exists()
    assert not (OS / "files/etc/sddm.conf.d").exists()
    assert "os/live" not in (OS / "Containerfile").read_text()
    assert smoke_test.LIVE_USER == "liveuser"


def test_install_entry_valid():
    f = LIVE / "files/usr/share/applications/os.ambar.Install.desktop"
    if shutil.which("desktop-file-validate"):
        r = subprocess.run(["desktop-file-validate", str(f)], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout
    assert "Exec=ambar-install" in f.read_text()


def test_installed_systems_create_account_on_first_boot():
    assert "plasma-setup" in (OS / "Containerfile").read_text()
    assert "customizations" not in tomllib.loads((OS / "config/rpi.toml").read_text())


# ---------------------------------------------------------------- installer

GB = 1024**3


def tree():
    return {"blockdevices": [
        {"name": "sda", "path": "/dev/sda", "size": 512 * GB, "type": "disk", "tran": "sata", "rm": False,
         "ro": False, "model": "Samsung SSD 870 ", "mountpoints": [None], "label": None},
        {"name": "nvme0n1", "path": "/dev/nvme0n1", "size": 1000 * GB, "type": "disk", "tran": "nvme",
         "rm": False, "ro": False, "model": "WD Blue", "mountpoints": [None], "label": None,
         "children": [{"name": "nvme0n1p1", "path": "/dev/nvme0n1p1", "type": "part",
                       "mountpoints": ["/mnt/data"], "label": "data"}]},
        {"name": "sdb", "path": "/dev/sdb", "size": 64 * GB, "type": "disk", "tran": "usb", "rm": True,
         "ro": False, "model": "USB Stick", "mountpoints": [None], "label": None,
         "children": [{"name": "sdb1", "path": "/dev/sdb1", "type": "part", "label": "AmbarOS-Live",
                       "mountpoints": ["/run/initramfs/live"]}]},
        {"name": "sdc", "path": "/dev/sdc", "size": 8 * GB, "type": "disk", "rm": True, "ro": False,
         "mountpoints": [None]},
        {"name": "sdd", "path": "/dev/sdd", "size": 256 * GB, "type": "disk", "rm": False, "ro": True,
         "mountpoints": [None]},
        {"name": "loop0", "path": "/dev/loop0", "size": 100 * GB, "type": "loop", "mountpoints": ["/x"]},
        {"name": "sr0", "path": "/dev/sr0", "size": 4 * GB, "type": "rom", "mountpoints": [None]},
        {"name": "zram0", "path": "/dev/zram0", "size": 64 * GB, "type": "disk", "mountpoints": ["[SWAP]"]},
    ]}


def test_installer_only_offers_safe_disks():
    ok, skipped = inst.candidates(tree())
    assert [d["path"] for d in ok] == ["/dev/sda"]
    why = dict(skipped)
    assert "live USB" in why["/dev/sdb"]
    assert "mounted" in why["/dev/nvme0n1"]
    assert "too small" in why["/dev/sdc"]
    assert "read-only" in why["/dev/sdd"]
    assert "/dev/loop0" not in why and "/dev/zram0" not in why
    assert inst.describe(ok[0]) == "/dev/sda · 550 GB · Samsung SSD 870 · SATA"


def test_installer_command():
    cmd = inst.install_command("/dev/sda")
    assert cmd[:3] == ["sudo", "-n", "podman"]
    i = cmd.index("bootc")
    assert cmd[i:i + 3] == ["bootc", "install", "to-disk"]
    assert cmd[-1] == "/dev/sda" and "--wipe" in cmd
    assert cmd[cmd.index("--target-imgref") + 1] == inst.IMAGE, "installed system must update from GHCR"
    for bad in ["sda", "/dev/sda; rm -rf /", "/dev/sda$(x)", "/dev/sd a", "/dev/`id`"]:
        with pytest.raises(ValueError):
            inst.install_command(bad)


def test_installer_cli_refuses_and_dry_runs(monkeypatch, capsys):
    monkeypatch.setattr(inst, "lsblk", tree)
    for disk, reason in [("/dev/sdb", "live USB"), ("/dev/nvme0n1", "mounted"), ("/dev/sdz", "not a disk")]:
        with pytest.raises(SystemExit) as e:
            inst.cli(["--disk", disk, "--yes"])
        assert reason in str(e.value)
    ran = []
    monkeypatch.setattr(inst.subprocess, "run", lambda *a, **k: ran.append(a) or None)
    assert inst.cli(["--disk", "/dev/sda", "--dry-run"]) == 0
    assert "bootc install to-disk --wipe" in capsys.readouterr().out and not ran
    monkeypatch.setattr("builtins.input", lambda _: "/dev/sdx")
    assert inst.cli(["--disk", "/dev/sda"]) == 1 and not ran, "wrong confirmation must cancel"
    assert inst.cli(["--list"]) == 0
    out = capsys.readouterr().out
    assert "/dev/sda · 550 GB" in out and "/dev/sdb  (skipped: this is the live USB" in out


GUI_CHECK = r"""
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location("ambar_install", sys.argv[1])
inst = importlib.util.module_from_spec(spec); spec.loader.exec_module(inst)
from PySide6.QtWidgets import QApplication
app = QApplication([])
w = inst.InstallerWindow("example/image:latest", json.loads(sys.argv[2]))
states = [w.disks.count(), "live USB" in w.note.text(), w.button.isEnabled()]
w.disks.setCurrentRow(0); states.append(w.button.isEnabled())
w.confirm.setText("/dev/sd"); states.append(w.button.isEnabled())
w.confirm.setText("/dev/sda"); states.append(w.button.isEnabled())
print(json.dumps(states))
"""


def test_installer_gui_needs_typed_confirmation():
    pytest.importorskip("PySide6.QtWidgets")
    # Own process: other tests in this session already created a QGuiApplication,
    # and widgets need a QApplication.
    r = subprocess.run([sys.executable, "-c", GUI_CHECK, str(LIVE / "installer" / "ambar_install.py"),
                        json.dumps(tree())], capture_output=True, text=True, timeout=60,
                       env={**os.environ, "QT_QPA_PLATFORM": "offscreen"})
    assert r.returncode == 0, r.stderr
    count, live_noted, *enabled = json.loads(r.stdout.strip().splitlines()[-1])
    assert count == 1 and live_noted
    assert enabled == [False, False, False, True], "only typing the exact disk name enables Install"


# ---------------------------------------------------------------- ARM / Pi

def test_arm_serial_console_karg():
    k = tomllib.loads((OS / "files/usr/lib/bootc/kargs.d/11-ambar-console-arm.toml").read_text())
    assert k["kargs"] == ["console=ttyAMA0,115200n8"] and k["match-architectures"] == ["aarch64"]
    assert "ttyAMA0" in (LIVE / "Containerfile").read_text()
    q = (OS / "test-qemu.sh").read_text()
    assert "qemu-system-aarch64" in q and "AAVMF" in q


def test_rpi_script_never_touches_uefi_chain():
    s = (OS / "rpi-firmware.sh").read_text()
    assert 'rm -rf "$WORK/fw/EFI"' in s, "Fedora firmware must not overwrite bootc's EFI/"
    for f in ["rpi-u-boot.bin", "start4.elf", "fixup4.dat", "bcm2711-rpi-4-b.dtb", "config.txt"]:
        assert f in s
    assert "kernel=rpi-u-boot.bin" in s and "arm_64bit=1" in s
    b = (OS / "build.sh").read_text()
    assert "rpi)" in b and "live-iso)" in b and "generic-iso" in b


# ---------------------------------------------------------------- smoke test, live mode

def test_smoke_harness_live_login(tmp_path):
    path = str(tmp_path / "s.sock")
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(path)
    srv.listen(1)
    seen = {}

    def serve():
        c, _ = srv.accept()
        c.sendall(b"ambar-os login: ")
        seen["user"] = c.recv(100)
        c.sendall(b"[liveuser@ambar-os ~]$ ")  # no Password: prompt
        data = b""
        while b"echo __END__" not in data:
            data += c.recv(65536)
        out = ["__BEGIN__", 'PRETTY_NAME="Ambar OS 0.1 (Fedora 45)"', "ambar 0.1.0", "active",
               "ambard is running (ok)", "dev.ambar.daily-notes 1.0.0",
               "2026-11-11 10:00:00  succeeded  smoke  1a2b3c4d  manual", "ok", "__END__"]
        c.sendall(("\r\n".join(out) + "\r\n").encode())
        c.close()

    threading.Thread(target=serve, daemon=True).start()
    code = smoke_test.run(path, None, boot_timeout=10, desktop=False, user="liveuser", password="")
    assert code == 0 and seen["user"] == b"liveuser\n"
    script = smoke_test.DESKTOP_SCRIPT.replace("{user}", "liveuser")
    assert "pgrep -u liveuser" in script and "{user}" not in script


# ---------------------------------------------------------------- hardware report

def fake_machine(root: Path):
    def w(rel, text):
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    w("etc/os-release", 'NAME="Ambar OS"\nPRETTY_NAME="Ambar OS 0.1 (Fedora 45)"\n')
    w("proc/sys/kernel/osrelease", "6.17.4-300.fc45.x86_64\n")
    w("arch", "x86_64")
    w("sys/class/dmi/id/sys_vendor", "LENOVO\n")
    w("sys/class/dmi/id/product_name", "21CB\n")
    w("sys/class/dmi/id/product_version", "ThinkPad X1 Carbon Gen 10\n")
    w("sys/class/dmi/id/product_serial", "PF3SECRET\n")
    w("sys/class/dmi/id/product_uuid", "1234-5678\n")
    w("proc/cpuinfo", "processor\t: 0\nmodel name\t: 12th Gen Intel(R) Core(TM) i7-1260P\n")
    w("proc/meminfo", "MemTotal:       16135924 kB\n")
    gpu = root / "sys/bus/pci/devices/0000:00:02.0"
    w("sys/bus/pci/devices/0000:00:02.0/class", "0x030000\n")
    w("sys/bus/pci/devices/0000:00:02.0/vendor", "0x8086\n")
    w("sys/bus/pci/devices/0000:00:02.0/device", "0x46a6\n")
    (root / "drivers/i915").mkdir(parents=True)
    (gpu / "driver").symlink_to(root / "drivers/i915")
    w("sys/bus/pci/devices/0000:00:14.3/class", "0x028000\n")  # Wi-Fi controller: not a GPU
    nic = root / "sys/class/net/wlp0s20f3"
    w("sys/class/net/wlp0s20f3/address", "aa:bb:cc:dd:ee:ff\n")
    (nic / "wireless").mkdir()
    (nic / "device").mkdir()
    (root / "drivers/iwlwifi").mkdir()
    (nic / "device/driver").symlink_to(root / "drivers/iwlwifi")
    w("sys/class/net/lo/address", "00:00:00:00:00:00\n")
    w("proc/asound/cards", " 0 [sofhdadsp      ]: sof-hda-dsp - sof-hda-dsp\n                      LENOVO-21CB\n")
    w("sys/power/state", "freeze mem disk\n")
    w("sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c", "")
    (root / "sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c").write_bytes(b"\x06\x00\x00\x00\x01")


def test_hwreport_collects_and_formats(tmp_path):
    fake_machine(tmp_path)
    m = hwreport(tmp_path)
    r = m.collect()
    assert r["machine"] == "LENOVO 21CB ThinkPad X1 Carbon Gen 10"
    assert r["cpu"].startswith("12th Gen Intel")
    assert r["memory_gb"] == 15.4
    assert r["boot"] == "UEFI" and r["secure_boot"] == "enabled"
    assert r["gpu"] == [{"id": "8086:46a6", "driver": "i915", "class": "0x030000"}]
    assert r["network"] == [{"kind": "wifi", "driver": "iwlwifi"}]
    assert r["audio"] == ["sof-hda-dsp - sof-hda-dsp"]
    assert r["sleep"] == ["freeze", "mem", "disk"]
    md = m.markdown(r)
    assert "| Graphics | `8086:46a6` (i915) |" in md
    assert "| LENOVO 21CB ThinkPad X1 Carbon Gen 10 | x86_64 | ? boots | ? iwlwifi | ? i915 | ? suspend |" in md


def test_hwreport_never_includes_identifiers(tmp_path):
    fake_machine(tmp_path)
    m = hwreport(tmp_path)
    r = m.collect()
    text = json.dumps(r) + m.markdown(r)
    for secret in ["PF3SECRET", "1234-5678", "aa:bb:cc:dd:ee:ff", "wlp0s20f3"]:
        assert secret not in text
    assert not re.search(r"([0-9a-f]{2}:){5}[0-9a-f]{2}", text)


def test_hwreport_arm_board(tmp_path):
    (tmp_path / "proc/device-tree").mkdir(parents=True)
    (tmp_path / "proc/device-tree/model").write_text("Raspberry Pi 4 Model B Rev 1.4\x00")
    (tmp_path / "proc/cpuinfo").write_text("processor\t: 0\nModel\t\t: Raspberry Pi 4 Model B Rev 1.4\n")
    m = hwreport(tmp_path)
    r = m.collect()
    assert r["machine"] == "Raspberry Pi 4 Model B Rev 1.4"
    assert r["boot"] == "BIOS" or r["boot"] == "UEFI"
    assert r["gpu"] == [] and r["network"] == [] and r["secure_boot"] == "n/a"
    assert "none found" in m.markdown(r)


def test_shipped_programs_are_executable():
    for f in [OS / "files/usr/bin/ambar-hwreport", LIVE / "files/usr/libexec/ambar-live-setup",
              LIVE / "installer/ambar_install.py", OS / "rpi-firmware.sh", OS / "build.sh"]:
        assert os.access(f, os.X_OK), f"{f} must be executable"
    assert "COPY --chmod=0755 os/live/installer/ambar_install.py /usr/bin/ambar-install" in (LIVE / "Containerfile").read_text()
