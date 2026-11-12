# Install Ambar OS

Start by [downloading the ISO](live-usb.md#download-the-iso). The file to use is `ambar-os-0.2-amd64.iso`, after you unzip the artifact. This page is what you do with that file.

| You have… | Do this |
|---|---|
| A PC you want to use for Ambar (alone or next to Windows) | [From the USB stick](#from-the-usb-stick) |
| A Mac, or you just want to try it | [Virtual machine](#virtual-machine) |

!!! warning "Early project"
    Ambar OS boots and runs in virtual machines, but it hasn't been tried on many real computers yet, see [Status](STATUS.md). Back up anything important first.

## Requirements

| | Minimum | Better |
|---|---|---|
| CPU | 64-bit Intel or AMD (x86_64) | 4 cores |
| Memory | 4 GB | 8 GB or more |
| Disk | 30 GB | SSD, 64 GB or more |
| Firmware | UEFI or BIOS | UEFI with Secure Boot |

## From the USB stick

1. [Write the ISO to a USB stick and start from it](live-usb.md).
2. Double-click **Install Ambar OS**.
3. Choose **language, keyboard and location**.
4. Choose where to install. **Erase disk** uses the whole disk. **Install alongside** keeps Windows or another OS, and you pick at start-up. **Manual** lets you partition yourself. Tick **Encrypt** to protect the disk with a password.
5. Enter your **name and password**.
6. Restart and remove the stick.

The installer copies the system from the stick, so it works offline.

## Virtual machine

Open `ambar-os-0.2-amd64.iso` in any virtual machine app. Use the `.iso` file from the zip, not the zip itself.

- **Windows:** VirtualBox, VMware Workstation, Hyper-V (a Generation 2 VM)
- **Linux:** GNOME Boxes, virt-manager, or `os/ubuntu/test-qemu.sh <iso> --interactive`

Give it 4 GB of RAM or more, 2 CPUs or more, and a 30 GB disk, then install as above.

On a **Mac with Apple silicon**, the current ISO is for Intel/AMD PCs. In UTM it runs under emulation, which works but is slow.

## After installing

- **First start:** the **Welcome** opens. It walks you through appearance, the optional AI assistant and starter automations. See [First steps](first-steps.md).
- **Apps:** open the **Ambar Store** from the Dock. See [Install apps](first-steps.md#install-apps).
- **Updates:** the Ambar Store tells you when updates are ready. Ambar OS gets Ubuntu 26.04 LTS security updates until 2031.
- **Snapshots and backup:** **Timeshift** keeps system snapshots you can go back to if an update goes wrong, and the KDE backup tool keeps copies of your files.
- **Hardware report:** run `ambar-hwreport` and [send it](hardware.md#report-your-computer).

## Other editions

**Ambar OS Security** uses this same installer. Download `ambar-os-security-0.2-amd64.iso` only when the security job on the [ISO workflow](https://github.com/alexbieber/AmbarOS/actions/workflows/os-ubuntu.yml) succeeded. See [Download the ISO](live-usb.md#download-the-iso).

The **atomic edition** is a separate experimental image based on Fedora. It is not `ambar-os-0.2-amd64.iso`, and these USB steps do not apply to it. See [OS images](os-image.md#atomic-edition-experimental).
