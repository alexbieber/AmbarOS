# Ambar OS

**Designed for Humans. Built for AI. Open to Everyone.**

Ambar OS is an AI-native, Linux-based operating system: the simplicity of macOS,
the openness of Linux, and automation built in from the start. See
[`docs/MASTER_PLAN.md`](docs/MASTER_PLAN.md) for the full vision and phases.

## Status

**Phases 1–2 (Foundation + Automation Engine) are implemented and tested.**
Later phases (AI, agents, desktop, ecosystem, bootable image) have not started.
See [`docs/STATUS.md`](docs/STATUS.md) for exactly what works, what doesn't, and
the known risks.

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

31 automated tests (18 unit, 13 end-to-end). `cargo fmt`, `cargo clippy -D warnings`
and `cargo audit` are clean.

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
  ambar-cli/          `ambar` command-line tool
examples/             Sample workflows
docs/                 Architecture, workflow format, status, master plan
```

## Quick start

Requires Rust 1.89+.

```sh
cargo build --release
./target/release/ambar validate examples/organize-downloads.toml
./target/release/ambar run examples/organize-downloads.toml   # asks before granting access
./target/release/ambar history --steps
./target/release/ambar audit
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
