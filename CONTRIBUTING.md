# Contributing to Ambar OS

Thanks for helping. Hardware reports, docs fixes and bug reports are as welcome as code.

## Ground rules

These come from the project plan, and reviews hold every change to them:

1. **Never claim a feature works unless a test shows it.** Docs and PR descriptions say what was tested and what wasn't.
2. **No arbitrary execution.** Automations, packages and AI output can't run programs or shell commands. The action set is closed. Don't add a way around that.
3. **Secure by default.** New access needs an explicit user grant and goes into the audit log.
4. **Tests and docs with every change.** User-visible behaviour gets a docs update in `docs/`.

## Set up

Using an AI editor (Cursor, Claude Code)? It picks up [`AGENTS.md`](AGENTS.md) and `.cursor/rules/` automatically. People should read `AGENTS.md` too: it's the quickest map of the repo.


You need Linux or macOS, Rust 1.89+ and Python 3.11+.

```sh
cargo build
pip install -e ./sdk/python -e ./desktop/automation-center "PySide6-Essentials>=6.6" pytest
bash scripts/verify.sh      # everything CI checks, plus a live ambard smoke test
```

| Part | Where | Tests |
|---|---|---|
| Automation engine | `crates/ambar-automation` | `cargo test -p ambar-automation` |
| Service, IPC, agents, approvals | `crates/ambar-service` | `cargo test -p ambar-service` |
| AI builder | `crates/ambar-ai` | `cargo test -p ambar-ai` (mock servers, no API key needed) |
| Packages | `crates/ambar-pkg` | `cargo test -p ambar-pkg` |
| Automation Center, KRunner, installer, theme | `desktop/`, `os/live/installer` | `QT_QPA_PLATFORM=offscreen python3 -m pytest desktop os/tests` |
| Ambar OS ISO (Ubuntu edition) | `os/ubuntu/` | `python3 -m pytest os/tests`; real build and boot: `sudo os/ubuntu/build.sh all && os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso` |
| macOS-style shell | `desktop/plasma/` | `python3 -m pytest desktop/plasma/tests os/tests/test_ubuntu.py` |
| Atomic edition (experimental) | `os/` | `python3 -m pytest os/tests`; `os/build.sh qcow2 && os/test-qemu.sh` |
| Docs site | `docs/`, `mkdocs.yml` | `mkdocs build --strict` |

## Making a change

1. For anything bigger than a small fix, open an issue or discussion first, so we agree on the approach before you spend time on it.
2. Branch from `main`. Keep each PR to one logical change.
3. Add or update tests. For security-relevant code, add a test that fails when the protection is removed.
4. Run `bash scripts/verify.sh`. It must pass.
5. Fill in the PR template, including **what you tested and what you didn't**.

Style: `cargo fmt`, `cargo clippy -D warnings`, no new `unsafe`. Python: standard library first, type hints, no new dependencies in `ambar_sdk`.

## Hardware reports

Run `ambar-hwreport` on the machine and open a **Hardware report** issue. Once confirmed, the machine gets a row in [`docs/hardware.md`](docs/hardware.md). You're welcome to send that row as a PR yourself.

## Licence

By contributing you agree that your contribution is licensed under the [Apache License 2.0](LICENSE), like the rest of the project.
