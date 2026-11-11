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


def test_descriptions_are_never_parsed_as_flags(ui, world, tmp_path):
    app, win, backend, warnings, tmp = ui
    script = tmp_path / "script.txt"
    script.write_text(wf(tmp, "flag-test", "Flag test").join(["```toml\n", "\n```"]))
    os.environ["AMBAR_AI_SCRIPT"] = str(script)
    backend._ambar = str(BIN / "ambar")
    got, failed = [], []
    backend.draftReady.connect(lambda d: got.append(d))
    backend.draftFailed.connect(lambda m: failed.append(m))
    out = tmp_path / "should-not-exist.toml"
    backend.describe(f"--add --enable --output {out}")
    deadline = time.time() + 30
    while not got and not failed and time.time() < deadline:
        pump(app, 0.1)
    del os.environ["AMBAR_AI_SCRIPT"]
    assert got and not failed, failed
    assert not out.exists(), "description was parsed as --output"
    _, data = world
    with Client(data) as c:
        assert all(w["id"] != "flag-test" for w in c.list_workflows()), "description was parsed as --add"
    from PySide6.QtCore import QObject
    win.findChild(QObject, "draftPopup").close()
    pump(app, 0.2)


def test_live_approval_from_the_desktop(ui, world):
    import threading
    app, win, backend, warnings, tmp = ui
    _, data = world
    (tmp / "work").mkdir(exist_ok=True)
    (tmp / "work" / "old.log").write_text("x")
    with Client(data) as c:
        c.register(f'''
id = "cleanup"
name = "Clean up old logs"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "{tmp}/work/old.log"
[permissions]
write = ["{tmp}/work"]
''')
        c.enable("cleanup")
    result = {}
    t = threading.Thread(target=lambda: result.update(
        Client(data, timeout=60).run_now("cleanup")), daemon=True)
    t.start()
    deadline = time.time() + 15
    while not backend.approvals and time.time() < deadline:
        backend.refresh()
        pump(app, 0.2)
    assert backend.approvals, "approval never showed up"
    assert "old.log" in backend.approvals[0]["description"]
    win.setProperty("page", "overview")
    shot(app, win, f"{win.property('themeOverride')}-approval")
    assert (tmp / "work" / "old.log").exists()
    backend.decideApproval(backend.approvals[0]["id"], True)
    t.join(30)
    assert result.get("status") == "succeeded", result
    assert not (tmp / "work" / "old.log").exists()
    assert not backend.approvals
    assert not warnings, warnings


def test_disconnected_state(tmp_path):
    from ambar_center.backend import Backend

    b = Backend(tmp_path / "none", poll_ms=0)
    assert not b.connected
    assert "isn't running" in b.error


def test_welcome_setup_flow(ui, world, tmp_path, monkeypatch):
    app, win, backend, warnings, tmp = ui
    market = ROOT / "marketplace" / "index.json"
    monkeypatch.setenv("AMBAR_MARKETPLACE", str(market))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "cfg"))
    monkeypatch.setenv("AMBAR_CONFIG_DIR", str(tmp_path / "cfg" / "ambar"))
    monkeypatch.setenv("AMBAR_AI_CONFIG", str(tmp_path / "cfg" / "ambar" / "ai.toml"))
    from ambar_center.backend import welcome_done
    backend._ambar = str(BIN / "ambar")
    backend.refresh()
    packs = backend.starterPacks
    assert {p["name"] for p in packs} == {"dev.ambar.daily-notes", "dev.ambar.screenshot-sorter",
                                         "dev.ambar.tidy-downloads"}
    for theme in ["light", "dark"]:
        win.setProperty("themeOverride", theme)
        win.setProperty("page", "welcome")
        shot(app, win, f"{theme}-welcome")
    preview = backend.packPreview("dev.ambar.daily-notes")
    assert "signature  valid" in preview and "Documents/Notes" in preview, preview

    done = []
    backend.toast.connect(lambda k, m: done.append((k, m)))
    backend.installPack("dev.ambar.daily-notes")
    deadline = time.time() + 30
    while not done and time.time() < deadline:
        pump(app, 0.1)
    assert done and done[-1][0] == "ok", done
    assert any(p["installed"] for p in backend.starterPacks if p["name"] == "dev.ambar.daily-notes")
    with Client(world[1]) as c:
        assert any(w["id"] == "daily-note" and w["state"] == "active" for w in c.list_workflows())

    done.clear()
    backend.configureAi("ollama", "llama3.2", "")
    deadline = time.time() + 30
    while not done and time.time() < deadline:
        pump(app, 0.1)
    assert done and done[-1][0] == "ok", done
    assert 'provider = "ollama"' in (tmp_path / "cfg" / "ambar" / "ai.toml").read_text()

    assert not welcome_done()
    backend.finishWelcome()
    assert welcome_done()
    assert not warnings, warnings


def test_recommended_apps_install_only_from_the_fixed_list(ui, monkeypatch):
    app, win, backend, warnings, tmp = ui
    from ambar_center import backend as be
    ran = []
    monkeypatch.setattr(be.shutil, "which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr(backend, "run_in_terminal", ran.append)
    monkeypatch.setattr(be, "installed_flatpaks", lambda: {"org.signal.Signal"})
    backend._flatpaks = None
    apps = backend.recommendedApps
    assert {"com.google.Chrome", "com.visualstudio.code", "com.valvesoftware.Steam"} <= {a["id"] for a in apps}
    assert next(a for a in apps if a["id"] == "org.signal.Signal")["installed"]
    toasts = []
    backend.toast.connect(lambda k, m: toasts.append((k, m)))
    # Anything outside the list is dropped, even a crafted ID or a flag.
    backend.installApps(["com.visualstudio.code", "evil.App", "--from=http://x", "com.google.Chrome"])
    assert ran == [["flatpak", "install", "--system", "-y", "flathub", "com.google.Chrome", "com.visualstudio.code"]], ran
    backend.installApps(["evil.App"])
    assert len(ran) == 1 and toasts[-1][0] == "info"
    win.setProperty("page", "welcome")
    pump(app, 0.3)
    from PySide6.QtCore import QObject
    step = win.findChild(QObject, "recommendedApps")
    assert step is not None and step.property("visible")
    flick = step.parentItem()
    while flick is not None and flick.property("contentY") is None:
        flick = flick.parentItem()
    if flick is not None:  # scroll to the step for the screenshot
        flick.setProperty("contentY", max(0, flick.property("contentHeight") - flick.property("height")))
    for theme in ["light", "dark"]:
        win.setProperty("themeOverride", theme)
        shot(app, win, f"{theme}-welcome-apps")
    assert not [w for w in warnings if "Main.qml" in w], warnings
