"""Automation Center against a real ambard, rendered offscreen.

Set AMBAR_SCREENSHOTS=<dir> to save a screenshot of every page in both themes.
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sdk" / "python"))
sys.path.insert(0, str(ROOT / "desktop" / "automation-center"))
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
BIN = Path(os.environ.get("AMBAR_TARGET_DIR", ROOT / "target" / "debug"))

from ambar_sdk import Client  # noqa: E402


def wf(root, wid, name, trigger="manual"):
    trig = {
        "manual": '[trigger]\ntype = "manual"',
        "schedule": '[trigger]\ntype = "schedule"\ncron = "0 18 * * 1-5"',
        "folder": f'[trigger]\ntype = "folder_watch"\npath = "{root}/inbox"\npattern = "*.pdf"',
    }[trigger]
    return f'''
id = "{wid}"
name = "{name}"
{trig}
[[actions]]
type = "write_file"
path = "{root}/out/{wid}.txt"
content = "ran"
append = true
[permissions]
write = ["{root}/out"]
read = ["{root}/inbox"]
'''


@pytest.fixture(scope="module")
def world(tmp_path_factory):
    tmp = tmp_path_factory.mktemp("center")
    (tmp / "inbox").mkdir()
    data = tmp / "d"
    env = {**os.environ, "AMBARD_NOTIFY": "log", "HOME": str(tmp)}
    proc = subprocess.Popen([str(BIN / "ambard"), "--data-dir", str(data)], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        if (data / "run" / "ambard.sock").exists():
            break
        time.sleep(0.05)
    with Client(data) as c:
        c.register(wf(tmp, "file-pdfs", "File new PDFs", "folder"))
        c.enable("file-pdfs")
        c.register(wf(tmp, "evening-report", "Evening work report", "schedule"))
        c.enable("evening-report")
        c.run_now("evening-report")
        c.pause("evening-report")
        c.register(wf(tmp, "weekly-notes", "Weekly notes file"))
        c.enable("weekly-notes")
        c.run_now("weekly-notes")
        c.run_now("weekly-notes")
        c.register(wf(tmp, "clean-desktop", "Tidy the desktop"))
        _, token = c.agent_create("planner", ["read", "propose", "memory"], "suggests automations")
        c.agent_create("reporter", ["read", "run:weekly-notes"])
        with Client(data, token=token) as a:
            a.propose(wf(tmp, "sort-screenshots", "Sort screenshots"), reason="Your Desktop has 40 screenshots")
            home_wide = wf(tmp, "back-up-everything", "Back up everything").replace(
                f'write = ["{tmp}/out"]', f'write = ["{tmp}"]')
            a.propose(home_wide, reason="Keep a copy of your files")
    yield tmp, data
    proc.terminate()
    proc.wait(timeout=30)


def pump(app, seconds=0.4):
    end = time.time() + seconds
    while time.time() < end:
        app.processEvents()
        time.sleep(0.01)


@pytest.fixture(scope="module")
def ui(world):
    from ambar_center.app import create

    tmp, data = world
    warnings = []
    app, engine, backend = create(["test"], str(data), theme="light", poll_ms=0)
    engine.warnings.connect(lambda ws: warnings.extend(w.toString() for w in ws))
    win = engine.rootObjects()[0]
    pump(app)
    yield app, win, backend, warnings, tmp


def shot(app, win, name):
    out = os.environ.get("AMBAR_SCREENSHOTS")
    pump(app, 0.3)
    if out:
        Path(out).mkdir(parents=True, exist_ok=True)
        assert win.grabWindow().save(str(Path(out) / f"{name}.png"))


def test_backend_reflects_service(ui):
    app, win, backend, warnings, _ = ui
    assert backend.connected
    states = {w["id"]: w["state"] for w in backend.workflows}
    assert states == {"clean-desktop": "inactive", "evening-report": "paused",
                      "file-pdfs": "active", "weekly-notes": "active"}
    assert len(backend.pendingProposals) == 2
    assert any(p["high"] for p in backend.pendingProposals)
    assert {a["name"] for a in backend.agents} == {"planner", "reporter"}
    assert len(backend.activity) == 3


@pytest.mark.parametrize("theme", ["light", "dark"])
def test_every_page_renders_without_qml_errors(ui, theme):
    app, win, backend, warnings, _ = ui
    win.setProperty("themeOverride", theme)
    for page in ["overview", "automations", "proposals", "agents", "activity"]:
        win.setProperty("page", page)
        shot(app, win, f"{theme}-{page}")
    assert not warnings, "\n".join(warnings)


def test_controls_reach_the_service(ui, world):
    app, win, backend, warnings, _ = ui
    _, data = world
    backend.control("enable", "clean-desktop")
    backend.control("resume", "evening-report")
    with Client(data) as c:
        states = {w["id"]: w["state"] for w in c.list_workflows()}
    assert states["clean-desktop"] == "active"
    assert states["evening-report"] == "active"
    low = next(p for p in backend.pendingProposals if not p["high"])
    backend.approve(low["id"], True)
    with Client(data) as c:
        assert any(w["id"] == "sort-screenshots" and w["state"] == "active" for w in c.list_workflows())
    assert len(backend.pendingProposals) == 1
    backend.control("bogus", "x")  # rejected locally, no crash
    assert not warnings


def test_ai_draft_review_and_add(ui, world, tmp_path):
    app, win, backend, warnings, tmp = ui
    _, data = world
    script = tmp_path / "script.txt"
    script.write_text(wf(tmp, "pdf-alert", "PDF alert").join(["```toml\n", "\n```"]))
    os.environ["AMBAR_AI_SCRIPT"] = str(script)
    backend._ambar = str(BIN / "ambar")
    got = []
    backend.draftReady.connect(lambda d: got.append(d))
    win.setProperty("page", "overview")
    backend.describe("tell me when a pdf arrives")
    deadline = time.time() + 30
    while not got and time.time() < deadline:
        pump(app, 0.1)
    assert got, "no draft"
    assert got[0]["id"] == "pdf-alert"
    shot(app, win, f"{win.property('themeOverride')}-draft")
    # Nothing was added yet.
    with Client(data) as c:
        assert all(w["id"] != "pdf-alert" for w in c.list_workflows())
    backend.addDraft(got[0]["toml"], True)
    with Client(data) as c:
        assert any(w["id"] == "pdf-alert" and w["state"] == "active" for w in c.list_workflows())
    del os.environ["AMBAR_AI_SCRIPT"]


def test_disconnected_state(tmp_path):
    from ambar_center.backend import Backend

    b = Backend(tmp_path / "none", poll_ms=0)
    assert not b.connected
    assert "isn't running" in b.error
