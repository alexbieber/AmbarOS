"""ambar-clean and ambar-share, for real, in a throwaway home folder and on loopback."""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

UTILS = Path(__file__).resolve().parents[1]


def clean(home: Path, *args, stdin=None, bins: Path | None = None):
    env = {**os.environ, "HOME": str(home), "AMBAR_CLEAN_ROOT": str(home / "sysroot"),
           "PATH": f"{bins or home / 'nobin'}:/usr/bin:/bin", "AMBAR_CLEAN_ASSUME_ROOT": "1"}
    return subprocess.run([sys.executable, str(UTILS / "ambar-clean"), *args], capture_output=True, text=True,
                          env=env, input=stdin, timeout=60)


@pytest.fixture()
def home(tmp_path):
    h = tmp_path / "home"
    (h / ".cache/thumbnails/large").mkdir(parents=True)
    (h / ".cache/thumbnails/large/a.png").write_bytes(b"x" * 300_000)
    (h / ".cache/pip/http").mkdir(parents=True)
    (h / ".cache/pip/http/b").write_bytes(b"y" * 200_000)
    (h / ".cache/keep-me").mkdir()
    (h / ".cache/keep-me/data").write_text("not a known cache")
    (h / ".local/share/Trash/files").mkdir(parents=True)
    (h / ".local/share/Trash/files/old.pdf").write_bytes(b"z" * 100_000)
    # A cache folder that is a symlink to real data must never be followed.
    (h / "Documents").mkdir()
    (h / "Documents/thesis.txt").write_text("precious")
    (h / ".cache/mesa_shader_cache").symlink_to(h / "Documents")
    (h / "sysroot/var/cache/apt/archives").mkdir(parents=True)
    (h / "sysroot/var/cache/apt/archives/vim.deb").write_bytes(b"d" * 50_000)
    return h


def test_scan_reports_sizes(home):
    r = clean(home, "scan", "--json")
    items = {i["id"]: i for i in json.loads(r.stdout)}
    assert items["caches"]["size"] == 500_000
    assert items["trash"]["size"] == 100_000 and items["trash"]["safe"] is False
    assert items["apt-cache"]["size"] == 50_000
    assert not any("Documents" in p for p in items["caches"]["paths"])


def test_safe_clean_keeps_trash_and_symlinked_data(home, tmp_path):
    bins = tmp_path / "bin"
    bins.mkdir()
    calls = tmp_path / "calls"
    (bins / "apt-get").write_text(f'#!/bin/sh\necho "apt-get $*" >> "{calls}"\n')
    (bins / "apt-get").chmod(0o755)
    r = clean(home, "run", bins=bins)
    assert r.returncode == 0, r.stderr
    assert not list((home / ".cache/thumbnails").iterdir()) and not list((home / ".cache/pip").iterdir())
    assert (home / ".cache/thumbnails").is_dir(), "the cache folder itself stays"
    assert (home / ".cache/keep-me/data").exists(), "unknown folders are never touched"
    assert (home / "Documents/thesis.txt").read_text() == "precious", "symlinks are not followed"
    assert (home / ".local/share/Trash/files/old.pdf").exists(), "trash only on request"
    assert "apt-get clean" in calls.read_text()


def test_trash_needs_confirmation(home):
    r = clean(home, "run", "trash")
    assert r.returncode == 2 and "permanent" in r.stderr
    assert (home / ".local/share/Trash/files/old.pdf").exists()
    r = clean(home, "run", "trash", "--yes")
    assert r.returncode == 0 and not (home / ".local/share/Trash/files/old.pdf").exists()


def test_dry_run_deletes_nothing(home):
    r = clean(home, "run", "--dry-run")
    assert "Would free about 550.0 KB" in r.stdout, r.stdout
    assert (home / ".cache/pip/http/b").exists()


# ----------------------------------------------------------------- share

def start_share(tmp_path, files, *args):
    p = subprocess.Popen([sys.executable, str(UTILS / "ambar-share"), *map(str, files), "--host", "127.0.0.1", *args],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    url = None
    for _ in range(50):
        line = p.stdout.readline()
        m = re.search(r"(http://127\.0\.0\.1:\d+/s/\S+)", line)
        if m:
            url = m.group(1)
            break
    assert url, "no link printed"
    return p, url


def get(url):
    try:
        with urllib.request.urlopen(url, timeout=5) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, b""


def test_share_serves_only_the_link(tmp_path):
    f = tmp_path / "photo.jpg"
    f.write_bytes(b"\xff\xd8 jpeg bytes")
    (tmp_path / "secret.txt").write_text("not shared")
    p, url = start_share(tmp_path, [f], "--minutes", "1")
    try:
        assert get(url) == (200, b"\xff\xd8 jpeg bytes")
        base = url.rsplit("/s/", 1)[0]
        token = url.split("/s/")[1].split("/")[0]
        assert len(token) >= 22, "at least 128 random bits"
        assert get(f"{base}/s/wrongtoken/photo.jpg")[0] == 404
        assert get(f"{base}/s/{token}/secret.txt")[0] == 404
        assert get(f"{base}/s/{token}/../secret.txt")[0] == 404
        assert get(f"{base}/")[0] == 404
        status, page = get(f"{base}/s/{token}/")
        assert status == 200 and b"photo.jpg" in page
    finally:
        p.terminate()
        p.wait(5)


def test_share_once_stops_after_the_download(tmp_path):
    f = tmp_path / "a.txt"
    f.write_text("hello")
    p, url = start_share(tmp_path, [f], "--once")
    assert get(url) == (200, b"hello")
    p.wait(10)
    out = p.stdout.read()
    assert "1 download" in out and "→ 127.0.0.1" in out


def test_share_expires(tmp_path):
    f = tmp_path / "a.txt"
    f.write_text("hello")
    p, url = start_share(tmp_path, [f], "--minutes", "0.02")  # about one second
    time.sleep(2)
    assert p.wait(10) == 0
    with pytest.raises(urllib.error.URLError):
        urllib.request.urlopen(url, timeout=2)


def test_share_rejects_missing_files(tmp_path):
    r = subprocess.run([sys.executable, str(UTILS / "ambar-share"), str(tmp_path / "nope")], capture_output=True, text=True)
    assert r.returncode == 2 and "isn't a file" in r.stderr
