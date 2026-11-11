#!/usr/bin/env python3
"""Logs into a booted Ambar OS over its serial console and checks it works.

Checks: branding, ambard running as a user service, CLI and service health,
the bundled marketplace, and a real workflow run through the service.
Exit 0 on success. Used by os/test-qemu.sh and the os-image CI job.
"""
import argparse
import re
import socket
import sys
import time

USER, PASSWORD = "ambar", "ambar"  # created by os/config/qcow2.toml (test images only)
LIVE_USER = "liveuser"  # live ISO: passwordless, created by ambar-live-setup.service

SCRIPT = r"""
echo __BEGIN__
grep PRETTY_NAME /etc/os-release
ambar --version
for i in $(seq 30); do systemctl --user is-active ambard && break; sleep 1; done
ambar service status
ambar pkg search notes --index /usr/share/ambar/marketplace/index.json
mkdir -p ~/smoke && cat > ~/smoke/wf.toml <<'TOML'
id = "smoke"
name = "Smoke test"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "~/smoke/out.txt"
content = "ok"
[permissions]
write = ["~/smoke"]
TOML
ambar workflow add ~/smoke/wf.toml --yes --enable
ambar workflow run smoke
cat ~/smoke/out.txt; echo
echo __END__
"""

# Phase 2: log the test user into Plasma and check the desktop integration.
DESKTOP_SCRIPT = r"""
echo __DBEGIN__
if ! pgrep -u {user} -x plasmashell >/dev/null; then
  echo {password} | sudo -S sh -c 'mkdir -p /etc/sddm.conf.d && printf "[Autologin]\nUser={user}\nSession=plasma\n" > /etc/sddm.conf.d/zz-smoke.conf && systemctl restart sddm' 2>/dev/null
fi
for i in $(seq 120); do pgrep -u {user} -x plasmashell >/dev/null && break; sleep 2; done
pgrep -u {user} -x plasmashell >/dev/null && echo PLASMA_OK
sleep 20
pgrep -u {user} -f "ambar-center --welcome" >/dev/null && echo WELCOME_OK
busctl --user call os.ambar.KRunner /runner org.kde.krunner1 Match s "automate tidy my desktop" 2>&1 | head -c 400; echo
plasma-apply-lookandfeel --list 2>/dev/null | grep os.ambar
echo __DEND__
"""

DESKTOP_EXPECT = [
    ("plasma session", r"^PLASMA_OK\s*$"),
    ("welcome autostart", r"^WELCOME_OK\s*$"),
    ("krunner runner", r"Create an automation: tidy my desktop"),
    ("ambar look-and-feel", r"^os\.ambar\.desktop\s*$"),
]

EXPECT = [
    ("branding", r'PRETTY_NAME="Ambar OS'),
    ("cli", r"ambar \d+\.\d+\.\d+"),
    ("user service", r"^active\s*$"),
    ("service health", r"ambard is running \(ok\)"),
    ("marketplace", r"dev\.ambar\.daily-notes"),
    ("workflow run", r"succeeded\s+smoke"),
    ("workflow output", r"^ok\s*$"),
]


ESCAPES = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b[=>]|\r")


def clean(text):
    """Strips terminal control sequences (colours, titles, and the OSC 3008
    context markers newer systemd shells print), so output lines can be matched."""
    return ESCAPES.sub("", text)


class Serial:
    def __init__(self, path, log):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        for _ in range(100):
            try:
                self.sock.connect(path)
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit(f"cannot connect to serial socket {path}")
        self.sock.settimeout(1)
        self.buf = ""
        self.buf_base = ""
        self.raw = ""
        self.log = open(log, "w") if log else None

    def read_until(self, pattern, timeout):
        rx = re.compile(pattern, re.M)
        end = time.time() + timeout
        while time.time() < end:
            m = rx.search(self.buf)
            if m:
                out, self.buf = self.buf[: m.end()], self.buf[m.end():]
                self.buf_base, self.raw = self.buf, ""
                return out
            try:
                chunk = self.sock.recv(4096).decode(errors="replace")
            except socket.timeout:
                continue
            if not chunk:
                break
            if self.log:
                self.log.write(chunk)
                self.log.flush()
            self.raw += chunk
            # Clean the whole tail, so escape sequences split across reads are removed too.
            self.buf = self.buf_base + clean(self.raw)
        raise TimeoutError(f"timed out waiting for {pattern!r}; last output:\n{self.buf[-800:]}")

    def send(self, text):
        self.sock.sendall(text.encode())


def screendump(monitor, out):
    """Saves the VM screen as PNG through the QEMU monitor."""
    m = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    m.connect(monitor)
    m.settimeout(5)
    try:
        m.recv(4096)
    except socket.timeout:
        pass
    m.sendall(f"screendump {out} -f png\n".encode())
    time.sleep(3)
    m.close()


def run(path, log=None, boot_timeout=900, monitor=None, screenshot=None, desktop=True,
        user=USER, password=PASSWORD):
    s = Serial(path, log)
    s.read_until(r"login:\s*$", boot_timeout)
    s.send(user + "\n")
    if password:  # a passwordless account (live ISO) goes straight to the shell
        s.read_until(r"Password:\s*$", 60)
        s.send(password + "\n")
    s.read_until(r"[$#]\s*$", 120)
    s.send(SCRIPT.strip() + "\n")
    out = s.read_until(r"^__END__\s*$", 300)
    out = out[out.find("__BEGIN__"):]
    failed = [name for name, rx in EXPECT if not re.search(rx, out, re.M)]
    print(out)
    checks = list(EXPECT)
    if desktop:
        script = DESKTOP_SCRIPT.replace("{user}", user).replace("{password}", password or "x")
        s.send(script.strip() + "\n")
        dout = s.read_until(r"^__DEND__\s*$", 600)
        dout = dout[dout.find("__DBEGIN__"):]
        print(dout)
        failed += [name for name, rx in DESKTOP_EXPECT if not re.search(rx, dout, re.M)]
        checks += DESKTOP_EXPECT
        if monitor and screenshot:
            screendump(monitor, screenshot)
            print(f"screenshot: {screenshot}")
    if failed:
        print("SMOKE TEST FAILED:", ", ".join(failed))
        return 1
    print("SMOKE TEST PASSED:", ", ".join(n for n, _ in checks))
    return 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("socket")
    ap.add_argument("--log")
    ap.add_argument("--boot-timeout", type=int, default=900)
    ap.add_argument("--monitor", help="QEMU monitor socket (for the desktop screenshot)")
    ap.add_argument("--screenshot", help="where to save the desktop screenshot (PNG)")
    ap.add_argument("--no-desktop", action="store_true", help="skip the graphical-session checks")
    ap.add_argument("--live", action="store_true", help="live ISO: log in as the passwordless live user")
    ap.add_argument("--user", help="account to log in as (default: the test user)")
    ap.add_argument("--password", help="its password ('' for none)")
    a = ap.parse_args()
    user = a.user or (LIVE_USER if a.live else USER)
    password = a.password if a.password is not None else ("" if a.live else PASSWORD)
    sys.exit(run(a.socket, a.log, a.boot_timeout, a.monitor, a.screenshot, not a.no_desktop,
                 user, password))
