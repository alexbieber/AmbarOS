<p align="center">
  <img src="docs/images/hero.jpg" alt="Ambar OS: Designed for Humans. Built for AI. Open to Everyone." width="100%">
</p>

<p align="center">
  <a href="https://github.com/alexbieber/AmbarOS/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/alexbieber/AmbarOS/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/alexbieber/AmbarOS/actions/workflows/os-image.yml"><img alt="OS image" src="https://github.com/alexbieber/AmbarOS/actions/workflows/os-image.yml/badge.svg"></a>
  <a href="https://alexbieber.github.io/AmbarOS/"><img alt="Docs" src="https://img.shields.io/badge/docs-alexbieber.github.io%2FAmbarOS-F2A43A"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-B35C05"></a>
  <img alt="Fedora 45 · KDE Plasma 6" src="https://img.shields.io/badge/base-Fedora%2045%20%C2%B7%20KDE%20Plasma%206-3A3129">
  <img alt="x86_64 and ARM64" src="https://img.shields.io/badge/arch-x86__64%20%C2%B7%20arm64-3A3129">
</p>

<p align="center">
  <b><a href="https://alexbieber.github.io/AmbarOS/">Documentation</a></b> ·
  <a href="docs/live-usb.md">Try it from a USB stick</a> ·
  <a href="docs/install.md">Install</a> ·
  <a href="docs/first-steps.md">First steps</a> ·
  <a href="docs/hardware.md">Hardware</a> ·
  <a href="docs/STATUS.md">Status</a>
</p>

---

**Ambar OS** is a Linux desktop that does the repetitive work for you. It's built on Fedora's KDE Plasma, and Ambar adds three things:

- **An automation service** that files, sorts, writes and tidies in the background.
- **An app to manage your automations**, the Automation Center.
- **An optional AI assistant** that writes new automations from a sentence.

Nothing runs without your OK.

<p align="center">
  <a href="docs/media/ambar-demo.mp4"><img src="docs/media/ambar-demo.gif" alt="Describing an automation, reviewing the AI draft and approving it" width="760"></a><br>
  <sub>▶ <a href="docs/media/ambar-demo.mp4"><b>Watch the full 1-minute demo (MP4)</b></a>. This is a recording of the real Automation Center and service. The AI reply is replayed from a file, so it needs no API key.</sub>
</p>

> [!IMPORTANT]
> **Early project.** The Ambar software is built and covered by 186 automated tests. The bootable OS images (install image, live USB, Raspberry Pi) are written, but **have not been built or booted on real hardware yet**. [Status](docs/STATUS.md) says exactly what is and isn't proven.

## Why Ambar

<table>
<tr>
<td width="33%" valign="top">

### 🧡 Automations you can trust
An automation can only use the built-in actions:

- create folders
- copy, move, write or trash files
- send notifications

Each automation is limited to the folders it declares. It can **never run a program or command**. You approve its access first, and everything it does goes into an audit log.

</td>
<td width="33%" valign="top">

### ✨ AI that asks first
Describe what you want in plain words, using Claude, any OpenAI-compatible service, or a local Ollama model. The AI writes a **draft**. Before you approve it, you see:

- the automation
- the folders it needs
- a risk check

</td>
<td width="33%" valign="top">

### ↩️ Updates you can undo
The whole OS is one signed image. Updates install in the background and apply on restart. If one goes wrong, **pick the previous version** in the boot menu. Your files are never touched.

</td>
</tr>
</table>

## A look around

<table>
<tr>
<td width="50%"><img src="docs/images/screens/overview-light.png" alt="Overview, light"><br><sub><b>Overview</b>: what's running, plus a box to describe a new automation</sub></td>
<td width="50%"><img src="docs/images/screens/overview-dark.png" alt="Overview, dark"><br><sub><b>Light and dark</b>, matched to the whole Plasma desktop</sub></td>
</tr>
<tr>
<td><img src="docs/images/screens/draft-review-light.png" alt="AI draft review"><br><sub><b>AI draft review</b>: the automation, the access it needs and a risk level, before anything is added</sub></td>
<td><img src="docs/images/screens/approval-light.png" alt="Live approval"><br><sub><b>Live approval</b>: sensitive actions like trash wait for you, and are denied if you don't answer</sub></td>
</tr>
<tr>
<td><img src="docs/images/screens/proposals-light.png" alt="Agent proposals"><br><sub><b>Agents propose, you decide</b>: assistants have limited scopes and can't approve anything</sub></td>
<td><img src="docs/images/screens/automations-dark.png" alt="Automations"><br><sub><b>Automations</b>: run, pause, resume or disable, each with its last result</sub></td>
</tr>
<tr>
<td><img src="docs/images/screens/welcome-dark.png" alt="First-run Welcome"><br><sub><b>First-run Welcome</b>: service, appearance, optional AI and signed starter automations</sub></td>
<td><img src="docs/images/screens/installer.png" alt="Live USB installer"><br><sub><b>Live USB installer</b>: only offers safe disks, and you type the disk name to confirm (shown with example disks)</sub></td>
</tr>
</table>

<sub>All screenshots are of the real software, generated by <code>scripts/demo/screenshots.py</code> against a running <code>ambard</code>.</sub>

## What's inside

| | |
|---|---|
| 🧠 **Automation engine** | Triggers (manual, schedule, folder watch), conditions, a closed set of actions, history and an audit log |
| ⚙️ **`ambard` service** | Runs as you, never as root, and is reached through a private local socket with a token. It handles live approvals and restores your automations at login |
| ✨ **AI builder** | Claude, OpenAI-compatible or Ollama. Writes drafts with a risk review. A model that insists on running commands never gets a draft through |
| 🤖 **Agents** | Assistants with their own identity and limited scopes (read, propose, memory, run). They're rate-limited and can be revoked instantly |
| 📦 **Packages and marketplace** | ed25519-signed workflow bundles, an explicit publisher trust store, and a bundled marketplace |
| 🖥️ **Desktop** | Automation Center (Qt/QML), Alt+Space search in KRunner, first-run Welcome, and the Ambar Plasma theme and logo |
| 💿 **The OS** | Fedora 45 Kinoite + bootc, built for x86_64 and ARM64. Install image, live USB with installer, Raspberry Pi 4 image, signed multi-arch updates with rollback |
| 🔧 **Tools** | `ambar` CLI, Python SDK, `ambar-hwreport` for hardware reports |

```mermaid
flowchart LR
  subgraph You
    AC[Automation Center] --- KR[KRunner search] --- W[Welcome]
  end
  AC & KR & W -- owner token --> D((ambard))
  AG[Agents] -- scoped token --> D
  AI[AI builder] -. draft only .-> AC
  D --> E[Automation engine] --> A[(History + audit log)]
  E -->|permission-checked| F[Your files and notifications]
  subgraph OS[Ambar OS: Fedora 45 Kinoite, bootc image]
    D
  end
```

## Get it

| | |
|---|---|
| **Try it without installing** | Write the live ISO to a USB stick and boot it → [guide](docs/live-usb.md) |
| **Install** | From the live USB, the installer ISO, `bootc switch`, a VM or a Raspberry Pi → [guide](docs/install.md) |
| **Mac with Apple silicon** | Use the ARM64 image in UTM → [guide](docs/install.md#virtual-machine) |
| **Just the software** | On any Linux, or on macOS (untested), see [Quick start](#quick-start) |

Images are built by CI on every change (the **OS image** workflow). Until the first release, download them from the latest run's artifacts.

## Quick start

Run the Ambar software on an existing Linux machine. Requires Rust 1.89+ and Python 3.11+.

```sh
cargo build --release && export PATH="$PWD/target/release:$PATH"

ambar service start                                   # the background service
ambar workflow add examples/organize-downloads.toml --enable
ambar workflow list
ambar history --steps

ambar ai config --provider ollama                     # or anthropic / openai
ambar ai create "when a PDF lands in Downloads, move it to Documents/PDFs and tell me"

pip install ./sdk/python ./desktop/automation-center && ambar-center
```

An automation is a short TOML file:

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

More: [First steps](docs/first-steps.md) · [Workflow format](docs/workflow-format.md) · [Developer guide](docs/developer-guide.md) · [Service](docs/service.md)

## Security model

- **No execution.** Workflows, packages and AI output can't run programs or shell commands. The action set is closed.
- **Grants first.** Nothing runs until you approve the workflow's declared access. Grants are re-read on every run, so revoking one works immediately.
- **Paths re-checked.** Every path is resolved and checked at run time, so symlinks and `..` can't escape a grant.
- **Sensitive means asked.** Trash and overwrite need your approval. Trash can be undone; nothing is permanently deleted.
- **Everything audited.** Every grant, change and refusal is logged.
- **Scoped agents.** An agent with every scope still can't grant, enable or approve anything.

Details: [Architecture](docs/architecture.md) · [Security policy](SECURITY.md) · [Known gaps](docs/STATUS.md#known-gaps-by-design-or-deferred)

## Project status

| | |
|---|---|
| ✅ Built and tested | Engine, service, approvals, AI builder (against mock APIs), agents, packages, SDK, Automation Center, KRunner, theme, installer safety rules, hardware report, docs |
| ⚠️ Written, not yet run | Bootable images (x86_64, ARM64, live USB, Raspberry Pi), CI build-and-boot jobs |
| ❌ Not yet done | Real hardware testing, a real LLM in the loop, macOS run, SELinux policy, Plymouth/installer branding |

**186 automated tests** pass, plus about 30,000 property-test cases. [Full status →](docs/STATUS.md)

## Development

```sh
bash scripts/verify.sh            # everything CI checks, plus a live ambard smoke test
```

| Area | Path |
|---|---|
| Rust crates | `crates/` (`ambar-automation`, `ambar-service`, `ambar-ai`, `ambar-pkg`, `ambar-cli`) |
| Desktop | `desktop/` (Automation Center, KRunner, Plasma theme, brand) |
| OS image | `os/` (Containerfile, live USB, Raspberry Pi, QEMU smoke test) |
| Docs site | `docs/` + `mkdocs.yml` |
| Demo video and screenshots | `scripts/demo/` (`record_demo.py`, `screenshots.py`, `hero.py`) |

CI: [`ci.yml`](.github/workflows/ci.yml) covers Rust on Linux and macOS, the MSRV check, `cargo audit`, and the Python, desktop, OS-recipe and docs tests. [`os-image.yml`](.github/workflows/os-image.yml) builds and boots the images on x86_64 and ARM64. [`docs.yml`](.github/workflows/docs.yml) publishes the site.

## Contributing

Hardware reports, docs fixes and bug reports are as welcome as code. Start with [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md). If you have five minutes, boot the live USB, run `ambar-hwreport`, and [send a hardware report](docs/hardware.md#report-your-computer).

## License

[Apache-2.0](LICENSE). Ambar OS is built on Fedora and is not affiliated with or endorsed by the Fedora Project. The full vision is in the [master plan](docs/MASTER_PLAN.md).
