<p align="center">
  <img src="docs/images/hero.jpg" alt="Ambar OS: Designed for Humans. Built for AI. Open to Everyone." width="100%">
</p>

<p align="center">
  <a href="https://github.com/alexbieber/AmbarOS/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/alexbieber/AmbarOS/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/alexbieber/AmbarOS/actions/workflows/os-ubuntu.yml"><img alt="Ambar OS ISO" src="https://github.com/alexbieber/AmbarOS/actions/workflows/os-ubuntu.yml/badge.svg"></a>
  <a href="https://alexbieber.github.io/AmbarOS/"><img alt="User guide" src="https://img.shields.io/badge/user%20guide-alexbieber.github.io%2FAmbarOS-F2A43A"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-B35C05"></a>
  <img alt="Ubuntu 26.04 LTS · KDE Plasma 6" src="https://img.shields.io/badge/base-Ubuntu%2026.04%20LTS%20%C2%B7%20KDE%20Plasma%206-3A3129">
  <img alt="x86_64" src="https://img.shields.io/badge/arch-x86__64-3A3129">
</p>

This repository is the source of **Ambar OS**: an Ubuntu 26.04 LTS desktop with a KDE Plasma 6 shell, safe automations, and a security workbench. The user guide, screenshots, and install steps live on the documentation site, not on this page.

**[User guide →](https://alexbieber.github.io/AmbarOS/)** · [Download and USB](docs/live-usb.md) · [Install](docs/install.md) · [First steps](docs/first-steps.md) · [Features](FEATURES.md) · [Status](docs/STATUS.md)

## What booting proved

The ISO builds from this tree and boots in QEMU to the Ambar desktop. A full install finishes, and the installed system reaches its own login screen. It has not been tried on a physical PC yet. [Status](docs/STATUS.md) is the list of what is proven.

<table>
<tr>
<td width="50%"><img src="docs/images/os/booted/desktop.jpg" alt="Booted ISO desktop"><br><sub>The ISO booted in QEMU</sub></td>
<td width="50%"><img src="docs/images/os/booted/files-with-menus.jpg" alt="Booted ISO with Files"><br><sub>Files, with its menus in the menu bar</sub></td>
</tr>
<tr>
<td width="50%"><img src="docs/images/os/booted/installer-done.jpg" alt="Installer finished"><br><sub>A full install onto a blank 40 GB disk</sub></td>
<td width="50%"><img src="docs/images/os/booted/installed-login.jpg" alt="Installed system login"><br><sub>The installed system at its login screen</sub></td>
</tr>
</table>

## Build from source

The Ambar software runs on an existing Linux machine. Rust 1.89+ and Python 3.11+.

```sh
cargo build --release && export PATH="$PWD/target/release:$PATH"

ambar service start
ambar workflow add examples/organize-downloads.toml --enable
ambar workflow list
ambar history --steps

ambar ai config --provider ollama
ambar ai create "when a PDF lands in Downloads, move it to Documents/PDFs and tell me"

pip install ./sdk/python ./desktop/automation-center && ambar-center
```

The ISO is a separate build, on Ubuntu or Debian, with about 25 GB free:

```sh
sudo apt-get install debootstrap squashfs-tools xorriso grub-efi-amd64-bin grub-pc-bin mtools dosfstools
sudo os/ubuntu/build.sh all
# → output/ubuntu/ambar-os-0.2-amd64.iso
```

Recipe and tests: [OS image](docs/os-image.md). Publishing a build: [Publish on GitHub](docs/github-setup.md).

An automation is a TOML file. It can create, copy, move, write and trash files, and send notifications. It cannot run a program.

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

[Workflow format](docs/workflow-format.md) · [Service](docs/service.md) · [Developer guide](docs/developer-guide.md) · [Security tools](docs/security.md) · [Ambar's own tools](docs/ambar-tools.md)

<p align="center">
  <a href="docs/media/ambar-demo.mp4"><img src="docs/media/ambar-demo.gif" alt="Describing an automation, reviewing the AI draft and approving it" width="760"></a><br>
  <sub>Recording of the real Automation Center and service. The AI reply is replayed from a file, so the demo needs no API key.</sub>
</p>

## Security model

- Workflows, packages and AI output cannot run programs or shell commands.
- Nothing runs until the user approves the workflow's declared folders. Grants are re-read on every run.
- Every path is resolved again at run time, so a symlink or `..` cannot leave the grant.
- Trash and overwrite ask first. Trash can be undone.
- Grants, changes and refusals are written to an audit log.
- An agent, even with every scope, cannot grant, enable or approve.

[Architecture](docs/architecture.md) · [Security policy](SECURITY.md) · [Known gaps](docs/STATUS.md#known-gaps-by-design-or-deferred)

## Repository

| Area | Path |
|---|---|
| The OS | `os/ubuntu/` (build.sh, packages, hooks, branding, boot test) |
| macOS-style shell | `desktop/plasma/` |
| Rust crates | `crates/` (`ambar-automation`, `ambar-service`, `ambar-ai`, `ambar-pkg`, `ambar-cli`) |
| Desktop apps | `desktop/` (Automation Center, KRunner, Plasma theme, brand) |
| Atomic edition (experimental) | `os/` (Fedora Kinoite, bootc, ARM64, Raspberry Pi) |
| User guide | `docs/` + `mkdocs.yml`, published by [docs.yml](.github/workflows/docs.yml) |
| Demo and screenshots | `scripts/demo/` |

```sh
bash scripts/verify.sh
```

[ci.yml](.github/workflows/ci.yml) runs Rust on Linux and macOS, the MSRV check, `cargo audit`, and the Python, desktop, OS-recipe and docs tests. [os-ubuntu.yml](.github/workflows/os-ubuntu.yml) builds and boot-tests the Ubuntu ISO. [os-image.yml](.github/workflows/os-image.yml) builds the experimental atomic edition.

An editor such as Cursor reads [AGENTS.md](AGENTS.md) and `.cursor/rules/`. The publishing playbook is [CURSOR.md](CURSOR.md).

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md). A hardware report is the most useful small contribution: boot the ISO, run `ambar-hwreport`, and follow [Hardware](docs/hardware.md).

## License

[Apache-2.0](LICENSE). Ambar OS is built on Ubuntu and KDE Plasma, and is not affiliated with or endorsed by Canonical or KDE. The product vision is in the [master plan](docs/MASTER_PLAN.md).
