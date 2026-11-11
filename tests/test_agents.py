"""AGENTS.md and .cursor/rules must stay true to the repo: paths exist, rules parse."""
from __future__ import annotations

import re
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
RULES = ROOT / ".cursor" / "rules"


def test_agents_md_paths_exist():
    text = (ROOT / "AGENTS.md").read_text()
    paths = set(re.findall(r"`((?:os|desktop|crates|sdk|docs|scripts|marketplace|\.github)/[^`*|<> ]+?)/?`", text))
    assert len(paths) >= 8
    missing = sorted(p for p in paths if not (ROOT / p).exists())
    assert not missing, f"AGENTS.md mentions paths that don't exist: {missing}"
    # The files named in the os/ubuntu row
    for name in ["build.sh", "packages.txt", "exclude.txt", "hooks", "overlay", "iso/grub.cfg",
                 "test-qemu.sh", "smoke.py", "desktop-shots.sh", "overlay/etc/calamares/modules"]:
        assert (ROOT / "os/ubuntu" / name).exists(), name
    for name in ["generate.py", "aurorae.py", "plasmoids", "control-center/ambar-control-center"]:
        assert (ROOT / "desktop/plasma" / name).exists(), name


def test_cursor_rules_have_valid_frontmatter():
    files = sorted(RULES.glob("*.mdc"))
    assert len(files) >= 5
    always = 0
    for f in files:
        m = re.match(r"^---\n(.*?)\n---\n", f.read_text(), re.S)
        assert m, f"{f.name}: missing frontmatter"
        meta = yaml.safe_load(m.group(1))
        assert meta.get("description"), f"{f.name}: needs a description"
        always += bool(meta.get("alwaysApply"))
        for glob in str(meta.get("globs") or "").split(","):
            glob = glob.strip()
            if glob:
                root = glob.split("*")[0].rstrip("/") or "."
                assert (ROOT / root).exists(), f"{f.name}: glob {glob!r} matches nothing"
    assert always == 1, "exactly one rule should always apply (the project overview)"


def test_features_list_matches_status_marks():
    features = (ROOT / "FEATURES.md").read_text()
    # FEATURES must not mark as proven what STATUS lists as unproven.
    assert "⚠️" in features and "STATUS.md" in features
    for unproven in ["Secure Boot", "Firefox", "real model", "touchpad"]:
        line = next(l for l in features.splitlines() if unproven.lower() in l.lower())
        assert "⚠️" in line, f"FEATURES.md overclaims: {line}"
