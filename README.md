# Ambar OS

**Designed for Humans. Built for AI. Open to Everyone.**

Ambar OS is an AI-native, Linux-based operating system: the simplicity of macOS,
the openness of Linux, and automation built in from the start. See
[`docs/MASTER_PLAN.md`](docs/MASTER_PLAN.md) for the full vision and phases.

## Status

**Phases 1–3 are implemented and tested:** the automation engine and the
`ambard` background service. AI, desktop, agents, ecosystem and the bootable
image have not started. Everything has been tested on Linux only; macOS is
not yet verified. See [`docs/STATUS.md`](docs/STATUS.md) for exactly what works,
what doesn't, and the known risks.

| Capability (Phase 2 deliverable) | Status | Test |
|---|---|---|
| Create and validate a workflow | ✅ | `create_and_validate` |
| Execute a scheduled workflow (interval + cron) | ✅ | `scheduled_workflow_runs`, `cron_schedules_are_accepted` |
| Monitor a selected folder | ✅ | `folder_watch_files_matching_files` |
| Perform approved file operations | ✅ | `sensitive_actions_need_approval`, `existing_files_are_not_overwritten_without_approval` |
| Send a notification | ✅ | asserted in schedule + folder tests |
| Record execution history | ✅ | `history_is_persisted` |
| Pause and cancel workflows | ✅ | `pause_*`, `cancel_stops_an_in_progress_run` |
| Enforce permissions | ✅ | `activation_requires_grant`, `runtime_scope_is_enforced`, `revoked_grant_denies_runs` |

Phase 3 (`ambard` background service): background runs, IPC with auth, restore
after restart, crash recovery, duplicate-run prevention, audit and health checks.
See [`docs/service.md`](docs/service.md).

67 automated tests (unit, in-process daemon, and real-process tests including
SIGKILL recovery). `cargo fmt`, `cargo clippy -D warnings` and `cargo audit` are clean.

## Repository layout

```
crates/
  ambar-automation/   Automation engine library (Rust)
    src/workflow.rs     Workflow definitions (TOML) + validation
    src/permissions.rs  Permission requests, grants, path scoping
    src/engine.rs       Lifecycle, dispatch, pause/cancel, approvals
    src/triggers.rs     Interval/cron schedules, folder watching
    src/actions.rs      Permission-checked file operations + notify
    src/history.rs      SQLite: workflows, grants, executions, audit log
    src/notify.rs       Notification backends
    tests/engine.rs     End-to-end tests of every Phase 2 capability
  ambar-service/      ambard: daemon, IPC protocol + auth, client, launchd/systemd units
    tests/daemon.rs     In-process daemon tests over a real socket
  ambar-cli/          `ambar` CLI and `ambard` binary
    tests/service.rs    Real-process tests (start/stop, restart, SIGKILL, SIGTERM)
examples/             Sample workflows
docs/                 Architecture, workflow format, status, master plan
```

## Quick start

Requires Rust 1.89+.

```sh
cargo build --release
export PATH="$PWD/target/release:$PATH"

ambar validate examples/organize-downloads.toml

# Background service
ambar service start
ambar workflow add examples/organize-downloads.toml --enable   # asks before granting access
ambar workflow list
ambar service status
ambar history --steps
ambar audit
ambar service stop

# Start at login, so enabled workflows survive a reboot (launchd / systemd --user)
ambar service install

# Or run one workflow in the foreground, without the service
ambar run examples/daily-log.toml --once
```

A workflow looks like this:

```toml
id = "organize-downloads"
name = "Organize downloads"

[trigger]
type = "folder_watch"
path = "~/Downloads"

[[conditions]]
type = "file_extension"
any_of = ["pdf", "png", "jpg"]

[[actions]]
type = "move_file"
to_dir = "~/Documents/Ambar Inbox/{file_ext}/{date}"

[[actions]]
type = "notify"
title = "Filed {file_name}"

[permissions]
write = ["~/Downloads", "~/Documents/Ambar Inbox"]
notify = true
```

Full reference: [`docs/workflow-format.md`](docs/workflow-format.md).

## Security model (short version)

- Workflows **cannot run programs or shell commands**. The action set is closed.
- Nothing runs until the user **grants** the workflow's declared permissions.
- Every path is **resolved and re-checked at run time**, so symlinks or `..` can't escape the grant.
- **Trash and overwrite need explicit approval**. Trash is recoverable; nothing is unlinked.
- Grants are **re-read on every run**, so revoking one takes effect immediately.
- Every grant, state change and denial is written to an **audit log**.

Details: [`docs/architecture.md`](docs/architecture.md).

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
```

CI (`.github/workflows/ci.yml`) runs these on Linux and macOS, plus an MSRV check.

## License

Apache-2.0 (declared in `Cargo.toml`; add the `LICENSE` file before publishing).
