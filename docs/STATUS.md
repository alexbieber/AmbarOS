# Status — Phases 1 & 2

_Last updated: 2026-11-11_

## Repository inspection (before work began)

The `AmbarOS` folder was empty: no code, dependencies or architecture existed.
Everything here is new.

## Done and tested

**Phase 1 — Foundation**
- Cargo workspace (edition 2024, MSRV 1.89), with shared dependency versions and lint policy (`unsafe_code = forbid`; clippy `unwrap_used`/`expect_used` warn)
- Two crates: `ambar-automation` (library) and `ambar-cli` (`ambar` binary)
- CI: fmt, clippy `-D warnings`, tests and docs on Linux + macOS; MSRV check; `cargo audit`
- Structured logging via `tracing` (`--json-logs`, `AMBAR_LOG=info`)
- Reproducible builds: `Cargo.lock` committed, CI uses `--locked`

**Phase 2 — Automation Engine**: all eight deliverables. The README maps each one to its test.

Verification run on 2026-11-11:
- `cargo test --workspace`: 31 passed (18 unit + 13 end-to-end); the end-to-end suite passed 10/10 repeated runs
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo fmt --check`: clean
- `cargo audit`: no advisories (142 crates)
- CLI smoke test: validate the three examples; trash refused without approval; trash succeeded when pre-approved (file landed in the recoverable trash); scheduled workflow `--once`; `history` and `audit` output

Not yet verified:
- CI has not run on GitHub yet; the repo hasn't been pushed.
- The MSRV job (Rust 1.89) wasn't run locally. 1.89 was taken from `rusqlite`'s declared `rust-version`.
- Tests ran on Linux only. The macOS folder-watch backend (FSEvents) is untested until CI runs on macOS.
- Desktop notifications (`notify-send` / `osascript`) were never shown on a real desktop. Tests use the in-memory notifier, and the CLI smoke test used `--notify log`.

## Known gaps and remaining Phase 2 work

1. **No daemon.** `ambar run` runs the engine in the foreground for a single workflow. `pause`/`resume`/`cancel` exist on the engine API and are tested, but a separate CLI process can't reach a running engine yet. Next step: `ambard` with a D-Bus or varlink interface, plus a systemd user unit.
2. **No restore on startup.** Stored workflows aren't re-activated when the engine restarts.
3. **Trash has no restore command.** Trashed files are named `<execution>-<name>`; the original path is only in the step message.
4. **Cancellation is cooperative.** It takes effect between actions. A long copy of a very large file finishes before the cancel is honoured.
5. **Loop risk.** A recursive folder watch that moves files into a subfolder of itself will retrigger itself. Validation doesn't detect this yet.
6. **macOS events.** FSEvents can report renames ambiguously; the watcher falls back to checking whether the path exists.

## Technical risks for the overall plan

| Risk | Impact | Mitigation |
|---|---|---|
| Scope: a full OS is years of work | Very high | Keep shipping vertical slices; the engine is useful on any Linux/macOS today, before Phase 7 |
| Phase 3 AI generating unsafe workflows | High | AI output must go through the same `validate` + grant + approval path; there is no exec action to target |
| KDE Plasma 6 customisation depth | Medium | Theme + Plasma widgets/QML first; avoid forking Plasma |
| bootc/Fedora image pipeline | Medium | Start from Fedora Kinoite/bootc base images; test in QEMU in CI |
| Single maintainer + interns | High | Strict CI, small crates, docs per module (this repo's pattern) |
| SELinux policy for `ambard` | Medium | Write a confined domain early, before the daemon gains capabilities |

## Suggested next milestones

1. `ambard` daemon + IPC + systemd user unit; `ambar pause/resume/cancel/list` talk to it
2. Restore active workflows on start; `ambar trash list/restore`
3. Phase 3 start: an LLM provider abstraction plus a "describe it → preview TOML → approve" flow, built on `Workflow::validate`
