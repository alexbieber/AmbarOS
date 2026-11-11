#!/usr/bin/env bash
# Rebrands an os-release file as Ambar OS. ID stays "fedora" so Fedora
# tooling (dnf, toolbox, bootc) keeps recognising the system.
#   os/branding.sh <os-release file> <fedora version>
set -euo pipefail
f="$1"; ver="$2"
set_kv() {  # set_kv KEY VALUE  (replace or append)
  if grep -q "^$1=" "$f"; then
    sed -i "s|^$1=.*|$1=$2|" "$f"
  else
    printf '%s=%s\n' "$1" "$2" >> "$f"
  fi
}
set_kv NAME '"Ambar OS"'
set_kv PRETTY_NAME "\"Ambar OS 0.1 (Fedora $ver)\""
set_kv VARIANT '"Ambar Desktop"'
set_kv VARIANT_ID 'ambar-desktop'
set_kv HOME_URL '"https://github.com/alexbieber/AmbarOS"'
set_kv IMAGE_ID 'ambar-os'
set_kv IMAGE_VERSION '0.1.0'
