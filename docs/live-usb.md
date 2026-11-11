# Try Ambar OS from a USB stick

The Ambar OS ISO is a live USB. It starts the complete desktop from a USB stick without touching your disk. Use it to:

- check that your computer's Wi-Fi, graphics, sound and sleep work
- try the apps
- install from the same stick when you're happy

## What you need

- A USB stick of **8 GB or more**. Everything on it will be erased.
- A 64-bit Intel or AMD PC (x86_64) with **4 GB of RAM or more** (8 GB is better).
- The ISO: `ambar-os-0.2-amd64.iso`. Until there are releases, download it from the latest successful **Ambar OS ISO** run on GitHub Actions (artifact `ambar-os-iso`), or [build it yourself](#build-the-iso-yourself).

## 1. Write the ISO to the stick

Pick one:

- **Fedora Media Writer** or **balenaEtcher** (Windows, macOS, Linux): choose the ISO, then the stick.
- **Rufus** (Windows): choose the ISO, and *DD image* mode if it asks.
- **Linux or macOS terminal.** Replace `/dev/sdX` with your stick. Check it twice with `lsblk` or `diskutil list`, because this erases the device you name:

    ```sh
    sudo dd if=ambar-os-0.2-amd64.iso of=/dev/sdX bs=4M status=progress oflag=sync
    ```

## 2. Start the computer from the stick

1. Plug the stick in and restart.
2. Open the boot menu. Common keys: **F12** (Dell, Lenovo), **F9** (HP), **F8** or **Esc** (ASUS), **F11** (MSI). On an Intel Mac, hold **Option**.
3. Choose the USB stick (the UEFI entry if there are two).
4. At the Ambar menu, choose **Try or Install Ambar OS**. If the screen stays black, restart and pick **safe graphics**.

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
