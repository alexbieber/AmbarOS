# Status

_Last updated: 2026-11-11_

Phase numbering follows the updated roadmap:

1. Phases 1–2: automation foundation
2. Phase 3: background service
3. Phase 4: AI automation builder
4. Phase 5: desktop
5. Phase 6: agents and security
6. Phase 7: developer ecosystem
7. Phase 8: bootable OS

| Phase | Status |
|---|---|
| 1–2 Automation foundation | Implemented and tested |
| 3 Background service (`ambard`) | Implemented and tested (see gaps below) |
| 4–8 | Not started |

## Test results (2026-11-11, Linux container)

| Suite | Tests | What it covers |
|---|---|---|
| `ambar-automation` unit | 18 | validation, permissions, symlink escape, actions, history |
| `ambar-automation` integration | 18 | the 8 Phase 2 capabilities, plus disable, overlap policy, graceful drain |
| `ambar-service` unit | 13 | private dirs, lock, token, protocol, unit files |
| `ambar-service` daemon | 13 | in-process daemon over a real socket (below) |
| `ambar-cli` service | 5 | real `ambar` and `ambard` binaries as separate processes, SIGKILL, SIGTERM |
| **Total** | **67** | the full suite passed 5/5 consecutive runs |

Checks:
- `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and `cargo doc` are clean.
- `cargo audit` is clean. New dependencies: `rustix` (process ids), `getrandom` (token). Both were already in the dependency tree.
- The generated systemd unit passes `systemd-analyze verify` with no warnings, and the plist is well-formed XML (`xmllint`).
- **Sabotage checks:**
  - Bypassing the token check fails `ipc_requires_valid_token_and_hello_first`.
  - Letting schedule ticks queue instead of skip fails `duplicate_execution_is_prevented`.
  - Skipping restore fails `enabled_workflows_are_restored_after_restart`.

### Phase 3 requirements → tests

| Requirement | Test(s) |
|---|---|
| Run in background without CLI open | `service_start_status_stop`, `workflows_survive_service_restart` |
| Local IPC | all `ambar-service` daemon tests, all CLI tests |
| start / stop / pause / resume / cancel / status | `workflow_control_over_ipc`, `workflows_survive_service_restart` |
| Persist definitions and state | `enabled_workflows_are_restored_after_restart` |
| Restore after restart | same; `failed_restores_are_reported_and_retried` |
| Graceful shutdown | `shutdown_lets_in_flight_runs_finish`, `sigterm_is_a_graceful_shutdown`, `shutdown_*` (engine) |
| Recovery after unexpected termination | `recovers_after_sigkill` (real SIGKILL), `crash_leftovers_are_recovered` |
| Duplicate execution prevention | `duplicate_execution_is_prevented`, `second_instance_is_refused`, `foreground_run_is_refused_while_service_owns_the_data_dir`, `overlapping_schedule_ticks_are_skipped_not_duplicated` |
| Audit every control operation | `workflow_control_over_ipc` (exact audit sequence incl. failures), `startup_health_and_graceful_shutdown` |
| Health checks | `startup_health_and_graceful_shutdown`, `failed_restores_are_reported_and_retried` |
| IPC authentication / authorization | `ipc_requires_valid_token_and_hello_first`, `peers_must_be_the_same_user`, file-mode assertions |
| Permission enforcement through the service | `runtime_permissions_still_enforced_through_the_service`, `sensitive_actions_are_refused_in_background_unless_preapproved` |
| Concurrent workflows | `concurrent_workflows_and_clients` (6 workflows, 20 clients × 20 requests) |

## Not verified — do not treat as done

- **Nothing has run on macOS yet.** Every result above is from Linux. This includes FSEvents folder watching, `getpeereid`-based peer credentials, `osascript` notifications and the 104-byte socket path limit. Run `cargo test --workspace` on your Mac first.
- **`ambar service install` / `uninstall` haven't been run** against a real launchd or systemd user session. The container has no user session manager.
- **The cross-user check is tested as a function only.** The peer-uid rule is unit-tested, but no connection from a second real user was attempted, because the container has only one user.
- **CI hasn't run on GitHub.**
- **This is not a security audit.** 67 passing tests don't establish production security. Still to do:
  - adversarial testing: fuzzing the protocol and workflow parser, races in path resolution
  - review of the dependency tree beyond `cargo audit`
  - platform-specific testing on macOS and Fedora

## Remaining work before Phase 4

1. Run the suite on macOS, then exercise `service install` on a Mac and on Fedora.
2. Fuzz `protocol::read_frame` / `Request` parsing and `Workflow::from_toml` (cargo-fuzz).
3. Approvals over IPC, so background sensitive actions can ask instead of needing `--allow-sensitive`.
4. Trash listing and restore (`ambar trash list/restore`).
5. Scoped client identities on the socket. These must exist before AI agents (Phase 6) are allowed to connect.
6. Optional: systemd `WatchdogSec` heartbeat.

## Technical risks

| Risk | Impact | Mitigation |
|---|---|---|
| macOS behaviour differs from Linux (FSEvents, launchd) | High until tested | Run the suite on the Mac now; macOS runner in CI |
| Same-user processes can drive the socket | Medium now, high once agents exist | Scoped tokens per client before Phase 6 |
| AI-generated workflows (Phase 4) | High | Must go through `validate` + consent + grant; there is no exec action |
| `ProtectSystem=strict` limits writes to home under systemd | Low | Documented; configurable |
| Scope: a full OS | Very high | Keep shipping working vertical slices |
