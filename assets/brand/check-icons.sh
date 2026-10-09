#!/bin/sh
# Check the caller's tools before evaluating; never install dependencies here.
set -eu
if ! command -v python3 >/dev/null 2>&1; then
    echo 'Brand renderer preflight failed: install Python 3, CairoSVG >=2.8, Pillow >=12.1 and system Cairo.' >&2
    echo 'See assets/brand/requirements.txt. This evaluation never installs tools.' >&2
    exit 2
fi
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# The generator performs module preflight before rendering in check mode.
exec python3 "$here/build-icons.py" --check --source "${1:?approved icon path is required}"
