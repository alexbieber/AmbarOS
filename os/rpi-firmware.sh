#!/usr/bin/env bash
# Makes an Ambar OS ARM64 raw disk image bootable on a Raspberry Pi 4 / 400.
#   sudo os/rpi-firmware.sh output/image/disk.raw [localhost/ambar-os:latest]
#
# A Pi doesn't boot UEFI by itself. Its GPU firmware loads U-Boot from the EFI
# system partition, and U-Boot then starts the normal UEFI boot (shim, GRUB).
# This copies onto the ESP:
#   - Pi firmware (bootcode/start/fixup) and overlays: Fedora's bcm283x packages
#   - U-Boot for 64-bit Pi as rpi-u-boot.bin: Fedora's uboot-images-armv8
#   - device trees from the Ambar image's own kernel, so they match it
#   - config.txt (only if the firmware package didn't provide one)
# Same approach as Fedora's own aarch64 atomic images (rhbz#2544508).
# Pi 5 is not supported by Fedora's firmware/U-Boot yet.
set -euo pipefail
DISK="${1:?usage: rpi-firmware.sh disk.raw [image]}"
IMAGE="${2:-localhost/ambar-os:latest}"
FEDORA="${FEDORA:-registry.fedoraproject.org/fedora:45}"
PODMAN="${PODMAN:-podman}"
[ "$(id -u)" = 0 ] || { echo "run as root (needs a loop device)" >&2; exit 2; }
WORK="$(mktemp -d)"
LOOP=""
cleanup() { umount "$WORK/esp" 2>/dev/null || true; [ -n "$LOOP" ] && losetup -d "$LOOP"; rm -rf "$WORK"; }
trap cleanup EXIT

echo "fetching Raspberry Pi firmware and U-Boot from Fedora"
mkdir -p "$WORK/fw"
$PODMAN run --rm -v "$WORK/fw:/out:z" "$FEDORA" sh -euc '
  dnf -y -q install --setopt=install_weak_deps=False --installroot=/r --use-host-config \
      bcm283x-firmware bcm2711-firmware bcm283x-overlays uboot-images-armv8 >/dev/null
  cp -a /r/boot/efi/. /out/
  cp /r/usr/share/uboot/rpi_arm64/u-boot.bin /out/rpi-u-boot.bin'

echo "device trees from $IMAGE"
mkdir -p "$WORK/dtb"
$PODMAN run --rm -v "$WORK/dtb:/out:z" "$IMAGE" sh -euc '
  d=$(ls -d /usr/lib/modules/*/dtb/broadcom | head -n1)
  cp "$d"/bcm27*.dtb /out/'
ls "$WORK/dtb"/bcm2711-rpi-4-b.dtb >/dev/null

LOOP="$(losetup --show -fP "$DISK")"
udevadm settle 2>/dev/null || true
ESP=""
for p in "$LOOP"p*; do
  [ "$(blkid -o value -s TYPE "$p" 2>/dev/null)" = vfat ] && { ESP="$p"; break; }
done
[ -n "$ESP" ] || { echo "no EFI system partition in $DISK" >&2; exit 1; }
mkdir -p "$WORK/esp" && mount "$ESP" "$WORK/esp"

# Never overwrite the UEFI boot chain (EFI/) that bootc installed.
rm -rf "$WORK/fw/EFI"
cp -r "$WORK/fw/." "$WORK/esp/"
cp "$WORK/dtb"/*.dtb "$WORK/esp/"
if [ ! -f "$WORK/esp/config.txt" ]; then
  cat > "$WORK/esp/config.txt" <<'TXT'
# Ambar OS on Raspberry Pi 4/400: firmware → U-Boot → UEFI (shim/GRUB) → Linux
arm_64bit=1
enable_uart=1
upstream_kernel=1
kernel=rpi-u-boot.bin
TXT
fi
for f in rpi-u-boot.bin start4.elf fixup4.dat bcm2711-rpi-4-b.dtb config.txt EFI; do
  [ -e "$WORK/esp/$f" ] || { echo "missing $f on the ESP" >&2; exit 1; }
done
sync
echo "Raspberry Pi boot files added to $DISK"
