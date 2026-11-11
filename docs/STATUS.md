# Status

_Last updated: 2026-11-11_

## Summary

The full Ambar software stack is built and tested on Linux:
- automation engine and `ambard` background service
- live approvals
- AI builder
- agent runtime
- Automation Center with first-run Welcome
- KRunner search
- Plasma theme
- SDK, packages and marketplace

The bootable OS image is **written but has never been built or booted**. That needs a machine with Fedora registry access and KVM: the `os-image` CI job, which runs on GitHub as soon as the repo is pushed, or a Linux PC.

**What is proven and what isn't:**
- **Proven:** the software, by 163 automated tests and ~30,000 property-test cases.
- **Not proven:** that the OS boots on any machine. The CI boot test is written to prove it: login, service, marketplace, a real workflow run, Plasma, Welcome and KRunner, plus a desktop screenshot.

| Phase | What | Status |
|---|---|---|
| 1–2 | Automation engine, CLI | ✅ built and tested |
| 3 | `ambard` service, authenticated IPC, **live approvals** | ✅ built and tested |
| 4 | AI automation builder (Claude / OpenAI-compatible / Ollama) | ✅ tested against mock APIs · ⚠ never called a real model |
| 5 | Automation Center, **Welcome setup**, **KRunner search**, Plasma theme | ✅ app tested offscreen with screenshots, KRunner on a real D-Bus bus · ⚠ never loaded in a real Plasma session |
| 6 | Agent identities, scopes, proposals, memory, rate limits | ✅ built and tested |
| 7 | Python SDK, signed packages, marketplace, developer guide | ✅ built and tested |
| 8 | Bootable image (bootc, Fedora 45 Kinoite), installer ISO, signed updates, rollback | ⚠ written and statically checked · ❌ **not built or booted** |

## Test results (2026-11-11, Ubuntu 24.04 container)

| Suite | Tests |
|---|---|
| Rust: 7 crates, unit + integration + real-process + property tests | **131** (property tests ≈ 30,000 generated cases) |
| Python SDK against a real `ambard` | 4 |
| Automation Center: real `ambard`, offscreen QML, both themes, AI drafts, live approvals, Welcome, starter packs | 9 |
| KRunner runner: matching + real D-Bus session bus | 2 |
| Plasma theme, desktop entries, KRunner/autostart registration | 9 |
| OS recipe: COPY sources, configs, systemd unit, shellcheck, branding, boot smoke harness (pass and fail) | 8 |
| **Total** | **163**: 3 consecutive full runs plus 6 runs under double load, all green |

Also clean: `cargo fmt`, `cargo clippy -D warnings`, `cargo doc` (0 warnings), `cargo audit`, `shellcheck`, `desktop-file-validate`, `systemd-analyze verify`, WCAG AA contrast.

### Sabotage checks (deliberately broken code must fail a test)

| Sabotage | Caught by |
|---|---|
| Owner-token check bypassed | `ipc_requires_valid_token_and_hello_first` |
| Schedule ticks queued instead of skipped | `duplicate_execution_is_prevented` |
| Restore after restart skipped | `enabled_workflows_are_restored_after_restart` |
| AI description passed without `--` (flag injection) | `test_descriptions_are_never_parsed_as_flags` |
| Agent allowed an owner-only operation | `agents_never_reach_owner_operations` (property test over all scope sets) |

### Attacks the tests simulate

- **Workflows:** symlink escape from a granted folder; `..` traversal; unknown fields and actions (e.g. `run_shell`).
- **AI:** a model output that writes `~/.ssh/authorized_keys` is flagged HIGH and not saved non-interactively. A model that insists on shell commands never produces a draft. API keys never appear in errors.
- **Agents:** an agent with every scope still can't register, enable, grant, remove, shut down or manage agents. A revoked agent is cut off mid-connection. The rate limit trips at 120 requests per minute.
- **Packages:** tampered contents; a re-signature with a different key under the same publisher name; files smuggled past the manifest; an index hash mismatch; path traversal in manifests.

## Bugs found and fixed during this work

- **Property testing:** a model reply with `]` before `[` crashed `ambar ai suggest`.
- **Test races:** two timing tests could see a run that was dispatched just before a pause or disable. Found by a one-off failure (1 in ~15 runs); fixed by letting in-flight runs settle.

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
7. **Security review:** this is not a security audit. The parsers are property-tested, and CI signs images, but these are still needed:
   - coverage-guided fuzzing (cargo-fuzz)
   - an external review of the agent and package trust model
   - SELinux confinement for `ambard`
   - client-side enforcement of image signatures

## Known gaps (by design or deferred)

- **Packages:** packages can't add new action types. That's deliberate, to keep the closed action set. Extending actions needs sandboxing (WASM or Podman).
- **Missed runs:** runs missed while `ambard` was stopped are not caught up.
- **Path races:** a same-user attacker who can swap a folder for a symlink between the permission check and the file operation could race the check. Using `openat`-style resolution would close this.
- **Rebranding and recovery:** the Plymouth splash and installer still look like Fedora. There's no recovery partition beyond bootc rollback.

## Next steps, in order

1. Push the repo. Make the `ci` and `os-image` workflows green, and fix whatever the first real image build and boot turns up.
2. Run `scripts/verify.sh` on the Mac.
3. Boot the ISO on one real laptop. Fill in the hardware table in `docs/os-image.md`.
4. Try `ambar ai create` with a real API key or a local Ollama model, and tune the prompt.
5. Fuzz the parsers, add cosign signing for images, and write an SELinux module for `ambard`.
