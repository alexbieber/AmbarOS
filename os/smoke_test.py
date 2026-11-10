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

EXPECT = [
    ("branding", r'PRETTY_NAME="Ambar OS'),
    ("cli", r"ambar \d+\.\d+\.\d+"),
    ("user service", r"^active\s*$"),
    ("service health", r"ambard is running \(ok\)"),
    ("marketplace", r"dev\.ambar\.daily-notes"),
    ("workflow run", r"succeeded\s+smoke"),
    ("workflow output", r"^ok\s*$"),
]


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
        self.log = open(log, "w") if log else None

    def read_until(self, pattern, timeout):
        rx = re.compile(pattern, re.M)
        end = time.time() + timeout
        while time.time() < end:
            m = rx.search(self.buf)
            if m:
                out, self.buf = self.buf[: m.end()], self.buf[m.end():]
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
            self.buf += chunk.replace("\r", "")
        raise TimeoutError(f"timed out waiting for {pattern!r}; last output:\n{self.buf[-800:]}")

    def send(self, text):
        self.sock.sendall(text.encode())


def run(path, log=None, boot_timeout=900):
    s = Serial(path, log)
    s.read_until(r"login:\s*$", boot_timeout)
    s.send(USER + "\n")
    s.read_until(r"Password:\s*$", 60)
    s.send(PASSWORD + "\n")
    s.read_until(r"[$#]\s*$", 120)
    s.send(SCRIPT.strip() + "\n")
    out = s.read_until(r"^__END__\s*$", 300)
    out = out[out.find("__BEGIN__"):]
    failed = [name for name, rx in EXPECT if not re.search(rx, out, re.M)]
    print(out)
    if failed:
        print("SMOKE TEST FAILED:", ", ".join(failed))
        return 1
    print("SMOKE TEST PASSED:", ", ".join(n for n, _ in EXPECT))
    return 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("socket")
    ap.add_argument("--log")
    ap.add_argument("--boot-timeout", type=int, default=900)
    a = ap.parse_args()
    sys.exit(run(a.socket, a.log, a.boot_timeout))
