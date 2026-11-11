"""Keeps the documentation honest.

- The site builds with strict link and anchor checking.
- Every `ambar …` command shown in the docs exists in the real CLI.
- Every flag shown for ambar-install / ambar-hwreport exists.
- Every page is in the navigation, and the community files the docs link to exist.
"""
import re
import shutil
import subprocess
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
PAGES = sorted(DOCS.glob("*.md"))
USER_DOCS = PAGES + [ROOT / "README.md", ROOT / "CONTRIBUTING.md"]


def ambar_bin():
    for p in (ROOT / "target/release/ambar", ROOT / "target/debug/ambar"):
        if p.exists():
            return p
    return shutil.which("ambar")


def help_commands(*args):
    out = subprocess.run([str(ambar_bin()), *args, "--help"], capture_output=True, text=True, check=True).stdout
    block = out.split("Commands:", 1)[1].split("Options:", 1)[0] if "Commands:" in out else ""
    return {m.group(1) for m in re.finditer(r"^\s{2}([a-z][a-z-]*)\s", block, re.M)} - {"help"}


def code_text(md: str) -> str:
    fenced = re.findall(r"```(?:sh|bash|console|text)?\n(.*?)```", md, re.S)
    inline = re.findall(r"`([^`\n]+)`", md)
    return "\n".join(fenced + inline)


def test_site_builds_strict(tmp_path):
    if not shutil.which("mkdocs"):
        pytest.skip("pip install mkdocs-material to check the site")
    r = subprocess.run(["mkdocs", "build", "--strict", "-q", "-d", str(tmp_path / "site")],
                       cwd=ROOT, capture_output=True, text=True, env={"NO_MKDOCS_2_WARNING": "1",
                                                                       "PATH": shutil.os.environ["PATH"]})
    assert r.returncode == 0, r.stderr[-3000:]


def nav_files(nav):
    for item in nav:
        for v in item.values() if isinstance(item, dict) else [item]:
            if isinstance(v, list):
                yield from nav_files(v)
            else:
                yield v


def test_every_page_is_in_the_nav():
    cfg = yaml.safe_load((ROOT / "mkdocs.yml").read_text().replace("!!", ""))
    in_nav = set(nav_files(cfg["nav"]))
    excluded = set(cfg.get("exclude_docs", "").split())
    for p in PAGES:
        assert p.name in in_nav or p.name in excluded, f"{p.name} is not in mkdocs.yml nav"
    for f in in_nav:
        assert (DOCS / f).exists(), f


def test_documented_ambar_commands_exist():
    if not ambar_bin():
        pytest.skip("build ambar first (cargo build)")
    top = help_commands()
    subs = {c: help_commands(c) for c in top}
    seen = 0
    for doc in USER_DOCS:
        for m in re.finditer(r"(?<![\w/-])ambar ([a-z][a-z-]*)(?: ([a-z][a-z-]*))?", code_text(doc.read_text())):
            cmd, sub = m.group(1), m.group(2)
            assert cmd in top, f"{doc.name}: `ambar {cmd}` is not a command"
            if subs[cmd] and sub and not sub.startswith("-"):
                assert sub in subs[cmd], f"{doc.name}: `ambar {cmd} {sub}` is not a command"
            seen += 1
    assert seen > 30, "expected the docs to show plenty of commands"


def test_documented_tool_flags_exist():
    inst = (ROOT / "os/live/installer/ambar_install.py").read_text()
    hw = (ROOT / "os/files/usr/bin/ambar-hwreport").read_text()
    for doc in USER_DOCS:
        text = code_text(doc.read_text())
        for flag in re.findall(r"ambar-install(?: [^\n`]*?)?((?: --[a-z-]+)+)", text):
            for f in flag.split():
                assert f'"{f}"' in inst, f"{doc.name}: ambar-install has no {f}"
        for flag in re.findall(r"ambar-hwreport (--[a-z-]+)", text):
            assert f'"{flag}"' in hw, f"{doc.name}: ambar-hwreport has no {flag}"


def test_community_files_exist():
    for f in ["LICENSE", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md",
              ".github/PULL_REQUEST_TEMPLATE.md"]:
        assert (ROOT / f).exists(), f
    assert "Apache License" in (ROOT / "LICENSE").read_text()
    tpl = ROOT / ".github/ISSUE_TEMPLATE"
    for doc in USER_DOCS:
        for name in re.findall(r"issues/new\?template=([\w.-]+)", doc.read_text()):
            assert (tpl / name).exists(), f"{doc.name} links to missing template {name}"
    for f in tpl.glob("*.yml"):
        y = yaml.safe_load(f.read_text())
        if f.name != "config.yml":
            assert y["name"] and y["body"], f.name


def test_readme_local_links_and_media_exist():
    text = (ROOT / "README.md").read_text()
    refs = re.findall(r'(?:src|href)="([^"#:]+)"', text) + re.findall(r"\]\(([^)#:]+)(?:#[^)]*)?\)", text)
    assert len(refs) > 20
    for ref in refs:
        assert (ROOT / ref).exists(), f"README points at missing {ref}"
    for f in ["docs/media/ambar-demo.mp4", "docs/media/ambar-demo.gif", "docs/images/hero.jpg"]:
        assert (ROOT / f).stat().st_size > 50_000, f
    assert "replayed from a file" in text, "the demo must say the AI reply is scripted"
