#!/bin/sh
# Run against this build's archives in a clean Linux container or a macOS runner.
set -eu

: "${ARTIFACTIZE_VERSION:?set the built archive version}"
: "${ARTIFACTIZE_DOWNLOAD_URL:?set the archive download base}"
: "${HOME:?set an isolated install home}"

sh "$1"
PATH="$HOME/.local/bin:$PATH"
export PATH
for bin in artifactize artifactize-tools; do
    test "$("$bin" --version)" = "$bin $ARTIFACTIZE_VERSION"
done
artifactize --state-dir "$HOME/doctor-state" doctor

# Both commands must warn about an older install that shadows the new one.
mkdir "$HOME/shadow"
for bin in artifactize artifactize-tools; do
    cp "$HOME/.local/bin/$bin" "$HOME/shadow/$bin"
done
PATH="$HOME/shadow:$PATH" sh "$1" >"$HOME/install.log" 2>&1
cat "$HOME/install.log"
for bin in artifactize artifactize-tools; do
    grep -F "another $bin on your PATH" "$HOME/install.log"
done
