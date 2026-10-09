#!/usr/bin/env bash
# Build the artifactize.dev site into website/dist: the landing page at
# dist/index.html and the mdBook docs at dist/docs/, then check every
# internal link. Needs mdBook $MDBOOK_VERSION (set MDBOOK to its path if it
# is not on PATH) and python3.
set -euo pipefail
cd "$(dirname "$0")"

MDBOOK_VERSION=0.5.4 # keep in sync with .github/workflows/website.yml
MDBOOK=${MDBOOK:-mdbook}
found=$("$MDBOOK" --version 2>/dev/null || true)
if [ "$found" != "mdbook v$MDBOOK_VERSION" ]; then
    echo "website/build.sh needs mdbook v$MDBOOK_VERSION (docs/theme/index.hbs derives from it); found: ${found:-none}" >&2
    exit 1
fi

rm -rf dist
mkdir -p dist
cp -R landing/. dist/
# Brand assets are committed only once, outside the website source tree.
brand="$PWD/../assets/brand"
cp "$brand"/favicon.{svg,png} "$brand/apple-touch-icon.png" dist/
cp "$brand"/artifactize-icon{,-mark}.svg "$brand/artifactize-icon-h56.png" \
    "$brand/og.jpg" dist/media/

# Inject the same favicons into an isolated mdBook source, never docs/theme/.
# mdBook fingerprints the copies and rewrites links in every generated page.
book=$(mktemp -d)
trap 'rm -rf "$book"' EXIT
cp -R docs/. "$book/"
cp "$brand"/favicon.{svg,png} "$book/theme/"
"$MDBOOK" build "$book" --dest-dir "$PWD/dist/docs"
python3 check-links.py dist
echo "built $(find dist -type f | wc -l) files into website/dist"
