"""ambar-control-center against stand-in system tools (nmcli, bluetoothctl, …)."""
import json
import os
import stat
import sys
import subprocess
from pathlib import Path

import pytest

HELPER = Path(__file__).resolve().parents[1] / "control-center" / "ambar-control-center"

TOOLS = {
    "nmcli": r'''case "$*" in
  "-t radio wifi") cat "$STATE/wifi" ;;
  "-t -f active,ssid dev wifi") printf 'no:Other\nyes:Home\\:5G\n' ;;
  "radio wifi on") echo enabled > "$STATE/wifi" ;;
  "radio wifi off") echo disabled > "$STATE/wifi" ;;
  *) exit 1 ;;
esac''',
    "bluetoothctl": r'''case "$1" in
  show) printf 'Controller 00:11:22:33:44:55 ambar\n\tPowered: %s\n' "$(cat "$STATE/bt")" ;;
  power) [ "$2" = on ] && echo yes > "$STATE/bt" || echo no > "$STATE/bt" ;;
esac''',
    "rfkill": r'''case "$1" in
  -J) s=$(cat "$STATE/rf"); printf '{"": [{"id":0,"type":"wlan","soft":"%s","hard":"unblocked"},{"id":1,"type":"bluetooth","soft":"%s","hard":"unblocked"}]}' "$s" "$s" ;;
  block) echo blocked > "$STATE/rf" ;;
  unblock) echo unblocked > "$STATE/rf" ;;
esac''',
    "wpctl": r'''case "$1" in
  get-volume) v=$(cat "$STATE/vol"); m=$(cat "$STATE/mute"); echo "Volume: $v$m" ;;
  set-volume) echo "$3" > "$STATE/vol" ;;
  set-mute) if [ "$3" = toggle ]; then [ -s "$STATE/mute" ] && : > "$STATE/mute" || echo " [MUTED]" > "$STATE/mute"; else : > "$STATE/mute"; fi ;;
esac''',
    "kreadconfig6": r'''f="$STATE/cfg-$2-$4-$6"; [ -f "$f" ] && cat "$f" || true''',
    "kwriteconfig6": r'''f="$STATE/cfg-$2-$4-$6"; shift 6
if [ "$1" = --delete ]; then rm -f "$f"; exit 0; fi
[ "$1" = --type ] && shift 2
echo "$1" > "$f"''',
    "plasma-apply-lookandfeel": r'''echo "$2" > "$STATE/cfg-kdeglobals-KDE-LookAndFeelPackage"''',
    "plasma-apply-colorscheme": r'''echo "colors $1" >> "$STATE/appearance.log"''',
    "plasma-apply-desktoptheme": r'''echo "plasma $1" >> "$STATE/appearance.log"''',
    "plasma-changeicons": r'''echo "icons $1" >> "$STATE/appearance.log"''',
    "plasma-apply-aurorae": r'''echo "decoration $1" >> "$STATE/appearance.log"''',
    "busctl": r'''echo "$*" >> "$STATE/busctl.log"
case "$*" in
  *get-property*Display\ Brightness) echo '{"type":"i","data":6000}' ;;
  *get-property*Display\ MaxBrightness) echo '{"type":"i","data":12000}' ;;
  *SetBrightness*) echo ok ;;
  *) exit 0 ;;
esac''',
}


@pytest.fixture
def env(tmp_path):
    bin_ = tmp_path / "bin"
    bin_.mkdir()
    state = tmp_path / "state"
    state.mkdir()
    for name, body in TOOLS.items():
        p = bin_ / name
        p.write_text("#!/bin/sh\n" + body + "\n")
        p.chmod(p.stat().st_mode | stat.S_IEXEC)
    for f, v in {"wifi": "enabled", "bt": "yes", "rf": "unblocked", "vol": "0.40", "mute": ""}.items():
        (state / f).write_text(v + ("\n" if v else ""))
    return {"PATH": f"{bin_}:/usr/bin:/bin", "STATE": str(state)}, state


def cc(env, *args):
    r = subprocess.run([sys.executable, str(HELPER), *args], env=env, capture_output=True, text=True, timeout=30)
    return r.returncode, r.stdout


def status(env):
    code, out = cc(env, "status")
    assert code == 0
    return json.loads(out)


def test_status_reads_every_control(env):
    e, _ = env
    s = status(e)
    assert s["wifi"] == {"on": True, "detail": "Home:5G"}
    assert s["bluetooth"]["on"] is True
    assert s["airplane"]["on"] is False
    assert s["dark"]["on"] is False and s["nightlight"]["on"] is False and s["dnd"]["on"] is False
    assert s["volume"] == {"value": 40, "muted": False}
    assert s["brightness"] == {"value": 50}


def test_toggles_round_trip(env):
    e, state = env
    for key in ["wifi", "bluetooth", "airplane", "dark", "nightlight", "dnd"]:
        before = status(e)[key]["on"]
        assert cc(e, "set", key, "off" if before else "on")[0] == 0
        assert status(e)[key]["on"] is (not before), key
        assert cc(e, "set", key, "on" if before else "off")[0] == 0
        assert status(e)[key]["on"] is before, key
    assert (state / "cfg-kdeglobals-KDE-LookAndFeelPackage").read_text().strip() == "os.ambar.desktop"
    assert "reconfigure" in (state / "busctl.log").read_text(), "night light must reload KWin"
    log = (state / "appearance.log").read_text().split("\n")
    # dark on, then off: every part of the desktop is switched each time
    assert log[:4] == ["colors AmbarDark", "plasma breeze-dark", "icons Papirus-Dark", "decoration __aurorae__svg__AmbarDark"]
    assert log[4:8] == ["colors AmbarLight", "plasma default", "icons Papirus", "decoration __aurorae__svg__Ambar"]


def test_sliders(env):
    e, state = env
    assert cc(e, "set", "volume", "75")[0] == 0
    assert status(e)["volume"]["value"] == 75
    assert cc(e, "set", "volume", "mute")[0] == 0
    assert status(e)["volume"]["muted"] is True
    assert cc(e, "set", "volume", "250")[0] == 0
    assert status(e)["volume"]["value"] == 100, "clamped"
    assert cc(e, "set", "brightness", "25")[0] == 0
    assert "SetBrightness iu 3000 0" in (state / "busctl.log").read_text()


def test_missing_tools_are_hidden_not_errors(tmp_path):
    e = {"PATH": str(tmp_path), "STATE": str(tmp_path)}
    s = status(e)
    for key in ["wifi", "bluetooth", "airplane", "volume", "brightness"]:
        assert s[key] is None, key


def test_rejects_bad_input(env):
    e, _ = env
    assert cc(e, "set", "wifi", "maybe")[0] != 0
    assert cc(e, "set", "volume", "loud")[0] != 0
    assert cc(e, "set", "rm", "-rf")[0] != 0
    assert cc(e)[0] == 2
