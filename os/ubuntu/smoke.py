#!/usr/bin/env python3
"""Boot test for the Ambar OS (Ubuntu) live ISO, over the serial console.

Logs in as the live user, then checks the things that make it a complete OS:
identity, the Plasma desktop session with the Ambar menu bar and dock, the
everyday apps, the app store and its sources, the installer, and Ambar's own
service. Saves screenshots through the QEMU monitor.
"""
import argparse
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from smoke_test import Serial, screendump  # noqa: E402

LIVE_USER = "ambar"

CHECKS = r"""
echo __BEGIN__
grep -E '^(NAME|PRETTY_NAME|ID|ID_LIKE)=' /etc/os-release
for i in $(seq 300); do pgrep -x plasmashell >/dev/null && [ -s ~/.config/plasma-org.kde.plasma.desktop-appletsrc ] && break; sleep 2; done
echo "--- sessions"; loginctl list-sessions --no-legend
echo "--- plasma config"; ls ~/.config | grep -E "plasma|kdeglobals|kwin" | tr '\n' ' '; echo
echo "--- session log"; journalctl --user -b --no-pager -o cat 2>/dev/null | grep -iE "plasmashell|kwin_wayland|error|fail" | tail -25
echo "--- sddm"; journalctl -b --no-pager -o cat -u sddm 2>/dev/null | tail -8
pgrep -x plasmashell >/dev/null && echo PLASMA_OK
pgrep -x kwin_wayland >/dev/null && echo KWIN_WAYLAND_OK
for a in dolphin konsole kate okular gwenview elisa haruna kcalc ark spectacle systemsettings plasma-discover \
         libreoffice kmail korganizer calamares flatpak snap distrobox wine timeshift kdeconnect-app \
         ambar ambard ambar-center ambar-control-center ambar-hwreport; do
  command -v "$a" >/dev/null && echo "APP_OK $a" || echo "APP_MISSING $a"
done
browser=$(cat /etc/ambar-browser); command -v "$browser" >/dev/null && echo "BROWSER_OK $browser"
for w in os.ambar.menu os.ambar.appname os.ambar.controlcenter org.kde.plasma.kickerdash org.kde.plasma.appmenu org.kde.plasma.icontasks; do
  grep -rqs "plugin=$w" /home/*/.config/plasma-org.kde.plasma.desktop-appletsrc && echo "WIDGET_OK $w"
done
grep -h '^\(Name\|Exec\)=' /usr/share/applications/org.kde.discover.desktop | head -2
flatpak remotes --system 2>/dev/null | grep -q flathub && echo FLATHUB_OK
systemctl --user is-active ambard && echo AMBARD_OK
ambar-control-center status
echo __END__
"""

EXPECT = [
    ("identity", r'^NAME="Ambar OS"'),
    ("ubuntu base", r'^ID_LIKE="ubuntu debian"'),
    ("plasma session", r"^PLASMA_OK"),
    ("ambar menu bar", r"^WIDGET_OK os\.ambar\.menu"),
    ("app name in menu bar", r"^WIDGET_OK os\.ambar\.appname"),
    ("control center", r"^WIDGET_OK os\.ambar\.controlcenter"),
    ("global app menus", r"^WIDGET_OK org\.kde\.plasma\.appmenu"),
    ("launchpad", r"^WIDGET_OK org\.kde\.plasma\.kickerdash"),
    ("dock", r"^WIDGET_OK org\.kde\.plasma\.icontasks"),
    ("browser", r"^BROWSER_OK "),
    ("ambar store", r"^Name=Ambar Store"),
    ("installer", r"^APP_OK calamares"),
    ("ambar service", r"^AMBARD_OK"),
    ("control center backend", r'"volume"'),
]


def run(sock, log, monitor, shots, boot_timeout):
    s = Serial(sock, log)
    s.read_until(r"login:\s*$", boot_timeout)
    s.send(LIVE_USER + "\n")
    s.read_until(r"[$#]\s*$", 120)
    if monitor and shots:
        screendump(monitor, str(Path(shots) / "boot-desktop.png"))
    s.send(CHECKS.strip() + "\n")
    out = s.read_until(r"^__END__\s*$", 900)
    out = out[out.find("__BEGIN__"):]
    print(out)
    if monitor and shots:
        time.sleep(30)
        screendump(monitor, str(Path(shots) / "desktop.png"))
    missing = [l.split()[1] for l in out.splitlines() if l.startswith("APP_MISSING")]
    failed = [n for n, rx in EXPECT if not re.search(rx, out, re.M)]
    if missing:
        print("apps missing:", " ".join(missing))
    if failed or missing:
        print("BOOT TEST FAILED:", ", ".join(failed + [f"app {m}" for m in missing]))
        return 1
    print("BOOT TEST PASSED:", ", ".join(n for n, _ in EXPECT))
    return 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("socket")
    ap.add_argument("--log")
    ap.add_argument("--monitor")
    ap.add_argument("--shots")
    ap.add_argument("--boot-timeout", type=int, default=2400)
    a = ap.parse_args()
    sys.exit(run(a.socket, a.log, a.monitor, a.shots, a.boot_timeout))
