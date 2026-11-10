"""Python SDK for Ambar OS.

Talk to the ``ambard`` background service over its local socket, either as the
owner (using the per-start token in the data directory) or as an agent (using
an agent token created with ``ambar agent create``).

    from ambar_sdk import Client

    with Client() as ambar:                      # owner
        for wf in ambar.list_workflows():
            print(wf["id"], wf["state"])

    with Client(token=os.environ["AMBAR_AGENT_TOKEN"]) as agent:   # agent
        agent.propose(open("tidy.toml").read(), reason="keeps Downloads tidy")
        agent.memory_put("last_check", "2026-11-11")

The SDK has no dependencies beyond the Python standard library.
"""

from .client import (
    AmbarError,
    Client,
    NotRunning,
    PROTOCOL_VERSION,
    default_data_dir,
)

__all__ = [
    "AmbarError",
    "Client",
    "NotRunning",
    "PROTOCOL_VERSION",
    "default_data_dir",
]
__version__ = "0.1.0"
