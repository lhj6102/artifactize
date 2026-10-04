#!/usr/bin/env python3
"""Check every internal link in the built site (website/dist by default).

Each href/src/poster in the HTML pages, each href in mdBook's sidebar script
(toc.js) and each url() in the CSS must name a file in the build, and each
#fragment must name an id in its target page. Links to https://artifactize.dev
count as internal; other external links are counted, not fetched.
Exit status: 0 when every internal link resolves, 1 otherwise.
"""
import html.parser
import pathlib
import re
import sys
import urllib.parse

DIST = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "dist").resolve()
SITE = "https://artifactize.dev"
SKIP_SCHEMES = ("mailto:", "tel:", "javascript:", "data:")


class Page(html.parser.HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids, self.links, self.base = set(), [], None

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        for key in ("id", "name"):
            if a.get(key) and (key == "id" or tag == "a"):
                self.ids.add(a[key])
        if tag == "base" and a.get("href"):
            self.base = a["href"]
        for key in ("href", "src", "poster"):
            value = a.get(key)
            if value is None:
                continue
            if tag == "link" and a.get("rel") in ("preconnect", "dns-prefetch"):
                continue
            if tag == "meta":
                continue
            self.links.append((tag, key, value.strip()))
        if tag == "meta" and a.get("property", "").startswith("og:") and a.get("content", "").startswith(SITE):
            self.links.append((tag, "content", a["content"]))


def parse(path):
    page = Page()
    page.feed(path.read_text(encoding="utf-8", errors="replace"))
    return page


pages = {p: parse(p) for p in sorted(DIST.rglob("*.html"))}


def resolve(url, origin_dir):
    """Map a URL to (file in DIST or None, fragment). None file = external."""
    if url.startswith(SITE):
        url = url[len(SITE):] or "/"
    parts = urllib.parse.urlsplit(url)
    if parts.scheme or parts.netloc:
        return None, None
    path = urllib.parse.unquote(parts.path)
    if not path:
        return "SELF", parts.fragment
    target = (DIST / path.lstrip("/")) if path.startswith("/") else (origin_dir / path)
    try:
        target = target.resolve()
        target.relative_to(DIST)
    except ValueError:
        return "OUTSIDE", parts.fragment
    candidates = [target]
    if path.endswith("/") or target.is_dir():
        candidates = [target / "index.html"]
    elif target.suffix == "":
        candidates.append(target.with_suffix(".html"))  # Workers serves /x as x.html
    for c in candidates:
        if c.is_file():
            return c, parts.fragment
    return "MISSING", parts.fragment


broken, checked, external = [], 0, set()


def check(source, url, origin_dir, page=None):
    global checked
    if not url or url.startswith(SKIP_SCHEMES):
        return
    target, fragment = resolve(url, origin_dir)
    if target is None:
        external.add(url)
        return
    checked += 1
    if target in ("MISSING", "OUTSIDE"):
        broken.append((source, url, "no such file" if target == "MISSING" else "outside the site"))
        return
    if target == "SELF":
        target = source
    if fragment and target.suffix == ".html":
        ids = pages[target].ids if target in pages else set()
        if fragment not in ids:
            broken.append((source, url, f"no id {fragment!r}"))


for path, page in pages.items():
    origin = path.parent
    if page.base:
        b = page.base
        origin = (DIST / b.lstrip("/")) if b.startswith("/") else (path.parent / b)
    for tag, key, url in page.links:
        check(path, url, origin)

# mdBook builds the sidebar from toc.js; its links are relative to the book root.
for toc in DIST.rglob("toc*.js"):
    for url in re.findall(r'href="([^"]+)"', toc.read_text()):
        check(toc, url, toc.parent)

for css in DIST.rglob("*.css"):
    for url in re.findall(r"url\(\s*['\"]?([^'\")]+)['\"]?\s*\)", css.read_text()):
        if not url.startswith("data:"):
            check(css, url, css.parent)

for source, url, why in broken:
    print(f"BROKEN {source.relative_to(DIST)}: {url} ({why})")
print(f"link check: {len(pages)} pages, {checked} internal links checked, "
      f"{len(broken)} broken, {len(external)} distinct external links not fetched")
sys.exit(1 if broken else 0)
