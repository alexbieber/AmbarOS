# Ambar OS image

Ambar OS is a **bootable container** ([bootc](https://bootc-dev.github.io/bootc/)) built on Fedora's KDE atomic desktop image (`quay.io/fedora-ostree-desktops/kinoite:45`). The whole OS is one image: it's built like a container and installed like a normal OS. Updates and rollbacks are atomic.

```
os/
  Containerfile          two stages: build Ambar (Rust + Python) → layer onto Kinoite
  branding.sh            os-release: "Ambar OS 0.1 (Fedora 45)", ID stays fedora
  files/                 copied to / : user service + preset, Plasma defaults,
                         login-screen wallpaper, serial console kargs, marketplace config
  config/qcow2.toml      bootc-image-builder: TEST user ambar/ambar
  config/iso.toml        bootc-image-builder: interactive Anaconda installer
  build.sh               container image → qcow2 | raw | anaconda-iso
  test-qemu.sh           boots a disk under UEFI and runs smoke_test.py
  smoke_test.py          serial-console login + checks that Ambar works inside the OS
```

## What's in the image

- **Programs:** `ambar` and `ambard` in `/usr/bin`. The Automation Center (`ambar-center`) and the Python SDK in the system Python, with PySide6 from Fedora (`python3-pyside6`).
- **Background service:** `ambard` is a systemd **user** service, enabled for every user by a global preset. It starts at login and restores enabled workflows. It runs as the logged-in user, never as root.
- **Desktop:** the Ambar look (colour schemes, look-and-feel, wallpaper, icons and launcher) is the default for new users through `/etc/xdg/kdeglobals`. Window buttons sit on the left, and the login screen uses the Ambar wallpaper.
- **Marketplace:** the bundled marketplace is at `/usr/share/ambar/marketplace`, and `ambar pkg search` uses it by default. Publishers still have to be trusted explicitly.
- **Branding:** `NAME`, `PRETTY_NAME`, `VARIANT` and `IMAGE_ID`. `ID=fedora` and `VERSION_ID` are kept so dnf, toolbox and bootc keep working.

## Build

You need a Linux machine with rootful podman. Fedora or Ubuntu 24.04 both work.

```sh
os/build.sh                  # container image: localhost/ambar-os:latest
os/build.sh qcow2            # + output/qcow2/disk.qcow2   (test user ambar / ambar)
os/build.sh anaconda-iso     # + output/bootiso/install.iso (interactive installer)
os/build.sh raw              # + output/image/disk.raw      (dd to a disk)
```

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

The `os-image` CI workflow (`.github/workflows/os-image.yml`) does all of this on every push to `main`. It builds the container, builds the qcow2, boots it under KVM, runs the smoke test, then pushes `ghcr.io/<owner>/ambar-os`.

## Install

| Starting point | How |
|---|---|
| Blank machine | Write `install.iso` to a USB stick and boot it. |
| Already on Fedora Kinoite / Silverblue / any bootc system | `sudo bootc switch ghcr.io/alexbieber/ambar-os:latest`, then reboot. |
| VM | Use `disk.qcow2`. |

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

Ambar OS uses Fedora 45's kernel, firmware and drivers, so its hardware support is Fedora's. **No physical machine has been tested yet.** Add rows as machines are verified:

| Machine | Boot | Wi-Fi | Graphics | Suspend | Notes |
|---|---|---|---|---|---|
| QEMU/KVM (q35, UEFI, virtio) | CI | n/a | virtio | n/a | smoke test |

## Status and known gaps

- **Not yet built or booted.** The image could not be built where it was written: there was no access to Fedora's registries and no virtualization. The recipe is checked statically (COPY sources, configs, the systemd unit with `systemd-analyze`, shellcheck, the branding script, the Python install step). The smoke harness is tested against a simulated console. The **first real build and boot happens in the `os-image` CI job**, and its results decide whether this is done.
- **Image signing:** not configured yet (sigstore/cosign). Add it before publishing images for others.
- **SELinux:** `ambard` runs unconfined as the user. A confined policy module is still to do.
- **Not rebranded:** Plymouth boot splash and the Anaconda installer still show Fedora.
- **Test credentials:** the qcow2 config's user and password are for tests only. Never ship them.
