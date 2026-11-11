# Try Ambar OS from a USB stick

The live USB starts the full Ambar OS from a USB stick without touching your disk. Use it to see whether your computer's Wi-Fi, graphics, sound and sleep work, and to try the automation tools. When you're happy, install from the same stick.

!!! warning "Not yet tested on hardware"
    The live ISO is built and boot-tested in a virtual machine by CI. It has not been tried on a physical computer yet. If you try it, please [send a hardware report](hardware.md#report-your-computer).

## What you need

- A USB stick of **8 GB or more**. Everything on it will be erased.
- A 64-bit PC (x86_64) or an ARM64 machine with UEFI, with **4 GB of RAM or more** (8 GB is better).
- The live ISO for your architecture: `ambar-os-live-x86_64.iso` or `ambar-os-live-aarch64.iso`. Until there are releases, download it from the latest successful **OS image** run on GitHub Actions (artifact `ambar-os-live-amd64` or `ambar-os-live-arm64`).

## 1. Write the ISO to the stick

Pick one:

- **Fedora Media Writer** (Windows, macOS, Linux): choose *Select .iso file*, pick the Ambar ISO, then pick your stick.
- **balenaEtcher** (Windows, macOS, Linux): *Flash from file*, then choose the stick.
- **Linux or macOS terminal.** Replace `/dev/sdX` with your stick. Check it twice with `lsblk` (Linux) or `diskutil list` (macOS), because this erases the device you name:

    ```sh
    sudo dd if=ambar-os-live-x86_64.iso of=/dev/sdX bs=4M status=progress oflag=sync
    ```

Don't copy the ISO onto the stick as a file. It has to be *written* as an image.

## 2. Start the computer from the stick

1. Plug the stick in and restart.
2. Open the boot menu. Common keys: **F12** (Dell, Lenovo), **F9** (HP), **F8** or **Esc** (ASUS), **F11** (MSI). On a Mac with an Intel chip, hold **Option**.
3. Choose the USB stick. Pick the UEFI entry if there are two.
4. At the Ambar menu, choose **Try Ambar OS**. If the screen stays black or garbled, restart and choose **Try Ambar OS (safe graphics)**.

!!! note "Secure Boot"
    Ambar OS boots through Fedora's signed shim. Secure Boot normally works without changes. If your firmware refuses to boot the stick, see [Troubleshooting](troubleshooting.md#the-usb-stick-wont-boot).

## 3. Use it

You're logged in as **liveuser**, which has no password. The first-run **Welcome** opens. Everything works as on an installed system:

- the Automation Center
- KRunner search (**Alt+Space**)
- the AI builder
- the bundled marketplace

**Nothing you do is kept.** Files, settings and automations disappear when you shut down.

Good things to check:

- Wi-Fi connects (system tray → network).
- Sound plays (System Settings → Sound).
- The screen uses its full resolution and brightness keys work.
- The machine sleeps and wakes (close the lid, then open it).
- Run `ambar-hwreport` in a terminal (Konsole) and [send the report](hardware.md#report-your-computer).

## 4. Install from the live session

Double-click **Install Ambar OS** on the desktop. Then:

1. Choose the disk. The installer only lists whole disks of 32 GB or more. It never lists the stick you started from, a read-only disk, or a disk that's currently mounted.
2. Type the disk's name (for example `/dev/nvme0n1`) to confirm. **Everything on that disk is erased.**
3. Wait. The installer downloads the current Ambar OS image, so you need an internet connection.
4. Remove the stick and restart. On first start, Ambar OS asks for your language, then creates your account.

From a terminal, the same installer is available with more control:

```sh
ambar-install --list                         # disks it would offer, and why others are hidden
ambar-install --disk /dev/sda --dry-run      # show the exact command, don't run it
ambar-install --disk /dev/sda                # asks you to type the disk name first
```

The install is done by `bootc install to-disk`, run from the Ambar OS image. The installed system then updates from that same image ([updates](os-image.md#updates-rollback-and-recovery)).

## Build the live ISO yourself

On Linux with rootful podman:

```sh
os/build.sh live-iso        # → output/live/ambar-os-live-<arch>.iso
os/test-qemu.sh output/live/ambar-os-live-x86_64.iso                 # automatic boot test
os/test-qemu.sh output/live/ambar-os-live-x86_64.iso --interactive   # try it in a window
```

The live image is the normal OS image with a few additions:

- the live boot modules (`dracut-live`)
- an ISO boot menu
- the passwordless `liveuser`, created at boot only when the kernel was started from live media (`rd.live.image`)
- the installer

The recipe is in `os/live/`.
