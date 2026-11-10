#!/usr/bin/env bash
# Builds the Ambar OS container image and, optionally, a disk image.
#   os/build.sh                 container image only
#   os/build.sh qcow2           + QEMU/KVM disk (test user ambar/ambar)
#   os/build.sh anaconda-iso    + installer ISO
#   os/build.sh raw             + raw disk for dd to a USB stick / VM
# Needs: podman (rootful, because bootc-image-builder runs privileged).
set -euo pipefail
cd "$(dirname "$0")/.."
TYPE="${1:-}"
IMAGE="${IMAGE:-localhost/ambar-os:latest}"
BIB="${BIB:-quay.io/centos-bootc/bootc-image-builder:latest}"
PODMAN="${PODMAN:-sudo podman}"

$PODMAN build --pull=newer -t "$IMAGE" -f os/Containerfile .
[ -z "$TYPE" ] && { echo "built $IMAGE"; exit 0; }

case "$TYPE" in
  qcow2|raw) CONFIG=os/config/qcow2.toml ;;
  anaconda-iso) CONFIG=os/config/iso.toml ;;
  *) echo "unknown type $TYPE (qcow2, raw, anaconda-iso)" >&2; exit 2 ;;
esac
mkdir -p output
$PODMAN run --rm --privileged --pull=newer \
  --security-opt label=type:unconfined_t \
  -v "$PWD/output:/output" \
  -v /var/lib/containers/storage:/var/lib/containers/storage \
  -v "$PWD/$CONFIG:/config.toml:ro" \
  "$BIB" --type "$TYPE" --rootfs btrfs "$IMAGE"
echo "disk image(s) in output/$TYPE/"
