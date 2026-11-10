# Ambar developer guide

There are four ways to build on Ambar:

1. **Workflow files:** the TOML format in [`workflow-format.md`](workflow-format.md).
2. **Packages:** signed bundles of workflows that people install from a marketplace.
3. **Agents:** programs (often AI assistants) with their own identity and limited scopes, talking to `ambard` through the SDK.
4. **The protocol:** newline-delimited JSON over the local socket. The SDKs wrap it.

## SDKs

| Language | Where | Notes |
|---|---|---|
| Python | `sdk/python` (`pip install ./sdk/python`) | Standard library only. Owner and agent modes. |
| Rust | `crates/ambar-service` (`ambar_service::Client`) | Async (tokio). Used by the CLI. |

```python
from ambar_sdk import Client, AmbarError

with Client() as ambar:                     # owner: reads <data>/run/ambard.token
    print(ambar.health()["status"])
    for wf in ambar.list_workflows():
        print(wf["id"], wf["state"], wf["last_run"])
```

## Writing an agent

The owner creates an identity. The token is shown once:

```sh
ambar agent create planner --scope read --scope propose --scope memory \
      --description "suggests automations"
```

The agent connects with that token:

```python
import os
from ambar_sdk import Client, AmbarError

with Client(token=os.environ["AMBAR_AGENT_TOKEN"], client_name="planner") as a:
    me = a.whoami()                         # {"actor": "agent:agent-1a2b…", "scopes": [...]}
    seen = a.memory_get("last_review")      # private, per-agent, ≤ 64 KiB per key
    proposal = a.propose(open("tidy.toml").read(), reason="Downloads has 300 files")
    a.memory_put("last_review", proposal["id"])
```

The owner reviews the proposal with `ambar agent proposals` / `ambar agent approve <id>`, or in the Automation Center.

### Scopes

| Scope | Allows |
|---|---|
| `read` | `health`, `list_workflows`, `history` |
| `propose` | `propose`, `proposal_list` (own proposals only) |
| `memory` | `memory_get/put/list/delete` (own namespace only) |
| `run:<id>` / `run:*` | `run_now` for those workflows (they must already be enabled) |
| `control:<id>` / `control:*` | `pause`, `resume`, `disable`, `cancel` for those workflows |

**Owner only, never delegable:** `register`, `enable`, `revoke`, `remove`, `shutdown`, agent management, and approving or rejecting proposals. An agent can never widen its own access or grant permissions.

### Limits and monitoring

- **Rate limit:** 120 requests per minute per agent. Over the limit, requests return `rate_limited`.
- **Proposals:** at most 20 pending proposals per agent. Memory is capped at 1000 keys of 64 KiB each.
- **Live checks:** the identity is re-checked on every request, so revoking an agent cuts off connections that are already open.
- **Audit:** every refused request is audited as `ipc.forbidden`, with the agent id. Runs an agent starts are recorded with trigger `agent:<id>`.
- **Activity:** `ambar agent list` shows requests, refusals and when each agent was last seen.

## Packages

```
my-package/
  ambar-package.toml
  workflows/
    first.toml
```

```toml
# ambar-package.toml
name = "dev.yourname.my-package"   # reverse-DNS style, lowercase
version = "1.0.0"                  # MAJOR.MINOR.PATCH
description = "One line"
author = "Your Name"
license = "Apache-2.0"
homepage = "https://…"
workflows = ["workflows/first.toml"]
```

```sh
ambar pkg keygen "Your Name"                          # once; ~/.config/ambar/keys (0600)
ambar pkg build my-package --key your-name            # → dev.yourname.my-package-1.0.0.ambarpkg
ambar pkg verify dev.yourname.my-package-1.0.0.ambarpkg
ambar pkg publish dev.yourname.my-package-1.0.0.ambarpkg --index ./market/index.json
```

### What installing checks

1. If installing from an index, the downloaded file's SHA-256 and signer fingerprint must match the index entry.
2. The ed25519 signature must cover the exact manifest and workflow files. Any edit breaks it.
3. The manifest must list exactly the files present, with safe relative paths, at most 50 workflows and 1 MiB.
4. Every workflow must parse (unknown fields and actions are rejected) and validate.
5. The signer's **key** must be trusted. Trust is by key, not by name, so someone who signs with a different key under the same name is a different publisher.
6. The risk review runs. High-risk packages can't be installed with `--yes`.
7. You consent to the permissions. Only then is each workflow registered, with a grant.
8. Workflow ids that belong to something else are never overwritten.

`ambar pkg remove <name>` removes the package's workflows and their grants. Run history is kept.

**Why packages can't add actions or run code:** Ambar's security rests on a closed set of file actions that are checked against permissions at run time. Plugins that add arbitrary code would need sandboxing (for example WASM or Podman with seccomp), which is planned for a later phase.

## Marketplace

A marketplace is a static `index.json` plus the package files. It can be served from any HTTPS host, or read from a folder:

```json
{ "format": 1, "name": "Ambar marketplace",
  "packages": [ { "name": "dev.ambar.daily-notes", "version": "1.0.0", "description": "…",
                  "author": "Ambar Examples", "url": "packages/dev.ambar.daily-notes-1.0.0.ambarpkg",
                  "sha256": "…", "signer": "1d9e53421fa89438" } ] }
```

Set a default index in `~/.config/ambar/marketplace.toml` with `index = "https://…/index.json"`, or pass `--index` to each command:

```sh
ambar pkg search notes
ambar pkg install dev.ambar.daily-notes --enable
```

`marketplace/` in this repo holds three example packages, signed by the "Ambar Examples" key (`marketplace/ambar-examples.pub`, fingerprint `1d9e53421fa89438`). **The private key was not committed.** Before running a public marketplace, generate a new publisher key, keep it offline, and re-sign.

## Protocol reference

**Transport:** `<data>/run/ambard.sock` (mode 0600), one JSON object per line, at most 1 MiB.

**Handshake:** the first message must be `hello`, sent within 5 s:

```json
{"op":"hello","token":"<owner token or agent token>","protocol":1,"client":"my-tool"}
```

**Responses** look like `{"ok":true,"data":…}` or `{"ok":false,"error":{"code":"…","message":"…"}}`.

**Error codes:**
- `unauthenticated`: bad token, wrong user, revoked agent
- `forbidden`: the identity lacks the scope
- `rate_limited`
- `bad_request`
- `not_found`
- `invalid`: the workflow failed validation
- `permission`: grant missing or too narrow
- `state`: not allowed in the current state, for example already running
- `shutting_down`
- `internal`

**Operations** (field names exactly as sent):

| op | fields | who |
|---|---|---|
| `health`, `list_workflows`, `who_am_i` | – | owner, `read` (`who_am_i`: anyone) |
| `history` | `workflow?`, `limit?` | owner, `read` |
| `register` | `definition`, `grant?: {allow_sensitive}`, `replace?` | owner |
| `enable` / `revoke` / `remove` | `id` | owner |
| `disable` / `pause` / `resume` / `cancel` | `id` | owner, `control:<id>` |
| `run_now` | `id` | owner, `run:<id>` |
| `propose` | `definition`, `reason?` | `propose` |
| `proposal_list` | – | owner (all), `propose` (own) |
| `proposal_approve` | `id`, `enable?`, `allow_sensitive?` | owner |
| `proposal_reject` | `id` | owner |
| `memory_get` / `memory_delete` | `key` | `memory` |
| `memory_put` | `key`, `value` | `memory` |
| `memory_list` | – | `memory` |
| `agent_create` | `name`, `scopes`, `description?` | owner |
| `agent_list`, `agent_revoke` (`id`) | | owner |
| `shutdown` | – | owner |

The authoritative definition is `crates/ambar-service/src/protocol.rs`. The authorization rules are one pure function, `agents::authorize`, and are unit-tested.
