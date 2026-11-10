#!/usr/bin/env bash
# Boots an Ambar OS disk image under UEFI and runs the serial-console smoke
# test (login, ambard running, CLI, marketplace, a real workflow run).
#   os/test-qemu.sh [output/qcow2/disk.qcow2] [--interactive]
set -euo pipefail
cd "$(dirname "$0")/.."
DISK="${1:-output/qcow2/disk.qcow2}"
MODE="${2:-}"
[ -f "$DISK" ] || { echo "no disk image at $DISK (run os/build.sh qcow2)" >&2; exit 2; }

OVMF=""
for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/qemu/OVMF.fd; do
  [ -f "$f" ] && { OVMF="$f"; break; }
done
[ -n "$OVMF" ] || { echo "UEFI firmware (OVMF) not found; install ovmf / edk2-ovmf" >&2; exit 2; }
VARS_SRC="${OVMF/CODE/VARS}"
WORK="$(mktemp -d)"; trap 'kill "${QPID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
cp "$VARS_SRC" "$WORK/vars.fd" 2>/dev/null || truncate -s 4M "$WORK/vars.fd"

ACCEL="tcg"; CPU="max"
if [ -w /dev/kvm ]; then ACCEL="kvm"; CPU="host"; fi
echo "booting $DISK (accel=$ACCEL)"

QEMU=(qemu-system-x86_64 -machine "q35,accel=$ACCEL" -cpu "$CPU" -m 4096 -smp 4
  -drive "if=pflash,format=raw,readonly=on,file=$OVMF"
  -drive "if=pflash,format=raw,file=$WORK/vars.fd"
  -drive "file=$DISK,if=virtio,snapshot=on"
  -nic "user,model=virtio-net-pci")

if [ "$MODE" = "--interactive" ]; then
  exec "${QEMU[@]}" -device virtio-vga -display gtk
fi
"${QEMU[@]}" -display none -serial "unix:$WORK/serial.sock,server,nowait" &
QPID=$!
for _ in $(seq 50); do [ -S "$WORK/serial.sock" ] && break; sleep 0.2; done
python3 os/smoke_test.py "$WORK/serial.sock" --log "${SERIAL_LOG:-output/serial.log}"
