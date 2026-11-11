"""Ambar in KDE's search (KRunner, Alt+Space).

A KRunner D-Bus runner (interface ``org.kde.krunner1``). Typing:

* part of an automation's name → "Run <name>" (manual and scheduled ones)
  and "Open <name> in Automation Center";
* ``automate …`` / ``ambar …`` → "Create an automation: …", which opens the
  Automation Center with a draft request (nothing is added without review);
* ``approval`` → how many sensitive actions are waiting for you.

Pure Python (jeepney); talks to ambard through the SDK as the owner.
Registered by ``/usr/share/krunner/dbusplugins/os.ambar.krunner.desktop``
and D-Bus activated by ``/usr/share/dbus-1/services/os.ambar.KRunner.service``.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import threading
import time
from typing import Optional

from ambar_sdk import AmbarError, Client

BUS_NAME = "os.ambar.KRunner"
OBJECT_PATH = "/runner"
INTERFACE = "org.kde.krunner1"
ICON = "os.ambar.AutomationCenter"

# KRunner match categories (Plasma: KRunner::QueryMatch::CategoryRelevance).
EXACT, HIGH, MODERATE = 100, 70, 50

INTROSPECTION = f"""<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="{INTERFACE}">
    <method name="Actions"><arg name="matches" type="a(sss)" direction="out"/></method>
    <method name="Match">
      <arg name="query" type="s" direction="in"/>
      <arg name="matches" type="a(sssida{{sv}})" direction="out"/>
    </method>
    <method name="Run">
      <arg name="matchId" type="s" direction="in"/>
      <arg name="actionId" type="s" direction="in"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect"><arg name="data" type="s" direction="out"/></method>
  </interface>
</node>"""

TRIGGERS = ("automate ", "ambar ", "automation ")


def match(query: str, workflows: list[dict], approvals: int) -> list[tuple]:
    """Pure matching logic: returns KRunner match tuples ``(id, text, icon,
    category, relevance, properties)``."""
    q = query.strip()
    ql = q.lower()
    out: list[tuple] = []
    if len(ql) < 2:
        return out
    for prefix in TRIGGERS:
        if ql.startswith(prefix) and len(q) > len(prefix) + 2:
            text = q[len(prefix):].strip()
            out.append((f"create:{text}", f"Create an automation: {text}", ICON, HIGH, 0.9,
                        {"subtext": ("s", "Opens a draft in Automation Center for you to review")}))
            break
    if "approv" in ql and approvals:
        noun = "action is" if approvals == 1 else "actions are"
        out.append(("approvals", f"{approvals} {noun} waiting for your approval", ICON, HIGH, 0.95,
                    {"subtext": ("s", "Open Automation Center")}))
    if "automation center" in ql or ql in ("ambar", "automations"):
        out.append(("open", "Automation Center", ICON, EXACT if ql == "ambar" else HIGH, 0.8,
                    {"subtext": ("s", "See and control your automations")}))
    for wf in workflows:
        name, wid = wf.get("name", ""), wf.get("id", "")
        hay = f"{name} {wid}".lower()
        if ql not in hay:
            continue
        exact = ql in (name.lower(), wid.lower())
        runnable = wf.get("state") == "active" and wf.get("trigger") != "folder_watch"
        state = {"inactive": "disabled"}.get(wf.get("state", ""), wf.get("state", ""))
        if runnable:
            out.append((f"run:{wid}", f"Run “{name}”", ICON, EXACT if exact else MODERATE,
                        1.0 if exact else 0.7, {"subtext": ("s", f"Ambar automation · {state}")}))
        out.append((f"show:{wid}", f"{name} — {state}", ICON, MODERATE, 0.5,
                    {"subtext": ("s", "Open in Automation Center")}))
    return out[:20]


class Runner:
    """State and actions behind the D-Bus interface."""

    def __init__(self, data_dir: Optional[str] = None, center: Optional[str] = None):
        self.data_dir = data_dir
        self.center = center or os.environ.get("AMBAR_CENTER_BIN") or shutil.which("ambar-center") \
            or f"{sys.executable} -m ambar_center"
        self._cache: tuple[float, list, int] = (0.0, [], 0)

    def _snapshot(self) -> tuple[list, int]:
        t, wfs, n = self._cache
        if time.monotonic() - t < 2:
            return wfs, n
        try:
            with Client(self.data_dir, client_name="ambar-krunner", timeout=2) as c:
                wfs = c.list_workflows()
                n = len(c.approvals())
        except (AmbarError, OSError, ValueError):
            wfs, n = [], 0
        self._cache = (time.monotonic(), wfs, n)
        return wfs, n

    def match(self, query: str) -> list[tuple]:
        wfs, n = self._snapshot()
        return match(query, wfs, n)

    def _open_center(self, *args: str) -> None:
        cmd = self.center.split() + list(args)
        subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)

    def run(self, match_id: str, _action_id: str = "") -> None:
        kind, _, arg = match_id.partition(":")
        if kind == "run":
            def go() -> None:
                try:
                    with Client(self.data_dir, client_name="ambar-krunner", timeout=600) as c:
                        c.run_now(arg)
                except (AmbarError, OSError):
                    pass  # the run and its outcome are recorded by ambard
            threading.Thread(target=go, daemon=True).start()
        elif kind == "create":
            self._open_center("--describe", arg)
        elif kind == "show":
            self._open_center("--page", "automations")
        elif kind == "approvals":
            self._open_center("--page", "overview")
        elif kind == "open":
            self._open_center()


def serve(runner: Runner, bus: str = "SESSION", ready: Optional[threading.Event] = None,
          stop: Optional[threading.Event] = None) -> None:
    """Serves ``org.kde.krunner1`` on the session bus until ``stop`` is set."""
    from jeepney import HeaderFields, MessageType, new_error, new_method_return
    from jeepney.bus_messages import message_bus
    from jeepney.io.blocking import open_dbus_connection

    conn = open_dbus_connection(bus=bus)
    reply = conn.send_and_get_reply(message_bus.RequestName(BUS_NAME, 4))  # DO_NOT_QUEUE
    if reply.body[0] not in (1, 4):  # primary owner / already owner
        raise SystemExit(f"{BUS_NAME} is already taken on the session bus")
    if ready:
        ready.set()
    while not (stop and stop.is_set()):
        try:
            msg = conn.receive(timeout=0.5)
        except TimeoutError:
            continue
        h = msg.header
        if h.message_type != MessageType.method_call:
            continue
        iface = h.fields.get(HeaderFields.interface)
        member = h.fields.get(HeaderFields.member)
        path = h.fields.get(HeaderFields.path)
        try:
            if path != OBJECT_PATH:
                conn.send(new_error(msg, "org.freedesktop.DBus.Error.UnknownObject"))
            elif member == "Introspect":
                conn.send(new_method_return(msg, "s", (INTROSPECTION,)))
            elif iface in (INTERFACE, None) and member == "Match":
                conn.send(new_method_return(msg, "a(sssida{sv})", (runner.match(msg.body[0]),)))
            elif iface in (INTERFACE, None) and member == "Actions":
                conn.send(new_method_return(msg, "a(sss)", ([],)))
            elif iface in (INTERFACE, None) and member == "Run":
                runner.run(*msg.body)
                conn.send(new_method_return(msg))
            else:
                conn.send(new_error(msg, "org.freedesktop.DBus.Error.UnknownMethod"))
        except Exception as e:  # noqa: BLE001 - never crash the runner on one bad call
            conn.send(new_error(msg, "org.freedesktop.DBus.Error.Failed", "s", (str(e),)))
    conn.close()


def main() -> int:
    serve(Runner(os.environ.get("AMBAR_DATA_DIR")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
