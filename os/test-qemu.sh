#!/usr/bin/env bash
# Boots an Ambar OS disk image or live ISO under UEFI and runs the
# serial-console smoke test (login, ambard running, CLI, marketplace, a real
# workflow run, then Plasma, Welcome and KRunner).
#   os/test-qemu.sh [output/qcow2/disk.qcow2] [--interactive]
#   os/test-qemu.sh output/live/ambar-os-live.iso          # live ISO, logs in as liveuser
#   ARCH=aarch64 os/test-qemu.sh output/qcow2/disk.qcow2   # ARM64 image
# Extra smoke-test flags: SMOKE_ARGS="--no-desktop"
set -euo pipefail
cd "$(dirname "$0")/.."
DISK="${1:-output/qcow2/disk.qcow2}"
MODE="${2:-}"
ARCH="${ARCH:-$(uname -m)}"
[ -f "$DISK" ] || { echo "no image at $DISK (run os/build.sh qcow2 or live-iso)" >&2; exit 2; }

case "$ARCH" in
  x86_64)
    FW_CANDIDATES=(/usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/qemu/OVMF.fd)
    QEMU_BIN=qemu-system-x86_64; MACHINE=q35; GPU=virtio-vga; CONSOLE=ttyS0 ;;
  aarch64|arm64)
    FW_CANDIDATES=(/usr/share/AAVMF/AAVMF_CODE.fd /usr/share/edk2/aarch64/QEMU_EFI-pflash.raw /usr/share/qemu-efi-aarch64/QEMU_EFI.fd)
    QEMU_BIN=qemu-system-aarch64; MACHINE=virt; GPU=virtio-gpu-pci; CONSOLE=ttyAMA0 ;;
  *) echo "unsupported ARCH=$ARCH" >&2; exit 2 ;;
esac
FW=""
for f in "${FW_CANDIDATES[@]}"; do [ -f "$f" ] && { FW="$f"; break; }; done
[ -n "$FW" ] || { echo "UEFI firmware not found for $ARCH; install ovmf (x86_64) or qemu-efi-aarch64" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'kill "${QPID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
VARS_SRC="${FW/CODE/VARS}"
if [ "$VARS_SRC" != "$FW" ] && [ -f "$VARS_SRC" ]; then cp "$VARS_SRC" "$WORK/vars.fd"; else truncate -s 64M "$WORK/vars.fd"; fi
# aarch64 pflash images must be exactly 64 MiB.
if [ "$MACHINE" = virt ]; then cp "$FW" "$WORK/code.fd"; truncate -s 64M "$WORK/code.fd"; FW="$WORK/code.fd"; fi

ACCEL="tcg"; CPU="max"
if [ -w /dev/kvm ] && [ "$ARCH" = "$(uname -m)" ]; then ACCEL="kvm"; CPU="host"; fi

SMOKE=()
case "$DISK" in
  *.iso) MEDIA=(-drive "file=$DISK,media=cdrom,readonly=on,if=none,id=cd0" -device "virtio-scsi-pci" -device "scsi-cd,drive=cd0,bootindex=0")
         SMOKE=(--live) ;;
  *)     MEDIA=(-drive "file=$DISK,if=virtio,snapshot=on") ;;
esac
echo "booting $DISK ($ARCH, accel=$ACCEL)"

QEMU=("$QEMU_BIN" -machine "$MACHINE,accel=$ACCEL" -cpu "$CPU" -m 6144 -smp 4
  -drive "if=pflash,format=raw,readonly=on,file=$FW"
  -drive "if=pflash,format=raw,file=$WORK/vars.fd"
  "${MEDIA[@]}"
  -nic "user,model=virtio-net-pci")
[ "$MACHINE" = virt ] && QEMU+=(-device qemu-xhci -device usb-kbd -device usb-tablet)

if [ "$MODE" = "--interactive" ]; then
  exec "${QEMU[@]}" -device "$GPU" -display gtk
fi
"${QEMU[@]}" -device "$GPU" -display none \
  -serial "unix:$WORK/serial.sock,server,nowait" \
  -monitor "unix:$WORK/monitor.sock,server,nowait" &
QPID=$!
for _ in $(seq 50); do [ -S "$WORK/serial.sock" ] && break; sleep 0.2; done
SHOT="$(realpath -m "${SCREENSHOT:-output/desktop.png}")"
mkdir -p "$(dirname "${SERIAL_LOG:-output/serial.log}")"
echo "serial console: $CONSOLE"
# shellcheck disable=SC2086  # SMOKE_ARGS is a list of flags
python3 os/smoke_test.py "$WORK/serial.sock" --log "${SERIAL_LOG:-output/serial.log}" \
  --monitor "$WORK/monitor.sock" --screenshot "$SHOT" "${SMOKE[@]}" ${SMOKE_ARGS:-}
