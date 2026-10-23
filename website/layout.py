#!/usr/bin/env python3
"""Lay out the landing page and its shared assets, without building the docs."""

from html import escape
from html.parser import HTMLParser
from pathlib import Path
import shutil
import sys


WEBSITE = Path(__file__).resolve().parent
BRAND = WEBSITE.parent / "assets" / "brand"

# The landing page's languages and where each is served. landing/index.html holds
# them all: an element with data-locale="ko" appears only on the Korean page, one
# with data-locale="en" only on the English page, and data-ko-ATTR="value" gives
# the element's ATTR that value on the Korean page.
LOCALES = {"en": "index.html", "ko": "ko/index.html"}
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
        "source", "track", "wbr"}


def _quote(value: str) -> str:
    return escape(value, quote=False).replace('"', "&quot;")


class _Edits(HTMLParser):
    """Collect the source spans to drop or rewrite for one locale."""

    def __init__(self, source: str, locale: str) -> None:
        super().__init__(convert_charrefs=False)
        self.source = source
        self.locale = locale
        self.line_starts = [0]
        for index, char in enumerate(source):
            if char == "\n":
                self.line_starts.append(index + 1)
        self.edits: list[tuple[int, int, str]] = []
        self.skipping: tuple[str, int, int] | None = None  # tag, depth, start

    def position(self) -> int:
        line, column = self.getpos()
        return self.line_starts[line - 1] + column

    def handle_starttag(self, tag, attrs):
        start = self.position()
        end = start + len(self.get_starttag_text())
        if self.skipping:
            name, depth, begin = self.skipping
            if tag == name:
                self.skipping = (name, depth + 1, begin)
            return
        names = dict(attrs)
        if "data-locale" in names:
            if names["data-locale"] not in LOCALES:
                raise ValueError(f"line {self.getpos()[0]}: unknown data-locale {names['data-locale']!r}")
            if names["data-locale"] != self.locale:
                if tag in VOID:
                    self.edits.append((start, end, ""))
                else:
                    self.skipping = (tag, 1, start)
                return
        rewritten = self.rewrite(attrs)
        if rewritten is not None:
            close = "/>" if self.get_starttag_text().endswith("/>") else ">"
            self.edits.append((start, end, f"<{tag}{rewritten}{close}"))

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if self.skipping and self.skipping[0] == tag and self.skipping[1] == 1:
            _, _, begin = self.skipping
            self.skipping = None
            self.edits.append((begin, self.position() + len(self.get_starttag_text()), ""))
        elif self.skipping and self.skipping[0] == tag:
            name, depth, begin = self.skipping
            self.skipping = (name, depth - 1, begin)

    def handle_endtag(self, tag):
        if not self.skipping or tag != self.skipping[0]:
            return
        name, depth, begin = self.skipping
        if depth > 1:
            self.skipping = (name, depth - 1, begin)
            return
        self.skipping = None
        end = self.source.index(">", self.position()) + 1
        self.edits.append((begin, end, ""))

    def rewrite(self, attrs) -> str | None:
        """The attributes for this locale, or None when they stay as written."""
        overrides = {}
        kept = []
        changed = False
        for name, value in attrs:
            parts = name.split("-", 2)
            if name == "data-locale":
                changed = True
            elif len(parts) == 3 and parts[0] == "data" and parts[1] in LOCALES:
                changed = True
                if parts[1] == self.locale:
                    overrides[parts[2]] = value
            else:
                kept.append((name, value))
        if not changed:
            return None
        out = []
        for name, value in kept:
            value = overrides.pop(name, value)
            out.append(name if value is None else f'{name}="{_quote(value)}"')
        out += [f'{name}="{_quote(value)}"' for name, value in overrides.items()]
        return "".join(" " + item for item in out)


def localize(source: str, locale: str) -> str:
    """The page for one locale: other locales' elements dropped, attributes chosen."""
    parser = _Edits(source, locale)
    parser.feed(source)
    parser.close()
    if parser.skipping:
        raise ValueError(f"<{parser.skipping[0]} data-locale> is never closed")
    out = []
    position = 0
    for start, end, text in sorted(parser.edits):
        if not text:
            # A dropped element alone on its lines takes its lines with it.
            line_start = source.rfind("\n", 0, start) + 1
            line_end = source.find("\n", end)
            if not source[line_start:start].strip() and line_end != -1 and not source[end:line_end].strip():
                start, end = line_start, line_end + 1
        out.append(source[position:start])
        out.append(text)
        position = end
    out.append(source[position:])
    return "".join(out)


def layout(destination: Path) -> None:
    """Copy the same landing layout for the full build and a private preview."""
    shutil.copytree(WEBSITE / "landing", destination, dirs_exist_ok=True)
    source = (WEBSITE / "landing" / "index.html").read_text(encoding="utf-8")
    for locale, path in LOCALES.items():
        page = destination / path
        page.parent.mkdir(parents=True, exist_ok=True)
        page.write_text(localize(source, locale), encoding="utf-8")
    for name in ("favicon.svg", "favicon.png", "apple-touch-icon.png"):
        shutil.copyfile(BRAND / name, destination / name)
    for name in (
        "artifactize-icon.svg", "artifactize-icon-mark.svg",
        "artifactize-icon-h56.png", "og.jpg",
    ):
        shutil.copyfile(BRAND / name, destination / "media" / name)
    for name in ("review-loop.gif", "graph-growth.gif"):
        shutil.copyfile(WEBSITE.parent / "assets" / "demos" / name, destination / "media" / name)
    # The hero's example graph is rendered from assets/others/graph-example.html, and its role
    # image, in English on every page, from assets/others/roles.html.
    for name in [f"graph-example.{locale}.png" for locale in LOCALES] + ["roles.en.png"]:
        shutil.copyfile(WEBSITE.parent / "assets" / "others" / name, destination / "media" / name)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: python3 website/layout.py DEST")
    layout(Path(sys.argv[1]))
