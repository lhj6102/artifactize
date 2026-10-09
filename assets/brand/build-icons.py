#!/usr/bin/env python3
"""Derive all brand assets from artifactize-icon.svg and og-template.svg.

Run with python3 assets/brand/build-icons.py; --check compares bytes without
writing. The template contains outlined text, so no installed fonts are needed.
"""

import argparse
from io import BytesIO
import math
from pathlib import Path
import re
import sys
import xml.etree.ElementTree as ET

BRAND = Path(__file__).resolve().parent
NS = "http://www.w3.org/2000/svg"
SUPERSAMPLE = 8


def preflight():
    """Use the caller's tools; never install packages or rely on HOME caches."""
    try:
        import cairosvg
        from PIL import Image
    except (ImportError, OSError) as error:
        fail_preflight(str(error))
    for package, found, minimum in (
        ("CairoSVG", cairosvg.__version__, (2, 8)),
        ("Pillow", Image.__version__, (12, 1)),
    ):
        numbers = tuple(map(int, re.findall(r"\d+", found)[:2]))
        if numbers < minimum:
            fail_preflight(f"{package} {found} is too old (needs {'.'.join(map(str, minimum))} or later)")
    globals().update(cairosvg=cairosvg, Image=Image)
    return f"CairoSVG {cairosvg.__version__}, Pillow {Image.__version__}"


def fail_preflight(problem):
    print(
        f"Brand renderer preflight failed for {sys.executable}: {problem}\n"
        "Install Python 3 with CairoSVG >=2.8 and Pillow >=12.1, plus system Cairo.\n"
        f"  python3 -m pip install -r {BRAND / 'requirements.txt'}\n"
        "On Ubuntu, the system packages are python3-cairosvg, python3-pil and libcairo2.\n"
        "Artifactize runtime uses a private HOME: pip --user modules and HOME-based\n"
        "environments may be hidden. This evaluation never installs anything.",
        file=sys.stderr,
    )
    raise SystemExit(2)


def cubic(p0, p1, p2, p3, t):
    return (1 - t) ** 3 * p0 + 3 * (1 - t) ** 2 * t * p1 + 3 * (1 - t) * t ** 2 * p2 + t ** 3 * p3


def extrema(p0, p1, p2, p3):
    """Exact cubic Bezier extrema, including endpoints."""
    a = -p0 + 3 * p1 - 3 * p2 + p3
    b = 2 * (p0 - 2 * p1 + p2)
    c = p1 - p0
    roots = []
    if abs(a) < 1e-12:
        if abs(b) >= 1e-12:
            roots.append(-c / b)
    elif b * b - 4 * a * c >= 0:
        delta = math.sqrt(b * b - 4 * a * c)
        roots.extend(((-b + delta) / (2 * a), (-b - delta) / (2 * a)))
    return [p0, p3] + [cubic(p0, p1, p2, p3, t) for t in roots if 0 < t < 1]


def artwork_bounds(source):
    # The outer silhouette encloses the artwork and uses only absolute M/C/Z.
    # Fail explicitly if a future export changes that path syntax.
    outer = next(e for e in ET.fromstring(source).iter() if e.get("id") == "outer-silhouette")
    path = outer.attrib["d"]
    commands = re.findall(r"[A-Za-z]", path)
    if commands[0] != "M" or commands[-1] != "Z" or any(c != "C" for c in commands[1:-1]):
        raise ValueError("outer-silhouette must use absolute M/C/Z commands")
    numbers = list(map(float, re.findall(r"[-+]?(?:\d*\.\d+|\d+)(?:[eE][-+]?\d+)?", path)))
    if (len(numbers) - 2) % 6:
        raise ValueError("Invalid outer-silhouette Bezier coordinates")
    point = numbers[:2]
    xs, ys = [point[0]], [point[1]]
    for index in range(2, len(numbers), 6):
        p1, p2, p3 = numbers[index:index + 2], numbers[index + 2:index + 4], numbers[index + 4:index + 6]
        xs.extend(extrema(point[0], p1[0], p2[0], p3[0]))
        ys.extend(extrema(point[1], p1[1], p2[1], p3[1]))
        point = p3
    stroke = float(outer.attrib["stroke-width"])
    return min(xs) - stroke / 2, min(ys) - stroke / 2, max(xs) - min(xs) + stroke, max(ys) - min(ys) + stroke


def svg_canvas(source, bounds, width, height):
    """Change only the canvas; preserve all original artwork and colours."""
    x, y, w, h = bounds
    opening = (
        f'<svg xmlns="{NS}" width="{width}" height="{height}" '
        f'viewBox="{x:.6f} {y:.6f} {w:.6f} {h:.6f}" '
        'role="img" aria-labelledby="title desc">'
    )
    result, count = re.subn(r"^<svg[^>]*>", opening, source, count=1)
    if count != 1:
        raise ValueError("Source artwork must begin with its SVG root element")
    return result


def square_canvas(source, bounds, occupancy, background=None):
    x, y, width, height = bounds
    side = max(width, height) / occupancy
    left, top = x + (width - side) / 2, y + (height - side) / 2
    svg = svg_canvas(source, (left, top, side, side), 48, 48)
    if background:
        rect = (f'\n  <rect x="{left:.6f}" y="{top:.6f}" '
                f'width="{side:.6f}" height="{side:.6f}" fill="{background}"/>')
        end = svg.index(">") + 1
        svg = svg[:end] + rect + svg[end:]
    return svg


def rasterize(svg, width, height):
    png = cairosvg.svg2png(bytestring=svg.encode("utf-8"),
                          output_width=width * SUPERSAMPLE, output_height=height * SUPERSAMPLE)
    with Image.open(BytesIO(png)) as image:
        return image.convert("RGBA").resize((width, height), Image.Resampling.LANCZOS)


def png_bytes(image):
    output = BytesIO()
    image.save(output, format="PNG", optimize=True, compress_level=9)
    return output.getvalue()


def generated_assets():
    source = (BRAND / "artifactize-icon.svg").read_text(encoding="utf-8")
    bounds = artwork_bounds(source)
    _, _, width, height = bounds
    mark = svg_canvas(source, bounds, f"{width:.6f}", f"{height:.6f}")
    favicon = square_canvas(source, bounds, 0.90)  # 5% horizontal padding.
    apple = rasterize(square_canvas(source, bounds, 0.80, "#090B0F"), 180, 180)
    assert apple.getchannel("A").getextrema() == (255, 255)

    template = (BRAND / "og-template.svg").read_text(encoding="utf-8")
    icon = re.sub(r'^<svg[^>]*>', (
        f'<svg xmlns="{NS}" x="320" y="127" width="94" height="90" '
        f'viewBox="{bounds[0]:.6f} {bounds[1]:.6f} {width:.6f} {height:.6f}">'
    ), mark, count=1)
    if template.count("{{icon}}") != 1:
        raise ValueError("OG template must contain exactly one {{icon}} placeholder")
    og = rasterize(template.replace("{{icon}}", icon), 1200, 630).convert("RGB")
    jpeg = BytesIO()
    og.save(jpeg, format="JPEG", quality=92, subsampling=0, optimize=True, progressive=True)
    return {
        "artifactize-icon-mark.svg": mark.encode("utf-8"),
        "artifactize-icon-h56.png": png_bytes(rasterize(mark, round(width / height * 56), 56)),
        "favicon.svg": favicon.encode("utf-8"),
        "favicon.png": png_bytes(rasterize(favicon, 48, 48)),
        "apple-touch-icon.png": png_bytes(apple.convert("RGB")),
        "og.jpg": jpeg.getvalue(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare output bytes without writing")
    parser.add_argument("--preflight-only", action="store_true", help="check the caller's Python environment")
    parser.add_argument("--source", type=Path, help="assert the approved source is the canonical brand icon")
    args = parser.parse_args()
    if args.source and args.source.resolve() != (BRAND / "artifactize-icon.svg").resolve():
        parser.error("--source must refer to assets/brand/artifactize-icon.svg")
    print("Brand renderer: " + preflight())
    if args.preflight_only:
        return 0
    changed = []
    for filename, data in generated_assets().items():
        path = BRAND / filename
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                changed.append(filename)
        else:
            path.write_bytes(data)
        if path.suffix == ".svg":
            dimensions = ET.fromstring(data).attrib["viewBox"]
        else:
            with Image.open(BytesIO(data)) as image:
                dimensions = f"{image.width}x{image.height} {image.mode}"
        print(f"{filename}: {dimensions}; {len(data):,} bytes")
    if changed:
        print("Brand asset drift: " + ", ".join(changed), file=sys.stderr)
        print("Regenerate with python3 assets/brand/build-icons.py and commit the outputs.", file=sys.stderr)
        return 1
    if args.check:
        print("All 6 generated brand assets match the source and template byte-for-byte.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
