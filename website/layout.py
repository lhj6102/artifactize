#!/usr/bin/env python3
"""Lay out the landing page and its shared assets, without building the docs."""

from pathlib import Path
import shutil
import sys


WEBSITE = Path(__file__).resolve().parent
BRAND = WEBSITE.parent / "assets" / "brand"


def layout(destination: Path) -> None:
    """Copy the same landing layout for the full build and a private preview."""
    shutil.copytree(WEBSITE / "landing", destination, dirs_exist_ok=True)
    for name in ("favicon.svg", "favicon.png", "apple-touch-icon.png"):
        shutil.copyfile(BRAND / name, destination / name)
    for name in (
        "artifactize-icon.svg", "artifactize-icon-mark.svg",
        "artifactize-icon-h56.png", "og.jpg",
    ):
        shutil.copyfile(BRAND / name, destination / "media" / name)
    for name in ("review-loop.gif", "graph-growth.gif"):
        shutil.copyfile(WEBSITE.parent / "assets" / "demos" / name, destination / "media" / name)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: python3 website/layout.py DEST")
    layout(Path(sys.argv[1]))
