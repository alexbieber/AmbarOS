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
import os
import shlex
import subprocess
import sys

IMAGE = "ghcr.io/alexbieber/ambar-os:latest"
MIN_BYTES = 32 * 1024**3
LIVE_LABEL = "AmbarOS-Live"
LIVE_MOUNTS = ("/run/initramfs/live", "/run/rootfsbase")
MARK = os.environ.get("AMBAR_LOGO", "/usr/share/icons/hicolor/scalable/apps/ambar-logo.svg")

# Ambar dark look (same colours as the Plasma theme and Automation Center).
STYLE = """
#installer { background: #15120F; }
QLabel { color: #F7F4EF; font-size: 13px; }
#title { font-size: 22px; font-weight: 600; }
#muted, #note { color: #A89F95; }
#step { color: #F2A43A; font-weight: 600; margin-top: 6px; }
#warn { color: #E8836B; }
QListWidget, QLineEdit, QPlainTextEdit {
  background: #1F1A15; color: #F7F4EF; border: 1px solid #3A3129; border-radius: 10px;
  padding: 6px; font-size: 13px; selection-background-color: #F2A43A; selection-color: #1A1612;
}
QListWidget::item { padding: 10px 8px; border-radius: 7px; }
QListWidget::item:selected { background: #3A2A16; color: #FFFFFF; border: 1px solid #F2A43A; }
QLineEdit:focus { border-color: #F2A43A; }
QPlainTextEdit { font-family: monospace; font-size: 12px; color: #CFC6BB; }
QPushButton#install {
  background: #F2A43A; color: #1A1612; border: none; border-radius: 9px;
  padding: 10px 18px; font-weight: 600; font-size: 13px;
}
QPushButton#install:disabled { background: #3A3129; color: #7D7369; }
"""


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
    from PySide6.QtWidgets import (QHBoxLayout, QLabel, QLineEdit, QListWidget, QListWidgetItem,
                                   QMessageBox, QPlainTextEdit, QPushButton,
                                   QVBoxLayout, QWidget)

    def _mark_pixmap(height: int):
        """The Ambar mark, drawn from the installed SVG (blank if it's missing)."""
        from PySide6.QtCore import QRectF, Qt
        from PySide6.QtGui import QGuiApplication, QPainter, QPixmap
        from PySide6.QtSvg import QSvgRenderer
        dpr = QGuiApplication.primaryScreen().devicePixelRatio() if QGuiApplication.primaryScreen() else 1
        w = int(height * 320 / 230)
        pm = QPixmap(int(w * dpr), int(height * dpr))
        pm.fill(Qt.GlobalColor.transparent)
        r = QSvgRenderer(MARK)
        if r.isValid():
            p = QPainter(pm)
            r.render(p, QRectF(0, 0, w * dpr, height * dpr))
            p.end()
        pm.setDevicePixelRatio(dpr)
        return pm

    class InstallerWindow(QWidget):
        def __init__(self, image: str, tree: dict | None = None):
            super().__init__()
            self.image = image
            self.setWindowTitle("Install Ambar OS")
            self.setObjectName("installer")
            self.resize(680, 600)
            self.setStyleSheet(STYLE)
            lay = QVBoxLayout(self)
            lay.setContentsMargins(32, 28, 32, 28)
            lay.setSpacing(12)
            head = QHBoxLayout()
            head.setSpacing(14)
            mark = QLabel(objectName="mark")
            mark.setPixmap(_mark_pixmap(46))
            head.addWidget(mark)
            titles = QVBoxLayout()
            titles.setSpacing(2)
            titles.addWidget(QLabel("Install Ambar OS", objectName="title"))
            titles.addWidget(QLabel("Replaces everything on the disk you choose. You'll create your "
                                    "account the first time it starts. Needs an internet connection.",
                                    objectName="muted", wordWrap=True))
            head.addLayout(titles, 1)
            lay.addLayout(head)
            lay.addSpacing(6)
            lay.addWidget(QLabel("1. Choose a disk", objectName="step"))
            self.disks = QListWidget(objectName="disks")
            self.disks.setMinimumHeight(120)
            lay.addWidget(self.disks)
            self.note = QLabel(objectName="note")
            self.note.setWordWrap(True)
            lay.addWidget(self.note)
            lay.addWidget(QLabel("2. Type its name to confirm", objectName="step"))
            self.confirm = QLineEdit(objectName="confirm")
            self.confirm.setPlaceholderText("e.g. /dev/nvme0n1")
            lay.addWidget(self.confirm)
            row = QHBoxLayout()
            row.addWidget(QLabel("Everything on that disk will be erased.", objectName="warn"), 1)
            self.button = QPushButton("Erase disk and install", objectName="install")
            self.button.setEnabled(False)
            row.addWidget(self.button)
            lay.addLayout(row)
            self.log = QPlainTextEdit(objectName="log")
            self.log.setReadOnly(True)
            self.log.setPlaceholderText("Progress appears here.")
            lay.addWidget(self.log, 1)
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
