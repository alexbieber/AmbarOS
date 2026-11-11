# AGENTS.md: working on Ambar OS

Instructions for AI coding agents (Cursor, Claude Code, Copilot and others) and for people new to the repo. Read this before changing anything. Topic-specific rules are in `.cursor/rules/`. To publish to GitHub, follow [`CURSOR.md`](CURSOR.md).

## What this project is

**Ambar OS** is a complete desktop operating system: **Ubuntu 26.04 LTS + KDE Plasma 6**, designed to look and work like macOS, with a built-in automation layer on top.

- The **OS** is a live/installer ISO built from scratch by `os/ubuntu/build.sh` (debootstrap → packages → Ambar layer → squashfs → hybrid UEFI/BIOS ISO).
- The **macOS-style shell** is Plasma configured and extended: the menu bar with the Ambar menu, app name and global menus, a Control Center, a floating Dock, Launchpad, traffic-light window buttons, and light/dark themes. It lives in `desktop/plasma/`.
- **Install anything:** the Ambar Store (Discover), `.deb`, Flatpak (Flathub), Snap, AppImage and Windows apps (Wine).
- The **automation layer** (an add-on, not the core) is written in Rust: the `ambard` service, the `ambar` CLI, signed packages, AI drafting of automations, and agents. It has a Qt/QML front end, the Automation Center.

## Repository map

| Path | What | Language |
|---|---|---|
| `os/ubuntu/` | **Main OS build**: `build.sh`, `packages.txt`, `exclude.txt`, `hooks/*.chroot`, `overlay/` (files copied into the image), `iso/grub.cfg`, `test-qemu.sh`, `smoke.py`, `desktop-shots.sh` | bash, Python |
| `os/` (rest) | Experimental **atomic edition** (Fedora Kinoite + bootc, ARM64, Raspberry Pi) and shared test helpers (`smoke_test.py`) | Containerfile, bash |
| `os/tests/` | Tests for both OS editions (static checks of the recipe, plus the boot harness) | pytest |
| `desktop/plasma/` | macOS-style shell: `generate.py` (layout + defaults), `aurorae.py` (window decorations), `plasmoids/` (Ambar menu, app name, Control Center), `control-center/ambar-control-center` (backend), colour schemes, wallpaper | Python, QML |
| `desktop/security/` | Security toolkit: `catalog.toml` (every tool: tier, category, source), `ambar-sec` CLI, `ambar-security` app (Ambar's kali-tweaks), Launchpad Security menu, `kali-default.txt` (Kali's default set, for the coverage test) | Python, TOML |
| `desktop/workbench/`, `desktop/guard/`, `desktop/utils/` | Ambar's own tools: Workbench (`ambar-engage` + app), Guard (`ambar-guard` + app), Clean and Share (CLI + app). Each folder has its own `tests/` | Python + PySide6 |
| `desktop/automation-center/` | Automation Center app, Welcome, KRunner plugin | Python + PySide6/QML |
| `desktop/share/` | `.desktop` files, autostart, icons | |
| `crates/` | Rust workspace: `ambar-automation` (engine), `ambar-service` (`ambard`), `ambar-cli` (`ambar`), `ambar-ai`, `ambar-pkg`, `ambar-recon` (scanner and the shared scope checker) | Rust |
| `sdk/python/` | Python SDK for talking to `ambard` | Python |
| `marketplace/` | Signed example packages and the index | TOML, JSON |
| `docs/` | User docs (MkDocs Material), `STATUS.md` (what is and isn't proven), images, media | Markdown |
| `scripts/` | `verify.sh` (runs everything CI runs), `demo/` (screenshots, videos, hero banner) | |
| `.github/workflows/` | `ci.yml`, `docs.yml`, `os-ubuntu.yml` (builds + boots the ISO), `os-image.yml` (atomic edition) | YAML |

Generated files that are **not** committed: `target/`, `output/` (ISOs, boot logs), `site/`.

## Commands

```sh
# Everything CI checks (format, clippy, Rust + Python tests, docs)
bash scripts/verify.sh

# Fast checks while editing
cargo test --workspace --locked
QT_QPA_PLATFORM=offscreen python3 -m pytest -q sdk/python/tests desktop os/tests tests
mkdocs build --strict

# The OS (Linux, root, ~25 GB free disk, network to the Ubuntu archive)
cargo build --release --locked --bin ambar --bin ambard
sudo os/ubuntu/build.sh all            # or: rootfs | ambar | iso
os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso                 # automatic boot test
os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso --interactive   # look at it yourself

# Re-generate the shell defaults and window decorations after editing their generators
python3 desktop/plasma/generate.py && python3 desktop/plasma/aurorae.py
```

After changing anything in `desktop/plasma/` or `os/ubuntu/hooks|overlay`, rebuilding is only `sudo os/ubuntu/build.sh ambar && sudo os/ubuntu/build.sh iso`. The base system is reused.

## Ground rules (from the project plan; reviews enforce them)

1. **Never claim something works unless a test or a real run shows it.** Update `docs/STATUS.md` honestly: ✅ only for what was run, ⚠ for what wasn't.
2. **No arbitrary execution.** Automations, packages and AI output can't run programs or shell commands. The action set is closed. Don't add a "run command" action or anything equivalent.
3. **Secure by default.** New access needs an explicit user grant and goes into the audit log. Never weaken signature checks, owner-token checks or path confinement.
4. **Tests and docs with every change.** A behaviour change comes with a test. A user-visible change also updates `docs/`.
5. **No secrets in the repo.** No API keys, tokens or passwords, not even for tests. Tests use mock servers.

## Facts that are easy to get wrong

- **Ubuntu 26.04's `sudo` is sudo-rs:** `sudo -E` is rejected and the environment is reset. Pass variables with `sudo /usr/bin/env VAR=value cmd`. See `/usr/bin/ambar-installer` in `hooks/40-installer.chroot`.
- **`firefox` and `thunderbird` in Ubuntu are snap stubs.** They're pinned out (`exclude.txt`). Firefox comes from Mozilla's APT repository when `BROWSER=firefox` (CI); otherwise the build uses Falkon.
- **The installer is Calamares with Kubuntu's settings.** Ambar overrides some modules in `os/ubuntu/overlay/etc/calamares/modules/`, because Kubuntu's versions broke on this image. Keep those overrides, and add a test for any new one.
- **The image's identity:** `/usr/lib/os-release` says `ID=ambar`, `ID_LIKE="ubuntu debian"`. `/etc/lsb-release` keeps `DISTRIB_ID=Ubuntu` on purpose, so PPAs and vendor `.deb` installers work. Hook 10 reads the diverted originals so it can be re-run.
- **Plasma 6 widgets:** `metadata.json` + `contents/ui/main.qml` with `PlasmoidItem`. Don't name QML properties `icon`, `state` or other names the base type already declares; that's a load error. The tests check this.
- **Aurorae decoration themes** are referenced as `__aurorae__svg__Ambar` / `__aurorae__svg__AmbarDark` with library `org.kde.kwin.aurorae.v2`.
- **QEMU without KVM:** use the CPU from `test-qemu.sh` (not `-cpu max`, which crashes Mesa's software renderer) and `GPU=VGA` if the screen stays black.
- **Security tools:** add or change tools only in `desktop/security/catalog.toml`. Prefer, in order: Ubuntu package (`apt`), `pipx`, `github` (only if the release publishes checksums), `kali` (container). A test fails if any tool in Kali's default set isn't covered or listed under `[excluded]` with a reason. Servers in the set must be listed under `[services] disable`. These tools are never callable from automations or the AI.
- **Scope:** every Ambar tool that touches other systems checks targets with `ambar-recon`'s scope code (`crates/ambar-recon/src/scope.rs`), including `ambar-engage`, which calls `ambar-recon scope`. Don't write a second scope checker, and fail closed if the check can't run. Without a scope file, only this computer may be scanned.
- **Editions:** `FLAVOR=security` builds Ambar OS Security (Default tier preinstalled). Use a separate `WORK` per flavour; `iso` refuses a rootfs of the other flavour.
- **Calamares runs 43 steps.** Its log is at `/root/.cache/calamares/session.log` in the live session.

## Style

- **Rust:** `cargo fmt`, `cargo clippy -D warnings`, no new `unsafe`, MSRV 1.89.
- **Python:** 3.11+, type hints, standard library first; no new dependencies in `ambar_sdk`.
- **Shell:** `set -euo pipefail`, shellcheck-clean.
- **Docs:** short sentences, plain words, no marketing claims that aren't backed by `STATUS.md`.
- **Commits:** imperative subject line; the body says what was tested.

## Before you finish a change

- [ ] `bash scripts/verify.sh` passes (or at least the suites for the parts you touched)
- [ ] new behaviour has a test; a security fix has a test that fails without it
- [ ] `docs/` and `docs/STATUS.md` updated if users would notice
- [ ] nothing in `output/`, `target/` or `site/` is staged, and no secrets
