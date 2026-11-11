#!/usr/bin/env python3
"""Ambar OS installer, used from the live USB.

Installs Ambar OS onto a whole disk with `bootc install to-disk`, run from the
Ambar OS container image (the documented way to install a bootc image from a
running Linux system). The disk is wiped. The user account is created on first
boot by plasma-setup, so nothing about the user is asked here.

    ambar-install                 graphical installer
    ambar-install --list          show disks that can be installed to
    ambar-install --disk /dev/sda --yes   non-interactive (tests, scripted installs)

Safety rules, enforced in code and tested:
  - only whole, writable, non-removable-medium-of-the-live-session disks
  - never the disk the live system booted from
  - never a disk smaller than MIN_BYTES
  - the GUI requires typing the disk name before it will wipe it
"""
from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys

IMAGE = "ghcr.io/alexbieber/ambar-os:latest"
MIN_BYTES = 32 * 1024**3
LIVE_LABEL = "AmbarOS-Live"
LIVE_MOUNTS = ("/run/initramfs/live", "/run/rootfsbase")


def lsblk() -> dict:
    out = subprocess.run(
        ["lsblk", "-J", "-b", "-o", "NAME,PATH,SIZE,TYPE,TRAN,RM,RO,MODEL,MOUNTPOINTS,LABEL"],
        check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def _walk(dev):
    yield dev
    for c in dev.get("children") or []:
        yield from _walk(c)


def _truthy(v) -> bool:
    return v in (True, 1, "1", "true")


def candidates(tree: dict) -> tuple[list[dict], list[tuple[str, str]]]:
    """Returns (installable disks, [(path, reason it was skipped)])."""
    ok, skipped = [], []
    for d in tree.get("blockdevices", []):
        path = d.get("path") or f"/dev/{d['name']}"
        if d.get("type") != "disk":
            continue
        if d["name"].startswith(("loop", "zram", "sr", "ram")):
            continue
        parts = list(_walk(d))
        mounts = [m for p in parts for m in (p.get("mountpoints") or []) if m]
        if any(p.get("label") == LIVE_LABEL for p in parts) or any(
                m.startswith(LIVE_MOUNTS) for m in mounts):
            skipped.append((path, "this is the live USB you started from"))
        elif _truthy(d.get("ro")):
            skipped.append((path, "read-only"))
        elif int(d.get("size") or 0) < MIN_BYTES:
            skipped.append((path, f"too small (needs {MIN_BYTES // 1024**3} GB)"))
        elif mounts:
            skipped.append((path, "in use (mounted)"))
        else:
            ok.append({"path": path, "size": int(d["size"]), "model": (d.get("model") or "").strip(),
                       "tran": d.get("tran") or "", "removable": _truthy(d.get("rm"))})
    return ok, skipped


def human(n: int) -> str:
    return f"{n / 1000**3:.0f} GB"


def describe(d: dict) -> str:
    bits = [d["path"], human(d["size"])]
    if d["model"]:
        bits.append(d["model"])
    if d["tran"]:
        bits.append(d["tran"].upper())
    if d["removable"]:
        bits.append("removable")
    return " · ".join(bits)


def install_command(disk: str, image: str = IMAGE, filesystem: str = "btrfs") -> list[str]:
    """The exact command run to install. `disk` must come from candidates()."""
    if not disk.startswith("/dev/") or any(c in disk for c in " ;&|$`'\"\\\n"):
        raise ValueError(f"not a disk path: {disk!r}")
    return [
        "sudo", "-n", "podman", "run", "--rm", "--privileged", "--pid=host",
        "--security-opt", "label=type:unconfined_t",
        "-v", "/dev:/dev", "-v", "/var/lib/containers:/var/lib/containers",
        "--pull=newer", image,
        "bootc", "install", "to-disk", "--wipe", "--filesystem", filesystem,
        "--target-imgref", image, disk,
    ]


def check_disk(disk: str) -> dict:
    ok, skipped = candidates(lsblk())
    for d in ok:
        if d["path"] == disk:
            return d
    reason = dict(skipped).get(disk, "not a disk that can be installed to")
    raise SystemExit(f"refusing to install to {disk}: {reason}")


def cli(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="ambar-install", description=__doc__.splitlines()[0])
    ap.add_argument("--list", action="store_true", help="list disks and exit")
    ap.add_argument("--disk", help="disk to install to (wiped)")
    ap.add_argument("--yes", action="store_true", help="don't ask (needs --disk)")
    ap.add_argument("--image", default=IMAGE, help="image to install (default: %(default)s)")
    ap.add_argument("--dry-run", action="store_true", help="print the command, don't run it")
    a = ap.parse_args(argv)

    if a.list:
        ok, skipped = candidates(lsblk())
        for d in ok:
            print(describe(d))
        for p, why in skipped:
            print(f"{p}  (skipped: {why})")
        return 0 if ok else 1
    if a.disk:
        d = check_disk(a.disk)
        cmd = install_command(d["path"], a.image)
        if a.dry_run:
            print(shlex.join(cmd))
            return 0
        if not a.yes:
            print(f"This erases EVERYTHING on {describe(d)}.")
            if input(f"Type {d['path']} to continue: ").strip() != d["path"]:
                print("cancelled")
                return 1
        return subprocess.run(cmd).returncode
    return gui(a.image)


def gui(image: str) -> int:  # pragma: no cover - exercised by test_installer_gui
    from PySide6.QtWidgets import QApplication
    app = QApplication.instance() or QApplication(sys.argv)
    w = _window_class()(image)
    w.show()
    return app.exec()


def _window_class():
    from PySide6.QtCore import QProcess
    from PySide6.QtWidgets import (QLabel, QLineEdit, QListWidget, QListWidgetItem,
                                   QMessageBox, QPlainTextEdit, QPushButton,
                                   QVBoxLayout, QWidget)

    class InstallerWindow(QWidget):
        def __init__(self, image: str, tree: dict | None = None):
            super().__init__()
            self.image = image
            self.setWindowTitle("Install Ambar OS")
            self.resize(640, 520)
            lay = QVBoxLayout(self)
            lay.addWidget(QLabel("<h2>Install Ambar OS</h2>"
                                 "Ambar OS will replace everything on the disk you choose. "
                                 "You'll create your account the first time it starts. "
                                 "Needs an internet connection."))
            self.disks = QListWidget(objectName="disks")
            lay.addWidget(self.disks)
            self.note = QLabel(objectName="note")
            self.note.setWordWrap(True)
            lay.addWidget(self.note)
            self.confirm = QLineEdit(objectName="confirm")
            self.confirm.setPlaceholderText("Type the disk name shown above to confirm, e.g. /dev/sda")
            lay.addWidget(self.confirm)
            self.button = QPushButton("Erase disk and install", objectName="install")
            self.button.setEnabled(False)
            lay.addWidget(self.button)
            self.log = QPlainTextEdit(objectName="log")
            self.log.setReadOnly(True)
            lay.addWidget(self.log)
            self.proc = None
            ok, skipped = candidates(tree if tree is not None else lsblk())
            for d in ok:
                it = QListWidgetItem(describe(d))
                it.setData(256, d["path"])
                self.disks.addItem(it)
            self.note.setText("" if ok else "No disk found that Ambar OS can be installed to.")
            if skipped:
                self.note.setText(self.note.text() + "\nNot shown: " +
                                  "; ".join(f"{p} ({w})" for p, w in skipped))
            self.disks.currentItemChanged.connect(self._update)
            self.confirm.textChanged.connect(self._update)
            self.button.clicked.connect(self._start)

        def selected(self):
            it = self.disks.currentItem()
            return it.data(256) if it else None

        def _update(self, *_):
            disk = self.selected()
            self.button.setEnabled(bool(disk) and self.confirm.text().strip() == disk
                                   and self.proc is None)

        def _start(self):
            disk = self.selected()
            if not disk or self.confirm.text().strip() != disk:
                return
            cmd = install_command(disk, self.image)
            self.log.appendPlainText("$ " + shlex.join(cmd))
            self.proc = QProcess(self)
            self.proc.setProcessChannelMode(QProcess.ProcessChannelMode.MergedChannels)
            self.proc.readyReadStandardOutput.connect(
                lambda: self.log.appendPlainText(bytes(self.proc.readAllStandardOutput()).decode(errors="replace").rstrip()))
            self.proc.finished.connect(self._done)
            self._update()
            self.proc.start(cmd[0], cmd[1:])

        def _done(self, code, _status):
            if code == 0:
                self.log.appendPlainText("\nInstalled. Remove the USB stick and restart.")
                QMessageBox.information(self, "Ambar OS installed",
                                        "Done. Remove the USB stick and restart your computer.")
            else:
                self.log.appendPlainText(f"\nInstallation failed (exit {code}). Nothing was "
                                         "changed if it failed before writing the disk.")
            self.proc = None
            self._update()

    return InstallerWindow


def __getattr__(name):
    if name == "InstallerWindow":
        return _window_class()
    raise AttributeError(name)


if __name__ == "__main__":
    sys.exit(cli())
