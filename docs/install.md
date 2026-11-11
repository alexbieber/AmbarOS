# Install Ambar OS

Choose the way that fits your situation. Each one installs the same OS image.

| You have… | Do this |
|---|---|
| A PC you want to wipe and use for Ambar | [From the live USB](#from-the-live-usb) (recommended) or [the installer ISO](#with-the-installer-iso) |
| Fedora Kinoite, Silverblue or another bootc system already | [Switch to Ambar](#switch-an-existing-fedora-atomic-system) |
| A Mac with Apple silicon, or any computer, and want a VM | [Virtual machine](#virtual-machine) |
| A Raspberry Pi 4 or 400 | [Raspberry Pi](#raspberry-pi-4-and-400) |

!!! warning "Early project"
    No physical machine has been tested yet, see [Status](STATUS.md). Keep a backup of anything important, and use a spare machine or a VM first.

## Requirements

| | Minimum | Better |
|---|---|---|
| CPU | 64-bit x86 (x86_64) or ARM64 with UEFI | 4 cores |
| Memory | 4 GB | 8 GB or more |
| Disk | 32 GB (the whole disk is used) | SSD, 64 GB or more |
| Firmware | UEFI | UEFI with Secure Boot |
| Network | needed for live-USB installs and updates | |

## From the live USB

Start the live USB ([how](live-usb.md)), then double-click **Install Ambar OS**. Full steps are in [Install from the live session](live-usb.md#4-install-from-the-live-session).

## With the installer ISO

The installer ISO uses Fedora's installer (Anaconda). It asks for your language, the disk and your account, and it works offline because the OS is inside the ISO.

```sh
os/build.sh anaconda-iso     # → output/bootiso/install.iso
```

Write it to a USB stick the same way as the [live ISO](live-usb.md#1-write-the-iso-to-the-stick), boot it, and follow the installer.

## Switch an existing Fedora Atomic system

If you already run Fedora Kinoite, Silverblue or another bootc-based system, you can switch it to Ambar OS. Your files in `/home` stay as they are.

```sh
sudo bootc switch ghcr.io/alexbieber/ambar-os:latest
sudo reboot
```

To go back, choose the previous entry in the boot menu, or run `sudo bootc rollback` and then reboot.

## Virtual machine

Build a VM disk (`os/build.sh qcow2`) or use the live ISO.

- **Linux (QEMU/KVM, GNOME Boxes, virt-manager):** create a UEFI VM with 4 GB RAM or more and attach `disk.qcow2` or the ISO. `os/test-qemu.sh <image> --interactive` does this for you.
- **Mac with Apple silicon (UTM):** use the **ARM64** image (`ambar-os-live-aarch64.iso`, or a qcow2 built on an ARM64 Linux machine). In UTM, choose *Virtualize → Linux*, enable UEFI, and give it 4 GB RAM or more. The x86_64 image would run under emulation, which is very slow.
- **Windows (Hyper-V, VirtualBox):** use the x86_64 live ISO in a *Generation 2* / EFI VM.

!!! note "Test images"
    The `qcow2` built by `os/build.sh qcow2` contains a test account, `ambar` with password `ambar`. Use it only for testing.

## Raspberry Pi 4 and 400

The Pi image is built on an ARM64 Linux machine (CI does this on every change):

```sh
os/build.sh rpi              # → output/image/disk.raw, with Pi firmware and U-Boot added
```

Write `disk.raw` (or the CI artifact `ambar-os-rpi4-aarch64.raw.xz`) to a microSD card of 32 GB or more with Fedora Media Writer, balenaEtcher or `dd`. Put it in the Pi and power it on. You'll create your account on first start.

- **Raspberry Pi 5 is not supported.** Fedora doesn't support it yet.
- **Expect a slow desktop.** A Pi 4 runs a full Plasma desktop slowly. 8 GB models and a USB SSD help a lot.
- **Device-tree updates.** The Pi device trees are copied onto the boot partition when the image is built. If a later kernel update needs new ones, they aren't updated automatically yet.

## After installing

- **First start:** create your account. The **Welcome** then opens and walks you through appearance, the optional AI assistant and starter automations. See [First steps](first-steps.md).
- **Updates:** installed automatically in the background and applied on the next restart. See [Updates, rollback and recovery](os-image.md#updates-rollback-and-recovery).
- **Hardware report:** run `ambar-hwreport` and [send it](hardware.md#report-your-computer). It's the fastest way to make Ambar OS work well on your model.
