#!/usr/bin/env python3
"""Generates the Ambar sound theme (desktop/sounds/ambar): original sounds, synthesised here.

One warm voice (a soft marimba / glass tone) in D major pentatonic, short motifs,
quiet levels. Event names follow the freedesktop sound naming spec, so Plasma and
other apps find them; anything not drawn here falls back to Ocean, Plasma's own theme.

Run: python3 desktop/sounds/generate.py   (needs numpy and ffmpeg with libvorbis)
"""
from __future__ import annotations

import shutil
import subprocess
import tempfile
import wave
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
OUT = HERE / "ambar"
RATE = 48000
PEAK = 10 ** (-14 / 20)  # quiet: about -14 dBFS at the loudest point

# Note frequencies (equal temperament, A4 = 440 Hz)
NOTES = {n: 440 * 2 ** ((i - 9) / 12 + o - 4)
         for o in range(3, 8) for i, n in enumerate(["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"])
         for n in [f"{n}{o}"]}


def tone(note: str, dur: float, decay: float = 4.0, bright: float = 0.35, glass: bool = False) -> np.ndarray:
    """A struck tone: a few inharmonic partials, a 4 ms attack and an exponential fade."""
    f = NOTES[note]
    t = np.arange(int(RATE * dur)) / RATE
    partials = [(1.0, 1.0), (3.0, 0.18 * bright), (4.0, 0.10 * bright)] if glass else \
               [(1.0, 1.0), (3.93, 0.25 * bright), (9.2, 0.06 * bright)]  # marimba-like
    sig = sum(a * np.sin(2 * np.pi * f * k * t) * np.exp(-decay * k ** 0.5 * t) for k, a in partials)
    attack = np.minimum(1.0, t / 0.004)
    tail = np.minimum(1.0, (dur - t) / 0.03)  # no click at the end
    return sig * attack * np.clip(tail, 0, 1)


def seq(*events: tuple[float, np.ndarray], length: float | None = None) -> np.ndarray:
    """Mixes sounds placed at start times (seconds)."""
    end = max(s + len(x) / RATE for s, x in events)
    out = np.zeros(int(RATE * (length or end)) + 1)
    for s, x in events:
        i = int(RATE * s)
        out[i:i + len(x)] += x[: len(out) - i]
    return out


def tick(dur: float = 0.035) -> np.ndarray:
    t = np.arange(int(RATE * dur)) / RATE
    return np.sin(2 * np.pi * 2400 * t) * np.exp(-140 * t) + 0.4 * np.sin(2 * np.pi * 1200 * t) * np.exp(-90 * t)


def whoosh(dur: float = 0.45, seed: int = 7) -> np.ndarray:
    """Filtered noise that swells and falls (trash, sent)."""
    rng = np.random.default_rng(seed)
    n = rng.standard_normal(int(RATE * dur))
    k = np.exp(-np.linspace(0, 6, 400))  # gentle low-pass
    n = np.convolve(n, k / k.sum(), mode="same")
    t = np.linspace(0, 1, len(n))
    return n * np.sin(np.pi * t) ** 2 * 3.0


def arp(notes: list[str], step: float, dur: float = 1.0, **kw) -> np.ndarray:
    return seq(*[(i * step, tone(n, dur, **kw)) for i, n in enumerate(notes)])


def stereo(x: np.ndarray, width: float = 0.12) -> np.ndarray:
    """A touch of width: the right channel is a few milliseconds late and slightly softer."""
    d = int(RATE * 0.006)
    r = np.concatenate([np.zeros(d), x])[: len(x)] * (1 - width) + x * width
    return np.stack([x, r], axis=1)


LOGIN = arp(["D5", "F#5", "A5", "D6"], 0.11, 1.6, decay=2.6, glass=True)
SOUNDS: dict[str, np.ndarray] = {
    "desktop-login": LOGIN,
    "theme-demo": LOGIN,
    "desktop-logout": arp(["A5", "F#5", "D5"], 0.12, 1.3, decay=3.0, glass=True),
    "service-login": arp(["D5", "A5"], 0.09, 0.8, glass=True),
    "service-logout": arp(["A5", "D5"], 0.09, 0.8, glass=True),
    "message-new-instant": arp(["E6", "A6"], 0.08, 0.7, decay=5),
    "message-new-email": arp(["A5", "E6"], 0.08, 0.7, decay=5),
    "message-attention": arp(["E6", "A6", "E6"], 0.09, 0.6, decay=5),
    "message-highlight": arp(["F#6", "B6"], 0.07, 0.6, decay=5),
    "message-contact-in": arp(["B5", "E6"], 0.07, 0.5, decay=6),
    "message-contact-out": arp(["E6", "B5"], 0.07, 0.5, decay=6),
    "message-sent-instant": seq((0, whoosh(0.25) * 0.5), (0.05, tone("A6", 0.4, decay=8) * 0.6)),
    "dialog-information": tone("A5", 0.9, decay=4),
    "dialog-question": arp(["A5", "B5"], 0.1, 0.8),
    "dialog-warning": arp(["F#5", "F#5"], 0.14, 0.7, decay=5),
    "dialog-warning-auth": arp(["F#5", "A5"], 0.12, 0.7, decay=5),
    "dialog-error": arp(["C5", "B4"], 0.13, 0.9, decay=4, bright=0.6),
    "dialog-error-serious": arp(["D5", "C5", "B4"], 0.12, 0.9, decay=4, bright=0.6),
    "dialog-error-critical": arp(["D5", "C5", "B4", "A4"], 0.12, 1.0, decay=3.5, bright=0.7),
    "outcome-success": arp(["D5", "F#5", "A5"], 0.07, 0.9),
    "outcome-failure": arp(["A4", "F4"], 0.12, 0.9, bright=0.6),
    "completion-success": arp(["A5", "D6"], 0.08, 0.9, glass=True),
    "completion-partial": arp(["A5", "B5"], 0.08, 0.8, glass=True),
    "completion-fail": arp(["C5", "B4"], 0.12, 0.8, bright=0.6),
    "completion-rotation": tick(0.05),
    "complete-media-burn": arp(["D5", "F#5", "A5", "D6"], 0.08, 1.0, glass=True),
    "complete-media-error": arp(["C5", "B4"], 0.12, 0.8, bright=0.6),
    "media-insert-request": tone("A5", 0.9, decay=4),
    "device-added": arp(["D5", "A5"], 0.07, 0.6, decay=6),
    "device-removed": arp(["A5", "D5"], 0.07, 0.6, decay=6),
    "power-plug": arp(["D5", "D6"], 0.06, 0.6, decay=6, glass=True),
    "power-unplug": arp(["D6", "D5"], 0.06, 0.6, decay=6, glass=True),
    "battery-low": arp(["F#4", "D4"], 0.15, 0.9, decay=4),
    "battery-caution": arp(["F#4", "D4", "F#4", "D4"], 0.15, 0.8, decay=4),
    "battery-full": arp(["D5", "F#5", "A5", "D6"], 0.06, 0.8, glass=True),
    "audio-volume-change": tick(0.04),
    "button-pressed": tick(0.03),
    "button-pressed-modifier": tick(0.03) * 0.7,
    "bell": tone("A5", 0.8, decay=5, glass=True),
    "bell-window-system": tone("A5", 0.6, decay=6, glass=True),
    "trash-empty": seq((0, whoosh(0.5)), (0.32, tone("D5", 0.4, decay=9) * 0.4)),
    "screen-capture": seq((0, tick(0.03)), (0.07, tick(0.04))),
    "alarm-clock-elapsed": seq(*[(r * 0.9 + i * 0.12, tone(n, 0.7, decay=5, glass=True))
                                 for r in range(3) for i, n in enumerate(["D6", "A5", "D6"])]),
    "phone-incoming-call": seq(*[(r * 1.4 + i * 0.1, tone(n, 0.9, decay=4))
                                 for r in range(3) for i, n in enumerate(["A5", "D6", "F#6", "D6"])]),
    "game-over-winner": arp(["D5", "F#5", "A5", "D6", "F#6"], 0.09, 1.2, glass=True),
    "game-over-loser": arp(["A5", "F5", "D5", "A4"], 0.14, 1.0),
}


def write_oga(name: str, x: np.ndarray, tmp: Path) -> None:
    x = x / (np.max(np.abs(x)) or 1) * PEAK
    pcm = (stereo(x) * 32767).astype("<i2")
    wav = tmp / f"{name}.wav"
    with wave.open(str(wav), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(wav), "-c:a", "libvorbis", "-q:a", "4",
                    "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact",
                    str(OUT / "stereo" / f"{name}.oga")], check=True)


def main() -> int:
    if OUT.exists():
        shutil.rmtree(OUT)
    (OUT / "stereo").mkdir(parents=True)
    (OUT / "index.theme").write_text(
        "[Sound Theme]\nName=Ambar\nComment=Ambar OS: soft, warm and quiet\n"
        "Inherits=ocean,freedesktop\nDirectories=stereo\nExample=theme-demo\n\n[stereo]\nOutputProfile=stereo\n")
    with tempfile.TemporaryDirectory() as tmp:
        for name, x in SOUNDS.items():
            write_oga(name, x, Path(tmp))
    print(f"wrote {len(SOUNDS)} sounds to {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
