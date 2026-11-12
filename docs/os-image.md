# OS images

There are two editions:

| Edition | Base | Status |
|---|---|---|
| **Ambar OS** (main) | Ubuntu 26.04 LTS + KDE Plasma 6, macOS-style desktop, live/installer ISO | built and booted in QEMU here, and in CI |
| Atomic edition (experimental) | Fedora 45 Kinoite + bootc, image updates with rollback, ARM64, Raspberry Pi | recipe written, built only in CI |

## Ambar OS ISO (main edition)

```
os/ubuntu/
  build.sh            debootstrap → packages → Ambar layer → squashfs → UEFI+BIOS ISO
  packages.txt        Kubuntu's full desktop + Ambar's additions (one per line)
  exclude.txt         kept out: Snap-only shims (firefox/thunderbird), games, KDE's welcome app
  hooks/*.chroot      run inside the image: identity, desktop defaults, apps, boot animation, installer, initramfs
  overlay/            files copied into the image: Calamares branding, Plymouth theme
  iso/                GRUB menu and background
  test-qemu.sh        boots the ISO in QEMU (UEFI) and runs smoke.py over the serial console
  smoke.py            checks: identity, Plasma, menu bar widgets, Dock, Launchpad, apps, store, installer, ambard
  desktop-shots.sh    runs the built desktop on a virtual screen to look at it and take screenshots
```

### What's in it

- **Desktop:** KDE Plasma 6.6 laid out like macOS:
    - a menu bar with the Ambar menu, the app's name and its menus, status icons, Control Center and the clock
    - a floating Dock with Launchpad and Trash
    - red, yellow and green window buttons on the left
    - Spotlight-style search on Meta+Space, and Mission Control (Overview)
    - light and dark themes covering colours, Papirus icons, the Inter font, wallpaper, login screen and boot animation
    - crisp text: greyscale antialiasing with slight hinting, like macOS, set for fontconfig, KDE and GTK alike; GTK apps use Breeze, Inter and left-hand window buttons so they match
- **Apps (all of Kubuntu's selection):**
    - Files (Dolphin), Terminal (Konsole), Kate, Okular, Gwenview, Elisa, Haruna, Spectacle, Ark, KCalc
    - LibreOffice, KMail and KOrganizer, KDE Connect
    - System Monitor, Partition Manager, printing and scanning
    - the browser: Firefox from Mozilla's repository when the build can reach it, otherwise Falkon
- **Install anything:**
    - the Ambar Store (Discover), with apt, Flathub and Snap
    - `.deb` files open in the Store
    - Wine for `.exe` and `.msi` files
    - distrobox and podman
    - Driver Manager, and media codecs
- **System:**
    - the Calamares installer, branded Ambar: erase disk, install alongside another OS, manual partitioning, or disk encryption
    - Timeshift snapshots, the ufw firewall with a settings page, fwupd firmware updates
    - fcitx5 for Indian and other input methods
- **Ambar:**
    - `ambard`, enabled for every user
    - the Automation Center, KRunner integration, the first-run Welcome
    - the marketplace and `ambar-hwreport`

### Build and test

```sh
sudo os/ubuntu/build.sh all          # rootfs (≈12 GB, ≈3 GB download) → output/ubuntu/ambar-os-0.2-amd64.iso
os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso
```

Steps can be run on their own: `rootfs` (resumable), `ambar` (re-applies the Ambar layer in a few minutes), `iso`. CI runs the whole thing in `.github/workflows/os-ubuntu.yml` with `BROWSER=firefox`, boots the ISO under KVM and keeps the ISO and boot evidence as artifacts.

## Atomic edition (experimental)

The sections below describe the Fedora/bootc recipe in `os/`.


Ambar OS is a **bootable container** ([bootc](https://bootc-dev.github.io/bootc/)) built on Fedora's KDE atomic desktop image (`quay.io/fedora-ostree-desktops/kinoite:45`). The whole OS is one image: it's built like a container and installed like a normal OS. Updates and rollbacks are atomic.

```
os/
  Containerfile          two stages: build Ambar (Rust + Python) → layer onto Kinoite
  branding.sh            os-release: "Ambar OS 0.1 (Fedora 45)", ID stays fedora
  files/                 copied to / : user service + preset, Plasma defaults,
                         login-screen wallpaper, serial console kargs, marketplace config
  config/qcow2.toml      bootc-image-builder: TEST user ambar/ambar
  config/iso.toml        bootc-image-builder: interactive Anaconda installer
  build.sh               container image → qcow2 | raw | anaconda-iso | live-iso | rpi
  live/                  live ISO layer: dracut-live, boot menu, liveuser, installer
  rpi-firmware.sh        adds Raspberry Pi firmware + U-Boot to an ARM64 raw disk
  config/rpi.toml        bootc-image-builder: Pi image, account made on first boot
  test-qemu.sh           boots a disk or ISO (x86_64 or aarch64) under UEFI, runs smoke_test.py
  smoke_test.py          serial-console login + checks that Ambar works inside the OS
```

## What's in the image

- **Programs:** `ambar` and `ambard` in `/usr/bin`. The Automation Center (`ambar-center`) and the Python SDK in the system Python, with PySide6 from Fedora (`python3-pyside6`).
- **Background service:** `ambard` is a systemd **user** service, enabled for every user by a global preset. It starts at login and restores enabled workflows. It runs as the logged-in user, never as root.
- **Desktop:** the Ambar look (colour schemes, look-and-feel, wallpaper, icons and launcher) is the default for new users through `/etc/xdg/kdeglobals`. Window buttons sit on the left, and the login screen uses the Ambar wallpaper.
- **Search:** typing in KRunner (Alt+Space) finds and runs automations, drafts new ones ("automate …"), and shows waiting approvals. It's a D-Bus runner, activated on demand.
- **First login:** the Welcome setup opens once and covers the service, appearance, AI and starter automations.
- **First boot:** systems installed without an account (from the live USB, Raspberry Pi images) create it with `plasma-setup` on first start.
- **Hardware reports:** `ambar-hwreport` prints what's needed for the [hardware list](hardware.md), without serial numbers or addresses.
- **Architectures:** x86_64 and aarch64 (ARM64), published as one multi-arch image.
- **Marketplace:** the bundled marketplace is at `/usr/share/ambar/marketplace`, and `ambar pkg search` uses it by default. Publishers still have to be trusted explicitly.
- **Branding:** the Ambar logo (`desktop/brand/`) is the launcher icon, the About-this-system logo and the Automation Center icon. In os-release: `NAME`, `PRETTY_NAME`, `VARIANT` and `IMAGE_ID`. `ID=fedora` and `VERSION_ID` are kept so dnf, toolbox and bootc keep working.

## Build

You need a Linux machine with rootful podman. Fedora or Ubuntu 24.04 both work.

```sh
os/build.sh                  # container image: localhost/ambar-os:latest
os/build.sh qcow2            # + output/qcow2/disk.qcow2   (test user ambar / ambar)
os/build.sh anaconda-iso     # + output/bootiso/install.iso (interactive installer)
os/build.sh raw              # + output/image/disk.raw      (dd to a disk)
os/build.sh live-iso         # + output/live/ambar-os-live-<arch>.iso (try without installing)
os/build.sh rpi              # + Raspberry Pi 4/400 SD-card image (on an ARM64 machine)
```

Everything builds for the architecture of the machine you build on. CI builds x86_64 and aarch64 on native runners.

The live ISO is made by `image-builder` (`generic-iso`), the successor of bootc-image-builder. The other disk types still use bootc-image-builder, which remains supported. This live ISO is not the Ubuntu image in [Try it from a USB stick](live-usb.md). That page installs `ambar-os-0.2-amd64.iso`.

- **Base image:** change it with `--build-arg BASE_IMAGE=…`. For example, `quay.io/fedora/fedora-bootc:45` plus your own KDE package set.
- **Fedora version:** change it with `--build-arg FEDORA_VERSION=…`.

## Test in a VM

```sh
os/test-qemu.sh output/qcow2/disk.qcow2              # headless smoke test
os/test-qemu.sh output/qcow2/disk.qcow2 --interactive  # open a window and use it
```

The smoke test boots the VM, logs in as the test user on the serial console, and checks:

- the branding
- that `ambard` is active as a user service
- `ambar service status` reports ok
- the bundled marketplace is searchable
- a workflow can be added and run, and it writes its file

It then logs the test user into the graphical session and checks:

- Plasma (`plasmashell`) is running
- the Welcome screen autostarted
- KRunner returns Ambar results over D-Bus
- the Ambar look-and-feel is installed

Finally it saves a screenshot of the desktop (`output/desktop.png`, uploaded by CI).

The live ISO is tested the same way, logging in as `liveuser` (`os/test-qemu.sh output/live/ambar-os-live-x86_64.iso`). For ARM64, set `ARCH=aarch64` on an x86 machine, or just run it on an ARM64 one.

The `os-image` CI workflow (`.github/workflows/os-image.yml`) runs on every push to `main`, once for each architecture:

1. build the container and the qcow2
2. boot the qcow2 and run the smoke test
3. build the live ISO and boot-test it (x86_64)
4. build the Raspberry Pi image (ARM64)

It then publishes `ghcr.io/<owner>/ambar-os` as one signed multi-arch image. The ISOs and the Pi image are kept as downloadable artifacts for 14 days.

## Install

Install the main edition from `ambar-os-0.2-amd64.iso`. The steps are [Try it from a USB stick](live-usb.md) and [Install](install.md).

The atomic edition is a different image. Its routes are experimental:

- its own live ISO, `ambar-os-live-<arch>.iso` (the live user is `liveuser`; the installer downloads the image and needs the internet)
- the Anaconda installer ISO
- `bootc switch` from Fedora Atomic
- a Raspberry Pi image

Those files come from the **OS image** workflow and are kept for 14 days. They are not a substitute for `ambar-os-0.2-amd64.iso`.

## Updates, rollback and recovery

| Task | Command |
|---|---|
| Update now | `sudo bootc upgrade` (staged; applied on reboot) |
| Automatic updates | `bootc-fetch-apply-updates.timer` (shipped by bootc; enable it if your base doesn't) |
| Roll back to the previous OS | `sudo bootc rollback`, then reboot, or pick the previous entry in the boot menu |
| See deployments | `sudo bootc status` |
| Switch channel | `sudo bootc switch ghcr.io/alexbieber/ambar-os:<tag>` |

`/usr` is read-only and replaced as a whole on update. `/etc` merges your changes, and `/var` and `/home` are never touched. So a bad update is one reboot away from the previous working system, and your files and Ambar data (`~/.local/share/ambar`) survive both updates and rollbacks.

## Hardware

The atomic edition uses Fedora 45's kernel, firmware and drivers. The main edition uses Ubuntu 26.04's. What has actually been tested, and how to report your machine, is in [Hardware](hardware.md).

## Status and known gaps

- **Live install is network-based:** the live USB's installer pulls the image from GHCR, so it needs internet, and the GHCR package must be public. An offline install uses the installer ISO.
- **Not yet built or booted.** The image could not be built where it was written: there was no access to Fedora's registries and no virtualization. The recipe is checked statically (COPY sources, configs, the systemd unit with `systemd-analyze`, shellcheck, the branding script, the Python install step). The smoke harness is tested against a simulated console. The **first real build and boot happens in the `os-image` CI job**, and its results decide whether this is done.
- **Signature enforcement on the client:** CI signs pushed images with cosign (keyless, sigstore). Installed systems don't yet *require* a valid signature before `bootc upgrade` (`/etc/containers/policy.json`). Add that once the signing identity is final.
- **SELinux:** `ambard` runs unconfined as the user. A confined policy module is still to do.
- **Not rebranded:** Plymouth boot splash and the Anaconda installer still show Fedora.
- **Test credentials:** the qcow2 config's user and password are for tests only. Never ship them.
