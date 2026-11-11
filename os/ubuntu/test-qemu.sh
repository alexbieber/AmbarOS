#!/usr/bin/env bash
# Boots the Ambar OS ISO in QEMU (UEFI) and runs the boot test.
#   os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso            # automatic test
#   os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso --interactive
# Without KVM it runs under emulation: slow (expect 10–30 minutes), but real.
set -euo pipefail
cd "$(dirname "$0")/../.."
ISO="${1:?usage: test-qemu.sh <iso> [--interactive]}"
MODE="${2:-}"
OUT="${SHOTS:-output/ubuntu/boot}"
mkdir -p "$OUT"
FW=""
for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
  [ -f "$f" ] && { FW="$f"; break; }
done
[ -n "$FW" ] || { echo "install ovmf" >&2; exit 2; }
WORK="$(mktemp -d)"; trap 'kill "${QPID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
cp "${FW/CODE/VARS}" "$WORK/vars.fd"
ACCEL=tcg; CPU=max
if [ -w /dev/kvm ]; then ACCEL=kvm; CPU=host; fi
MEM="${MEM:-4096}"; SMP="${SMP:-$(nproc)}"
QEMU=(qemu-system-x86_64 -machine "q35,accel=$ACCEL" -cpu "$CPU" -m "$MEM" -smp "$SMP"
  -drive "if=pflash,format=raw,readonly=on,file=$FW" -drive "if=pflash,format=raw,file=$WORK/vars.fd"
  -cdrom "$ISO" -boot d -nic user,model=virtio-net-pci -device virtio-vga -device qemu-xhci -device usb-tablet)
echo "booting $ISO (accel=$ACCEL, ${MEM} MB, $SMP CPUs)"
if [ "$MODE" = --interactive ]; then exec "${QEMU[@]}" -display gtk; fi
"${QEMU[@]}" -display none -serial "unix:$WORK/serial.sock,server,nowait" -monitor "unix:$WORK/mon.sock,server,nowait" &
QPID=$!
for _ in $(seq 50); do [ -S "$WORK/serial.sock" ] && break; sleep 0.2; done
python3 os/ubuntu/smoke.py "$WORK/serial.sock" --log "$OUT/serial.log" --monitor "$WORK/mon.sock" --shots "$(realpath "$OUT")"
