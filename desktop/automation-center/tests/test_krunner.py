"""KRunner integration: matching logic, and the real D-Bus interface on a
private session bus (dbus-daemon), called the way KRunner calls it."""
import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sdk" / "python"))
sys.path.insert(0, str(ROOT / "desktop" / "automation-center"))

from ambar_center import krunner  # noqa: E402

WFS = [
    {"id": "daily-note", "name": "Daily note", "state": "active", "trigger": "schedule"},
    {"id": "file-pdfs", "name": "File PDFs", "state": "active", "trigger": "folder_watch"},
    {"id": "old-report", "name": "Old report", "state": "inactive", "trigger": "manual"},
]


def ids(ms):
    return [m[0] for m in ms]


def test_match_logic():
    assert ids(krunner.match("daily note", WFS, 0)) == ["run:daily-note", "show:daily-note"]
    assert ids(krunner.match("pdfs", WFS, 0)) == ["show:file-pdfs"], "folder workflows aren't run by hand"
    assert ids(krunner.match("old", WFS, 0)) == ["show:old-report"], "disabled ones aren't runnable"
    m = krunner.match("automate move my screenshots off the desktop", WFS, 0)
    assert m[0][0] == "create:move my screenshots off the desktop"
    assert ids(krunner.match("approvals", WFS, 2)) == ["approvals"]
    assert krunner.match("approvals", WFS, 0) == []
    assert krunner.match("x", WFS, 0) == []
    for mt in krunner.match("daily", WFS, 0):
        assert len(mt) == 6 and isinstance(mt[4], float) and isinstance(mt[5], dict)


@pytest.fixture
def session_bus(tmp_path):
    if not shutil.which("dbus-daemon"):
        pytest.skip("dbus-daemon not installed")
    p = subprocess.Popen(["dbus-daemon", "--session", "--print-address", "--nofork"],
                         stdout=subprocess.PIPE, text=True)
    addr = p.stdout.readline().strip()
    old = os.environ.get("DBUS_SESSION_BUS_ADDRESS")
    os.environ["DBUS_SESSION_BUS_ADDRESS"] = addr
    yield addr
    p.terminate()
    if old is None:
        os.environ.pop("DBUS_SESSION_BUS_ADDRESS", None)
    else:
        os.environ["DBUS_SESSION_BUS_ADDRESS"] = old


def test_dbus_interface(session_bus, tmp_path):
    from jeepney import DBusAddress, new_method_call
    from jeepney.io.blocking import open_dbus_connection

    log = tmp_path / "center-args.txt"
    fake_center = tmp_path / "fake-center"
    fake_center.write_text(f"#!/bin/sh\necho \"$@\" >> {log}\n")
    fake_center.chmod(0o755)

    class Fixed(krunner.Runner):
        def _snapshot(self):
            return WFS, 1

    stop, ready = threading.Event(), threading.Event()
    t = threading.Thread(target=krunner.serve, args=(Fixed(center=str(fake_center)),),
                         kwargs={"ready": ready, "stop": stop}, daemon=True)
    t.start()
    assert ready.wait(10)
    addr = DBusAddress(krunner.OBJECT_PATH, bus_name=krunner.BUS_NAME, interface=krunner.INTERFACE)
    conn = open_dbus_connection()
    try:
        r = conn.send_and_get_reply(new_method_call(addr, "Match", "s", ("daily",)))
        assert r.header.signature if hasattr(r.header, "signature") else True
        matches = r.body[0]
        assert matches[0][0] == "run:daily-note" and matches[0][1] == "Run “Daily note”"
        assert matches[0][5]["subtext"][1].startswith("Ambar automation")
        assert conn.send_and_get_reply(new_method_call(addr, "Actions")).body == ([],)
        intro = DBusAddress(krunner.OBJECT_PATH, bus_name=krunner.BUS_NAME,
                            interface="org.freedesktop.DBus.Introspectable")
        xml = conn.send_and_get_reply(new_method_call(intro, "Introspect")).body[0]
        assert "org.kde.krunner1" in xml
        conn.send_and_get_reply(new_method_call(addr, "Run", "ss", ("create:tidy my desktop", "")))
        for _ in range(50):
            if log.exists():
                break
            time.sleep(0.1)
        assert log.read_text().strip() == "--describe tidy my desktop"
    finally:
        conn.close()
        stop.set()
        t.join(5)
