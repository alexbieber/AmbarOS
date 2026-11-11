"""Ambar's design for Qt Widgets apps: the same look as the Automation Center.

    import ambar_ui
    app = QApplication(sys.argv)
    ui = ambar_ui.apply(app)                 # follows the system's light/dark setting
    shell = ambar_ui.Shell("Workbench", "Security engagements", icon="ambar-workbench")
    page = shell.add_page("Findings", ambar_ui.Page("Findings", "What the scans found."))
    page.body.addWidget(ambar_ui.Card(...))

Tokens are the Automation Center's (desktop/automation-center/ambar_center/qml/Main.qml),
which come from the Plasma palette (desktop/plasma/generate.py). Every text/background
pair here meets WCAG AA; tests/test_ambar_ui.py checks it.
"""
from __future__ import annotations

import os
import tempfile
from pathlib import Path

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtGui import QColor, QFont, QFontDatabase, QGuiApplication, QIcon, QPalette, QPixmap
from PySide6.QtWidgets import (QAbstractItemView, QApplication, QFrame, QGraphicsDropShadowEffect, QHBoxLayout,
                               QHeaderView, QLabel, QListWidget, QListWidgetItem, QPushButton, QSizePolicy,
                               QStackedWidget, QStyleFactory, QTableWidget, QVBoxLayout, QWidget)

TOKENS = {
    "light": {
        "bg": "#F7F4EF", "side": "#EFE9E0", "surface": "#FFFFFF", "surface2": "#F4EFE8", "border": "#E3DACD",
        "text": "#1F1A15", "muted": "#6F655A", "accent": "#B35C05", "accent_soft": "#FBE9D1", "on_accent": "#FFFFFF",
        "good": "#21805A", "warn": "#9C6500", "bad": "#BE3A2E", "info": "#4A5A6E", "hover": "#EBE3D8",
        "select": "#FBE9D1", "focus": "#D98A33",
    },
    "dark": {
        "bg": "#15120F", "side": "#1C1814", "surface": "#231E19", "surface2": "#2C261F", "border": "#3B332A",
        "text": "#F4EEE6", "muted": "#A89D90", "accent": "#F2A43A", "accent_soft": "#3B2B16", "on_accent": "#1B1209",
        "good": "#62C796", "warn": "#E8B54E", "bad": "#F07B6F", "info": "#9DB2CC", "hover": "#2E2721",
        "select": "#3B2B16", "focus": "#F2A43A",
    },
}
# Severity and status colours used by badges and dots.
TONES = {"good": "good", "pass": "good", "succeeded": "good", "low": "warn", "warn": "warn", "medium": "warn",
         "high": "bad", "critical": "bad", "bad": "bad", "fail": "bad", "info": "info", "skip": "muted",
         "neutral": "muted", "accent": "accent"}

HERE = Path(__file__).resolve().parent
MARKS = [HERE / "ambar-mark.svg", Path("/usr/share/ambar/ambar-mark.svg"),
         HERE.parents[1] / "brand/ambar-mark.svg" if len(HERE.parents) > 1 else HERE]


def mode() -> str:
    forced = os.environ.get("AMBAR_THEME", "").lower()
    if forced in TOKENS:
        return forced
    app = QGuiApplication.instance()
    if app and app.styleHints().colorScheme() == Qt.ColorScheme.Dark:
        return "dark"
    return "light"


def mix(a: str, b: str, t: float) -> str:
    ca, cb = QColor(a), QColor(b)
    return QColor(round(ca.red() + (cb.red() - ca.red()) * t), round(ca.green() + (cb.green() - ca.green()) * t),
                  round(ca.blue() + (cb.blue() - ca.blue()) * t)).name()


def luminance(hex_colour: str) -> float:
    """Relative luminance (WCAG)."""
    def ch(v: int) -> float:
        c = v / 255
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
    q = QColor(hex_colour)
    return 0.2126 * ch(q.red()) + 0.7152 * ch(q.green()) + 0.0722 * ch(q.blue())


def badge_text(tone_colour: str, surface: str, text: str) -> str:
    return mix(tone_colour, text, 0.2) if luminance(surface) > 0.5 else tone_colour


def stylesheet(t: dict, check: str = "", dot: str = "") -> str:
    def badge_colours(tone: str) -> tuple[str, str]:
        c = t[tone] if tone != "muted" else t["muted"]
        bg = mix(t["surface"], c, 0.14)
        # On light backgrounds the tint lowers contrast, so the text is pulled towards the body colour.
        return badge_text(c, t["surface"], t["text"]), bg

    badge = "\n".join(
        f'QLabel[badge="{name}"], QFrame[card] QLabel[badge="{name}"] {{ color: {badge_colours(tone)[0]}; '
        f'background: {badge_colours(tone)[1]}; }}'
        for name, tone in TONES.items())
    return f"""
* {{ outline: 0; }}
QWidget {{ color: {t['text']}; font-size: 10pt; }}
QMainWindow, QDialog {{ background: {t['bg']}; }}
QWidget#AmbarContent, QStackedWidget#AmbarPages, QScrollArea, QScrollArea > QWidget > QWidget {{ background: {t['bg']}; }}
QToolTip {{ color: {t['bg']}; background: {t['text']}; border: 0; padding: 6px 8px; border-radius: 6px; }}

/* Sidebar */
QWidget#AmbarSidebar {{ background: {t['side']}; border-right: 1px solid {t['border']}; }}
QLabel#AmbarAppName {{ font-size: 13pt; font-weight: 700; }}
QLabel#AmbarAppSub {{ color: {t['muted']}; font-size: 9.5pt; }}
QListWidget#AmbarNav {{ background: transparent; border: 0; font-size: 10.5pt; }}
QListWidget#AmbarNav::item {{ color: {t['muted']}; padding: 9px 12px; margin: 1px 0; border-radius: 9px; }}
QListWidget#AmbarNav::item:hover {{ background: {t['hover']}; color: {t['text']}; }}
QListWidget#AmbarNav::item:selected {{ background: {t['accent_soft']}; color: {t['text']}; font-weight: 600; }}

/* Page header and text roles */
QLabel[role="title"] {{ font-size: 21pt; font-weight: 700; letter-spacing: -0.3px; }}
QLabel[role="subtitle"] {{ color: {t['muted']}; font-size: 10.5pt; }}
QLabel[role="section"] {{ color: {t['muted']}; font-size: 8.5pt; font-weight: 700; letter-spacing: 1px; }}
QLabel[role="cardtitle"] {{ font-size: 11.5pt; font-weight: 650; }}
QLabel[role="muted"] {{ color: {t['muted']}; }}
QLabel[role="hero"] {{ font-size: 30pt; font-weight: 700; letter-spacing: -0.5px; }}
QLabel[role="mono"] {{ font-family: "JetBrains Mono", "DejaVu Sans Mono", monospace; font-size: 9.5pt; }}
QLabel[role="notice"] {{ color: {t['text']}; background: {t['accent_soft']}; border: 1px solid {mix(t['accent_soft'], t['accent'], 0.35)};
    border-radius: 10px; padding: 10px 12px; }}

/* Cards */
QFrame[card="true"] {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 12px; }}
QFrame[card="true"] QLabel, QFrame[card="true"] QCheckBox, QFrame[card="true"] QRadioButton {{ background: transparent; }}
QFrame[card="stat"] {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 12px; }}
QFrame[card="stat"] QLabel {{ background: transparent; }}
QFrame[divider="true"] {{ background: {t['border']}; max-height: 1px; min-height: 1px; border: 0; }}

/* Badges */
QLabel[badge], QFrame[card] QLabel[badge] {{ border-radius: 10px; padding: 3px 10px; font-size: 9pt; font-weight: 650; }}
{badge}

/* Buttons */
QPushButton {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; padding: 7px 14px;
    font-weight: 550; min-height: 18px; }}
QPushButton:hover {{ background: {t['surface2']}; }}
QPushButton:pressed {{ background: {t['hover']}; }}
QPushButton:focus {{ border: 1px solid {t['focus']}; }}
QPushButton:disabled {{ color: {t['muted']}; background: {t['surface2']}; border-color: {t['border']}; }}
QPushButton[kind="primary"] {{ background: {t['accent']}; color: {t['on_accent']}; border: 1px solid {t['accent']}; }}
QPushButton[kind="primary"]:hover {{ background: {mix(t['accent'], t['text'], 0.12)}; }}
QPushButton[kind="primary"]:disabled {{ background: {t['surface2']}; color: {t['muted']}; border-color: {t['border']}; }}
QPushButton[kind="danger"] {{ color: {t['bad']}; }}
QPushButton[kind="ghost"] {{ background: transparent; border: 0; color: {t['accent']}; padding: 4px 6px; }}
QPushButton[kind="ghost"]:disabled {{ color: {t['muted']}; background: transparent; }}

/* Inputs */
QLineEdit, QPlainTextEdit, QTextEdit, QComboBox, QSpinBox {{ background: {t['surface']}; border: 1px solid {t['border']};
    border-radius: 8px; padding: 7px 10px; selection-background-color: {t['accent']}; selection-color: {t['on_accent']}; }}
QLineEdit:focus, QPlainTextEdit:focus, QTextEdit:focus, QComboBox:focus {{ border: 1px solid {t['focus']}; }}
QLineEdit[search="true"] {{ background: {t['surface2']}; border-radius: 10px; padding-left: 12px; }}
QPlainTextEdit[console="true"] {{ background: {mix(t['bg'], t['text'], 0.04)}; font-family: "JetBrains Mono", "DejaVu Sans Mono", monospace;
    font-size: 9.5pt; border-radius: 10px; padding: 10px; }}
QComboBox::drop-down {{ border: 0; width: 22px; }}
QComboBox QAbstractItemView {{ background: {t['surface']}; border: 1px solid {t['border']}; selection-background-color: {t['select']};
    selection-color: {t['text']}; padding: 4px; }}
QCheckBox, QRadioButton {{ spacing: 9px; }}
QCheckBox::indicator, QRadioButton::indicator, QTableView::indicator, QListView::indicator {{ width: 16px; height: 16px; border: 1.5px solid {t['muted']}; background: {t['surface']}; }}
QCheckBox::indicator, QTableView::indicator, QListView::indicator {{ border-radius: 5px; }}
QRadioButton::indicator {{ border-radius: 9px; }}
QCheckBox::indicator:checked, QTableView::indicator:checked, QListView::indicator:checked {{ background: {t['accent']}; border-color: {t['accent']}; image: url({check}); }}
QCheckBox::indicator:disabled {{ background: {t['surface2']}; border-color: {t['border']}; }}
QRadioButton::indicator:checked {{ background: {t['accent']}; border-color: {t['accent']}; image: url({dot}); }}
QCheckBox:disabled, QRadioButton:disabled {{ color: {t['muted']}; }}

/* Tables and lists */
QTableWidget, QTableView, QListWidget#AmbarList {{ background: {t['surface']}; alternate-background-color: {mix(t['surface'], t['surface2'], 0.5)};
    border: 1px solid {t['border']}; border-radius: 12px; gridline-color: transparent;
    selection-background-color: {t['select']}; selection-color: {t['text']}; }}
QTableWidget::item, QTableView::item {{ padding: 6px 8px; border-bottom: 1px solid {mix(t['surface'], t['border'], 0.6)}; }}
QListWidget#AmbarList::item {{ padding: 8px 10px; border-radius: 8px; margin: 2px 4px; }}
QListWidget#AmbarList::item:selected {{ background: {t['select']}; color: {t['text']}; }}
QHeaderView {{ background: transparent; }}
QHeaderView::section {{ background: {t['surface']}; color: {t['muted']}; border: 0; border-bottom: 1px solid {t['border']};
    padding: 8px; font-size: 8.5pt; font-weight: 700; text-transform: uppercase; }}
QTableCornerButton::section {{ background: {t['surface']}; border: 0; }}

/* Scroll bars: thin, macOS-like */
QScrollBar:vertical {{ background: transparent; width: 10px; margin: 2px; }}
QScrollBar:horizontal {{ background: transparent; height: 10px; margin: 2px; }}
QScrollBar::handle {{ background: {mix(t['bg'], t['muted'], 0.45)}; border-radius: 4px; min-height: 30px; min-width: 30px; }}
QScrollBar::handle:hover {{ background: {t['muted']}; }}
QScrollBar::add-line, QScrollBar::sub-line, QScrollBar::add-page, QScrollBar::sub-page {{ background: none; border: 0; width: 0; height: 0; }}
QProgressBar {{ background: {t['surface2']}; border: 0; border-radius: 4px; height: 8px; text-align: center; }}
QProgressBar::chunk {{ background: {t['accent']}; border-radius: 4px; }}
QSplitter::handle {{ background: {t['border']}; }}
QMenu {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 10px; padding: 5px; }}
QMenu::item {{ padding: 6px 18px; border-radius: 6px; }}
QMenu::item:selected {{ background: {t['select']}; }}
"""


SHAPES = {
    "check": '<path d="M3.5 8.3l3 3 6-6.6" fill="none" stroke="{c}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
    "dot": '<circle cx="8" cy="8" r="3.2" fill="{c}"/>',
}


def _mark(shape: str, colour: str) -> str:
    """A tick or a radio dot as an SVG file (Qt style sheets can't take inline SVG)."""
    d = Path(os.environ.get("XDG_RUNTIME_DIR") or tempfile.gettempdir()) / "ambar-ui"
    d.mkdir(parents=True, exist_ok=True)
    f = d / f"{shape}-{colour.lstrip('#')}.svg"
    if not f.exists():
        f.write_text('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">' + SHAPES[shape].format(c=colour) + "</svg>")
    return f.as_posix()


class Theme:
    def __init__(self, name: str):
        self.name = name
        self.t = TOKENS[name]

    def __getitem__(self, key: str) -> str:
        return self.t[key]

    def tone(self, kind: str) -> str:
        key = TONES.get(kind, kind)
        return self.t.get(key, self.t["muted"])


_theme: Theme | None = None


def theme() -> Theme:
    return _theme or Theme(mode())


def apply(app: QApplication) -> Theme:
    """Style the whole app. Call once, right after creating the QApplication."""
    global _theme
    _theme = Theme(mode())
    t = _theme.t
    app.setStyle(QStyleFactory.create("Fusion"))
    pal = QPalette()
    for role, key in [(QPalette.Window, "bg"), (QPalette.Base, "surface"), (QPalette.AlternateBase, "surface2"),
                      (QPalette.Button, "surface"), (QPalette.Text, "text"), (QPalette.WindowText, "text"),
                      (QPalette.ButtonText, "text"), (QPalette.Highlight, "accent"), (QPalette.HighlightedText, "on_accent"),
                      (QPalette.PlaceholderText, "muted"), (QPalette.ToolTipBase, "text"), (QPalette.ToolTipText, "bg"),
                      (QPalette.Link, "accent")]:
        pal.setColor(role, QColor(t[key]))
    pal.setColor(QPalette.Disabled, QPalette.Text, QColor(t["muted"]))
    pal.setColor(QPalette.Disabled, QPalette.WindowText, QColor(t["muted"]))
    pal.setColor(QPalette.Disabled, QPalette.ButtonText, QColor(t["muted"]))
    app.setPalette(pal)
    families = set(QFontDatabase.families())
    family = next((f for f in ("Inter", "Inter Variable", "Noto Sans", "DejaVu Sans") if f in families), app.font().family())
    font = QFont(family, 10)
    font.setHintingPreference(QFont.PreferNoHinting)  # crisp, even spacing, like macOS
    font.setStyleStrategy(QFont.PreferAntialias)
    app.setFont(font)
    app.setStyleSheet(stylesheet(t, _mark("check", t["on_accent"]), _mark("dot", t["on_accent"])))
    return _theme


# ----------------------------------------------------------------- building blocks

def label(text: str = "", role: str | None = None, wrap: bool = False) -> QLabel:
    lb = QLabel(text)
    if role:
        lb.setProperty("role", role)
    lb.setWordWrap(wrap)
    lb.setTextInteractionFlags(Qt.TextSelectableByMouse if role in ("mono",) else Qt.NoTextInteraction)
    return lb


def badge(text: str, kind: str) -> QLabel:
    b = QLabel(text)
    b.setProperty("badge", kind if kind in TONES else "neutral")
    b.setAlignment(Qt.AlignCenter)
    b.setSizePolicy(QSizePolicy.Maximum, QSizePolicy.Fixed)
    return b


def button(text: str, kind: str | None = None, icon: str | None = None) -> QPushButton:
    b = QPushButton(text)
    if kind:
        b.setProperty("kind", kind)
    if icon:
        b.setIcon(QIcon.fromTheme(icon))
    b.setCursor(Qt.PointingHandCursor)
    return b


def restyle(w: QWidget) -> None:
    """Re-apply the stylesheet after changing a dynamic property."""
    w.style().unpolish(w)
    w.style().polish(w)


class Card(QFrame):
    """A white, rounded panel with an optional title, like the Automation Center's cards."""

    def __init__(self, title: str | None = None, subtitle: str | None = None, padding: int = 18, shadow: bool = False):
        super().__init__()
        self.setProperty("card", "true")
        self.body = QVBoxLayout(self)
        self.body.setContentsMargins(padding, padding - 2, padding, padding)
        self.body.setSpacing(10)
        if title:
            self.body.addWidget(label(title, "cardtitle"))
        if subtitle:
            self.body.addWidget(label(subtitle, "muted", wrap=True))
        if shadow:
            fx = QGraphicsDropShadowEffect(self)
            fx.setBlurRadius(24)
            fx.setOffset(0, 4)
            fx.setColor(QColor(0, 0, 0, 28))
            self.setGraphicsEffect(fx)


class Stat(QFrame):
    """A number with a label and a coloured underline."""

    def __init__(self, caption: str, value: str = "0", kind: str = "accent"):
        super().__init__()
        self.setProperty("card", "stat")
        v = QVBoxLayout(self)
        v.setContentsMargins(16, 12, 16, 14)
        v.setSpacing(2)
        v.addWidget(label(caption, "muted"))
        self.value = label(value, "hero")
        v.addWidget(self.value)
        self.bar = QFrame()
        self.bar.setFixedSize(36, 4)
        v.addWidget(self.bar)
        self.set(value, kind)

    def set(self, value: str, kind: str | None = None) -> None:
        self.value.setText(value)
        if kind:
            self.bar.setStyleSheet(f"background: {theme().tone(kind)}; border-radius: 2px;")


class Page(QWidget):
    """A page with a large title, a subtitle, optional actions on the right, and a body."""

    def __init__(self, title: str, subtitle: str = ""):
        super().__init__()
        self.setObjectName("AmbarContent")
        outer = QVBoxLayout(self)
        outer.setContentsMargins(32, 26, 32, 24)
        outer.setSpacing(18)
        head = QHBoxLayout()
        text = QVBoxLayout()
        text.setSpacing(2)
        self.title = label(title, "title")
        text.addWidget(self.title)
        self.subtitle = label(subtitle, "subtitle", wrap=True)
        if subtitle:
            text.addWidget(self.subtitle)
        head.addLayout(text, 1)
        self.actions = QHBoxLayout()
        self.actions.setSpacing(8)
        head.addLayout(self.actions)
        head.setAlignment(self.actions, Qt.AlignTop)
        outer.addLayout(head)
        self.body = QVBoxLayout()
        self.body.setSpacing(14)
        outer.addLayout(self.body, 1)

    def section(self, text: str) -> QLabel:
        lb = label(text.upper(), "section")
        self.body.addWidget(lb)
        return lb


class Shell(QWidget):
    """Sidebar navigation (logo, app name, pages, footer) next to stacked pages."""

    pageChanged = Signal(int)

    def __init__(self, name: str, subtitle: str, icon: str | None = None, icon_file: str | Path | None = None):
        super().__init__()
        h = QHBoxLayout(self)
        h.setContentsMargins(0, 0, 0, 0)
        h.setSpacing(0)
        side = QWidget()
        side.setObjectName("AmbarSidebar")
        side.setAttribute(Qt.WA_StyledBackground, True)
        side.setFixedWidth(232)
        sv = QVBoxLayout(side)
        sv.setContentsMargins(16, 22, 16, 16)
        sv.setSpacing(14)
        brand = QHBoxLayout()
        brand.setSpacing(10)
        logo = QLabel()
        logo.setPixmap(app_icon(icon, icon_file).pixmap(QSize(36, 36)))
        brand.addWidget(logo)
        names = QVBoxLayout()
        names.setSpacing(0)
        n = label(name)
        n.setObjectName("AmbarAppName")
        s = label(subtitle)
        s.setObjectName("AmbarAppSub")
        names.addWidget(n)
        names.addWidget(s)
        brand.addLayout(names, 1)
        sv.addLayout(brand)
        sv.addSpacing(6)
        self.nav = QListWidget()
        self.nav.setObjectName("AmbarNav")
        self.nav.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.nav.setFrameShape(QFrame.NoFrame)
        sv.addWidget(self.nav)
        self.side = QVBoxLayout()  # optional sidebar content under the pages, e.g. a list of documents
        self.side.setSpacing(8)
        sv.addLayout(self.side, 20)
        sv.addStretch(1)  # keeps the footer at the bottom when `side` is empty
        self.footer = QVBoxLayout()
        self.footer.setSpacing(8)
        sv.addLayout(self.footer)
        h.addWidget(side)
        self.pages = QStackedWidget()
        self.pages.setObjectName("AmbarPages")
        h.addWidget(self.pages, 1)
        self.nav.currentRowChanged.connect(self._go)

    def add_page(self, name: str, page: QWidget, icon: str | None = None) -> QWidget:
        item = QListWidgetItem(QIcon.fromTheme(icon) if icon else QIcon(), name)
        item.setSizeHint(QSize(0, 38))
        self.nav.addItem(item)
        self.nav.setFixedHeight(self.nav.count() * 40 + 4)
        self.pages.addWidget(page)
        if self.nav.count() == 1:
            self.nav.setCurrentRow(0)
        return page

    def _go(self, row: int) -> None:
        self.pages.setCurrentIndex(row)
        self.pageChanged.emit(row)

    # QTabWidget-like API, so callers can say shell.setCurrentIndex(n)
    def setCurrentIndex(self, i: int) -> None:
        self.nav.setCurrentRow(i)

    def currentIndex(self) -> int:
        return self.nav.currentRow()

    def setTabText(self, i: int, text: str) -> None:
        if 0 <= i < self.nav.count():
            self.nav.item(i).setText(text)


def app_icon(name: str | None, file: str | Path | None = None) -> QIcon:
    """The app's themed icon, else its own SVG next to the program, else the Ambar mark."""
    icon = QIcon.fromTheme(name) if name else QIcon()
    if not icon.isNull():
        return icon
    for p in ([Path(file)] if file else []) + MARKS:
        if p.exists():
            return QIcon(str(p))
    return QIcon()


def svg_icon(path: Path, size: int = 64) -> QPixmap:
    return QIcon(str(path)).pixmap(QSize(size, size))


def table(headers: list[str], stretch: int | None = None) -> QTableWidget:
    t = QTableWidget(0, len(headers))
    t.setHorizontalHeaderLabels(headers)
    t.verticalHeader().hide()
    t.setShowGrid(False)
    t.setAlternatingRowColors(False)
    t.setEditTriggers(QAbstractItemView.NoEditTriggers)
    t.setSelectionBehavior(QAbstractItemView.SelectRows)
    t.setFocusPolicy(Qt.NoFocus)
    t.verticalHeader().setDefaultSectionSize(40)
    t.horizontalHeader().setHighlightSections(False)
    t.horizontalHeader().setDefaultAlignment(Qt.AlignLeft | Qt.AlignVCenter)
    if stretch is not None:
        t.horizontalHeader().setSectionResizeMode(stretch, QHeaderView.Stretch)
    return t


def divider() -> QFrame:
    f = QFrame()
    f.setProperty("divider", "true")
    return f


def row(*widgets, stretch_at: int | None = None, spacing: int = 10) -> QHBoxLayout:
    h = QHBoxLayout()
    h.setSpacing(spacing)
    for i, w in enumerate(widgets):
        if w is None:
            h.addStretch(1)
        elif isinstance(w, QHBoxLayout | QVBoxLayout):
            h.addLayout(w)
        else:
            h.addWidget(w, 1 if i == stretch_at else 0)
    return h
