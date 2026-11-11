"""Checks for the Ubuntu-based Ambar OS: the build recipe, the macOS-style
desktop pieces (menu-bar widgets, layout, window decoration), branding, and
the ISO boot test harness. The real build and boot are run by build.sh and
test-qemu.sh (and the os-ubuntu CI job)."""
import json
import re
import shutil
import socket
import subprocess
import sys
import threading
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
U = ROOT / "os" / "ubuntu"
PLASMA = ROOT / "desktop" / "plasma"
sys.path.insert(0, str(U))
sys.path.insert(0, str(ROOT / "os"))
import smoke as ubuntu_smoke  # noqa: E402


def listfile(p):
    return [l.strip() for l in p.read_text().splitlines() if l.strip() and not l.lstrip().startswith("#")]


def test_package_lists():
    pkgs, excl = listfile(U / "packages.txt"), listfile(U / "exclude.txt")
    assert len(pkgs) == len(set(pkgs)), "duplicate packages"
    assert not set(pkgs) & set(excl), "a package is both installed and excluded"
    for must in ["kubuntu-desktop", "linux-generic", "casper", "calamares", "flatpak", "snapd",
                 "plasma-discover-backend-flatpak", "kwin-style-aurorae", "fonts-inter", "papirus-icon-theme",
                 "libavcodec-extra", "wine", "distrobox", "timeshift", "falkon"]:
        assert must in pkgs, must
    for snap_shim in ["firefox", "thunderbird", "chromium-browser"]:
        assert snap_shim in excl, f"{snap_shim} is a snap shim and must not be installed at build time"


def test_scripts_parse():
    scripts = [U / "build.sh", U / "test-qemu.sh", U / "desktop-shots.sh", *sorted((U / "hooks").glob("*.chroot"))]
    for s in scripts:
        subprocess.run(["bash", "-n", str(s)], check=True)
        assert s.stat().st_mode & 0o111, f"{s.name} must be executable"
    if shutil.which("shellcheck"):
        r = subprocess.run(["shellcheck", "-S", "warning", "-e", "SC2054", *map(str, scripts)], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout


def test_hooks_run_in_order_and_initramfs_last():
    hooks = sorted(p.name for p in (U / "hooks").glob("*.chroot"))
    assert hooks[-1].startswith("90-initramfs"), "initramfs must be rebuilt after the boot theme"
    identity = (U / "hooks" / "10-identity.chroot").read_text()
    assert 'NAME="Ambar OS"' in identity and 'ID_LIKE="ubuntu debian"' in identity
    assert "dpkg-divert" in identity, "os-release must survive base-files upgrades"
    assert 'USERNAME="ambar"' in identity
    # lsb-release: Ubuntu ID kept for PPAs, our description; re-runs read the originals.
    assert "/etc/lsb-release" in identity and "DISTRIB_ID=${DISTRIB_ID}" in identity
    assert identity.count("dpkg-divert --truename") == 2


def test_desktop_defaults_hook():
    h = (U / "hooks" / "20-desktop.chroot").read_text()
    assert "LookAndFeelPackage=os.ambar.desktop" in h
    assert "DefaultDarkLookAndFeel=os.ambar.desktop.dark" in h
    assert "theme=__aurorae__svg__Ambar" in h and "ButtonsOnLeft=XIA" in h
    assert "Theme=Papirus" in h and "font=Inter" in h
    assert "_launch=Meta+Space" in h, "Spotlight-style search on Meta+Space"
    assert "Name=Ambar Store" in h
    assert "flathub" in h and "ambar-flathub.service" in h
    assert "90-ambar.conf" in h, "SDDM config must sort after Kubuntu's 20-kubuntu.conf"


def test_grub_menu():
    g = (U / "iso" / "grub.cfg").read_text()
    entries = re.findall(r'menuentry "([^"]+)"', g)
    assert entries[0] == "Try or Install Ambar OS"
    assert any("safe graphics" in e for e in entries)
    assert "boot=casper" in g and "console=ttyS0" in g
    assert (U / "iso" / "background.png").exists()


def test_installer_branding():
    b = U / "overlay/etc/calamares/branding/ambar"
    desc = yaml.safe_load((b / "branding.desc").read_text())
    assert desc["componentName"] == "ambar"
    assert desc["strings"]["productName"] == "Ambar OS"
    for img in desc["images"].values():
        assert (b / img).exists(), img
    assert (b / desc["slideshow"]).exists()
    assert "branding: ambar" in (U / "hooks" / "40-installer.chroot").read_text()


def test_boot_animation():
    t = U / "overlay/usr/share/plymouth/themes/ambar"
    cfg = (t / "ambar.plymouth").read_text()
    assert "ModuleName=script" in cfg and "ambar.script" in cfg
    s = (t / "ambar.script").read_text()
    assert "logo.png" in s and "SetBootProgressFunction" in s and "SetDisplayPasswordFunction" in s
    assert (t / "logo.png").read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"


# ------------------------------------------------------------ macOS-style desktop

WIDGETS = ["os.ambar.menu", "os.ambar.appname", "os.ambar.controlcenter"]


@pytest.mark.parametrize("wid", WIDGETS)
def test_menu_bar_widgets_are_valid_plasmoids(wid):
    d = PLASMA / "plasmoids" / wid
    meta = json.loads((d / "metadata.json").read_text())
    assert meta["KPlugin"]["Id"] == wid
    assert meta["KPackageStructure"] == "Plasma/Applet"
    assert float(meta["X-Plasma-API-Minimum-Version"]) >= 6.0
    qml = (d / "contents/ui/main.qml").read_text()
    assert "PlasmoidItem" in qml
    # Properties that clash with built-ins break loading ("Cannot override FINAL property").
    assert not re.search(r"property \w+ (icon|state|data)\b", qml), "clashes with a built-in property"


def test_ambar_menu_items():
    qml = (PLASMA / "plasmoids/os.ambar.menu/contents/ui/main.qml").read_text()
    for item in ["About This Computer", "System Settings", "Ambar Store", "Force Quit", "Sleep",
                 "Restart", "Shut Down", "Lock Screen", "Log Out"]:
        assert item in qml, item
    for call in ["session.suspend()", "session.requestReboot()", "session.requestShutdown()", "session.lock()"]:
        assert call in qml


def test_control_center_uses_tested_backend():
    qml = (PLASMA / "plasmoids/os.ambar.controlcenter/contents/ui/main.qml").read_text()
    assert "ambar-control-center" in qml
    for key in ["wifi", "bluetooth", "airplane", "dnd", "dark", "nightlight", "brightness", "volume"]:
        assert f'key: "{key}"' in qml, key


def test_layout_is_macos_style():
    js = (PLASMA / "look-and-feel/os.ambar.desktop/contents/layouts/org.kde.plasma.desktop-layout.js").read_text()
    top, dock = js.split("// ------------------------------------------------------------ dock")
    assert 'location = "top"' in top
    order = [top.index(w) for w in ['"os.ambar.menu"', '"os.ambar.appname"', '"org.kde.plasma.appmenu"',
                                     '"org.kde.plasma.systemtray"', '"os.ambar.controlcenter"',
                                     '"org.kde.plasma.digitalclock"']]
    assert order == sorted(order), "menu bar order: Ambar menu, app name, app menus … tray, control, clock"
    assert 'location = "bottom"' in dock and "floating = true" in dock and 'lengthMode = "fit"' in dock
    assert dock.index("kickerdash") < dock.index("icontasks") < dock.index("org.kde.plasma.trash")
    assert '"showRecentApps", "false"' in dock, "Launchpad opens on all apps"
    assert "applicationExists" in js, "pinned apps must exist on the system"
    assert "file:///usr/share/wallpapers/Ambar/" in js


def test_window_decoration_themes():
    for name in ["Ambar", "AmbarDark"]:
        d = PLASMA / "aurorae" / name
        root = ET.fromstring((d / "decoration.svg").read_text())
        ids = {e.get("id") for e in root.iter() if e.get("id")}
        for prefix in ["decoration", "decoration-inactive"]:
            for piece in ["topleft", "top", "topright", "left", "center", "right", "bottomleft", "bottom", "bottomright"]:
                assert f"{prefix}-{piece}" in ids, (name, prefix, piece)
        for b, color in [("close", "#FF5F57"), ("minimize", "#FEBC2E"), ("maximize", "#28C840")]:
            svg = (d / f"{b}.svg").read_text()
            ET.fromstring(svg)
            assert color in svg
            for state in ["active", "hover", "pressed", "inactive", "deactivated"]:
                assert f'id="{state}-center"' in svg
        rc = (d / f"{name}rc").read_text()
        assert "TitleAlignment=Center" in rc
        assert f"X-KDE-PluginInfo-Name={name}" in (d / "metadata.desktop").read_text()
    # The generator is the source of truth.
    before = {p: p.read_bytes() for p in (PLASMA / "aurorae").rglob("*") if p.is_file()}
    subprocess.run([sys.executable, str(PLASMA / "aurorae.py")], check=True, capture_output=True)
    after = {p: p.read_bytes() for p in (PLASMA / "aurorae").rglob("*") if p.is_file()}
    assert before == after, "aurorae/ is out of date: run desktop/plasma/aurorae.py"


def test_dark_variant_switches_decoration_and_icons():
    d = (PLASMA / "look-and-feel/os.ambar.desktop.dark/contents/defaults").read_text()
    assert "theme=__aurorae__svg__AmbarDark" in d and "Theme=Papirus-Dark" in d


# ------------------------------------------------------------ ISO boot test harness

def fake_live_console(path, good=True, flavor="standard", sec_broken=False):
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(path)
    srv.listen(1)

    def serve():
        c, _ = srv.accept()
        c.sendall(b"Ambar OS 0.2 ambar-os ttyS0\r\n\r\nambar-os login: ")
        assert c.recv(100) == b"ambar\n"
        c.sendall(b"ambar@ambar-os:~$ ")
        data = b""
        while b"echo __END__" not in data:
            data += c.recv(65536)
        out = ["__BEGIN__", 'NAME="Ambar OS"', 'PRETTY_NAME="Ambar OS 0.2"', "ID=ambar", 'ID_LIKE="ubuntu debian"',
               "PLASMA_OK", "KWIN_WAYLAND_OK"]
        apps = ["dolphin", "konsole", "calamares", "ambard", "flatpak"]
        out += [f"APP_OK {a}" for a in apps]
        if not good:
            out.append("APP_MISSING libreoffice")
        out += ["BROWSER_OK falkon"]
        out += [f"WIDGET_OK {w}" for w in ["os.ambar.menu", "os.ambar.appname", "os.ambar.controlcenter",
                                           "org.kde.plasma.kickerdash", "org.kde.plasma.appmenu", "org.kde.plasma.icontasks"]]
        out += ["Name=Ambar Store", "FLATHUB_OK", "active", "AMBARD_OK",
                '{"wifi": null, "volume": {"value": 40, "muted": false}}', "SEC_CLI_OK"]
        if flavor == "security":
            out += ["FLAVOR_SECURITY", "SEC_TOOL_OK nmap", "SEC_TOOL_OK nuclei"]
            out += ["SERVICE_ON apache2"] if sec_broken else ["SEC_TOOL_OK nxc"]
        out += ["__END__"]
        c.sendall(("\r\n".join(out) + "\r\n").encode())
        c.close()

    threading.Thread(target=serve, daemon=True).start()


@pytest.mark.parametrize("good", [True, False])
def test_iso_boot_harness(tmp_path, good, capsys):
    sock = str(tmp_path / "s")
    fake_live_console(sock, good)
    code = ubuntu_smoke.run(sock, str(tmp_path / "serial.log"), None, None, boot_timeout=10)
    out = capsys.readouterr().out
    assert code == (0 if good else 1), out
    if not good:
        assert "app libreoffice" in out


@pytest.mark.parametrize("broken", [False, True])
def test_security_iso_boot_harness(tmp_path, broken, capsys):
    sock = str(tmp_path / "s")
    fake_live_console(sock, True, flavor="security", sec_broken=broken)
    code = ubuntu_smoke.run(sock, str(tmp_path / "serial.log"), None, None, boot_timeout=10)
    out = capsys.readouterr().out
    assert code == (1 if broken else 0), out
    if broken:
        assert "service apache2 enabled" in out and "netexec (pipx)" in out


def test_security_flavour_in_the_build():
    b = (U / "build.sh").read_text()
    assert 'FLAVOR="${FLAVOR:-standard}"' in b and "ambar-sec install default --system-only" in b
    assert 'ambar-os$SUFFIX-$VERSION-amd64.iso' in b
    assert "etc/xdg/menus/applications-merged/ambar-security.menu" in b
    assert 'AMBAR_FLAVOR="$FLAVOR"' in b
    h = (U / "hooks" / "26-security.chroot").read_text()
    assert "boot=casper" in h and "--first-run" in h
    wf = (U.parents[1] / ".github/workflows/os-ubuntu.yml").read_text()
    assert "flavor: [standard, security]" in wf


def test_install_anything_file_handlers():
    h = (U / "hooks" / "25-apps.chroot").read_text()
    assert "application/vnd.debian.binary-package=org.kde.discover.desktop" in h, ".deb opens in the Ambar Store"
    assert "application/x-ms-dos-executable=wine.desktop" in h, ".exe opens with Wine"
    assert "Exec=wine start /unix %f" in h


def test_installer_launches_on_wayland_and_has_no_kubuntu_names():
    h = (U / "hooks" / "40-installer.chroot").read_text()
    # sudo resets the environment: the launcher must pass the display through.
    assert "exec sudo /usr/bin/env" in h and 'WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}"' in h
    assert "Exec=ambar-installer" in h
    assert '"ambar_root"' in h and "batch-identifier: ambar-" in h
    assert "sudo -H -u ambar xdg-open" in h
    assert "libxcb-cursor0" in listfile(U / "packages.txt"), "Qt's X11 fallback needs it"


def test_installer_steps_match_ubuntu_2604_rootfs():
    """Each of these broke a real install in QEMU before it was fixed."""
    mods = U / "overlay/etc/calamares/modules"
    # Kubuntu's step copies a cdrom.sources file our image doesn't have.
    ctx = yaml.safe_load((mods / "before_bootloader_context.conf").read_text())
    assert "cdrom.sources" not in str(ctx) and ctx["firmwareType"]["bios"] == "-/bin/true"
    rm = yaml.safe_load((mods / "shellprocess_rmcdrom.conf").read_text())
    assert all("rm -f" in s or s.startswith("-") for s in rm["script"])
    # The mirror step deletes /etc/apt/sources.list and fails if it's missing.
    assert "sources have moved" in (U / "build.sh").read_text()
    assert yaml.safe_load((mods / "automirror.conf").read_text())["distribution"] == "Ambar"


def test_only_one_installer_launcher():
    h = (U / "hooks" / "40-installer.chroot").read_text()
    assert "NoDisplay=true" in h and "grep -q '^Exec=ambar-installer'" in h


def test_installer_package_cleanup_tolerates_missing_packages():
    # apt-get remove fails the whole install on one unknown name (kubuntu-installer-prompt).
    conf = yaml.safe_load((U / "overlay/etc/calamares/modules/packages.conf").read_text())
    keys = [k for op in conf["operations"] for k in op]
    assert "remove" not in keys and "try_remove" in keys
    removed = next(op["try_remove"] for op in conf["operations"] if "try_remove" in op)
    assert "calamares" in removed and "^live-*" in removed


def test_ambar_tools_are_in_the_image():
    b = (U / "build.sh").read_text()
    assert "--bin ambar-recon" in b and '"$ROOT/usr/bin/ambar-recon"' in b
    assert "for d in workbench guard utils" in b
    assert "ambar-guard check --json" in b, "the build runs the Guard once"
    pk = listfile(U / "packages.txt")
    assert {"qrencode", "unattended-upgrades", "ufw", "iproute2"} <= set(pk)
    smoke = (U / "smoke.py").read_text()
    for tool in ["ambar-recon", "ambar-engage", "ambar-workbench", "ambar-guard", "ambar-clean", "ambar-share"]:
        assert tool in smoke, tool
    repo = U.parents[1]
    for d in ["workbench", "guard", "utils"]:
        assert list((repo / "desktop" / d).glob("os.ambar.*.desktop")), d
    assert "--bin ambar-recon" in (repo / ".github/workflows/os-ubuntu.yml").read_text()
