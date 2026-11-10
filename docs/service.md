# ambard: the background service

`ambard` runs Ambar automations without a terminal open. It owns one
automation engine per data directory, restores enabled workflows when it
starts, and is controlled by the `ambar` CLI over a local socket.

```
 ambar CLI ──┐                                  ┌── launchd (macOS) / systemd --user (Linux)
             │  Unix socket, JSON lines          │      starts at login, restarts on crash
             ▼                                   ▼
        ┌────────────────────────────────────────────┐
        │ ambard                                     │
        │  auth: socket 0600 in 0700 dir · peer uid  │
        │        · per-start token                   │
        │  dispatch → audit every control op         │
        │  restore enabled workflows at start        │
        │  graceful shutdown (SIGTERM / IPC)         │
        │ ┌────────────────────────────────────────┐ │
        │ │ ambar-automation Engine (unchanged API,│ │
        │ │ + disable / overlap policy / drain)    │ │
        │ └────────────────────────────────────────┘ │
        └───────────────┬────────────────────────────┘
                        ▼
              <data>/ambar.db (SQLite, WAL)
              workflows · grants · executions · steps · audit
```

The engine stays platform-independent. Everything Unix-specific (socket,
peer credentials, unit files) lives in `crates/ambar-service`.

## Commands

| Command | What it does |
|---|---|
| `ambar service start` | Starts ambard: through launchd/systemd if installed, otherwise as a detached process logging to `<data>/logs/ambard.log` |
| `ambar service stop` | Graceful stop: triggers stop, runs in progress get up to 30 s to finish |
| `ambar service status [--json]` | Health report. Exit 0 = ok, 1 = degraded, 3 = not running |
| `ambar service install` / `uninstall` | Writes/removes `~/Library/LaunchAgents/os.ambar.ambard.plist` or `~/.config/systemd/user/ambard.service` and loads it, so ambard starts at login |
| `ambar workflow add <file> [--enable] [--allow-sensitive] [--replace] [--yes]` | Shows the permissions, asks for consent, registers and grants |
| `ambar workflow list [--json]` | State, whether running, last run |
| `ambar workflow enable / disable <id>` | Start / stop the trigger. Enabled workflows are restored after restart |
| `ambar workflow pause / resume <id>` | Skip new firings and hold a run in progress before its next step |
| `ambar workflow cancel <id>` | Stop the trigger and abort a run in progress at its next step |
| `ambar workflow run <id>` | Run a manual or scheduled workflow now |

`ambar history` and `ambar audit` read the database directly and work
whether or not the service is running.

### disable vs cancel vs pause

| | New firings | Run in progress | Restored after restart |
|---|---|---|---|
| **pause** | recorded as `skipped` | waits before its next action | yes, as paused |
| **disable** | none | finishes | no |
| **cancel** | none | stops at its next action (`cancelled`) | no |

## Lifecycle

**Start**
1. Create `<data>` and `<data>/run` with mode 0700, and refuse if either is owned by another user.
2. Take an exclusive lock on `run/ambard.lock`. If another ambard holds it, exit with code 2.
3. Remove any stale socket or token. Holding the lock proves they're left over.
4. Open the database. Runs still marked `running`, left by a crash, are closed as `failed (interrupted)`.
5. Write a fresh 256-bit token to `run/ambard.token` with mode 0600.
6. Restore: re-register every stored workflow and re-enable those saved as `active` or `paused`. A workflow that can't be restored (for example, its watched folder is on an unplugged drive) is reported in `status` as *degraded*. Its saved state is kept so the next start retries it.
7. Listen on `run/ambard.sock` with mode 0600, and audit `service.started`.

**Stop** (SIGTERM, SIGINT or `ambar service stop`)
1. Stop accepting connections and remove the socket and token.
2. Stop all triggers and refuse new runs.
3. Wait up to `--grace-secs` (default 30) for runs in progress. After that, cancel them at their next action boundary.
4. Audit `service.stopped`, release the lock and exit 0.

Workflow states are not changed by a stop, so they come back on the next start.

**Crash** (SIGKILL, power loss): the OS releases the lock. The next start removes the stale socket, closes interrupted runs as failed and restores workflows. Tested with a real `SIGKILL`.

## Duplicate execution prevention

- **Across processes:** the instance lock allows only one engine per data directory. `ambar run` (foreground) takes the same lock, so it refuses while ambard is running.
- **Within a workflow:** at most one run at a time.
  - A schedule tick that arrives during a run is recorded as `skipped` ("previous run still in progress"), so it neither queues up nor overlaps.
  - A manual `run` during a run is refused ("already running").
  - File events queue, so each file is still handled once.
- **Across re-registration:** replacing a workflow keeps its run lock, so a finishing run of the old definition can't overlap with the new one.
- **Missed schedules:** ticks missed while ambard was stopped are not caught up. Interval schedules restart counting at service start, and cron schedules resume at their next occurrence.

## IPC security

**Transport:** `run/ambard.sock`, newline-delimited JSON, at most 1 MiB per frame. See `crates/ambar-service/src/protocol.rs`. Unknown fields and unknown operations are rejected.

**Authentication.** All three checks must pass:
1. **Filesystem:** the socket is 0600 inside a 0700 directory owned by the service user.
2. **Peer credentials:** the kernel-reported uid of the connecting process must equal ambard's uid. Root is not exempt.
3. **Token:** the first request must be `hello` with the current token, sent within 5 s. Tokens are regenerated at every start and compared in constant time.

**Authorization:** an authenticated client is the service's own user and may use every operation. Read-only and agent-scoped roles are planned for the agent runtime phase.

**Audit:** every control operation (`register`, `enable`, `disable`, `pause`, `resume`, `cancel`, `run_now`, `revoke`, `shutdown`) writes `ipc.<op>` with the client name, peer pid and outcome. This includes failures. Rejected connections are logged as `ipc.denied` with the peer pid, uid and reason. Service start and stop are logged as `service.*`.

**Consent:** a grant is only sent when the user approved it at the CLI prompt or passed `--yes`. The service can't tell whether a human did that; see "Known limitations".

**Sensitive actions:** trash and overwrite are refused in the background (`DenyAll`) unless the workflow was added with `--allow-sensitive`.

## Unit files

Both units run as the logged-in user, never root.

**launchd (macOS):**
- `RunAtLoad` starts ambard at login.
- `KeepAlive { SuccessfulExit = false }` restarts it after a crash, but a graceful `service stop` (exit 0) stays stopped.
- Logs go to `<data>/logs/ambard.log`.

**systemd (Linux, user unit):**
- `Restart=on-failure` and `WantedBy=default.target`.
- Hardening: `NoNewPrivileges`, `PrivateTmp`, `ProtectSystem=strict`, `ReadWritePaths=%h` and `UMask=0077`.
- With `ProtectSystem=strict`, workflows can only write inside your home folder when run under systemd. That fits the current use cases; widen `ReadWritePaths` if needed.

CI checks both files: `systemd-analyze verify` on Linux, where any warning fails the build, and `plutil -lint` on macOS.

## Known limitations

- **Untested service managers:** `service install` / `uninstall` (`launchctl bootstrap`, `systemctl --user enable`) haven't been run on a real Mac or a Linux desktop with a user session. The unit files are validated, but loading them isn't. Without `install`, ambard runs only until logout or reboot.
- **No in-service approvals:** a sensitive action can't ask you at run time. It needs `--allow-sensitive` up front. A desktop approval prompt over IPC is planned with the Automation Center.
- **Consent is by convention:** any process running as you can connect and grant permissions. That's the standard Unix trust boundary, but it means an AI agent running as you could too. Per-client identities and scoped tokens belong to the agent runtime phase and must exist before agents get socket access.
- **No systemd watchdog yet:** health is available on demand (`service status`), but there's no push heartbeat.
- **Unix only:** Linux and macOS. No Windows support.
