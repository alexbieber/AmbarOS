# Status

_Last updated: 2026-11-11_

## Summary

The full Ambar software stack is built and tested on Linux. The bootable OS image is **written but has never been built or booted**: that happens in the `os-image` CI job or on your machine. Until that passes, Ambar OS is a working, tested set of software that runs *on top of* Linux (and, untested, macOS), plus a ready-to-build image recipe. It is not yet a verified installable operating system.

| Phase | What | Status |
|---|---|---|
| 1–2 | Automation engine, CLI | ✅ built and tested |
| 3 | `ambard` background service, authenticated IPC | ✅ built and tested |
| 4 | AI automation builder (Claude / OpenAI-compatible / Ollama) | ✅ built and tested against mock APIs · ⚠ never called a real model |
| 5 | Automation Center (Qt/QML) + Ambar Plasma theme | ✅ app tested offscreen with screenshots · ⚠ theme never loaded in a real Plasma session |
| 6 | Agent identities, scopes, proposals, memory, rate limits | ✅ built and tested |
| 7 | Python SDK, signed packages, marketplace, developer guide | ✅ built and tested |
| 8 | Bootable image (bootc on Fedora 45 Kinoite), installer, updates | ⚠ written and statically checked · ❌ **not built or booted** |

## Test results (2026-11-11, Ubuntu 24.04 container)

| Suite | Tests |
|---|---|
| Rust (`cargo test --workspace`): 7 crates, unit + integration + real-process | **114** |
| Python SDK against a real `ambard` | 4 |
| Automation Center: real `ambard`, offscreen QML, both themes, AI draft flow | 7 |
| Plasma theme files: colour schemes, WCAG AA contrast, packages, desktop entry | 8 |
| OS recipe: COPY sources, configs, systemd unit, shellcheck, branding, smoke harness | 8 |
| **Total** | **141** (3/3 consecutive full runs passed) |

Also clean: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo doc` (0 warnings), `cargo audit` (no advisories), `shellcheck`, `desktop-file-validate`, `systemd-analyze verify`.

### Sabotage checks (deliberately broken code must fail a test)

| Sabotage | Caught by |
|---|---|
| Owner-token check bypassed | `ipc_requires_valid_token_and_hello_first` |
| Schedule ticks queued instead of skipped | `duplicate_execution_is_prevented` |
| Restore after restart skipped | `enabled_workflows_are_restored_after_restart` |
| AI description passed without `--` (flag injection) | `test_descriptions_are_never_parsed_as_flags` |

### Attacks the tests simulate

- **Workflows:** symlink escape from a granted folder; `..` traversal; unknown fields and actions (e.g. `run_shell`).
- **AI:** a model output that writes `~/.ssh/authorized_keys` is flagged HIGH and not saved non-interactively. A model that insists on shell commands never produces a draft. API keys never appear in errors.
- **Agents:** an agent with every scope still can't register, enable, grant, remove, shut down or manage agents. A revoked agent is cut off mid-connection. The rate limit trips at 120 requests per minute.
- **Packages:** tampered contents; a re-signature with a different key under the same publisher name; files smuggled past the manifest; an index hash mismatch; path traversal in manifests.

## Bugs found and fixed during this work

- **Duplicate runs:** re-registering a workflow could have let two runs overlap. The run lock is now kept across re-registration.
- **Agent revocation:** revoking an agent didn't cut off its already-open connections. The identity is now re-checked on every request.
- **Agent shutdown:** an agent's `shutdown` request could have stopped the service even though it was refused. Shutdown now requires a successful response.
- **Contrast:** the light-theme accent (`#D2700A`) failed WCAG AA for button text (3.46:1). Changed to `#B35C05` (4.72:1).
- **Flag injection:** the Automation Center passed the AI description so that it could be parsed as CLI flags. Fixed and tested.
- **Minimum Rust version:** the declared minimum (1.85) was wrong for `rusqlite`. It is now 1.89.

## Not verified — do not treat as done

1. **The OS image has never been built or booted.** This environment had no access to Fedora's registries and no KVM or QEMU. The first real build, disk image and boot are in `.github/workflows/os-image.yml`. Push to GitHub and check that job.
2. **macOS:** nothing has run on macOS (FSEvents, launchd, `osascript`, peer credentials). Run `bash scripts/verify.sh` on the Mac.
3. **Real LLMs:** the AI builder was never called against a real model. Only mock servers that copy each API's request and response shape. Prompt quality with real models is unmeasured.
4. **Real Plasma:** the Plasma look-and-feel, panel layout script, colour schemes and SDDM background were never loaded in a Plasma session. They are only syntax- and schema-checked.
5. **Login and boot:** `ambar service install` (launchd/systemd enable at login) was never run against a real session manager.
6. **Hardware:** no physical hardware has been tested.
7. **Security review:** this is not a security audit. Still needed:
   - fuzzing (protocol frames, workflow TOML, package JSON)
   - an external review of the agent authorization and package trust model
   - SELinux confinement for `ambard`
   - image signing

## Known gaps (by design or deferred)

- **Approvals:** a sensitive action running in the background can't ask you at that moment. It needs `--allow-sensitive` up front. Interactive approvals over IPC to the Automation Center are the next step.
- **Packages:** packages can't add new action types. That's deliberate, to keep the closed action set. Extending actions needs sandboxing (WASM or Podman).
- **Missed runs:** runs missed while `ambard` was stopped are not caught up.
- **Path races:** a same-user attacker who can swap a folder for a symlink between the permission check and the file operation could race the check. Using `openat`-style resolution would close this.
- **Undecided:** a global search or KRunner integration, and the AI assistant inside the Plasma shell itself, are not built.
- **Rebranding and recovery:** the Plymouth splash and installer still look like Fedora. There's no recovery partition beyond bootc rollback.

## Next steps, in order

1. Push the repo. Make the `ci` and `os-image` workflows green, and fix whatever the first real image build and boot turns up.
2. Run `scripts/verify.sh` on the Mac.
3. Boot the ISO on one real laptop. Fill in the hardware table in `docs/os-image.md`.
4. Try `ambar ai create` with a real API key or a local Ollama model, and tune the prompt.
5. Fuzz the parsers, add cosign signing for images, and write an SELinux module for `ambard`.
