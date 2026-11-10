"""Ambar Automation Center entry point."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Optional

# Use the fully custom-styled Basic controls everywhere (consistent on KDE, GNOME, macOS).
os.environ.setdefault("QT_QUICK_CONTROLS_STYLE", "Basic")

from PySide6.QtCore import QUrl  # noqa: E402
from PySide6.QtGui import QGuiApplication, QIcon  # noqa: E402
from PySide6.QtQml import QQmlApplicationEngine  # noqa: E402
from PySide6.QtQuick import QQuickWindow  # noqa: E402,F401  (registers the window type)

from .backend import Backend  # noqa: E402

QML = Path(__file__).with_name("qml") / "Main.qml"


def create(argv: list[str], data_dir: Optional[str] = None, theme: str = "",
           page: str = "overview", poll_ms: int = 2000):
    """Builds the app without running its event loop (used by tests)."""
    app = QGuiApplication.instance() or QGuiApplication(argv)
    app.setApplicationName("Ambar Automation Center")
    app.setOrganizationName("Ambar")
    app.setDesktopFileName("os.ambar.AutomationCenter")
    icon = Path(__file__).with_name("icon.svg")
    if icon.exists():
        app.setWindowIcon(QIcon(str(icon)))
    backend = Backend(data_dir, poll_ms=poll_ms)
    engine = QQmlApplicationEngine()
    ctx = engine.rootContext()
    ctx.setContextProperty("backend", backend)
    ctx.setContextProperty("initialTheme", theme)
    ctx.setContextProperty("initialPage", page)
    engine.load(QUrl.fromLocalFile(str(QML)))
    if not engine.rootObjects():
        raise RuntimeError(f"could not load {QML}")
    return app, engine, backend


def main(argv: Optional[list[str]] = None) -> int:
    argv = list(sys.argv if argv is None else argv)
    ap = argparse.ArgumentParser(prog="ambar-center", description="Ambar Automation Center")
    ap.add_argument("--data-dir", default=None, help="ambard data directory")
    ap.add_argument("--theme", choices=["", "light", "dark"], default="",
                    help="force a theme (default: follow the system)")
    args = ap.parse_args(argv[1:])
    app, engine, _backend = create(argv[:1], args.data_dir, args.theme)
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
