# Hardware

Ambar OS uses Fedora 45's kernel, drivers and firmware, so in principle it runs wherever Fedora 45 KDE runs. This page records what has **actually been tested**. A machine gets a row only after someone has run Ambar OS on it.

## Compatibility list

**Legend**

| Mark | Meaning |
|---|---|
| ✅ | works |
| ⚠️ | works with a note |
| ❌ | doesn't work |
| — | not applicable |
| ? | not tested |

| Machine | Arch | Boots | Wi-Fi | Graphics | Sleep | Tested with | Notes |
|---|---|---|---|---|---|---|---|
| QEMU/KVM (q35, UEFI, virtio) | x86_64 | CI | — | virtio | — | every change | automatic boot test of disk image and live ISO |
| QEMU (virt, UEFI) | aarch64 | CI | — | virtio | — | every change | automatic boot test, CLI checks only |

**No physical machine has been tested yet.** The first rows will come from the [hardware reports](#report-your-computer) below.

## What to expect, by type of hardware

This guidance is based on Fedora's hardware support. It isn't Ambar test results.

| Hardware | Expectation |
|---|---|
| Intel graphics (integrated) | Works out of the box. |
| AMD graphics | Works out of the box. |
| NVIDIA graphics | Works with the open-source `nouveau` driver, which is slower. The proprietary driver isn't included yet. |
| Intel Wi-Fi (`iwlwifi`) | Works out of the box. |
| MediaTek, Qualcomm, Realtek Wi-Fi | Usually works; some newer chips need a newer kernel. |
| Broadcom Wi-Fi (`wl`) | Often doesn't work. Fedora doesn't ship that driver by default. |
| Fingerprint readers | Depends on the model (`fprintd`). |
| Apple Mac with Apple silicon (M1 and later) | Doesn't run directly. Use a [VM in UTM](install.md#virtual-machine) with the ARM64 image. |
| Intel Mac | Should boot; Wi-Fi is often Broadcom (see above). |
| Raspberry Pi 4 / 400 | Image provided ([how](install.md#raspberry-pi-4-and-400)), untested. Desktop will be slow. |
| Raspberry Pi 5 | Not supported (Fedora doesn't support it yet). |
| Other ARM64 boards | Only boards with UEFI firmware are supported. |

## Report your computer

Every report makes Ambar OS better for everyone with the same model. It takes five minutes.

1. Start Ambar OS on the machine, either from the [live USB](live-usb.md) or installed.
2. Open a terminal (Konsole) and run:

    ```sh
    ambar-hwreport
    ```

3. Try the things in the report's last line:
    - Did it start?
    - Does Wi-Fi connect?
    - Does the screen look right?
    - Does it sleep and wake up?
4. Open a [**Hardware report** issue](https://github.com/alexbieber/AmbarOS/issues/new?template=hardware_report.yml) and paste the output with your results.

### What the report contains, and what it doesn't

| It includes | It never includes |
|---|---|
| the computer's maker and model | serial numbers |
| CPU and memory size | MAC or IP addresses |
| graphics, network and audio chips and their drivers | device UUIDs |
| boot mode and Secure Boot state | user or computer names |
| supported sleep states | anything from your home folder |
| whether the Ambar service is running | |

It doesn't send anything; you choose what to paste. Use `ambar-hwreport --json` to get the same data in machine-readable form.

## Architectures

| Architecture | Image | How it's built and tested |
|---|---|---|
| x86_64 (64-bit Intel/AMD PCs) | disk images, live ISO, installer ISO | built and boot-tested (including the desktop) on every change |
| aarch64 (ARM64 with UEFI) | disk images, live ISO | built natively on ARM64 runners and boot-tested (command-line checks) on every change |
| aarch64, Raspberry Pi 4/400 | SD-card image | built on every change; can't be boot-tested in CI |

The published image `ghcr.io/alexbieber/ambar-os:latest` is multi-arch, so `bootc switch` and updates pick the right one automatically.
