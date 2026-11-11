#!/usr/bin/env bash
# Builds the Ambar OS container image and, optionally, a disk image or ISO.
#   os/build.sh                 container image only
#   os/build.sh qcow2           + QEMU/KVM disk (test user ambar/ambar)
#   os/build.sh raw             + raw disk for dd to a disk / VM
#   os/build.sh anaconda-iso    + installer ISO
#   os/build.sh live-iso        + live ISO: try it from a USB stick, install from there
#   os/build.sh rpi             + Raspberry Pi 4/400 SD-card image (on an ARM64 machine)
# Builds for the machine's own architecture (x86_64 or aarch64).
# Needs: podman (rootful, because the image builders run privileged).
set -euo pipefail
cd "$(dirname "$0")/.."
TYPE="${1:-}"
IMAGE="${IMAGE:-localhost/ambar-os:latest}"
LIVE_IMAGE="${LIVE_IMAGE:-localhost/ambar-os-live:latest}"
BIB="${BIB:-quay.io/centos-bootc/bootc-image-builder:latest}"
IB="${IB:-ghcr.io/osbuild/image-builder-cli:latest}"
PODMAN="${PODMAN:-sudo podman}"
ARCH="$(uname -m)"

$PODMAN build --pull=newer -t "$IMAGE" -f os/Containerfile .
[ -z "$TYPE" ] && { echo "built $IMAGE ($ARCH)"; exit 0; }
mkdir -p output

bib() {  # bib <type> <config>
  $PODMAN run --rm --privileged --pull=newer \
    --security-opt label=type:unconfined_t \
    -v "$PWD/output:/output" \
    -v /var/lib/containers/storage:/var/lib/containers/storage \
    -v "$PWD/$2:/config.toml:ro" \
    "$BIB" --type "$1" --rootfs btrfs "$IMAGE"
}

case "$TYPE" in
  qcow2|raw) bib "$TYPE" os/config/qcow2.toml; echo "disk image in output/$TYPE/" ;;
  anaconda-iso) bib anaconda-iso os/config/iso.toml; echo "installer ISO in output/bootiso/" ;;
  live-iso)
    $PODMAN build -t "$LIVE_IMAGE" --build-arg BASE="$IMAGE" -f os/live/Containerfile .
    $PODMAN run --rm --privileged --pull=newer \
      --security-opt label=type:unconfined_t \
      -v "$PWD/output:/output" \
      -v /var/lib/containers/storage:/var/lib/containers/storage \
      "$IB" build --bootc-ref "$LIVE_IMAGE" --bootc-default-fs ext4 \
      --output-dir /output/live generic-iso
    iso="$(find output/live -name '*.iso' | head -n1)"
    [ -n "$iso" ] || { echo "no ISO produced" >&2; exit 1; }
    mv "$iso" "output/live/ambar-os-live-$ARCH.iso"
    echo "live ISO: output/live/ambar-os-live-$ARCH.iso" ;;
  rpi)
    [ "$ARCH" = aarch64 ] || { echo "rpi images are built on an ARM64 machine" >&2; exit 2; }
    bib raw os/config/rpi.toml
    disk="$(find output/image -name 'disk.raw' | head -n1)"
    sudo os/rpi-firmware.sh "$disk" "$IMAGE"
    echo "Raspberry Pi image: $disk (write it to an SD card; create your account on first boot)" ;;
  *) echo "unknown type $TYPE (qcow2, raw, anaconda-iso, live-iso, rpi)" >&2; exit 2 ;;
esac
