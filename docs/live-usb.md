# Try Ambar OS from a USB stick

The Ambar OS ISO is a live USB. It starts the complete desktop from a USB stick without touching your disk. Use it to:

- check that your computer's Wi-Fi, graphics, sound and sleep work
- try the apps
- install from the same stick when you're happy

## What you need

- A USB stick of **8 GB or more**. Everything on it will be erased.
- A 64-bit Intel or AMD PC (x86_64) with **4 GB of RAM or more** (8 GB is better).
- The standard ISO, `ambar-os-0.2-amd64.iso`, from [Download the ISO](#download-the-iso).

## Download the ISO

The ISO is a built disk image, about 3.6 GB. It is not in the source folder. GitHub keeps each build for **14 days** on the [Ambar OS ISO workflow](https://github.com/alexbieber/AmbarOS/actions/workflows/os-ubuntu.yml).

1. Open the latest run where the **standard** job succeeded. Sign in to GitHub.
2. At the bottom of the run, download the artifact **ambar-os-standard-iso**.
3. Unzip it. The files inside are `ambar-os-0.2-amd64.iso` and `ambar-os-0.2-amd64.iso.sha256`.
4. Check the download:

    ```sh
    sha256sum -c ambar-os-0.2-amd64.iso.sha256          # Linux
    shasum -a 256 -c ambar-os-0.2-amd64.iso.sha256      # macOS
    ```

Write `ambar-os-0.2-amd64.iso` to the stick. The zip itself will not boot.

**Ambar OS Security** is the same desktop with Kali's tools already installed. On a run where the **security** job succeeded, download **ambar-os-security-iso** and use `ambar-os-security-0.2-amd64.iso`. The boot menu then says **Try or Install Ambar OS Security**. The rest of this page is the same. If that job failed, the artifact is not there; use the standard ISO.

To build the image on an Ubuntu or Debian machine, see [Build the ISO yourself](#build-the-iso-yourself).

## 1. Write the ISO to the stick

Pick one:

- **balenaEtcher** (Windows, macOS, Linux): choose the `.iso`, then the stick.
- **Rufus** (Windows): choose the `.iso`, and *DD image* mode if it asks.
- **Fedora Media Writer** (Windows, macOS, Linux): it writes any ISO, including this one. Choose the `.iso`, then the stick.
- **Linux terminal.** Replace `/dev/sdX` with your stick. Check it twice with `lsblk`, because this erases the device you name:

    ```sh
    sudo dd if=ambar-os-0.2-amd64.iso of=/dev/sdX bs=4M status=progress oflag=sync
    ```

- **macOS terminal.** Replace `N` with the stick's disk number from `diskutil list`. This erases that disk:

    ```sh
    diskutil unmountDisk /dev/diskN
    sudo dd if=ambar-os-0.2-amd64.iso of=/dev/rdiskN bs=4m
    ```

## 2. Start the computer from the stick

1. Plug the stick in and restart.
2. Open the boot menu. Common keys: **F12** (Dell, Lenovo), **F9** (HP), **F8** or **Esc** (ASUS), **F11** (MSI). On an Intel Mac, hold **Option**.
3. Choose the USB stick (the UEFI entry if there are two).
4. At the Ambar menu, choose **Try or Install Ambar OS**. If the screen stays black, restart and choose **Try or Install Ambar OS (safe graphics)**.

The ISO boots on UEFI (with Secure Boot, through Ubuntu's signed shim) and on older BIOS PCs.

## 3. Use it

You're logged straight into the desktop as **ambar**, which has no password. Everything works as on an installed system:

- the Dock and Launchpad
- the menu bar with the Ambar menu and Control Center
- Search (**Meta+Space**)
- the browser, Files, Terminal and Office
- the Ambar Store
- the Automation Center

**Nothing you do is kept** unless you install.

Good things to check:

- Wi-Fi connects (Control Center or the network icon).
- Sound plays, and the screen uses its full resolution.
- The machine sleeps and wakes (close the lid, then open it).
- Run `ambar-hwreport` in Terminal and [send the report](hardware.md#report-your-computer).

## 4. Install

Double-click **Install Ambar OS** on the desktop, or find it in Launchpad. The installer asks for:

1. **Language, keyboard and location.**
2. **Where to install.** You can erase the whole disk, install next to Windows or another OS, or partition by hand. You can also encrypt the disk.
3. **Your name, computer name and password.**

It copies the system from the stick, so **no internet connection is needed**. When it's done, remove the stick and restart.

## Build the ISO yourself

On Ubuntu or Debian, as root, with network access to the Ubuntu archive:

```sh
sudo apt-get install debootstrap squashfs-tools xorriso grub-efi-amd64-bin grub-pc-bin mtools dosfstools
cargo build --release
sudo os/ubuntu/build.sh all                     # → output/ubuntu/ambar-os-0.2-amd64.iso
os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso              # automatic boot test
os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso --interactive  # try it in a window
```

The build takes about an hour and needs about 25 GB of free space. `BROWSER=firefox` adds Mozilla's APT repository for the real Firefox; without it, KDE's Falkon browser is used. The recipe is in `os/ubuntu/`.
