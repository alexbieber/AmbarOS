# Status

_Last updated: 2026-11-11_

## Summary

**Ambar OS is a complete desktop operating system**, built and booted in this project:

- **Base:** Ubuntu 26.04 LTS, Linux 7.0 and KDE Plasma 6.6 on Wayland, with a macOS-style shell.
- **Apps:** the full everyday set, plus every common way to install more.
- **Installer:** the Ambar installer.
- **Ambar automation layer** on top.

**Proven in a virtual machine (QEMU, UEFI, no 3D acceleration):**

- The ISO (`ambar-os-0.2-amd64.iso`, 3.6 GB) is built from scratch by `os/ubuntu/build.sh`.
- It boots through GRUB and the Ambar boot menu, and the live user is logged straight into a **Plasma Wayland session** with the Ambar desktop:
    - the menu bar with the Ambar menu, the app name, app menus and Control Center
    - the Dock with Launchpad and Trash
    - the Ambar wallpaper and theme
- Search (Alt+Space) finds apps, and opening Files puts its menus in the menu bar, as on macOS.
- `ambard` runs as a systemd user service. The Welcome shows "Service running".
- The Control Center reads the real Wi-Fi, volume and display brightness.
- All 27 checked apps are installed. These include the browser, Files, Terminal, LibreOffice, mail and calendar, media apps, System Settings, the Ambar Store, Flatpak, Snap, Wine and Timeshift.
- **The installer runs**, with Ambar branding throughout. It took a 40 GB disk through Location, Keyboard, Erase disk (GPT, EFI + ext4) and a user account, then installed. **The full install completes:** all 43 installer steps, ending at *All done. Ambar OS 0.2 has been installed on your computer.*
- **The installed system starts on its own** (no ISO): UEFI boot, the Ambar login screen with the user created in the installer, password login, the Ambar service and the Welcome. `/etc/os-release` and the console say *Ambar OS 0.2 (based on Ubuntu 26.04.1 LTS)*.
- ⚠ **Open problem:** on the installed system under emulation (2 CPUs, software rendering), the wallpaper, menu bar and Dock were not drawn within 17 minutes. Plasma has created them (both panels and their widgets are loaded), but they never appear on screen, while the live session draws them in about 8 minutes. Next step: repeat on KVM (the CI workflow) or real hardware to tell a slow-emulation problem from a real bug.

**Found by booting it, and fixed in the recipe:**

- **Black screen in the VM:** QEMU's emulated `-cpu max` crashed Mesa's software renderer. The test now emulates a plain x86-64-v2 CPU.
- **Installer wouldn't open:** `sudo` dropped the display variables. `ambar-installer` now passes them through, and the X11 fallback library is installed.
- **Search timed out on slow machines:** KRunner now gets a longer start-up timeout.
- **GRUB's themed menu** was missing its modules and font.
- **Kubuntu names** remained in the installer: partition labels, donate link and OEM ID.
- **Installer steps from Kubuntu that broke on Ambar:** the bootloader step copied a file we don't ship, the mirror step deleted one, and the package step failed on a Kubuntu package that isn't installed. All three are fixed with Ambar versions of those steps.
- **Live USB:** the Welcome opened over the installer, and Launchpad showed a second, broken installer icon.
- **File handlers:** `.exe` files had no handler, and `.deb` files now open in the Ambar Store first.

**Not proven yet:**

- **Real hardware:** no physical PC has run it.
- **Firefox:** builds here use Falkon. CI adds Firefox from Mozilla's repository.
- **Flathub:** it's added on first boot with internet, and wasn't reachable from this workspace.
- **A real AI model:** none has been called yet.
- **macOS:** the Ambar CLI hasn't been run on macOS.

| Area | Status |
|---|---|
| Automation engine, `ambard`, approvals, agents, SDK, packages | ✅ built and tested (131 Rust tests, ~30,000 property cases) |
| AI automation builder | ✅ tested against mock APIs · ⚠ never called a real model |
| **Ambar OS ISO** (Ubuntu 26.04 + Plasma 6) | ✅ built here · ✅ boots to the Ambar desktop in QEMU (Wayland) · ⚠ not on real hardware |
| **macOS-style shell:** menu bar, Ambar menu, app name, global menus, Control Center, Dock, Launchpad, search, Mission Control, traffic-light windows, light/dark | ✅ seen working in the built desktop and the booted ISO · ⚠ gestures untested (no touchpad) |
| Apps and install-anything (Store, `.deb`, Flatpak, Snap, AppImage, Wine) | ✅ installed and launch · ⚠ Flathub needs internet on first boot |
| Installer (Calamares, Ambar branding) | ✅ full install in QEMU (Erase disk, 43/43 steps) · ✅ installed system boots to login and logs in · ⚠ installed desktop not drawn under emulation, under investigation |
| Atomic edition (Fedora Kinoite, bootc, ARM64, Raspberry Pi) | ⚠ experimental: recipe written, built only in CI |
| Docs site, community files | ✅ strict build; documented commands checked against the real CLI |

## Test results (2026-11-11, Ubuntu 24.04 container)

| Suite | Tests |
|---|---|
| Rust: 7 crates, unit + integration + real-process + property tests | **131** (property tests ≈ 30,000 generated cases) |
| Python SDK against a real `ambard` | 4 |
| Automation Center: real `ambard`, offscreen QML, both themes, AI drafts, live approvals, Welcome, starter packs | 9 |
| KRunner runner: matching + real D-Bus session bus | 2 |
| Plasma theme, desktop entries, KRunner/autostart registration, brand logo assets | 10 |
| OS recipe: COPY sources, configs, systemd unit, shellcheck, branding, boot smoke harness (pass and fail) | 8 |
| Live USB, installer, ARM64/Pi, hardware report (atomic edition) | 16 |
| Ambar OS (Ubuntu): package lists, build hooks, identity, installer steps, boot harness, desktop shell layout and decorations | 22 |
| Control Center backend with stand-in system tools | 5 |
| Docs: strict site build, nav completeness, documented commands/flags exist, community files, README links and media | 6 |
| **Total** | **213** (131 Rust + 82 Python). 14 full Python runs: 13 green, 1 run had one failure that took 25 s instead of 10 s and didn't reproduce in 11 retries, so a timing-sensitive test is suspected |

Also clean: `cargo fmt`, `cargo clippy -D warnings`, `cargo doc` (0 warnings), `cargo audit`, `shellcheck`, `desktop-file-validate`, `systemd-analyze verify`, WCAG AA contrast.

### Sabotage checks (deliberately broken code must fail a test)

| Sabotage | Caught by |
|---|---|
| Owner-token check bypassed | `ipc_requires_valid_token_and_hello_first` |
| Schedule ticks queued instead of skipped | `duplicate_execution_is_prevented` |
| Restore after restart skipped | `enabled_workflows_are_restored_after_restart` |
| AI description passed without `--` (flag injection) | `test_descriptions_are_never_parsed_as_flags` |
| Agent allowed an owner-only operation | `agents_never_reach_owner_operations` (property test over all scope sets) |
| Installer allowed to offer the live USB itself | `test_installer_only_offers_safe_disks` and two more |
| A made-up `ambar` command or installer flag in the docs | `test_documented_ambar_commands_exist`, `test_documented_tool_flags_exist` |

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

1. **The OS image on real hardware.** The Ambar OS ISO is built and boot-tested only in QEMU under software emulation (no KVM here). Still open:
   - the **installed desktop** isn't drawn under emulation (see the summary above); it needs a run on KVM or real hardware
   - Secure Boot with a real firmware, Wi-Fi, sleep, and NVIDIA graphics
   - the experimental atomic edition (Fedora Kinoite) is still built only in CI
2. **macOS:** nothing has run on macOS (FSEvents, launchd, `osascript`, peer credentials). Run `bash scripts/verify.sh` on the Mac.
3. **Real LLMs:** the AI builder was never called against a real model. Only mock servers that copy each API's request and response shape. Prompt quality with real models is unmeasured.
4. **Plasma on the installed system:** the look-and-feel, layout, colour schemes, SDDM theme and Control Center run in the live session, and the login screen and Welcome run on the installed system, but the installed desktop's panels weren't drawn under emulation.
5. **Login and boot:** on Linux, `ambard` starts at login from the global systemd user unit, both in the live session and on the installed system. On macOS, `ambar service install` (launchd) was never run.
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
- **Live USB install needs the network:** the installer pulls the image from GHCR, and the GHCR package must be made public. For offline installs, use the installer ISO.
- **Live session isn't persistent:** nothing is saved between live boots.
- **Raspberry Pi device trees:** they're copied when the image is built and not refreshed by kernel updates. Pi 5 isn't supported.

## Next steps, in order

1. Push the repo. Make the `ci`, `docs` and `os-image` workflows green, and fix whatever the first real image build and boot turns up. Turn on GitHub Pages (source: GitHub Actions) and Discussions, and make the `ambar-os` package public.
2. Run `scripts/verify.sh` on the Mac.
3. Write the live ISO to a USB stick and boot one real laptop. Run `ambar-hwreport` and add the first row to `docs/hardware.md`. Then install from the live session.
4. Try `ambar ai create` with a real API key or a local Ollama model, and tune the prompt.
5. Fuzz the parsers, add cosign signing for images, and write an SELinux module for `ambard`.
