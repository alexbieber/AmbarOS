#!/usr/bin/env python3
"""Renders docs/images/hero.jpg from scripts/demo/hero.html (run screenshots.py first)."""
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2]
with sync_playwright() as p:
    b = p.chromium.launch()
    page = b.new_page(viewport={"width": 1600, "height": 760}, device_scale_factor=2)
    page.goto((ROOT / "scripts/demo/hero.html").as_uri())
    page.wait_for_timeout(400)
    page.screenshot(path=str(ROOT / "docs/images/hero.jpg"), type="jpeg", quality=88)
    b.close()
print(ROOT / "docs/images/hero.jpg")
