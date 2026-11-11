#!/usr/bin/env bash
# Builds Ambar OS (Ubuntu 26.04 LTS base, KDE Plasma 6) as a live/installer ISO.
#
#   sudo os/ubuntu/build.sh rootfs    # debootstrap + packages → $WORK/rootfs
#   sudo os/ubuntu/build.sh ambar     # (re)apply the Ambar layer to the rootfs
#   sudo os/ubuntu/build.sh iso       # rootfs → $OUT/ambar-os-<version>-amd64.iso
#   sudo os/ubuntu/build.sh all       # everything
#   sudo FLAVOR=security os/ubuntu/build.sh all   # Ambar OS Security (Kali's tools)
#
# Needs: debootstrap squashfs-tools xorriso grub-efi-amd64-bin grub-pc-bin mtools
#        dosfstools, root, and network access to the Ubuntu archive.
# BROWSER=firefox adds Mozilla's APT repository for the real Firefox (CI does
# this); the default falls back to Falkon when that repository isn't reachable.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
SUITE="${SUITE:-resolute}"
MIRROR="${MIRROR:-http://archive.ubuntu.com/ubuntu}"
WORK="${WORK:-/var/tmp/ambar-build}"
OUT="${OUT:-$REPO/output/ubuntu}"
ROOT="$WORK/rootfs"
VERSION="${VERSION:-0.2}"
BROWSER="${BROWSER:-auto}"
# FLAVOR=security builds "Ambar OS Security": the same OS with Kali's default tool
# set preinstalled (Ubuntu, pipx and verified binaries; Kali-container tools are
# added on first use). Every flavour includes ambar-sec and the Security app.
FLAVOR="${FLAVOR:-standard}"
case "$FLAVOR" in
  standard) SUFFIX=""; LABEL="Ambar OS"; VOLID="AMBAR_OS" ;;
  security) SUFFIX="-security"; LABEL="Ambar OS Security"; VOLID="AMBAR_OS_SEC" ;;
  *) echo "FLAVOR must be standard or security" >&2; exit 2 ;;
esac
export DEBIAN_FRONTEND=noninteractive LC_ALL=C.UTF-8

log() { printf '\n\033[1;33m== %s\033[0m\n' "$*"; }
[ "$(id -u)" = 0 ] || { echo "run as root" >&2; exit 2; }

mounts_up() {
  for d in dev dev/pts proc sys run; do
    mountpoint -q "$ROOT/$d" && continue
    mkdir -p "$ROOT/$d"
    case $d in
      proc) mount -t proc proc "$ROOT/proc" ;;
      sys) mount -t sysfs sys "$ROOT/sys" ;;
      run) mount -t tmpfs run "$ROOT/run" ;;
      *) mount --bind "/$d" "$ROOT/$d" ;;
    esac
  done
  # Name resolution inside the chroot: /etc/resolv.conf may point into /run
  # (systemd-resolved), which is an empty tmpfs here.
  mkdir -p "$ROOT/run/systemd/resolve"
  cp -L /etc/resolv.conf "$ROOT/run/systemd/resolve/stub-resolv.conf"
  cp -L /etc/resolv.conf "$ROOT/run/systemd/resolve/resolv.conf"
  [ -L "$ROOT/etc/resolv.conf" ] || cp -L /etc/resolv.conf "$ROOT/etc/resolv.conf"
}
mounts_down() {
  for d in run sys proc dev/pts dev; do umount -l "$ROOT/$d" 2>/dev/null || true; done
}
in_root() { chroot "$ROOT" /usr/bin/env -i HOME=/root PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  DEBIAN_FRONTEND=noninteractive LC_ALL=C.UTF-8 "$@"; }

rootfs() {
  if [ -f "$WORK/.debootstrap-done" ] && [ -d "$ROOT/usr" ] && [ -z "${FRESH:-}" ]; then
    log "reusing base system in $ROOT (FRESH=1 to start over)"
  else
    log "debootstrap $SUITE"
    rm -rf "$ROOT" "$WORK/.debootstrap-done"; mkdir -p "$ROOT"
    local script=/usr/share/debootstrap/scripts/$SUITE
    [ -e "$script" ] || script=/usr/share/debootstrap/scripts/gutsy
    debootstrap --arch=amd64 --components=main,restricted,universe,multiverse \
      "$SUITE" "$ROOT" "$MIRROR" "$script"
    touch "$WORK/.debootstrap-done"   # outside the rootfs, so the ISO step leaves it
  fi

  trap mounts_down EXIT
  mounts_up
  # Stock Ubuntu keeps a one-line stub here; the installer's mirror step removes it
  # and fails if it's missing.
  echo "# Ubuntu sources have moved to /etc/apt/sources.list.d/ubuntu.sources" > "$ROOT/etc/apt/sources.list"
  cat > "$ROOT/etc/apt/sources.list.d/ubuntu.sources" <<EOF
Types: deb
URIs: $MIRROR
Suites: $SUITE $SUITE-updates $SUITE-security $SUITE-backports
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
  # Keep snap-only shims, games and Kubuntu artwork out.
  : > "$ROOT/etc/apt/preferences.d/ambar-exclude"
  grep -v '^\s*#' "$HERE/exclude.txt" | grep -v '^\s*$' | while read -r p; do
    printf 'Package: %s\nPin: release *\nPin-Priority: -1\n\n' "$p" >> "$ROOT/etc/apt/preferences.d/ambar-exclude"
  done
  # No services start inside the build chroot.
  printf '#!/bin/sh\nexit 101\n' > "$ROOT/usr/sbin/policy-rc.d"; chmod +x "$ROOT/usr/sbin/policy-rc.d"
  echo ambar-os > "$ROOT/etc/hostname"

  log "packages"
  in_root apt-get update
  in_root apt-get -y full-upgrade
  mapfile -t PKGS < <(grep -v '^\s*#' "$HERE/packages.txt" | grep -v '^\s*$')
  in_root apt-get -y install "${PKGS[@]}"

  browser
  log "rootfs done: $(du -sxh "$ROOT" 2>/dev/null | cut -f1)"
}

browser() {
  local want="$BROWSER"
  if [ "$want" = auto ]; then
    if curl -fsS --max-time 15 -o /dev/null https://packages.mozilla.org/apt/dists/mozilla/InRelease 2>/dev/null; then
      want=firefox; else want=falkon; fi
  fi
  if [ "$want" = firefox ]; then
    log "Firefox from Mozilla's APT repository"
    install -d -m 0755 "$ROOT/etc/apt/keyrings"
    curl -fsSL https://packages.mozilla.org/apt/repo-signing-key.gpg -o "$ROOT/etc/apt/keyrings/packages.mozilla.org.asc"
    cat > "$ROOT/etc/apt/sources.list.d/mozilla.sources" <<'EOF'
Types: deb
URIs: https://packages.mozilla.org/apt
Suites: mozilla
Components: main
Signed-By: /etc/apt/keyrings/packages.mozilla.org.asc
EOF
    # Mozilla's deb outranks Ubuntu's snap shim.
    sed -i '/^Package: firefox$/,/^$/d' "$ROOT/etc/apt/preferences.d/ambar-exclude"
    printf 'Package: firefox*\nPin: origin packages.mozilla.org\nPin-Priority: 1000\n' > "$ROOT/etc/apt/preferences.d/mozilla"
    in_root apt-get update
    in_root apt-get -y install firefox
    echo firefox > "$ROOT/etc/ambar-browser"
  else
    log "Firefox repository not reachable: Falkon is the browser in this build"
    echo falkon > "$ROOT/etc/ambar-browser"
  fi
}

ambar() {
  trap mounts_down EXIT
  mounts_up
  log "Ambar software"
  [ -x "$REPO/target/release/ambard" ] || (cd "$REPO" && cargo build --release --locked --bin ambar --bin ambard)
  install -Dm755 "$REPO/target/release/ambar" "$ROOT/usr/bin/ambar"
  install -Dm755 "$REPO/target/release/ambard" "$ROOT/usr/bin/ambard"
  rm -rf "$ROOT/tmp/ambar-src"; mkdir -p "$ROOT/tmp/ambar-src"
  cp -r "$REPO/sdk/python" "$ROOT/tmp/ambar-src/sdk"
  cp -r "$REPO/desktop/automation-center" "$ROOT/tmp/ambar-src/center"
  in_root pip install --no-deps --no-build-isolation --break-system-packages --prefix /usr \
    /tmp/ambar-src/sdk /tmp/ambar-src/center
  rm -rf "$ROOT/tmp/ambar-src"
  # Desktop data shared with the Fedora image.
  cp -r "$REPO/desktop/plasma/color-schemes/." "$ROOT/usr/share/color-schemes/"
  mkdir -p "$ROOT/usr/share/wallpapers" && cp -r "$REPO/desktop/plasma/wallpapers/Ambar" "$ROOT/usr/share/wallpapers/"
  cp -r "$REPO/desktop/share/icons/." "$ROOT/usr/share/icons/"
  cp -r "$REPO/desktop/share/applications/." "$ROOT/usr/share/applications/"
  cp -r "$REPO/desktop/share/krunner" "$REPO/desktop/share/dbus-1" "$ROOT/usr/share/"
  install -Dm644 -t "$ROOT/etc/xdg/autostart" "$REPO"/desktop/share/autostart/*.desktop
  mkdir -p "$ROOT/usr/share/ambar/marketplace"
  cp -r "$REPO/marketplace/index.json" "$REPO/marketplace/packages" "$REPO/marketplace/ambar-examples.pub" \
    "$ROOT/usr/share/ambar/marketplace/"
  install -Dm644 "$REPO/os/files/usr/lib/systemd/user/ambard.service" "$ROOT/usr/lib/systemd/user/ambard.service"
  install -Dm644 "$REPO/os/files/etc/xdg/ambar/marketplace.toml" "$ROOT/etc/xdg/ambar/marketplace.toml"
  install -Dm755 "$REPO/os/files/usr/bin/ambar-hwreport" "$ROOT/usr/bin/ambar-hwreport"
  in_root systemctl --global enable ambard.service

  log "Ambar desktop: look-and-feel, menu bar widgets, Control Center"
  mkdir -p "$ROOT/usr/share/plasma/look-and-feel" "$ROOT/usr/share/plasma/plasmoids"
  cp -r "$REPO"/desktop/plasma/look-and-feel/* "$ROOT/usr/share/plasma/look-and-feel/"
  cp -r "$REPO"/desktop/plasma/plasmoids/* "$ROOT/usr/share/plasma/plasmoids/"
  install -Dm755 "$REPO/desktop/plasma/control-center/ambar-control-center" "$ROOT/usr/bin/ambar-control-center"
  mkdir -p "$ROOT/usr/share/aurorae/themes"
  cp -r "$REPO"/desktop/plasma/aurorae/* "$ROOT/usr/share/aurorae/themes/"

  log "Ambar Security toolkit (ambar-sec, the Security app, Launchpad menu)"
  local sec="$REPO/desktop/security"
  install -Dm755 "$sec/ambar-sec" "$ROOT/usr/bin/ambar-sec"
  install -Dm755 "$sec/ambar-security" "$ROOT/usr/bin/ambar-security"
  install -Dm644 "$sec/catalog.toml" "$ROOT/usr/share/ambar/security/catalog.toml"
  install -Dm644 "$sec/kali-default.txt" "$ROOT/usr/share/ambar/security/kali-default.txt"
  install -Dm644 "$sec/os.ambar.Security.desktop" "$ROOT/usr/share/applications/os.ambar.Security.desktop"
  install -Dm644 "$sec/ambar-security.svg" "$ROOT/usr/share/icons/hicolor/scalable/apps/ambar-security.svg"
  install -Dm644 "$sec/menu/ambar-security.menu" "$ROOT/etc/xdg/menus/applications-merged/ambar-security.menu"
  install -d "$ROOT/usr/share/desktop-directories"
  cp "$sec"/menu/*.directory "$ROOT/usr/share/desktop-directories/"

  log "Ambar desktop, branding and defaults (overlay + hooks)"
  cp -a "$HERE/overlay/." "$ROOT/"
  for h in "$HERE"/hooks/*.chroot; do
    log "hook $(basename "$h")"
    install -m755 "$h" "$ROOT/tmp/hook"
    in_root env AMBAR_VERSION="$VERSION" AMBAR_FLAVOR="$FLAVOR" /tmp/hook
    rm -f "$ROOT/tmp/hook"
  done
  in_root ambar --version
  in_root python3 -c "import ambar_center, ambar_sdk"
  in_root ambar-sec list --tier top10 >/dev/null
  if [ "$FLAVOR" = security ]; then
    log "Security edition: Kali's default tool set (Ubuntu, pipx, verified binaries)"
    in_root ambar-sec install default --system-only
    in_root ambar-sec status
  fi
}

iso() {
  # One rootfs per flavour: a security rootfs has the tools installed.
  local built; built="$(cat "$ROOT/etc/ambar-flavor" 2>/dev/null || echo standard)"
  [ "$built" = "$FLAVOR" ] || { echo "rootfs in $WORK is the '$built' flavour; use FLAVOR=$built or another WORK" >&2; exit 2; }
  log "clean up rootfs"
  mounts_down
  rm -f "$ROOT/usr/sbin/policy-rc.d"
  rm -rf "$ROOT/var/cache/apt/archives/"*.deb "$ROOT/var/lib/apt/lists/"* "$ROOT/tmp/"* "$ROOT/root/.cache"
  : > "$ROOT/etc/machine-id"

  local stage="$WORK/iso"
  rm -rf "$stage"; mkdir -p "$stage/casper" "$stage/boot/grub" "$stage/EFI/BOOT" "$stage/.disk"
  log "kernel + initrd"
  cp "$(ls -1 "$ROOT"/boot/vmlinuz-* | sort -V | tail -n1)" "$stage/casper/vmlinuz"
  cp "$(ls -1 "$ROOT"/boot/initrd.img-* | sort -V | tail -n1)" "$stage/casper/initrd"
  log "squashfs (this takes a while)"
  chroot "$ROOT" dpkg-query -W --showformat='${Package} ${Version}\n' > "$stage/casper/filesystem.manifest"
  mksquashfs "$ROOT" "$stage/casper/filesystem.squashfs" -noappend -comp "${SQUASH_COMP:-zstd}" \
    -Xcompression-level "${SQUASH_LEVEL:-15}" -e boot/grub -wildcards -e 'var/cache/apt/*' -quiet
  du -sx --block-size=1 "$ROOT" | cut -f1 > "$stage/casper/filesystem.size"
  echo "$LABEL $VERSION \"Ubuntu 26.04 LTS\" - amd64" > "$stage/.disk/info"

  cp "$HERE/iso/grub.cfg" "$stage/boot/grub/grub.cfg"
  sed -i "s/@VERSION@/$VERSION/g; s/Ambar OS/$LABEL/g" "$stage/boot/grub/grub.cfg"
  [ -f "$HERE/iso/background.png" ] && cp "$HERE/iso/background.png" "$stage/boot/grub/"
  # GRUB modules and font for the themed menu (gfxterm, png background).
  mkdir -p "$stage/boot/grub/x86_64-efi" "$stage/boot/grub/fonts"
  cp "$ROOT"/usr/lib/grub/x86_64-efi/*.mod "$ROOT"/usr/lib/grub/x86_64-efi/*.lst "$stage/boot/grub/x86_64-efi/" 2>/dev/null \
    || cp /usr/lib/grub/x86_64-efi/*.mod /usr/lib/grub/x86_64-efi/*.lst "$stage/boot/grub/x86_64-efi/"
  cp "$ROOT/usr/share/grub/unicode.pf2" "$stage/boot/grub/fonts/" 2>/dev/null || cp /usr/share/grub/unicode.pf2 "$stage/boot/grub/fonts/"

  log "UEFI boot (signed shim + GRUB) and BIOS boot"
  cp "$ROOT/usr/lib/shim/shimx64.efi.signed.latest" "$stage/EFI/BOOT/BOOTx64.EFI" 2>/dev/null \
    || cp "$ROOT/usr/lib/shim/shimx64.efi.signed" "$stage/EFI/BOOT/BOOTx64.EFI"
  cp "$ROOT/usr/lib/grub/x86_64-efi-signed/gcdx64.efi.signed" "$stage/EFI/BOOT/grubx64.efi"
  cp "$ROOT/usr/lib/shim/mmx64.efi" "$stage/EFI/BOOT/" 2>/dev/null || true
  # Signed GRUB looks for its config on the ISO in /EFI/ubuntu and /boot/grub.
  mkdir -p "$stage/EFI/ubuntu" && cp "$stage/boot/grub/grub.cfg" "$stage/EFI/ubuntu/grub.cfg"
  local efiimg="$stage/boot/grub/efi.img"
  truncate -s 10M "$efiimg"; mkfs.vfat -n AMBAR_EFI "$efiimg" >/dev/null
  mmd -i "$efiimg" ::/EFI ::/EFI/BOOT ::/EFI/ubuntu
  mcopy -i "$efiimg" "$stage/EFI/BOOT/"* ::/EFI/BOOT/
  mcopy -i "$efiimg" "$stage/boot/grub/grub.cfg" ::/EFI/ubuntu/grub.cfg
  grub-mkstandalone --format=i386-pc --output="$WORK/core.img" \
    --install-modules="linux normal iso9660 biosdisk memdisk search tar ls all_video gfxterm png part_gpt part_msdos fat ext2" \
    --modules="linux normal iso9660 biosdisk search" --locales="" --fonts="" \
    "boot/grub/grub.cfg=$stage/boot/grub/grub.cfg"
  cat /usr/lib/grub/i386-pc/cdboot.img "$WORK/core.img" > "$stage/boot/grub/bios.img"

  mkdir -p "$OUT"
  local iso="$OUT/ambar-os$SUFFIX-$VERSION-amd64.iso"
  log "xorriso → $iso"
  xorriso -as mkisofs -r -V "$VOLID" -o "$iso" -J -joliet-long -l \
    -iso-level 3 -partition_offset 16 --grub2-mbr /usr/lib/grub/i386-pc/boot_hybrid.img \
    -append_partition 2 0xef "$efiimg" -appended_part_as_gpt \
    -c /boot.catalog \
    -b /boot/grub/bios.img -no-emul-boot -boot-load-size 4 -boot-info-table --grub2-boot-info \
    -eltorito-alt-boot -e '--interval:appended_partition_2:all::' -no-emul-boot \
    "$stage"
  (cd "$OUT" && sha256sum "$(basename "$iso")" > "$(basename "$iso").sha256")
  log "done: $iso ($(du -h "$iso" | cut -f1))"
}

case "${1:-all}" in
  rootfs) rootfs ;;
  ambar) ambar ;;
  iso) iso ;;
  all) rootfs; ambar; iso ;;
  *) echo "usage: $0 rootfs|ambar|iso|all" >&2; exit 2 ;;
esac
