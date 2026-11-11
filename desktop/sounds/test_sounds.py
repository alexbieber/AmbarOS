"""The Ambar sound theme: complete, quiet, valid audio, and installed as the default."""
from __future__ import annotations

import configparser
import shutil
import subprocess
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
THEME = HERE / "ambar"
REPO = HERE.parents[1]
# Every event Plasma's own theme (Ocean) has, so nothing falls back to a different style.
OCEAN = """alarm-clock-elapsed audio-volume-change battery-caution battery-full battery-low bell bell-window-system
button-pressed button-pressed-modifier complete-media-burn complete-media-error completion-fail completion-partial
completion-rotation completion-success desktop-login desktop-logout device-added device-removed dialog-error
dialog-error-critical dialog-error-serious dialog-information dialog-question dialog-warning dialog-warning-auth
game-over-loser game-over-winner media-insert-request message-attention message-contact-in message-contact-out
message-highlight message-new-instant message-sent-instant outcome-failure outcome-success phone-incoming-call
power-plug power-unplug service-login service-logout theme-demo trash-empty""".split()


def test_index_theme():
    cp = configparser.ConfigParser()
    cp.read(THEME / "index.theme")
    assert cp["Sound Theme"]["Name"] == "Ambar"
    assert cp["Sound Theme"]["Directories"] == "stereo" and cp["stereo"]["OutputProfile"] == "stereo"
    assert "ocean" in cp["Sound Theme"]["Inherits"]


def test_covers_every_plasma_event():
    have = {p.stem for p in (THEME / "stereo").glob("*.oga")}
    assert not set(OCEAN) - have, sorted(set(OCEAN) - have)


@pytest.mark.skipif(not shutil.which("ffprobe"), reason="needs ffprobe")
@pytest.mark.parametrize("name", ["desktop-login", "message-new-instant", "dialog-error", "trash-empty", "phone-incoming-call"])
def test_sounds_are_valid_short_and_quiet(name):
    f = THEME / "stereo" / f"{name}.oga"
    info = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration:stream=codec_name,channels",
                           "-of", "csv=p=0", str(f)], capture_output=True, text=True, check=True).stdout.split()
    assert info[0].startswith("vorbis,2") or "vorbis" in info[0]
    assert float(info[-1]) < 5.0, "event sounds stay short"
    vol = subprocess.run(["ffmpeg", "-i", str(f), "-af", "volumedetect", "-f", "null", "-"],
                         capture_output=True, text=True).stderr
    peak = float(vol.split("max_volume:")[1].split("dB")[0])
    assert -20 < peak < -10, f"{name} peak {peak} dB: quiet, but audible"


def test_installed_as_the_default():
    assert '"$REPO/desktop/sounds/ambar"' in (REPO / "os/ubuntu/build.sh").read_text()
    assert "[Sounds]\nTheme=ambar" in (REPO / "os/ubuntu/hooks/20-desktop.chroot").read_text()
    for pkg in ("os.ambar.desktop", "os.ambar.desktop.dark"):
        assert "[kdeglobals][Sounds]\nTheme=ambar" in (REPO / f"desktop/plasma/look-and-feel/{pkg}/contents/defaults").read_text()
