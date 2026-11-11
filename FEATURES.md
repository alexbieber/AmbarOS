# Ambar OS features

Everything Ambar OS 0.2 includes, with how far each feature has been proven. The marks follow [`docs/STATUS.md`](docs/STATUS.md), which has the detail.

| Mark | Meaning |
|---|---|
| ✅ | seen working: a test or a real run in a virtual machine shows it |
| ⚠️ | built in, but not proven yet (needs real hardware, the internet or a real AI model) |
| 🧪 | experimental |

<p align="center"><a href="docs/media/ambar-os-tour.mp4"><img src="docs/media/ambar-os-tour.gif" width="640" alt="Ambar OS tour"></a><br><sub>▶ <a href="docs/media/ambar-os-tour.mp4">Watch the 50-second tour</a>. Every frame is a screenshot of the real system.</sub></p>

## 1. A complete operating system

| Feature | Status |
|---|---|
| Ubuntu 26.04 LTS base, Linux 7.0, security updates until 2031 | ✅ built from scratch by `os/ubuntu/build.sh` |
| KDE Plasma 6.6 on Wayland | ✅ live session and installed system start it |
| Live USB: try it without installing | ✅ boots to the full desktop in QEMU |
| UEFI with Secure Boot (Ubuntu's signed shim) and legacy BIOS | ✅ UEFI booted in QEMU · ⚠️ Secure Boot and BIOS not booted yet |
| Ambar boot menu, boot animation, login screen and installer branding | ✅ |
| Safe-graphics and "load into memory" boot options | ✅ in the boot menu · ⚠️ not booted yet |
| Installer: erase disk, install next to Windows, manual partitioning, disk encryption | ✅ full install with Erase disk (43/43 steps) · ⚠️ the other modes not run yet |
| The installed system boots on its own and logs in | ✅ login screen, password login, Ambar service · ⚠️ desktop not yet drawn under slow emulation, see STATUS |
| Identifies as "Ambar OS 0.2 (based on Ubuntu 26.04.1 LTS)", still compatible with Ubuntu software and PPAs | ✅ |
| System snapshots and restore (Timeshift), firewall, firmware updates, Driver Manager | ✅ installed · ⚠️ not exercised |

## 2. Designed like macOS

| Feature | What it does | Status |
|---|---|---|
| Menu bar | Ambar menu, the active app's name, the app's own menus, status icons, Control Center, clock | ✅ |
| Ambar menu | About This Computer, System Settings, Ambar Store, Automations, Force Quit, Sleep, Restart, Shut Down, Lock Screen, Log Out | ✅ |
| Global app menus | Files, Terminal and other apps show their menus in the menu bar | ✅ seen with Files |
| Control Center | Wi-Fi, Bluetooth, Airplane mode, Do Not Disturb, dark mode, Night Light, brightness and volume sliders, screenshot | ✅ reads real Wi-Fi, volume and brightness in the VM · ⚠️ Bluetooth and Wi-Fi switching need hardware |
| Dock | Floating, centred, with pinned apps, running indicators and Trash | ✅ |
| Launchpad | Full-screen grid of every app | ✅ |
| Spotlight-style search | Meta+Space or Alt+Space finds apps, files, settings and automations | ✅ |
| Mission Control | All windows at a glance (Meta+W or the top-left screen corner) | ✅ |
| Window buttons | Red, yellow and green traffic lights on the left | ✅ |
| Light and dark | One switch changes colours, panels, icons and window frames | ✅ |
| Touchpad gestures | Plasma's three- and four-finger gestures | ⚠️ needs a touchpad |

## 3. Apps out of the box

| Area | Apps | Status |
|---|---|---|
| Web | Firefox (release builds) or Falkon | ✅ Falkon here · ⚠️ Firefox only in CI builds |
| Files and system | Files (Dolphin), Terminal (Konsole), System Settings, System Monitor, Disk Usage, Find, Ark | ✅ in the image; 27 key apps are checked at every boot test |
| Office | LibreOffice Writer, Calc, Impress; Okular (PDF) | ✅ |
| Mail and calendar | KMail, KOrganizer, Kontact | ✅ |
| Media | Elisa (music), Haruna (video), Gwenview (photos), Spectacle (screenshots), Kamoso (camera), Skanpage (scanner) | ✅ |
| Tools | Kate, KCalc, KDE Connect (phone link), Colour Picker, Sweeper | ✅ |
| Input | fcitx5 for Indian and other non-Latin scripts | ✅ installed |

## 4. Install anything

| Source | How | Status |
|---|---|---|
| Ambar Store | Ubuntu apps, Flathub and Snap in one place, plus updates | ✅ opens · ⚠️ Flathub needs internet on first boot |
| `.deb` from any website | Double-click (Chrome, VS Code, Zoom, Steam, …) | ✅ opens in the Store |
| Flatpak | Flathub set up on first boot | ⚠️ needs internet |
| Snap | `snapd` included | ✅ installed |
| AppImage | Download, make executable, run | ✅ supported |
| Windows apps | Double-click a `.exe` or `.msi` to run it with Wine | ✅ file handler set · ⚠️ app compatibility varies |
| Other Linux distros | distrobox + podman containers | ✅ installed |
| Codecs and drivers | Media codecs; Driver Manager for NVIDIA and others | ✅ codecs installed · ⚠️ drivers need hardware |

## 5. Ambar automations (the extra layer)

| Feature | Status |
|---|---|
| Automations from short TOML files: schedule, folder-watch and manual triggers; conditions; actions to create folders, copy, move, write or trash files and send notifications | ✅ 131 Rust tests, ~30,000 property cases |
| Safe by design: can never run a program or command; limited to granted folders; trash and overwrite wait for your approval; everything in an audit log | ✅ attack and sabotage tests |
| `ambard` background service, starts at login, runs as you (never root) | ✅ running in the live and installed systems |
| Automation Center app, light and dark, with live approvals | ✅ |
| AI drafts automations from plain English (Claude, OpenAI-compatible, Ollama), with a risk review before anything is added | ✅ against mock APIs · ⚠️ never run against a real model |
| Agents: assistants with limited scopes that can propose but never approve | ✅ |
| Signed packages and a marketplace of starter automations | ✅ |
| First-run Welcome: appearance, optional AI, starter automations | ✅ |
| `ambar` CLI and Python SDK | ✅ |

## 6. For developers and testers

| Feature | Status |
|---|---|
| One-command ISO build: `sudo os/ubuntu/build.sh all` | ✅ |
| Automatic boot test in QEMU over the serial console, with screenshots | ✅ |
| CI builds and boots the ISO on every change (`.github/workflows/os-ubuntu.yml`) | ⚠️ written, runs once the repo is on GitHub |
| `ambar-hwreport` for hardware reports, with no serial numbers or addresses | ✅ |
| `AGENTS.md` and `.cursor/rules/` for AI editors | ✅ |
| Atomic edition: Fedora Kinoite + bootc, image updates and rollback, ARM64, Raspberry Pi | 🧪 recipe written, built only in CI |
