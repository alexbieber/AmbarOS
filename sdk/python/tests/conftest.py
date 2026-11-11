"""Starts a real ambard (built from this repo) in a temporary data directory."""
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sdk" / "python"))
BIN = Path(os.environ.get("AMBAR_TARGET_DIR", ROOT / "target" / "debug"))


@pytest.fixture
def ambard(tmp_path):
    data = tmp_path / "d"
    env = {**os.environ, "AMBARD_NOTIFY": "log", "AMBARD_MIN_INTERVAL": "100ms", "HOME": str(tmp_path)}
    proc = subprocess.Popen(
        [str(BIN / "ambard"), "--data-dir", str(data)],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    )
    sock = data / "run" / "ambard.sock"
    for _ in range(100):
        if sock.exists():
            break
        time.sleep(0.05)
    else:
        proc.kill()
        raise RuntimeError(proc.stderr.read().decode())
    yield data, tmp_path
    proc.terminate()
    proc.wait(timeout=30)
