# Automation engine architecture

## Components

```
           ┌──────────────┐
 TOML ───► │  Workflow    │  parse (unknown fields rejected) + validate
           └──────┬───────┘
                  │ register
           ┌──────▼───────┐   grant_permissions (user consent)   ┌──────────┐
           │   Engine     │ ───────────────────────────────────► │  Store   │
           │              │   activate: grant must cover request │ (SQLite) │
           └──┬────────┬──┘                                      │ workflows│
   triggers   │        │  execute                                │ grants   │
 ┌────────────▼──┐  ┌──▼──────────────────────────┐              │ runs     │
 │ schedule      │  │ for each action:            │ ───────────► │ steps    │
 │ (every/cron)  │  │   checkpoint (pause/cancel) │              │ audit    │
 │ folder watch  │  │   approval if sensitive     │              └──────────┘
 │ (debounced)   │  │   Grant::check_path         │
 └───────────────┘  │   perform on resolved path  │ ──► Notifier
                    └─────────────────────────────┘
```

- **`workflow`**: the data model and static validation. It checks the id
  format, schedule syntax, minimum interval, glob syntax, template variables
  (including file variables used without a file trigger), path safety (absolute,
  no `..`), and that every path an action touches is covered by the declared
  permissions.
- **`permissions`**: `PermissionRequest` (declared) vs `Grant` (approved,
  resolved paths). `resolve_path` canonicalizes the longest existing ancestor,
  which follows symlinks. `check_path` returns the resolved path, and actions
  use that path, so there is no gap between check and use at the path level.
- **`engine`**: the lifecycle state machine, a dispatcher per active workflow,
  and the execution loop. Executions of one workflow are serialized. State is
  broadcast through a `watch` channel, so in-progress runs react to pause and
  cancel at action boundaries.
- **`triggers`**: interval (`tokio::time::interval`, missed ticks skipped),
  cron (`croner`, local time), and folder watch (`notify`). A file event fires
  only after the path has been quiet for the debounce window (default 500 ms),
  so half-written files aren't processed. A create followed by writes still
  counts as `created`.
- **`actions`**: a closed set of operations: create directory, copy, move,
  write/append, trash, notify. There is no exec action.
- **`history`**: SQLite (bundled, WAL). Runs left `running` after a crash are
  marked `failed` at startup.

## Permission flow

1. The workflow declares `permissions.read`, `permissions.write` and
   `permissions.notify`.
2. Validation rejects workflows whose actions touch paths outside that
   declaration.
3. The user approves. `Engine::grant_permissions` stores a `Grant` with
   resolved paths. The CLI asks interactively before doing this.
4. `activate` refuses if no grant exists or if the grant doesn't cover the
   current request, for example after a workflow is edited to ask for more.
5. Each run re-reads the grant and checks every concrete path again.
6. Sensitive actions (`trash_file`, `overwrite = true`, `write_file` replacing
   an existing file) also need either `allow_sensitive` on the grant or a
   `true` from the `Approver`. The default is `DenyAll`.

## Execution statuses

| Status | Meaning |
|---|---|
| `succeeded` | All actions completed |
| `failed` | An action errored; later actions did not run |
| `denied` | A permission check failed, or an approval was refused |
| `cancelled` | The workflow was cancelled mid-run |
| `skipped` | The trigger fired while the workflow was paused |

Firings filtered out by conditions are not recorded, so history doesn't fill
up with non-matching files.

## Where this goes next

The engine is a library on purpose. Phase 3 (natural-language builder) will
generate `Workflow` TOML and pass it through the same `validate` and grant
flow, so AI output gets no extra privileges. The planned daemon (`ambard`)
will host one `Engine` and expose it over D-Bus/varlink to the desktop
Automation Center and the CLI.
