#!/usr/bin/env bash
# Boots the Ambar OS ISO in QEMU (UEFI) and runs the boot test.
#   os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso            # automatic test
#   os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso --interactive
#   TARGET_DISK=disk.qcow2 os/ubuntu/test-qemu.sh output/ubuntu/ambar-os-0.2-amd64.iso   # install onto disk.qcow2
#   TARGET_DISK=disk.qcow2 KEEP=1 os/ubuntu/test-qemu.sh disk                             # boot the installed disk
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
# Under emulation, "max" advertises AVX-512 that TCG emulates poorly; Mesa's software
# renderer (llvmpipe) then crashes. A plain x86-64-v2 CPU is reliable.
ACCEL=tcg; CPU="${CPU:-qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt,+cx16}"
if [ -w /dev/kvm ]; then ACCEL=kvm; CPU=host; fi
if [ "$ISO" = disk ]; then   # no ISO: start from the installed TARGET_DISK
  : "${TARGET_DISK:?set TARGET_DISK to the installed disk image}"; MEDIA=(-boot c)
else
  MEDIA=(-cdrom "$ISO" -boot d)
fi
MEM="${MEM:-4096}"; SMP="${SMP:-$(nproc)}"
QEMU=(qemu-system-x86_64 -machine "q35,accel=$ACCEL" -cpu "$CPU" -m "$MEM" -smp "$SMP"
  -drive "if=pflash,format=raw,readonly=on,file=$FW" -drive "if=pflash,format=raw,file=$WORK/vars.fd"
  "${MEDIA[@]}" ${TARGET_DISK:+-drive "file=$TARGET_DISK,if=virtio,format=qcow2"} -nic user,model=virtio-net-pci -device "${GPU:-virtio-vga}" -device qemu-xhci -device usb-tablet)
echo "booting ${ISO/#disk/$TARGET_DISK} (accel=$ACCEL, ${MEM} MB, $SMP CPUs)"
if [ "$MODE" = --interactive ]; then exec "${QEMU[@]}" -display gtk; fi
"${QEMU[@]}" -display none -serial "unix:$WORK/serial.sock,server,nowait" -monitor "unix:$WORK/mon.sock,server,nowait" &
QPID=$!
for _ in $(seq 50); do [ -S "$WORK/serial.sock" ] && break; sleep 0.2; done
if [ -n "${KEEP:-}" ]; then   # leave the VM running for manual inspection
  echo "serial: $WORK/serial.sock  monitor: $WORK/mon.sock"; trap - EXIT; wait "$QPID"; exit 0
fi
python3 os/ubuntu/smoke.py "$WORK/serial.sock" --log "$OUT/serial.log" --monitor "$WORK/mon.sock" --shots "$(realpath "$OUT")"
