"""Socket client for ambard (protocol version 1: newline-delimited JSON)."""

from __future__ import annotations

import json
import os
import socket
from pathlib import Path
from typing import Any, Optional

PROTOCOL_VERSION = 1
MAX_FRAME = 1024 * 1024


class AmbarError(Exception):
    """The service refused a request. ``code`` is the protocol error code,
    e.g. ``forbidden``, ``permission``, ``not_found``, ``invalid``."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


class NotRunning(AmbarError):
    """ambard is not running for this data directory."""

    def __init__(self, socket_path: Path):
        super().__init__("not_running", f"ambard is not running (no socket at {socket_path})")


def default_data_dir() -> Path:
    """Same rules as the Rust side: $AMBAR_DATA_DIR, $XDG_DATA_HOME/ambar, ~/.local/share/ambar."""
    if os.environ.get("AMBAR_DATA_DIR"):
        return Path(os.environ["AMBAR_DATA_DIR"])
    if os.environ.get("XDG_DATA_HOME"):
        return Path(os.environ["XDG_DATA_HOME"]) / "ambar"
    return Path.home() / ".local" / "share" / "ambar"


class Client:
    """An authenticated connection to ambard.

    ``token=None`` authenticates as the owner by reading ``run/ambard.token``.
    Pass an agent token to act as that agent; the service then enforces the
    agent's scopes.
    """

    def __init__(
        self,
        data_dir: Optional[os.PathLike | str] = None,
        token: Optional[str] = None,
        client_name: str = "ambar-sdk-python",
        timeout: float = 10.0,
    ):
        self.data_dir = Path(data_dir) if data_dir else default_data_dir()
        self.socket_path = self.data_dir / "run" / "ambard.sock"
        self.timeout = timeout
        self._sock: Optional[socket.socket] = None
        self._buf = b""
        try:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.settimeout(timeout)
            sock.connect(str(self.socket_path))
        except (FileNotFoundError, ConnectionRefusedError):
            raise NotRunning(self.socket_path) from None
        self._sock = sock
        if token is None:
            token = (self.data_dir / "run" / "ambard.token").read_text().strip()
        self.welcome = self.call(
            {"op": "hello", "token": token, "protocol": PROTOCOL_VERSION, "client": client_name}
        )

    # -- plumbing ---------------------------------------------------------

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def close(self) -> None:
        if self._sock is not None:
            try:
                self._sock.close()
            finally:
                self._sock = None

    def _read_line(self) -> bytes:
        assert self._sock is not None
        while b"\n" not in self._buf:
            chunk = self._sock.recv(65536)
            if not chunk:
                raise AmbarError("protocol", "connection closed by ambard")
            self._buf += chunk
            if len(self._buf) > MAX_FRAME + 1:
                raise AmbarError("protocol", "response too large")
        line, self._buf = self._buf.split(b"\n", 1)
        return line

    def call(self, request: dict) -> Any:
        """Sends one request and returns its ``data``; raises :class:`AmbarError`."""
        if self._sock is None:
            raise AmbarError("protocol", "client is closed")
        self._sock.sendall(json.dumps(request).encode() + b"\n")
        resp = json.loads(self._read_line())
        if not resp.get("ok"):
            err = resp.get("error") or {}
            raise AmbarError(err.get("code", "internal"), err.get("message", "unknown error"))
        return resp.get("data")

    def _op(self, op: str, **fields: Any) -> Any:
        return self.call({"op": op, **fields})

    # -- service ----------------------------------------------------------

    def health(self) -> dict:
        return self._op("health")

    def whoami(self) -> dict:
        return self._op("who_am_i")

    def shutdown(self) -> None:
        self._op("shutdown")

    # -- workflows --------------------------------------------------------

    def list_workflows(self) -> list[dict]:
        return self._op("list_workflows")

    def register(
        self,
        definition: str,
        grant: bool = True,
        allow_sensitive: bool = False,
        replace: bool = False,
    ) -> dict:
        """Owner only. ``grant=True`` approves the workflow's declared
        permissions; only do that after the user has agreed."""
        g = {"allow_sensitive": allow_sensitive} if grant else None
        return self._op("register", definition=definition, grant=g, replace=replace)

    def enable(self, workflow_id: str) -> None:
        self._op("enable", id=workflow_id)

    def disable(self, workflow_id: str) -> None:
        self._op("disable", id=workflow_id)

    def pause(self, workflow_id: str) -> None:
        self._op("pause", id=workflow_id)

    def resume(self, workflow_id: str) -> None:
        self._op("resume", id=workflow_id)

    def cancel(self, workflow_id: str) -> None:
        self._op("cancel", id=workflow_id)

    def run_now(self, workflow_id: str) -> dict:
        return self._op("run_now", id=workflow_id)

    def remove(self, workflow_id: str) -> None:
        """Owner only. Deletes the workflow and its grant; history is kept."""
        self._op("remove", id=workflow_id)

    def revoke(self, workflow_id: str) -> None:
        self._op("revoke", id=workflow_id)

    def history(self, workflow: Optional[str] = None, limit: int = 50) -> list[dict]:
        return self._op("history", workflow=workflow, limit=limit)

    # -- agents & proposals ----------------------------------------------

    def propose(self, definition: str, reason: str = "") -> dict:
        return self._op("propose", definition=definition, reason=reason)

    def proposals(self) -> list[dict]:
        return self._op("proposal_list")

    def approve(self, proposal_id: str, enable: bool = False, allow_sensitive: bool = False) -> dict:
        return self._op("proposal_approve", id=proposal_id, enable=enable, allow_sensitive=allow_sensitive)

    def reject(self, proposal_id: str) -> dict:
        return self._op("proposal_reject", id=proposal_id)

    def memory_get(self, key: str) -> Optional[str]:
        return self._op("memory_get", key=key)

    def memory_put(self, key: str, value: str) -> None:
        self._op("memory_put", key=key, value=value)

    def memory_list(self) -> list[str]:
        return self._op("memory_list")

    def memory_delete(self, key: str) -> bool:
        return bool(self._op("memory_delete", key=key)["deleted"])

    def approvals(self) -> list[dict]:
        """Owner only. Sensitive actions waiting for a decision."""
        return self._op("approval_list")

    def decide(self, approval_id: str, allow: bool) -> dict:
        """Owner only. Allow or deny a waiting sensitive action."""
        return self._op("approval_decide", id=approval_id, allow=allow)

    def agent_create(self, name: str, scopes: list[str], description: str = "") -> tuple[dict, str]:
        d = self._op("agent_create", name=name, scopes=scopes, description=description)
        return d["agent"], d["token"]

    def agents(self) -> list[dict]:
        return self._op("agent_list")

    def agent_revoke(self, agent: str) -> dict:
        return self._op("agent_revoke", id=agent)
