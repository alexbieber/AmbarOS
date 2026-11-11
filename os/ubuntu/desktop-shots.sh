#!/usr/bin/env bash
# Runs the real Ambar OS desktop from the built root filesystem on a virtual
# screen and drives it, so we can look at it and take screenshots.
#
#   sudo os/ubuntu/desktop-shots.sh start        # session up on :5, user "alex"
#   sudo os/ubuntu/desktop-shots.sh run <cmd…>   # run a command in the session as alex
#   sudo os/ubuntu/desktop-shots.sh shot <file>  # save the screen as PNG
#   sudo os/ubuntu/desktop-shots.sh stop
#
# The rootfs stays untouched: everything happens in an overlay on top of it,
# with three extra packages that aren't shipped (Xvfb, xdotool, imagemagick).
# The session is Plasma 6 on X11 here because a virtual X server is easy to
# capture; the ISO boots the same desktop on Wayland (see test-qemu.sh).
set -euo pipefail
WORK="${WORK:-/var/tmp/ambar-build}"
ROOT="$WORK/rootfs"
OV="$WORK/shots"
M="$OV/merged"
W="${SHOT_W:-1600}"; H="${SHOT_H:-1000}"

as_alex() {
  chroot "$M" /usr/bin/env -i HOME=/home/alex USER=alex LOGNAME=alex SHELL=/bin/bash \
    PATH=/usr/local/bin:/usr/bin:/bin DISPLAY=:5 XDG_RUNTIME_DIR=/run/user/1000 \
    XDG_SESSION_TYPE=x11 XDG_CURRENT_DESKTOP=KDE KDE_FULL_SESSION=true LANG=en_US.UTF-8 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus XDG_MENU_PREFIX=plasma- \
    XDG_CONFIG_DIRS=/etc/xdg:/usr/share/kubuntu-default-settings/kf5-settings \
    XDG_DATA_DIRS=/usr/share/plasma:/usr/local/share:/usr/share:/var/lib/flatpak/exports/share \
    /usr/sbin/runuser -u alex -- "$@"
}

start() {
  mkdir -p "$OV/upper" "$OV/work" "$M"
  mountpoint -q "$M" || mount -t overlay overlay -o "lowerdir=$ROOT,upperdir=$OV/upper,workdir=$OV/work" "$M"
  for d in dev dev/pts proc sys; do mountpoint -q "$M/$d" || mount --bind "/$d" "$M/$d"; done
  mountpoint -q "$M/run" || mount -t tmpfs run "$M/run"
  mountpoint -q "$M/dev/shm" || mount -t tmpfs -o mode=1777 shm "$M/dev/shm"
  mkdir -p "$M/run/systemd/resolve" && cp -L /etc/resolv.conf "$M/run/systemd/resolve/stub-resolv.conf"
  if ! chroot "$M" command -v Xvfb >/dev/null; then
    chroot "$M" env DEBIAN_FRONTEND=noninteractive apt-get update -qq
    chroot "$M" env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq xvfb xdotool imagemagick kwin-x11 \
      plasma-session-x11 >/dev/null || chroot "$M" env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
      xvfb xdotool imagemagick kwin-x11 >/dev/null
  fi
  chroot "$M" sh -c 'id alex >/dev/null 2>&1 || useradd -m -s /bin/bash -c "Alex" alex'
  chroot "$M" locale-gen en_US.UTF-8 >/dev/null 2>&1 || true
  install -d -m 700 -o 1000 -g 1000 "$M/run/user/1000"
  # No systemd user manager in a chroot: let startplasma launch services itself.
  chroot "$M" runuser -u alex -- kwriteconfig6 --file startkderc --group General --key systemdBoot false
  chroot "$M" sh -c 'Xvfb :5 -screen 0 '"${W}x${H}"'x24 -nolisten tcp +extension GLX >/tmp/xvfb.log 2>&1 &'
  sleep 2
  chroot "$M" runuser -u alex -- sh -c 'dbus-daemon --session --address=unix:path=/run/user/1000/bus --fork --nopidfile'
  as_alex sh -c 'nohup startplasma-x11 >/tmp/plasma.log 2>&1 &'
  for _ in $(seq 120); do as_alex pgrep -x plasmashell >/dev/null && break; sleep 1; done
  sleep "${SETTLE:-25}"
  echo "desktop is up"
}

stop() {
  chroot "$M" pkill -u alex 2>/dev/null || true
  chroot "$M" pkill Xvfb 2>/dev/null || true
  sleep 2
  for d in run sys proc dev/shm dev/pts dev; do umount -l "$M/$d" 2>/dev/null || true; done
  umount -l "$M" 2>/dev/null || true
}

case "${1:-}" in
  start) start ;;
  run) shift; as_alex "$@" ;;
  bg) shift; as_alex sh -c "nohup $* >/dev/null 2>&1 &" ;;
  shot) chroot "$M" sh -c 'DISPLAY=:5 import -window root /tmp/shot.png' && cp "$M/tmp/shot.png" "$2" && echo "$2" ;;
  log) cat "$M/tmp/plasma.log" ;;
  stop) stop ;;
  reset) stop; rm -rf "$OV" ;;
  *) echo "usage: $0 start|run|bg|shot <file>|log|stop|reset" >&2; exit 2 ;;
esac
