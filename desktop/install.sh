#!/usr/bin/env bash
# Installs the Ambar desktop pieces for the current user (no root needed):
# colour schemes, look-and-feel, wallpaper, icons, launcher entry, and the
# Automation Center Python app. On Plasma it also applies the Ambar look.
#   ./install.sh            light
#   ./install.sh --dark     dark
set -euo pipefail
cd "$(dirname "$0")"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
LNF=os.ambar.desktop; [ "${1:-}" = "--dark" ] && LNF=os.ambar.desktop.dark

install -d "$DATA/color-schemes" "$DATA/plasma/look-and-feel" "$DATA/wallpapers" \
           "$DATA/applications" "$DATA/icons/hicolor/scalable/apps"
cp plasma/color-schemes/*.colors "$DATA/color-schemes/"
rm -rf "$DATA/plasma/look-and-feel/os.ambar.desktop" "$DATA/plasma/look-and-feel/os.ambar.desktop.dark" "$DATA/wallpapers/Ambar"
cp -r plasma/look-and-feel/os.ambar.desktop plasma/look-and-feel/os.ambar.desktop.dark "$DATA/plasma/look-and-feel/"
cp -r plasma/wallpapers/Ambar "$DATA/wallpapers/"
cp share/icons/hicolor/scalable/apps/*.svg "$DATA/icons/hicolor/scalable/apps/"
cp share/applications/*.desktop "$DATA/applications/"
install -d "$DATA/krunner/dbusplugins" "$DATA/dbus-1/services" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
cp share/krunner/dbusplugins/*.desktop "$DATA/krunner/dbusplugins/"
sed "s|^Exec=.*|Exec=$HOME/.local/bin/ambar-krunner|" share/dbus-1/services/os.ambar.KRunner.service \
  > "$DATA/dbus-1/services/os.ambar.KRunner.service"
cp share/autostart/os.ambar.Welcome.desktop "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/"

python3 -m pip install --user --quiet ../sdk/python ./automation-center \
  || echo "note: install the app with pipx or a venv if your Python is externally managed"

if command -v plasma-apply-lookandfeel >/dev/null; then
  plasma-apply-lookandfeel --apply "$LNF" --resetLayout
  echo "Applied $LNF. Log out and back in if the panels look stale."
else
  echo "Installed. Not running Plasma: choose the Ambar look in your desktop's settings."
fi
