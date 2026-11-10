"""Bridge between the QML interface and ambard.

The Automation Center is an *owner* client: it authenticates with the
per-start token in the data directory, exactly like the ``ambar`` CLI. Slow
operations (AI drafts, workflow runs) happen on worker threads; results come
back to the UI thread through Qt signals.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Optional

from PySide6.QtCore import Property, QObject, QTimer, Signal, Slot

from ambar_sdk import AmbarError, Client, NotRunning, default_data_dir


def _ago(ts: Optional[str]) -> str:
    if not ts:
        return "never"
    try:
        t = datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except ValueError:
        return ts
    secs = int((datetime.now(timezone.utc) - t).total_seconds())
    if secs < 45:
        return "just now"
    if secs < 3600:
        return f"{max(1, secs // 60)} min ago"
    if secs < 86400:
        return f"{secs // 3600} h ago"
    return t.astimezone().strftime("%d %b %H:%M")


class Backend(QObject):
    changed = Signal()
    busyChanged = Signal()
    toast = Signal(str, str)  # kind (ok|error|info), message
    draftReady = Signal("QVariantMap")
    draftFailed = Signal(str)
    _done = Signal(object)  # worker → UI thread

    def __init__(self, data_dir: Optional[os.PathLike] = None, ambar_bin: Optional[str] = None,
                 poll_ms: int = 2000):
        super().__init__()
        self._data_dir = Path(data_dir) if data_dir else default_data_dir()
        self._ambar = ambar_bin or os.environ.get("AMBAR_BIN") or shutil.which("ambar") or "ambar"
        self._connected = False
        self._health: dict = {}
        self._workflows: list = []
        self._proposals: list = []
        self._agents: list = []
        self._activity: list = []
        self._error = ""
        self._busy = ""
        self._done.connect(lambda fn: fn())
        self._timer = QTimer(self)
        self._timer.setInterval(poll_ms)
        self._timer.timeout.connect(self.refresh)
        if poll_ms > 0:
            self._timer.start()
        self.refresh()

    # ---------------------------------------------------------------- state

    def _client(self) -> Client:
        return Client(self._data_dir, client_name="ambar-center", timeout=5)

    @Slot()
    def refresh(self) -> None:
        try:
            with self._client() as c:
                self._health = c.health()
                wfs = c.list_workflows()
                props = c.proposals()
                agents = c.agents()
                hist = c.history(limit=60)
        except NotRunning:
            self._connected = False
            self._error = "The Ambar service isn't running."
            self.changed.emit()
            return
        except (AmbarError, OSError, ValueError) as e:
            self._connected = False
            self._error = str(e)
            self.changed.emit()
            return
        names = {a["id"]: a["name"] for a in agents}
        for w in wfs:
            lr = w.get("last_run") or {}
            w["last_status"] = lr.get("status", "")
            w["last_when"] = _ago(lr.get("started_at")) if lr else "never run"
        for p in props:
            p["agent_name"] = names.get(p["agent_id"], p["agent_id"])
            p["high"] = any(r.startswith("high") for r in p.get("risks", []))
            p["when"] = _ago(p.get("created_at"))
        for a in agents:
            a["when"] = _ago(a.get("last_seen"))
            a["scope_list"] = a.get("scopes", [])
        for h in hist:
            h["when"] = _ago(h.get("started_at"))
            h["steps_count"] = len(h.get("steps", []))
        self._workflows = wfs
        self._proposals = props
        self._agents = agents
        self._activity = hist
        self._connected = True
        self._error = ""
        self.changed.emit()

    @Property(bool, notify=changed)
    def connected(self) -> bool:
        return self._connected

    @Property("QVariantMap", notify=changed)
    def health(self) -> dict:
        return self._health

    @Property("QVariantList", notify=changed)
    def workflows(self) -> list:
        return self._workflows

    @Property("QVariantList", notify=changed)
    def pendingProposals(self) -> list:
        return [p for p in self._proposals if p["status"] == "pending"]

    @Property("QVariantList", notify=changed)
    def decidedProposals(self) -> list:
        return [p for p in self._proposals if p["status"] != "pending"][:20]

    @Property("QVariantList", notify=changed)
    def agents(self) -> list:
        return self._agents

    @Property("QVariantList", notify=changed)
    def activity(self) -> list:
        return self._activity

    @Property(str, notify=changed)
    def error(self) -> str:
        return self._error

    @Property(str, notify=busyChanged)
    def busy(self) -> str:
        """What is in progress (empty when idle)."""
        return self._busy

    def _set_busy(self, what: str) -> None:
        self._busy = what
        self.busyChanged.emit()

    # -------------------------------------------------------------- actions

    def _do(self, label: str, fn: Callable[[Client], Any], ok: Optional[str] = None) -> None:
        try:
            with self._client() as c:
                fn(c)
            if ok:
                self.toast.emit("ok", ok)
        except AmbarError as e:
            self.toast.emit("error", f"{label}: {e.message}")
        except OSError as e:
            self.toast.emit("error", f"{label}: {e}")
        self.refresh()

    def _in_background(self, what: str, work: Callable[[], Any],
                       done: Callable[[Any, Optional[BaseException]], None]) -> None:
        if self._busy:
            self.toast.emit("info", f"Still working on: {self._busy}")
            return
        self._set_busy(what)

        def run() -> None:
            try:
                result, err = work(), None
            except BaseException as e:  # noqa: BLE001 - reported to the UI
                result, err = None, e

            def finish() -> None:
                self._set_busy("")
                done(result, err)
            self._done.emit(finish)

        threading.Thread(target=run, daemon=True).start()

    @Slot(str, str)
    def control(self, op: str, workflow_id: str) -> None:
        verbs = {"pause": "Paused", "resume": "Resumed", "enable": "Enabled",
                 "disable": "Disabled", "cancel": "Cancelled"}
        if op not in verbs:
            self.toast.emit("error", f"Unknown action {op}")
            return
        self._do(op.capitalize(), lambda c: getattr(c, op)(workflow_id),
                 f"{verbs[op]} {workflow_id}")

    @Slot(str)
    def runNow(self, workflow_id: str) -> None:
        def work() -> dict:
            with self._client() as c:
                c.timeout = 600
                return c.run_now(workflow_id)

        def done(rec: Any, err: Optional[BaseException]) -> None:
            if err:
                self.toast.emit("error", f"Run {workflow_id}: {getattr(err, 'message', err)}")
            elif rec["status"] == "succeeded":
                self.toast.emit("ok", f"{workflow_id} finished")
            else:
                self.toast.emit("error", f"{workflow_id} {rec['status']}: {rec.get('error') or ''}")
            self.refresh()

        self._in_background(f"running {workflow_id}", work, done)

    @Slot(str, bool)
    def approve(self, proposal_id: str, enable: bool) -> None:
        self._do("Approve", lambda c: c.approve(proposal_id, enable=enable),
                 "Approved" + (" and enabled" if enable else ""))

    @Slot(str)
    def reject(self, proposal_id: str) -> None:
        self._do("Reject", lambda c: c.reject(proposal_id), "Proposal rejected")

    @Slot(str)
    def revokeAgent(self, agent_id: str) -> None:
        self._do("Revoke", lambda c: c.agent_revoke(agent_id), "Agent revoked")

    @Slot(str)
    def describe(self, text: str) -> None:
        """Asks the AI builder for a draft. Nothing is saved or registered."""
        text = text.strip()
        if not text:
            return

        def work() -> dict:
            env = {**os.environ, "AMBAR_DATA_DIR": str(self._data_dir)}
            proc = subprocess.run([self._ambar, "ai", "create", "--json", text],
                                  capture_output=True, text=True, timeout=300, env=env)
            if proc.returncode != 0:
                msg = proc.stderr.strip().splitlines()
                raise RuntimeError(msg[-1].removeprefix("Error: ") if msg else "the AI builder failed")
            return json.loads(proc.stdout)

        def done(draft: Any, err: Optional[BaseException]) -> None:
            if err:
                self.draftFailed.emit(str(err))
                return
            draft["high"] = any(r["level"] == "high" for r in draft.get("risks", []))
            self.draftReady.emit(draft)

        self._in_background("drafting an automation", work, done)

    @Slot(str, bool)
    def addDraft(self, toml_text: str, enable: bool) -> None:
        """The user reviewed the draft and clicked Add: register and grant."""
        def fn(c: Client) -> None:
            info = c.register(toml_text, grant=True)
            if enable:
                c.enable(info["id"])
        self._do("Add automation", fn, "Automation added" + (" and enabled" if enable else ""))

    @Slot()
    def startService(self) -> None:
        def work() -> str:
            env = {**os.environ, "AMBAR_DATA_DIR": str(self._data_dir)}
            p = subprocess.run([self._ambar, "service", "start"], capture_output=True, text=True,
                               timeout=30, env=env)
            if p.returncode != 0:
                raise RuntimeError(p.stderr.strip() or "could not start the service")
            return p.stdout

        def done(_: Any, err: Optional[BaseException]) -> None:
            if err:
                self.toast.emit("error", str(err))
            else:
                self.toast.emit("ok", "Ambar service started")
            self.refresh()

        self._in_background("starting the service", work, done)
